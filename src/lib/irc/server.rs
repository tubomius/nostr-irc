use std::collections::HashMap;
use log::*;
use std::error::Error;
use nostr::{ClientMessage, Event, EventBuilder, Keys, Kind, KindBase, RelayMessage, Sha256Hash, SubscriptionFilter, Tag};
use nostr::event::{TagData, TagKind};
use nostr::key::XOnlyPublicKey;
use nostr::url::Url;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;
use crate::irc::channel::IRCChannel;
use crate::irc::nickname::IRCNickname;
use crate::irc::peer::IRCPeer;
use crate::nostr::client::{NostrClient, RelayMessageMetadata};
use crate::nostr::metadata;

pub struct IRCServer {
    nicknames: HashMap<XOnlyPublicKey, IRCNickname>,
    channels: HashMap<Sha256Hash, IRCChannel>,
    tasks: JoinSet<()>,
    peers_txs: HashMap<u64, UnboundedSender<ServerMessage>>,
    peers_info: HashMap<u64, PeerInfo>,
    tx: UnboundedSender<PeerMessage>,
    rx: UnboundedReceiver<PeerMessage>,
    peer_id_counter: u64,
    nostr_tx: UnboundedSender<ClientMessage>,
    nostr_rx: UnboundedReceiver<(RelayMessage, RelayMessageMetadata)>,
}

impl IRCServer {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        // Just to avoid Option, these will be overwritten in run
        let (dummy_tx, _) = mpsc::unbounded_channel();
        let (_, dummy_rx) = mpsc::unbounded_channel();

        Self {
            nicknames: HashMap::new(),
            channels: HashMap::new(),
            tasks: JoinSet::new(),
            peers_txs: HashMap::new(),
            peers_info: HashMap::new(),
            tx,
            rx,
            nostr_tx: dummy_tx,
            nostr_rx: dummy_rx,
            peer_id_counter: 0,
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        let (nostr_relay_tx, nostr_relay_rx) = mpsc::unbounded_channel();
        let (nostr_client_tx, nostr_client_rx) = mpsc::unbounded_channel();

        let relays = vec![
            "wss://relay.nostr.ch".to_string(),
            "wss://nostr.rocks".to_string(),
            "wss://no.str.cr".to_string(),
            "wss://relay.damus.io".to_string(),
            "wss://nostr-pub.semisol.dev".to_string(),
            "wss://relay.nostrmoto.xyz".to_string(),
            "wss://relay.nostr.info".to_string(),
            "wss://nostr.zerofeerouting.com".to_string(),
            "wss://nostr.hyperlingo.com".to_string(),
            "wss://nostr.sandwich.farm".to_string(),
            "wss://nostr.shadownode.org".to_string(),
            "wss://nostr-01.bolt.observer".to_string(),
            "wss://relay.r3d.red".to_string(),
            "wss://nostr.slothy.win".to_string(),
            "wss://nostr-relay.untethr.me".to_string(),
            "wss://nostr-pub.wellorder.net".to_string(),
            "wss://nostr.bitcoiner.social".to_string(),
            "wss://nostr.onsats.org".to_string(),
            "wss://relayer.fiatjaf.com".to_string(),
            "wss://nostr-2.zebedee.cloud".to_string(),
            "wss://expensive-relay.fiatjaf.com".to_string(),
            "wss://nostr.zebedee.cloud".to_string(),
        ];

        let mut nostr_client = NostrClient::new(relays, nostr_relay_tx, nostr_client_rx);

        self.nostr_rx = nostr_relay_rx;
        self.nostr_tx = nostr_client_tx;

        self.tasks.spawn(async move {
           nostr_client.run().await;
        });

        let addr = "127.0.0.1:6667".to_string();

        let listener = TcpListener::bind(&addr).await?;

        info!("IRC server listening on: {}", addr);
        info!("Connect using your IRC client with a nostr private key as password");

        loop {
            tokio::select!(
                Ok((socket, _)) = listener.accept() => {
                    self.handle_irc_client(socket).await;
                }
                Some(message) = self.rx.recv() => {
                    self.handle_peer_message(message).await;
                }
                Some(message) = self.nostr_rx.recv() => {
                    self.handle_nostr_message(message).await;
                }
            );
        }
    }

