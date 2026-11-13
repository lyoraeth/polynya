// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::VecDeque;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

#[derive(Debug, PartialEq)]
pub enum Event {
    Changed(String),
    Complete(String),
    Cancel,
    Invalid,
}

struct Recorder {
    pressed: [bool; 256],
    pending: Option<Event>,
    finished: bool,
}

impl Default for Recorder {
    fn default() -> Self {
        Self { pressed: [false; 256], pending: None, finished: false }
    }
}

impl Recorder {
    fn key(&mut self, key: u32, up: bool) -> Option<Event> {
        if self.finished || key >= 256 {
            return None;
        }
        let repeat = self.pressed[key as usize];
        self.pressed[key as usize] = !up;
        if up && !self.pressed.iter().any(|pressed| *pressed) && self.pending.is_some() {
            self.finished = true;
            return self.pending.take();
        }
        if repeat && !up {
            return None;
        }
        let mut parts = Vec::new();
        for (name, keys) in
            [("Win", &[0x5B, 0x5C][..]), ("Ctrl", &[0x11, 0xA2, 0xA3][..]), ("Alt", &[0x12, 0xA4, 0xA5][..]), ("Shift", &[0x10, 0xA0, 0xA1][..])]
        {
            if keys.iter().any(|key| self.pressed[*key]) {
                parts.push(name.to_string());
            }
        }
        let is_modifier = |key| matches!(key, 0x10..=0x12 | 0x5B..=0x5C | 0xA0..=0xA5);
        if up && !is_modifier(key) {
            return None;
        }
        if up || is_modifier(key) {
            return self.pending.is_none().then(|| Event::Changed(parts.join("+")));
        }
        if parts.is_empty() {
            self.pending = match key {
                0x1B => Some(Event::Cancel),
                0x2E => Some(Event::Complete(String::new())),
                _ => None,
            };
            return self.pending.is_none().then_some(Event::Invalid);
        }
        if self.pressed.iter().enumerate().any(|(other, pressed)| *pressed && other as u32 != key && !is_modifier(other as u32)) {
            self.pending = None;
            return Some(Event::Invalid);
        }
        let name = match key {
            0x30..=0x39 | 0x41..=0x5A => char::from_u32(key)?.to_string(),
            0x70..=0x87 => format!("F{}", key - 0x70 + 1),
            _ => [
                "Space",
                "Enter",
                "Tab",
                "Insert",
                "Home",
                "End",
                "PageUp",
                "PageDown",
                "Backquote",
                "Minus",
                "Equal",
                "Comma",
                "Period",
                "Slash",
                "Semicolon",
                "Escape",
                "Delete",
                "Backspace",
                "Left",
                "Up",
                "Right",
                "Down",
            ]
            .into_iter()
            .find(|name| super::vk(&name.to_ascii_lowercase()) == Some(key))
            .map(String::from)
            .unwrap_or_default(),
        };
        if name.is_empty() {
            self.pending = None;
            return Some(Event::Invalid);
        }
        parts.push(name);
        let keys = parts.join("+");
        self.pending = Some(Event::Complete(keys.clone()));
        Some(Event::Changed(super::display(&keys)))
    }
}

pub struct Capture {
    recorder: Recorder,
    events: VecDeque<Event>,
}

impl Capture {
    pub fn start() -> Self {
        let mut recorder = Recorder::default();
        for key in [0x10, 0x11, 0x12, 0x5B, 0x5C] {
            // SAFETY: querying a virtual key has no preconditions.
            recorder.pressed[key] = unsafe { GetAsyncKeyState(key as i32) } < 0;
        }
        proto::info!("hotkeys", "launcher capture started");
        Self { recorder, events: VecDeque::new() }
    }

    pub fn key(&mut self, key: u32, up: bool) {
        if let Some(event) = self.recorder.key(key, up) {
            self.events.push_back(event);
        }
    }

    pub fn registered(&mut self, modifiers: u32, key: u32) {
        for (modifier, keys) in [(MOD_CONTROL, &[0x11][..]), (MOD_ALT, &[0x12][..]), (MOD_SHIFT, &[0x10][..]), (MOD_WIN, &[0x5B, 0x5C][..])] {
            if modifiers & modifier.0 != 0 && !keys.iter().any(|key| self.recorder.pressed[*key]) {
                self.recorder.pressed[keys[0]] = true;
            }
        }
        // Registered combinations arrive without the terminal key's ordinary key-down message.
        self.key(key, false);
        self.key(key, true);
    }

