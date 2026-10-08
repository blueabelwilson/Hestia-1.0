//! Fuzzy matching ("vsc" finds "Visual Studio Code") and usage-based ranking.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Score how well `query` matches `text`. None = no match. Higher is better.
pub fn score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().collect();
    let tl: Vec<char> = text.to_lowercase().chars().collect();
    if tl.len() != t.len() {
        // Rare case-folding length change; fall back to substring test.
        return text.to_lowercase().contains(&query.to_lowercase()).then_some(10);
    }

    let mut score = 0i32;
    let mut qi = 0;
    let mut prev_match: Option<usize> = None;
    for (i, c) in tl.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            let mut s = 1;
            let word_start = i == 0 || !t[i - 1].is_alphanumeric() || (t[i].is_uppercase() && t[i - 1].is_lowercase());
            if word_start {
                s += 8;
            }
            if let Some(p) = prev_match {
                if p + 1 == i {
                    s += 6; // consecutive letters
                } else {
                    s -= ((i - p - 1) as i32).min(5); // gap penalty
                }
            } else {
                s -= (i as i32).min(10); // later first match is worse
            }
            score += s;
            prev_match = Some(i);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    let ql: String = q.iter().collect();
    let tls: String = tl.iter().collect();
    if tls.starts_with(&ql) {
        score += 25;
    } else if tls.contains(&ql) {
        score += 12;
    }
    // Prefer shorter names when everything else is equal.
    score -= (t.len() as i32 / 8).min(6);
    Some(score)
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub count: u32,
    /// Unix seconds.
    pub last: u64,
}

#[derive(Default, Serialize, Deserialize)]
pub struct UsageStore {
    pub items: HashMap<String, Usage>,
    #[serde(default)]
    pub run_history: Vec<String>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl UsageStore {
    fn path() -> std::path::PathBuf {
        crate::config::data_dir().join("usage.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(Self::path(), s);
        }
    }
    pub fn record(&mut self, id: &str) {
        let u = self.items.entry(id.to_string()).or_default();
        u.count += 1;
        u.last = now();
        self.save();
    }
    pub fn record_run(&mut self, cmd: &str) {
        self.run_history.retain(|c| c != cmd);
        self.run_history.insert(0, cmd.to_string());
        self.run_history.truncate(50);
        self.save();
    }
    /// "Frecency": often and recently used things float up.
    pub fn boost(&self, id: &str) -> i32 {
        let Some(u) = self.items.get(id) else { return 0 };
        let age_days = (now().saturating_sub(u.last)) as f32 / 86400.0;
        let recency = if age_days < 1.0 {
            1.0
        } else if age_days < 7.0 {
            0.7
        } else if age_days < 30.0 {
            0.4
        } else {
            0.2
        };
        ((u.count as f32 + 1.0).ln() * 12.0 * recency) as i32
    }
}
