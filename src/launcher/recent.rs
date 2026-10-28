// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recent launches: for result ordering and the list on an empty query.
//!
//! File `%APPDATA%\Polynya\recent.txt`, lines `launch_count<TAB>time<TAB>id`.

use std::{collections::HashMap, fs, path::PathBuf, time::SystemTime};

/// Score bonus per launch and its cap: a frequent app rises within its match tier but does
/// not jump over a more exact match.
const BONUS_PER_LAUNCH: u32 = 40;
const BONUS_MAX: u32 = 800;

#[derive(Default)]
pub struct Recent {
    /// id → (launch count, last launch time in seconds).
    map: HashMap<String, (u32, u64)>,
}

fn path() -> PathBuf {
    crate::config::path().with_file_name("recent.txt")
}

impl Recent {
    pub fn load() -> Self {
        let mut map = HashMap::new();
        for line in fs::read_to_string(path()).unwrap_or_default().lines() {
            let mut it = line.splitn(3, '\t');
            if let (Some(n), Some(t), Some(id)) = (it.next(), it.next(), it.next()) {
                map.insert(id.to_string(), (n.parse().unwrap_or(0), t.parse().unwrap_or(0)));
            }
        }
        Self { map }
    }

    pub fn bump(&mut self, id: &str) {
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let e = self.map.entry(id.to_string()).or_insert((0, 0));
        e.0 += 1;
        e.1 = now;
        self.save();
    }

    pub fn contains(&self, id: &str) -> bool {
        self.map.contains_key(id)
    }

    /// Removes from recents along with the accumulated score bonus.
    pub fn forget(&mut self, id: &str) {
        if self.map.remove(id).is_some() {
            self.save();
        }
    }

    fn save(&self) {
        let text: String = self.map.iter().map(|(id, (n, t))| format!("{n}\t{t}\t{id}\n")).collect();
        if let Err(e) = fs::write(path(), text) {
            proto::warn!("launcher", "save recent: {e}");
        }
    }

    pub fn bonus(&self, id: &str) -> u32 {
        self.map.get(id).map_or(0, |(n, _)| (n * BONUS_PER_LAUNCH).min(BONUS_MAX))
    }

    /// Recent ids, newest first.
    pub fn latest(&self) -> Vec<&str> {
        let mut v: Vec<(&str, u64)> = self.map.iter().map(|(id, (_, t))| (id.as_str(), *t)).collect();
        v.sort_by_key(|&(_, t)| std::cmp::Reverse(t));
        v.into_iter().map(|(id, _)| id).collect()
    }
}
