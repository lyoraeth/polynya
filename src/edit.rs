// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! A one-line text field: the text, the caret, the selection and undo. The search line and the input
//! rows of module screens share it, so both edit the same way: arrows and words, Shift selection,
//! the clipboard, Ctrl+Z, and the mouse (the caret goes where it is clicked, a drag selects).

use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_A, VK_BACK, VK_C, VK_DELETE, VK_END, VK_HOME, VK_LEFT, VK_RIGHT, VK_V, VK_X, VK_Z};

#[derive(Debug, Default, Clone)]
pub struct LineEdit {
    pub text: String,
    /// Caret position in bytes, always on a character boundary.
    pub caret: usize,
    /// The other end of the selection; selected from it to the caret.
    anchor: Option<usize>,
    /// Previous states for Ctrl+Z: text and caret.
    undo: Vec<(String, usize)>,
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edited {
    /// Not an editing key: the caller handles it.
    No,
    /// The caret or the selection moved.
    Moved,
    /// The text changed.
    Changed,
}

impl LineEdit {
    pub fn new(text: &str) -> Self {
        Self { text: text.to_string(), caret: text.len(), ..Default::default() }
    }

    /// Replaces the text, caret at the end; undo starts over.
    pub fn set(&mut self, text: &str) {
        *self = Self::new(text);
    }

    fn utf16(&self, at: usize) -> u32 {
        self.text[..at].encode_utf16().count() as u32
    }

    /// Byte position of a UTF-16 position, as DirectWrite reports hits.
    fn byte_at(&self, at: u32) -> usize {
        let mut units = 0;
        for (i, c) in self.text.char_indices() {
            if units >= at {
                return i;
            }
            units += c.len_utf16() as u32;
        }
        self.text.len()
    }

    /// Caret in UTF-16 units, as DirectWrite counts positions.
    pub fn caret_utf16(&self) -> u32 {
        self.utf16(self.caret)
    }

    /// Selection in bytes, if not empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.caret).then(|| (a.min(self.caret), a.max(self.caret)))
    }

    /// Selection in UTF-16 units, for highlighting.
    pub fn selection_utf16(&self) -> Option<(u32, u32)> {
        self.selection().map(|(a, b)| (self.utf16(a), self.utf16(b)))
    }

    /// Remembers the state before an edit, for Ctrl+Z.
    fn snapshot(&mut self) {
        if self.undo.last().is_none_or(|(t, _)| *t != self.text) {
            self.undo.push((self.text.clone(), self.caret));
        }
    }