    pub async fn handle_irc_client(&mut self, socket: TcpStream) {
        let (tx, rx) = mpsc::unbounded_channel();

        self.peer_id_counter += 1;

        let peer_id = self.peer_id_counter;

        self.peers_txs.insert(peer_id, tx);
        self.peers_info.insert(peer_id, PeerInfo::new());

        let mut peer = IRCPeer::new(socket, self.tx.clone(), rx, peer_id);

        self.tasks.spawn(async move {
            peer.run().await;
        });
    }

    pub async fn handle_peer_message(&mut self, message: PeerMessage) {
        info!("{message:?}");

        match message {
            PeerMessage::ChangeNickname { peer_id, public_key, nickname } => {
                let peer_info = self.peers_info.get(&peer_id);

                if let Some(peer_info) = peer_info {
                    let metadata = nostr::Metadata::new().name(&nickname);

                    if let Some(keys) = &peer_info.keys {
                        let event: Event = EventBuilder::set_metadata(keys, metadata).unwrap().to_event(keys).unwrap();

                        let msg = ClientMessage::new_event(event);

                        self.nostr_tx.send(msg).ok();
                    }
                }
            }
            PeerMessage::CreateChannel { peer_id, name } => {
                let peer_info = self.peers_info.get(&peer_id);

                if let Some(peer_info) = peer_info {
                    let metadata = nostr::Metadata::new().name(&name);

                    if let Some(keys) = &peer_info.keys {
                        let event: Event = EventBuilder::new_channel(metadata).unwrap().to_event(keys).unwrap();

                        let public_key = event.id;

                        let msg = ClientMessage::new_event(event);

                        self.nostr_tx.send(msg).ok();

                        let peer_tx = self.peers_txs.get(&peer_id);

                        if let Some(peer_tx) = peer_tx {
                            peer_tx.send(ServerMessage::CreatedChannel { channel: public_key }).ok();
                        }
                    }
                }
            }
            PeerMessage::AddChannel { peer_id, public_key } => {
                let peer_info = self.peers_info.get_mut(&peer_id);

                if let Some(peer_info) = peer_info {
                    if !peer_info.channels.contains(&public_key) {
                        peer_info.channels.push(public_key);

                        let channel = self.channels.get(&public_key);

                        let peer_tx = self.peers_txs.get(&peer_id);

                        if let Some(channel) = channel {
                            if let Some(peer_tx) = peer_tx {
                                if let Some(topic) = &channel.topic {
                                    peer_tx.send(ServerMessage::ChangeTopic { channel: public_key, topic: topic.clone() }).ok();
                                }

                                for m in channel.messages.iter() {
                                    peer_tx.send(m.clone()).ok();
                                }
                            }
                        } else {
                            self.channels.insert(public_key, IRCChannel::new(public_key));

                            if let Some(peer_tx) = peer_tx {
                                peer_tx.send(ServerMessage::ChangeTopic { channel: public_key, topic: "Channel is loading...".to_string() }).ok();
                            }

                            let channel_info = ClientMessage::new_req(
                                format!("{}-info", public_key),
                                vec![SubscriptionFilter::new().id(public_key.to_string())],
                            );

                            self.nostr_tx.send(channel_info).ok();

                            let channel_messages = ClientMessage::new_req(
                                format!("{}-messages", public_key),
                                vec![SubscriptionFilter::new().kind(Kind::Base(KindBase::ChannelMessage)).limit(200).event(public_key)],
                            );

                            self.nostr_tx.send(channel_messages).ok();
                        }

                        if let Some(peer_tx) = peer_tx {
                            let nickname = self.nicknames.get(&peer_info.keys.as_ref().unwrap().public_key());

                            if let Some(nickname) = nickname {
                                peer_tx.send(ServerMessage::PeerJoined { channel: public_key, nickname: nickname.get_displayed_name() }).ok();
                            }
                        }
                    }
                }
            }
            PeerMessage::LeaveChannel { peer_id, public_key } => {
                let peer_info = self.peers_info.get_mut(&peer_id);

                if let Some(peer_info) = peer_info {
                    peer_info.channels.retain(|x| *x != public_key);

                    let peer_tx = self.peers_txs.get(&peer_id);

                    if let Some(peer_tx) = peer_tx {
                        let nickname = self.nicknames.get(&peer_info.keys.as_ref().unwrap().public_key());

                        if let Some(nickname) = nickname {
                            peer_tx.send(ServerMessage::PeerLeft { channel: public_key, nickname: nickname.get_displayed_name() }).ok();
                        }
                    }
                }
            }
            PeerMessage::AddNickname { peer_id, public_key } => {
                let peer_tx = self.peers_txs.get(&peer_id);

                if let Some(peer_tx) = peer_tx {
                    let nickname = self.nicknames.get(&public_key);

                    if let Some(nickname) = nickname {
                        // Nick already added, send info to peer
                        if let IRCNickname::Nick(nickname) = nickname {
                            peer_tx.send(ServerMessage::ChangeNickname {
                                public_key,
                                old_nickname: public_key.to_string(),
                                new_nickname: nickname.clone(),
                            }).ok();
                        }
                    } else {
                        self.nicknames.insert(public_key, IRCNickname::Key(public_key));

                        // Subscribe to nostr user metadata
                        let client_message = ClientMessage::new_req(
                            format!("{}-user-metadata", public_key.to_string()),
                            vec![SubscriptionFilter::new().author(public_key).kind(Kind::Base(KindBase::Metadata)).limit(1)],
                        );

                        self.nostr_tx.send(client_message).ok();
                    }
                }
            }
            PeerMessage::SetKeys { peer_id, keys } => {
                if let Some(peer_info) = self.peers_info.get_mut(&peer_id) {
                    peer_info.keys = Some(keys);
                }
            }
            PeerMessage::SendMessage { peer_id, channel, message } => {
                if let Some(peer_info) = self.peers_info.get(&peer_id) {
                    if let Some(keys) = &peer_info.keys {
                        let event: Event = EventBuilder::new(
                            Kind::Base(KindBase::ChannelMessage),
                            message,
                            &[Tag::new(TagData::Generic(
                                TagKind::E,
                                vec![channel.to_string()],
                            ))],
                        ).to_event(keys).unwrap();

                        let msg = ClientMessage::new_event(event);

                        self.nostr_tx.send(msg).ok();
                    }
                }
            }
        }
    }

