//! Host work log for the Canvas: Start, New Sandbox, Environment realize.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const MAX: usize = 2000;

#[derive(Clone)]
pub struct Progress {
    lines: Arc<Mutex<VecDeque<String>>>,
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

impl Progress {
    pub fn new() -> Self {
        Self {
            lines: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn line(&self, msg: impl AsRef<str>) {
        let msg = msg.as_ref();
        if msg.is_empty() {
            return;
        }
        eprintln!("{msg}");
        let mut lines = self.lines.lock().expect("progress");
        if lines.len() >= MAX {
            lines.pop_front();
        }
        lines.push_back(msg.to_string());
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.lines
            .lock()
            .expect("progress")
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_kept_in_order() {
        let p = Progress::new();
        p.line("one");
        p.line("two");
        assert_eq!(p.snapshot(), ["one", "two"]);
    }

    #[test]
    fn empty_lines_are_ignored() {
        let p = Progress::new();
        p.line("");
        p.line("ok");
        assert_eq!(p.snapshot(), ["ok"]);
    }
}