    pub fn take(&mut self) -> Option<Event> {
        self.events.pop_front()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        proto::info!("hotkeys", "launcher capture stopped");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

    #[test]
    fn hotkey_capture_resets_modifier_only_display() {
        let mut recorder = Recorder::default();
        assert_eq!(recorder.key(0x11, false), Some(Event::Changed("Ctrl".into())));
        assert_eq!(recorder.key(0x11, true), Some(Event::Changed(String::new())));
        assert!(!recorder.finished);
    }

    #[test]
    fn hotkey_capture_does_not_overwrite_a_chord_with_another_key() {
        let mut recorder = Recorder::default();
        recorder.key(0x11, false);
        recorder.key(0x41, false);
        assert_eq!(recorder.key(0x42, false), Some(Event::Invalid));
        for key in [0x41, 0x42, 0x11] {
            assert!(!matches!(recorder.key(key, true), Some(Event::Complete(_))));
        }
        assert!(!recorder.finished);
    }

    #[test]
    fn hotkey_capture_keeps_the_displayed_chord_when_modifiers_change() {
        let mut recorder = Recorder::default();
        recorder.key(0x11, false);
        recorder.key(0x41, false);
        assert_eq!(recorder.key(0x10, false), None);
        recorder.key(0x41, true);
        recorder.key(0x11, true);
        assert_eq!(recorder.key(0x10, true), Some(Event::Complete("Ctrl+A".into())));
    }

    #[test]
    fn hotkey_capture_handles_repeat_and_every_release_order() {
        for order in [[0x11, 0x12, 0x41], [0x11, 0x41, 0x12], [0x12, 0x11, 0x41], [0x12, 0x41, 0x11], [0x41, 0x11, 0x12], [0x41, 0x12, 0x11]] {
            let mut recorder = Recorder::default();
            for key in [0x11, 0x12, 0x41] {
                recorder.key(key, false);
            }
            assert_eq!(recorder.key(0x41, false), None);
            assert_eq!(recorder.key(order[0], true), None);
            assert_eq!(recorder.key(order[1], true), None);
            assert_eq!(recorder.key(order[2], true), Some(Event::Complete("Ctrl+Alt+A".into())));
            assert_eq!(recorder.key(order[2], true), None);
        }
    }

    #[test]
    fn hotkey_capture_modified_controls_are_bindings() {
        for (key, expected) in [(0x1B, "Ctrl+Escape"), (0x2E, "Ctrl+Delete"), (0x08, "Ctrl+Backspace")] {
            let mut recorder = Recorder::default();
            recorder.key(0x11, false);
            recorder.key(key, false);
            recorder.key(key, true);
            assert_eq!(recorder.key(0x11, true), Some(Event::Complete(expected.into())));
            assert!(super::super::parse(expected).is_some());
        }
        let mut recorder = Recorder::default();
        assert_eq!(recorder.key(0x08, false), Some(Event::Invalid));
        assert!(!recorder.finished);
        assert_eq!(recorder.key(256, false), None);
        assert_eq!(recorder.key(u32::MAX, true), None);
    }

    #[test]
    fn hotkey_capture_supported_keys_round_trip_through_display_and_parser() {
        for key in [
            0x08, 0x0D, 0x09, 0x1B, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E, 0x30, 0x39, 0x41, 0x5A, 0x70, 0x87, 0xC0, 0xBD, 0xBB,
            0xBC, 0xBE, 0xBF, 0xBA,
        ] {
            let mut recorder = Recorder::default();
            recorder.key(0x11, false);
            recorder.key(key, false);
            recorder.key(key, true);
            let Some(Event::Complete(keys)) = recorder.key(0x11, true) else { panic!("key {key:#x}") };
            assert_eq!(super::super::parse(&super::super::display(&keys)), Some((MOD_CONTROL, key)));
        }
    }

    #[test]
    fn hotkey_capture_waits_until_all_keys_are_released() {
        let mut recorder = Recorder::default();
        recorder.key(0xA2, false);
        recorder.key(0xA4, false);
        assert_eq!(recorder.key(0x20, false), Some(Event::Changed("Ctrl+Alt+Space".into())));
        assert_eq!(recorder.key(0x20, true), None);
        assert_eq!(recorder.key(0xA2, true), None);
        assert_eq!(recorder.key(0xA4, true), Some(Event::Complete("Ctrl+Alt+Space".into())));
        assert_eq!(super::super::parse("Ctrl+Alt+Space"), Some((MOD_CONTROL | MOD_ALT, 0x20)));
    }

    #[test]
    fn hotkey_capture_accepts_an_already_registered_combination() {
        let mut capture = Capture { recorder: Recorder::default(), events: VecDeque::new() };
        capture.key(0x12, false);
        capture.registered(MOD_ALT.0, 0x20);
        capture.key(0x12, true);
        let events: Vec<_> = std::iter::from_fn(|| capture.take()).collect();
        assert_eq!(events.last(), Some(&Event::Complete("Alt+Space".into())));
    }

    #[test]
    fn hotkey_capture_cancels_clears_and_rejects_plain_keys() {
        for (key, expected) in [(0x1B, Event::Cancel), (0x2E, Event::Complete(String::new()))] {
            let mut recorder = Recorder::default();
            recorder.key(key, false);
            assert_eq!(recorder.key(key, true), Some(expected));
        }
        let mut recorder = Recorder::default();
        assert_eq!(recorder.key(0x41, false), Some(Event::Invalid));
        assert_eq!(recorder.key(0x41, true), None);
        recorder.key(0x5B, false);
        assert_eq!(recorder.key(0x24, false), Some(Event::Changed("Win+Home".into())));
        recorder.key(0x24, true);
        assert_eq!(recorder.key(0x5B, true), Some(Event::Complete("Win+Home".into())));
        assert_eq!(super::super::parse("Win+Home"), Some((MOD_WIN, 0x24)));
        assert_eq!(super::super::parse("Ctrl+Shift+F2"), Some((MOD_CONTROL | MOD_SHIFT, 0x71)));
    }
}
