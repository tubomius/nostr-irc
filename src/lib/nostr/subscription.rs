use std::collections::{HashMap, HashSet};
use std::time::Instant;
use nostr::{RelayMessage, Sha256Hash};
use nostr::url::Url;

pub struct NostrSubscription {
    pub name: String,
    pub data: HashMap<Sha256Hash, RelayMessage>,
    pub asked_relays: Vec<Url>,
    pub responded_relays: Vec<Url>,
    pub started: Instant,
    pub done: bool,
    pub end_of_stored_events_message: Option<RelayMessage>,
    pub seen_ids: HashSet<String>,
}

impl NostrSubscription {
    pub fn new(name: String) -> Self {
        Self {
            name,
            data: HashMap::new(),
            asked_relays: vec![],
            responded_relays: vec![],
            started: Instant::now(),
            done: false,
            end_of_stored_events_message: None,
            seen_ids: HashSet::new(),
        }
    }
}