    pub async fn handle_nostr_message(&mut self, message: (RelayMessage, RelayMessageMetadata)) {
        let (message, message_metadata) = message;

        info!("{message:?}");

        match &message {
            RelayMessage::Event { event, subscription_id } => {
                match event.kind {
                    Kind::Base(k) => {
                        match k {
                            KindBase::Metadata => {
                                let user_id = event.pubkey;

                                let metadata = serde_json::from_slice::<metadata::Metadata>(event.content.as_ref()).unwrap();

                                let nickname = self.nicknames.get_mut(&user_id);

                                if let Some(mut new_nickname) = metadata.name {
                                    new_nickname = new_nickname.chars().filter(|c| c.is_ascii_alphanumeric()).collect();

                                    if let Some(nickname) = nickname {
                                        let old_nickname = nickname.get_displayed_name();

                                        if old_nickname != new_nickname {
                                            *nickname = IRCNickname::Nick(new_nickname.clone());

                                            let server_message = ServerMessage::ChangeNickname {
                                                public_key: user_id,
                                                old_nickname,
                                                new_nickname,
                                            };

                                            for (_, peer_tx) in self.peers_txs.iter() {
                                                peer_tx.send(server_message.clone()).ok();
                                            }
                                        }
                                    }
                                }
                            }
                            KindBase::TextNote => {}
                            KindBase::RecommendRelay => {}
                            KindBase::ContactList => {}
                            KindBase::EncryptedDirectMessage => {}
                            KindBase::EventDeletion => {}
                            KindBase::Boost => {}
                            KindBase::Reaction => {}
                            KindBase::ChannelCreation => {
                                let channel_id = event.id;

                                if let Some(channel) = self.channels.get_mut(&channel_id) {
                                    let metadata = serde_json::from_slice::<nostr::Metadata>(event.content.as_ref()).unwrap();
                                    let topic = metadata.name.unwrap_or("No topic".to_string());

                                    channel.topic = Some(topic.clone());

                                    for (peer_id, peer_info) in self.peers_info.iter() {
                                        if peer_info.channels.contains(&channel_id) {
                                            let peer_tx = self.peers_txs.get(&peer_id);

                                            if let Some(peer_tx) = peer_tx {
                                                peer_tx.send(ServerMessage::ChangeTopic { channel: channel_id, topic: topic.clone() }).ok();
                                            }
                                        }
                                    }
                                }
                            }
                            KindBase::ChannelMetadata => {}
                            KindBase::ChannelMessage => {
                                for tag in &event.tags {
                                    if let Ok(tk) = tag.kind() {
                                        match tk {
                                            TagKind::E => {
                                                let channel_id = tag.content().unwrap().parse::<Sha256Hash>().unwrap();

                                                if let Some(channel) = self.channels.get_mut(&channel_id) {
                                                    let content = event.content.clone();
                                                    let public_key = event.pubkey;
                                                    let nickname = self.nicknames.get(&public_key);

                                                    if let None = nickname {
                                                        self.nicknames.insert(public_key, IRCNickname::Key(public_key));

                                                        // Subscribe to nostr user metadata
                                                        let client_message = ClientMessage::new_req(
                                                            format!("{}-user-metadata", public_key.to_string()),
                                                            vec![SubscriptionFilter::new().author(public_key).kind(Kind::Base(KindBase::Metadata)).limit(1)],
                                                        );

                                                        self.nostr_tx.send(client_message).ok();
                                                    }

                                                    let nickname = self.nicknames.get(&public_key).unwrap();

                                                    let server_message = ServerMessage::SendMessage { channel: channel_id, message: content.clone(), nickname: nickname.get_displayed_name() };
                                                    let join_message = ServerMessage::Join { channel: channel_id, nickname: nickname.get_displayed_name() };

                                                    channel.messages.push(server_message.clone());

                                                    let mut send_join = false;
                                                    if !channel.known_users.contains(&public_key) {
                                                        send_join = true;
                                                        channel.known_users.insert(public_key);
                                                    }

                                                    for (peer_id, peer_info) in self.peers_info.iter() {
                                                        if peer_info.channels.contains(&channel_id) {
                                                            if peer_info.keys.as_ref().unwrap().public_key() == public_key && !message_metadata.is_history {
                                                                continue;
                                                            }

                                                            let peer_tx = self.peers_txs.get(&peer_id);

                                                            if let Some(peer_tx) = peer_tx {
                                                                if send_join {
                                                                    peer_tx.send(join_message.clone()).ok();
                                                                }

                                                                peer_tx.send(server_message.clone()).ok();
                                                            }
                                                        }
                                                    }
                                                }

                                                break;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            KindBase::ChannelHideMessage => {}
                            KindBase::ChannelMuteUser => {}
                        }
                    }
                    Kind::Custom(_) => {}
                }
            }
            RelayMessage::Notice { .. } => {}
            RelayMessage::EndOfStoredEvents { .. } => {}
            RelayMessage::Ok { .. } => {}
            RelayMessage::Empty => {}
        }
    }
}

#[derive(Debug)]
pub enum PeerMessage {
    ChangeNickname{ peer_id: u64, public_key: XOnlyPublicKey, nickname: String },
    CreateChannel{ peer_id: u64, name: String },
    AddChannel{ peer_id: u64, public_key: Sha256Hash },
    LeaveChannel{ peer_id: u64, public_key: Sha256Hash },
    AddNickname{ peer_id: u64, public_key: XOnlyPublicKey },
    SetKeys{ peer_id: u64, keys: Keys },
    SendMessage{ peer_id: u64, channel: Sha256Hash, message: String },
}

#[derive(Debug, Clone)]
pub enum ServerMessage {
    SendMessage{ channel: Sha256Hash, message: String, nickname: String },
    ChangeNickname{ public_key: XOnlyPublicKey, old_nickname: String, new_nickname: String },
    ChangeTopic{ channel: Sha256Hash, topic: String },
    PeerJoined { channel: Sha256Hash, nickname: String },
    PeerLeft { channel: Sha256Hash, nickname: String },
    CreatedChannel { channel: Sha256Hash },
    Join { channel: Sha256Hash, nickname: String },
}

pub struct PeerInfo {
    pub keys: Option<Keys>,
    pub channels: Vec<Sha256Hash>,
}

impl PeerInfo {
    pub fn new() -> Self {
        Self {
            keys: None,
            channels: vec![],
        }
    }
}
