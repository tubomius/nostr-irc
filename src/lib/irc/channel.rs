use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use async_recursion::async_recursion;
use nostr::{ClientMessage, Event, EventBuilder, Keys, Kind, KindBase, RelayMessage, Sha256Hash, SubscriptionFilter, Tag};
use nostr::event::{TagData, TagKind};
use nostr::hashes::sha256::Hash;
use nostr::key::XOnlyPublicKey;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc::UnboundedSender;
use crate::irc::client_data::IRCClientData;
use crate::irc::server::ServerMessage;
use crate::nostr::metadata::Metadata;

pub enum IRCChannelMessage {
    IRC(String),
    Relay(RelayMessage),
    Client(ClientMessage),
}

pub type IRCChannelHolder = Arc<RwLock<IRCChannel>>;

pub struct IRCChannel {
    pub id: Sha256Hash,
    pub topic: Option<String>,
    pub warming_up: bool,
    pub got_metadata: bool,
    pub got_messages: bool,
    pub got_nicks: bool,
    pub known_users: HashSet<XOnlyPublicKey>,
    pub warm_up_events: Vec<RelayMessage>,
    pub messages: Vec<ServerMessage>,
}

impl IRCChannel {
    pub fn new(id: Hash) -> Self {
        // println!("#{id}: new channel");

        Self {
            id,
            topic: None,
            warming_up: true,
            got_metadata: false,
            got_messages: false,
            got_nicks: false,
            known_users: HashSet::new(),
            warm_up_events: vec![],
            messages: vec![],
        }
    }
}