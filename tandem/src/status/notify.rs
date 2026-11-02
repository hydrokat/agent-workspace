use std::collections::HashSet;

use crate::coordination::schema::Query;
use crate::sanitize::clean;
use crate::tmux::client::TmuxClient;
use crate::tmux::keys::{default_keymap, display_shortcut, key_for};

pub const ACK_LIMIT: usize = 20;

pub fn order_queries(queries: &[Query]) -> Vec<(&'static str, Vec<Query>)> {
    let mut open: Vec<Query> = queries
        .iter()
        .filter(|q| q.status == "open")
        .cloned()
        .collect();
    open.sort_by(|a, b| {
        a.created
            .partial_cmp(&b.created)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut answered: Vec<Query> = queries
        .iter()
        .filter(|q| q.status == "answered")
        .cloned()
        .collect();
    answered.sort_by(|a, b| {
        a.created
            .partial_cmp(&b.created)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut acked: Vec<Query> = queries
        .iter()
        .filter(|q| q.status == "acknowledged")
        .cloned()
        .collect();
    acked.sort_by(|a, b| {
        b.created
            .partial_cmp(&a.created)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if acked.len() > ACK_LIMIT {
        acked.truncate(ACK_LIMIT);
    }

    let mut groups = Vec::new();
    if !open.is_empty() {
        groups.push(("Open", open));
    }
    if !answered.is_empty() {
        groups.push(("Answered (awaiting pickup)", answered));
    }
    if !acked.is_empty() {
        groups.push(("Acknowledged (last 20)", acked));
    }
    groups
}

pub struct Notifier {
    tmux: Option<TmuxClient>,
    seen: Option<HashSet<String>>,
}

impl Notifier {
    pub fn new(tmux: Option<TmuxClient>) -> Self {
        Self { tmux, seen: None }
    }

    pub fn new_queries(&mut self, queries: &[Query]) -> Vec<Query> {
        let current: HashSet<String> = queries
            .iter()
            .filter(|q| q.status == "open")
            .map(|q| q.id.clone())
            .collect();

        if self.seen.is_none() {
            self.seen = Some(current);
            return Vec::new();
        }

        let seen = self.seen.as_mut().unwrap();
        let fresh: Vec<Query> = queries
            .iter()
            .filter(|q| q.status == "open" && !seen.contains(&q.id))
            .cloned()
            .collect();

        seen.extend(current);
        fresh
    }

    pub fn notify(&mut self, queries: &[Query]) {
        let fresh = self.new_queries(queries);
        if let Some(ref client) = self.tmux {
            for q in fresh {
                let cleaned = clean(&q.asked_by);
                let escaped = cleaned.replace('#', "##");
                let who = if escaped.len() > 40 {
                    &escaped[..40]
                } else {
                    &escaped
                };
                let platform = if cfg!(target_os = "macos") {
                    "darwin"
                } else {
                    "linux"
                };
                let km = default_keymap();
                let raw_key = key_for("queries", &km);
                let shortcut =
                    display_shortcut(&raw_key, platform).unwrap_or_else(|_| "Opt+q".to_string());
                let msg = format!("tandem: new question from {who} ({shortcut} to answer)");
                let _ = client.run(&["display-message", "-d", "4000", &msg]);
            }
        }
    }
}
