// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screen readers (Narrator, NVDA, JAWS): the island's controls through UI Automation, via AccessKit.
//!
//! Nothing runs until an assistive technology asks the window for its tree (`WM_GETOBJECT`); from
//! then on the tree follows the redraws, and only a changed island sends an update. Pressing a
//! control from a screen reader becomes a click at its center, the same path the mouse takes.
//!
//! Events are raised from a posted message rather than from inside a redraw: raising one may send
//! the window a nested `WM_GETOBJECT`, which must find the app state free.

use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};

use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, Affine, Live, Node, NodeId, Rect, Role as AkRole, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use accesskit_windows::Adapter;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

use crate::island::{Role, Snapshot};

const ROOT: NodeId = NodeId(0);

/// An assistive technology has asked for the tree: from now on it follows the island.
static ACTIVE: AtomicBool = AtomicBool::new(false);

thread_local! {
    static ADAPTER: RefCell<Option<Adapter>> = const { RefCell::new(None) };
    /// The last tree sent, with the scale: an unchanged island sends nothing.
    static LAST: RefCell<Option<(Snapshot, f32)>> = const { RefCell::new(None) };
    /// A changed tree waiting for `flush`.
    static PENDING: RefCell<bool> = const { RefCell::new(false) };
}

/// Messages to the island window: `sync` sends a staged tree, `action` presses the control whose id
/// is in `wParam`.
#[derive(Clone, Copy)]
pub struct Messages {
    pub sync: u32,
    pub action: u32,
}

static MESSAGES: std::sync::OnceLock<Messages> = std::sync::OnceLock::new();

struct Actions {
    hwnd: usize,
    action: u32,
}

impl ActionHandler for Actions {
    // Called on a UI Automation thread: hand the request to the window.
    fn do_action(&mut self, r: ActionRequest) {
        if r.action == Action::Click {
            // SAFETY: posting a message has no memory-safety preconditions; a stale window only fails.
            let _ = unsafe { PostMessageW(Some(HWND(self.hwnd as _)), self.action, WPARAM(r.target_node.0 as usize), LPARAM(0)) };
        }
    }
}

struct Initial(Option<TreeUpdate>);

impl ActivationHandler for Initial {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.take()
    }
}

/// Binds the adapter to the island window; called again for a recreated window.
pub fn init(hwnd: HWND, messages: Messages) {
    let _ = MESSAGES.set(messages);
    let adapter = Adapter::new(hwnd, false, Actions { hwnd: hwnd.0 as usize, action: messages.action });
    ADAPTER.with(|a| *a.borrow_mut() = Some(adapter));
    LAST.with(|l| *l.borrow_mut() = None);
}

/// Answers `WM_GETOBJECT`; `None` leaves it to `DefWindowProc`. `snapshot` is `None` when the app
/// state is busy (a nested call); the adapter then shows a placeholder until the next update.
pub fn get_object(wparam: WPARAM, lparam: LPARAM, snapshot: Option<(Snapshot, f32)>) -> Option<LRESULT> {
    let mut initial = Initial(snapshot.as_ref().map(|(s, scale)| tree(s, *scale)));
    let result = ADAPTER.with(|a| a.borrow_mut().as_mut()?.handle_wm_getobject(wparam, lparam, &mut initial));
    // The UI Automation call behind `into` may send a nested WM_GETOBJECT: no borrows here.
    let result = result.map(Into::into);
    if result.is_some() {
        ACTIVE.store(true, Ordering::Relaxed);
        if initial.0.is_none()
            && let Some(s) = snapshot
        {
            LAST.with(|l| *l.borrow_mut() = Some(s));
        }
    }
    result
}

