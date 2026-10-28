// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzzy matching of a query against a name: the more "natural" the match, the higher the score.

/// Match score of `query` against `name`; `None` means no match. Case-insensitive.
///
/// In descending order: the name starts with the query → the query starts one of the words →
/// the query is the words' initials ("vsc" → Visual Studio Code) → the query is inside the name →
/// the query's letters appear in the name in order. Within a tier a shorter name ranks higher:
/// "Code" fits "co" better than "Command Prompt".
pub fn score(name: &str, query: &str) -> Option<u32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let n = name.to_lowercase();
    let words: Vec<&str> = n.split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.' | '(' | ')')).filter(|w| !w.is_empty()).collect();
    let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
    let level = if n.starts_with(&q) {
        5
    } else if words.iter().any(|w| w.starts_with(&q)) {
        4
    } else if initials.starts_with(&q) && q.chars().count() > 1 {
        3
    } else if n.contains(&q) {
        2
    } else if subsequence(&n, &q) {
        1
    } else {
        return None;
    };
    // Names are never longer than 100 characters, so the length penalty never crosses a tier.
    Some(level * 1000 - n.chars().count().min(100) as u32)
}

fn subsequence(hay: &str, needle: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        // The length penalty is at most 100; round to the tier.
        let s = |n: &str, q: &str| (score(n, q).unwrap_or(0) + 100) / 1000;
        assert_eq!(s("Visual Studio Code", "vis"), 5);
        assert_eq!(s("Visual Studio Code", "code"), 4);
        assert_eq!(s("Visual Studio Code", "vsc"), 3);
        assert_eq!(s("Telegram Desktop", "egra"), 2);
        assert_eq!(s("Telegram Desktop", "tgd"), 1);
        assert_eq!(score("Telegram", "xyz"), None);
    }

    #[test]
    fn shorter_wins_within_level() {
        assert!(score("Code", "co") > score("Command Prompt", "co"));
    }

    #[test]
    fn cyrillic_case_insensitive() {
        assert!(score("Блокнот", "блок").is_some());
        assert!(score("Блокнот", "БЛОК").is_some());
    }
}