    /// Deletes the selection. Returns `true` if there was anything.
    fn cut_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else { return false };
        self.text.replace_range(a..b, "");
        self.caret = a;
        self.anchor = None;
        true
    }

    /// Moves the caret; with Shift it extends the selection, without it clears it.
    fn move_to(&mut self, at: usize, shift: bool) {
        if shift {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = at;
    }

    /// Types text over the selection.
    pub fn insert(&mut self, s: &str) {
        self.snapshot();
        self.cut_selection();
        self.text.insert_str(self.caret, s);
        self.caret += s.len();
    }

    /// Word boundary left or right of `at`, for Ctrl+arrows, Ctrl+Backspace and double clicks.
    fn word_edge(&self, at: usize, left: bool) -> usize {
        let is_word = |c: char| c.is_alphanumeric();
        if left {
            let head = &self.text[..at];
            let trimmed = head.trim_end_matches(|c: char| !is_word(c));
            trimmed.trim_end_matches(is_word).len()
        } else {
            let tail = &self.text[at..];
            let skip = tail.len() - tail.trim_start_matches(|c: char| !is_word(c)).len();
            let rest = &tail[skip..];
            at + skip + (rest.len() - rest.trim_start_matches(is_word).len())
        }
    }

    fn prev_char(&self) -> usize {
        self.text[..self.caret].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next_char(&self) -> usize {
        self.text[self.caret..].chars().next().map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    /// A typed character (`WM_CHAR`); control characters are left to [`key`](Self::key).
    pub fn char(&mut self, c: char) -> Edited {
        if c.is_control() {
            return Edited::No;
        }
        self.insert(c.encode_utf8(&mut [0; 4]));
        Edited::Changed
    }

    /// An editing key: arrows, Home/End, Backspace/Delete (with Ctrl by words), Ctrl+A/C/X/V/Z.
    pub fn key(&mut self, vk: VIRTUAL_KEY, ctrl: bool, shift: bool) -> Edited {
        match vk {
            // Without Shift an arrow collapses the selection to its edge, as in a regular input field.
            VK_LEFT | VK_RIGHT if !shift && !ctrl && self.selection().is_some() => {
                let (a, b) = self.selection().unwrap();
                self.move_to(if vk == VK_LEFT { a } else { b }, false);
            }
            VK_LEFT => self.move_to(if ctrl { self.word_edge(self.caret, true) } else { self.prev_char() }, shift),
            VK_RIGHT => self.move_to(if ctrl { self.word_edge(self.caret, false) } else { self.next_char() }, shift),
            VK_HOME => self.move_to(0, shift),
            VK_END => self.move_to(self.text.len(), shift),
            VK_BACK | VK_DELETE if self.selection().is_some() => {
                self.snapshot();
                self.cut_selection();
                return Edited::Changed;
            }
            VK_BACK if self.caret > 0 => {
                self.snapshot();
                let from = if ctrl { self.word_edge(self.caret, true) } else { self.prev_char() };
                self.text.replace_range(from..self.caret, "");
                self.caret = from;
                return Edited::Changed;
            }
            VK_DELETE if self.caret < self.text.len() => {
                self.snapshot();
                let to = if ctrl { self.word_edge(self.caret, false) } else { self.next_char() };
                self.text.replace_range(self.caret..to, "");
                return Edited::Changed;
            }
            VK_BACK | VK_DELETE => return Edited::Moved,
            VK_A if ctrl => {
                self.anchor = Some(0);
                self.caret = self.text.len();
            }
            VK_C | VK_X if ctrl => {
                let Some((a, b)) = self.selection() else { return Edited::Moved };
                crate::launcher::copy(&self.text[a..b]);
                if vk == VK_X {
                    self.snapshot();
                    self.cut_selection();
                    return Edited::Changed;
                }
            }
            VK_V if ctrl => {
                // Only the clipboard's first line goes into the line.
                let Some(s) = crate::launcher::clipboard_text() else { return Edited::Moved };
                self.insert(s.lines().next().unwrap_or(""));
                return Edited::Changed;
            }
            VK_Z if ctrl => {
                let Some((text, caret)) = self.undo.pop() else { return Edited::Moved };
                self.text = text;
                self.caret = caret.min(self.text.len());
                self.anchor = None;
                return Edited::Changed;
            }
            _ => return Edited::No,
        }
        Edited::Moved
    }

    /// The mouse went down at UTF-16 position `at`: the caret goes there, Shift extends the selection,
    /// a double click selects the word.
    pub fn press(&mut self, at: u32, shift: bool, double: bool) {
        let at = self.byte_at(at);
        if double {
            self.anchor = Some(self.word_edge(at, true));
            self.caret = self.word_edge(at, false);
        } else {
            self.move_to(at, shift);
            self.anchor.get_or_insert(at);
        }
    }

    /// The mouse dragged to UTF-16 position `at` with the button down: the selection follows it.
    pub fn drag(&mut self, at: u32) {
        let at = self.byte_at(at);
        self.anchor.get_or_insert(self.caret);
        self.caret = at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_selection_and_undo() {
        let mut e = LineEdit::new("font size");
        assert_eq!(e.key(VK_LEFT, true, false), Edited::Moved);
        assert_eq!(e.caret, 5);
        e.key(VK_LEFT, true, true);
        assert_eq!(e.selection(), Some((0, 5)));
        e.insert("ab");
        assert_eq!(e.text, "absize");
        e.key(VK_Z, true, false);
        assert_eq!(e.text, "font size");
    }

    #[test]
    fn mouse() {
        let mut e = LineEdit::new("привет мир");
        e.press(3, false, false);
        assert_eq!((e.caret, e.selection()), ("при".len(), None));
        e.drag(10);
        assert_eq!(e.selection(), Some(("при".len(), e.text.len())));
        e.press(8, false, true);
        assert_eq!(e.selection(), Some(("привет ".len(), e.text.len())));
    }
}
