use crate::store::now;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

pub type Logs = Arc<Mutex<LogBuffer>>;

#[derive(Default)]
pub struct LogBuffer {
    entries: VecDeque<Value>,
    dropped: u64,
}

fn fit_text(text: &str, mut budget: usize) -> String {
    text.chars()
        .take_while(|ch| {
            let bytes = match ch {
                '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
                ch if *ch < '\u{20}' => 6,
                ch => ch.len_utf8(),
            };
            if bytes > budget {
                false
            } else {
                budget -= bytes;
                true
            }
        })
        .collect()
}

impl LogBuffer {
    pub fn push(&mut self, level: &str, text: &str, request_id: &str, details: Option<&Value>) {
        self.push_at(now(), level, text, request_id, details);
    }

    fn push_at(
        &mut self,
        at: u64,
        level: &str,
        text: &str,
        request_id: &str,
        details: Option<&Value>,
    ) {
        if self
            .entries
            .iter()
            .rev()
            .take_while(|entry| entry["at"].as_u64().unwrap() + 60000 > at)
            .count()
            >= 240
        {
            self.dropped += 1;
            return;
        }
        let mut entry = json!({"at":at,"level":level,"text":"","requestId":request_id});
        let room = 2048 - serde_json::to_vec(&entry).unwrap().len();
        entry["text"] = json!(fit_text(text, room));
        if let Some(details) = details {
            entry["details"] = details.clone();
            if serde_json::to_vec(&entry).unwrap().len() > 2048 {
                entry["details"] = json!("");
                let size = serde_json::to_vec(&entry).unwrap().len();
                if size <= 2048 {
                    entry["details"] = json!(fit_text(&details.to_string(), 2048 - size));
                } else {
                    entry.as_object_mut().unwrap().remove("details");
                }
            }
        }
        self.entries.push_back(entry);
        if self.entries.len() > 2000 {
            self.entries.pop_front();
        }
    }

    pub fn snapshot(&self) -> Vec<Value> {
        let mut result: Vec<_> = self
            .entries
            .iter()
            .skip(usize::from(self.dropped > 0 && self.entries.len() == 2000))
            .cloned()
            .collect();
        if self.dropped > 0 {
            result.push(json!({"at":now(),"level":"warn","text":format!("日志超出速率限制，已丢弃 {} 行",self.dropped),"dropped":self.dropped}));
        }
        result
    }
}

pub(crate) fn validation_log(logs: &Logs, level: &str, text: &str, request_id: &str) {
    logs.lock().unwrap().push(level, text, request_id, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_entries_share_rate_ring_and_serialized_byte_limits() {
        let mut logs = LogBuffer::default();
        let details = json!({"reason":"界\n".repeat(5000)});
        for i in 0..250 {
            logs.push_at(
                100000,
                "error",
                &"界\n\u{1}".repeat(2048),
                &format!("request-{i}"),
                Some(&details),
            );
        }
        assert_eq!(logs.entries.len(), 240);
        assert_eq!(logs.snapshot().last().unwrap()["dropped"], 10);
        for minute in 1..10 {
            for i in 0..240 {
                logs.push_at(
                    100000 + minute * 60000,
                    "warn",
                    &format!("{minute}:{i}"),
                    "request",
                    Some(&details),
                );
            }
        }
        assert_eq!(logs.entries.len(), 2000);
        assert_eq!(logs.entries.back().unwrap()["text"], "9:239");
        let snapshot = logs.snapshot();
        assert_eq!(snapshot.len(), 2000);
        assert!(snapshot
            .iter()
            .all(|entry| serde_json::to_vec(entry).unwrap().len() <= 2048));
        assert!(snapshot.iter().any(|entry| entry["details"]
            .as_str()
            .is_some_and(|v| v.contains("reason"))));
    }
}