/// Whether anyone listens: the island builds its snapshot only then.
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Stages the island's current state; sent by [`flush`] from a posted message if it changed.
pub fn stage(hwnd: HWND, snapshot: Snapshot, scale: f32) {
    let changed = LAST.with(|l| {
        let mut l = l.borrow_mut();
        if l.as_ref().is_some_and(|(s, k)| *s == snapshot && *k == scale) {
            return false;
        }
        *l = Some((snapshot, scale));
        true
    });
    if changed
        && !PENDING.with(|p| p.replace(true))
        && let Some(m) = MESSAGES.get()
    {
        // SAFETY: posting a message has no memory-safety preconditions; `hwnd` is our window.
        let _ = unsafe { PostMessageW(Some(hwnd), m.sync, WPARAM(0), LPARAM(0)) };
    }
}

/// Sends the staged tree and raises its events. Called from the window procedure, outside the app state.
pub fn flush() {
    PENDING.with(|p| p.replace(false));
    let Some(update) = LAST.with(|l| l.borrow().as_ref().map(|(s, scale)| tree(s, *scale))) else { return };
    let events = ADAPTER.with(|a| a.borrow_mut().as_mut()?.update_if_active(|| update));
    if let Some(e) = events {
        e.raise();
    }
}

/// The window gained or lost keyboard focus.
pub fn focused(is: bool) {
    let events = ADAPTER.with(|a| a.borrow_mut().as_mut()?.update_window_focus_state(is));
    if let Some(e) = events {
        e.raise();
    }
}

/// Center of a control in window DIPs, to press it with a click.
pub fn center(id: u64) -> Option<(f32, f32)> {
    LAST.with(|l| {
        let l = l.borrow();
        let (l, t, r, b) = l.as_ref()?.0.elements.iter().find(|e| e.id == id)?.rect?;
        Some(((l + r) / 2.0, (t + b) / 2.0))
    })
}

fn role(r: Role) -> AkRole {
    match r {
        Role::Button => AkRole::Button,
        Role::Switch => AkRole::Switch,
        Role::TextInput => AkRole::TextInput,
        Role::SearchInput => AkRole::SearchInput,
        Role::ListBox => AkRole::ListBox,
        Role::Option => AkRole::ListBoxOption,
        Role::Group => AkRole::Group,
        Role::Label => AkRole::Label,
        Role::Status => AkRole::Status,
    }
}

/// The whole tree: small enough to send in full on every change.
fn tree(s: &Snapshot, scale: f32) -> TreeUpdate {
    let children = |parent: u64| s.elements.iter().filter(|e| e.parent == parent).map(|e| NodeId(e.id)).collect::<Vec<_>>();
    let mut root = Node::new(AkRole::Window);
    root.set_label(crate::i18n::t().names.island);
    // Bounds are in DIPs; UI Automation wants pixels of the window.
    root.set_transform(Affine::scale(scale as f64));
    root.set_children(children(0));
    let mut nodes = vec![(ROOT, root)];
    for e in &s.elements {
        let mut n = Node::new(role(e.role));
        if !e.name.is_empty() {
            n.set_label(e.name.as_str());
        }
        if !e.description.is_empty() {
            n.set_description(e.description.as_str());
        }
        if let Some(v) = &e.value {
            n.set_value(v.as_str());
        }
        if let Some((l, t, r, b)) = e.rect {
            n.set_bounds(Rect::new(l as f64, t as f64, r as f64, b as f64));
        }
        if let Some(sel) = e.selected {
            n.set_selected(sel);
        }
        if let Some(on) = e.toggled {
            n.set_toggled(if on { Toggled::True } else { Toggled::False });
        }
        if e.role == Role::Status {
            n.set_live(Live::Polite);
        }
        if matches!(e.role, Role::Button | Role::Switch | Role::Option) {
            n.add_action(Action::Click);
        }
        let kids = children(e.id);
        if !kids.is_empty() {
            n.set_children(kids);
        }
        nodes.push((NodeId(e.id), n));
    }
    let focus = s.focus.filter(|f| s.elements.iter().any(|e| e.id == *f)).map_or(ROOT, NodeId);
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo { root: ROOT, toolkit_name: Some("Polynya".into()), toolkit_version: Some(env!("POLYNYA_VERSION").into()) }),
        tree_id: TreeId::ROOT,
        focus,
    }
}
