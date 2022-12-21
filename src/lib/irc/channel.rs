use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use async_recursion::async_recursion;
use nostr::{ClientMessage, Event, EventBuilder, Keys, Kind, KindBase, RelayMessage, SubscriptionFilter, Tag};
use nostr::event::{TagData, TagKind};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc::UnboundedSender;
use crate::irc::client_data::IRCClientData;
use crate::nostr::metadata::Metadata;

pub enum IRCChannelMessage {
    IRC(String),
    Relay(RelayMessage),
    Client(ClientMessage),
}

pub type IRCChannelHolder = Arc<RwLock<IRCChannel>>;

pub struct IRCChannel {
    id: String,
    warming_up: bool,
    got_metadata: bool,
    got_messages: bool,
    got_nicks: bool,
    nicknames: HashMap<String, Option<String>>,
    warm_up_events: Vec<RelayMessage>,
}

impl IRCChannel {
    pub fn new(id: String) -> Self {
        println!("#{id}: new channel");

        Self {
            id,
            warming_up: true,
            got_metadata: false,
            got_messages: false,
            got_nicks: false,
            nicknames: HashMap::new(),
            warm_up_events: vec![],
        }
    }

    pub async fn handle_nostr_metadata(&mut self, message: &RelayMessage, writer: &mut OwnedWriteHalf, nostr_tx: &UnboundedSender<ClientMessage>, client_data: &IRCClientData) {
        match message {
            RelayMessage::Event { event, .. } => {
                let user_id = event.pubkey.to_string();

                let metadata = serde_json::from_slice::<Metadata>(event.content.as_ref()).unwrap();

                let user = self.nicknames.get_mut(&user_id);

                let new_nick = metadata.name.unwrap_or(user_id.clone()).chars().filter(|c| c.is_ascii_alphanumeric()).collect();

                // println!("{}: metadata: new nick {new_nick}", event.pubkey.to_string());

                if let Some(user) = user {
                    if !self.got_nicks {
                        *user = Some(new_nick);
                    } else {
                        let old_nick = user.as_ref().unwrap_or(&user_id);
                        // Changed nick
                        if old_nick != &new_nick {
                            // println!("{}: metadata: send NICK", event.pubkey.to_string());

                            writer.write(
                                format!(
                                    ":{old_nick} NICK :{new_nick}\r\n",
                                ).as_ref()
                            ).await.ok();

                            *user = Some(new_nick);
                        }
                    }
                }
            }
            RelayMessage::Notice { .. } => {}
            RelayMessage::EndOfStoredEvents { subscription_id } => {
                // println!("{}: metadata EndOfStoredEvents {subscription_id}", self.id);

                let user_id = subscription_id.split_once("-").unwrap().0;

                let user = self.nicknames.get_mut(user_id);

                if let Some(user) = user {
                    if user.is_none() {
                        *user = Some(user_id.to_string());
                    }
                }

                let mut done = true;
                for (_, u) in self.nicknames.iter() {
                    if u.is_none() {
                        done = false;

                        break;
                    }
                }

                if !self.got_nicks && done {
                    self.got_nicks = true;

                    self.check_if_warmup_done(writer, nostr_tx, client_data).await;
                }
            }
            RelayMessage::Ok { .. } => {}
            RelayMessage::Empty => {}
        }
    }

    pub async fn check_if_warmup_done(&mut self, writer: &mut OwnedWriteHalf, nostr_tx: &UnboundedSender<ClientMessage>, client_data: &IRCClientData) {
        // println!("#{}: check_if_warmup_done: {} {} {}", self.id, self.got_metadata, self.got_messages, self.got_nicks);

        if self.got_metadata && self.got_messages && self.got_nicks {
            println!("#{}: channel warmup done, {} messages, {} nicks", self.id, self.warm_up_events.len(), self.nicknames.len());

            self.warming_up = false;

            let mut messages = vec![];

            for m in self.warm_up_events.drain(..) {
                messages.push(m);
            }

            for (user_id, nick) in self.nicknames.iter() {
                let nick = nick.as_ref().unwrap_or(user_id);
                writer.write(
                    format!(
                        ":{nick} JOIN #{}\r\n",
                        self.id,
                    ).as_ref()
                ).await.ok();
            }

            for m in messages {
                self.handle_nostr_channel_message(m, writer, nostr_tx, &client_data, true).await;
            }
        }
    }

    #[async_recursion]
    pub async fn handle_nostr_channel_message(&mut self, message: RelayMessage, writer: &mut OwnedWriteHalf, nostr_tx: &UnboundedSender<ClientMessage>, client_data: &IRCClientData, is_history: bool) {
        match &message {
            RelayMessage::Event { event, .. } => {
                for t in &event.tags {
                    if let Ok(tt) = t.kind() {
                        match tt {
                            TagKind::P => {
                                // println!("nostr_channel_message: P: {:?}", t.as_vec());
                            }
                            TagKind::E => {
                                // println!("nostr_channel_message: E: {:?}", t.as_vec());
                            }
                            TagKind::Nonce => {}
                            TagKind::Delegation => {}
                        }
                    }
                }

                let user_id = event.pubkey.to_string();

                let nick = self.nicknames.get(&user_id);

                if let None = nick {
                    self.nicknames.insert(user_id.clone(), None);

                    let user_metadata = ClientMessage::new_req(
                        format!("{}-user-metadata", user_id),
                        vec![SubscriptionFilter::new().author(user_id.parse().unwrap()).kind(Kind::Base(KindBase::Metadata)).limit(1)],
                    );

                    nostr_tx.send(user_metadata).ok();

                    let user_metadata_close = ClientMessage::close(format!("{}-user-metadata", user_id));

                    nostr_tx.send(user_metadata_close).ok();

                    let my_pubkey = client_data.identity.as_ref().unwrap().public_key().to_string();

                    if user_id != my_pubkey {
                        writer.write(
                            format!(
                                ":{user_id} JOIN #{}\r\n",
                                self.id,
                            ).as_ref()
                        ).await.ok();
                    }
                }
            }
            _ => {},
        };

        if self.warming_up {
            match &message {
                RelayMessage::Event { .. } => {
                    self.warm_up_events.push(message);
                }
                RelayMessage::EndOfStoredEvents { .. } => {
                    // All channel messages retrieved, now wait for nicks
                    self.got_messages = true;
                    self.check_if_warmup_done(writer, nostr_tx, client_data).await;
                }
                _ => return,
            }

            return;
        }

        match &message {
            RelayMessage::Event { event, .. } => {
                let mut cur_nick = event.pubkey.to_string();

                if !is_history {
                    // Don't send our own messages since these are displayed by the IRC client
                    let our_nick = client_data.identity.as_ref().unwrap().public_key().to_string();
                    if cur_nick == our_nick {
                        return;
                    }
                }

                let nickname = self.nicknames.get(&cur_nick);

                if let Some(Some(nickname)) = nickname {
                    cur_nick = nickname.clone();
                }

                writer.write(
                    format!(
                        ":{cur_nick} PRIVMSG #{} :{}\r\n",
                        self.id,
                        event.content.replace("\n", " ").replace("\r", " ")
                    ).as_ref()
                ).await.ok();
            }
            _ => return,
        }
    }
    pub async fn handle_nostr_channel_metadata(&mut self, _message: RelayMessage, _writer: &mut OwnedWriteHalf, _nostr_tx: &UnboundedSender<ClientMessage>, _client_data: &IRCClientData) {
        // @todo haven't seen this used anywhere, maybe implement later
    }

    pub async fn handle_nostr_channel_creation(&mut self, message: RelayMessage, writer: &mut OwnedWriteHalf, nostr_tx: &UnboundedSender<ClientMessage>, client_data: &IRCClientData) {
        let event = match message {
            RelayMessage::Event { event, .. } => event,
            _ => return,
        };

        for t in &event.tags {
            if let Ok(tt) = t.kind() {
                match tt {
                    TagKind::P => {
                        // println!("nostr_channel_creation: P: {:?}", t.as_vec());
                    }
                    TagKind::E => {
                        // println!("nostr_channel_creation: E: {:?}", t.as_vec());
                    }
                    TagKind::Nonce => {}
                    TagKind::Delegation => {}
                }
            }
        }

        let metadata = serde_json::from_slice::<Metadata>(event.content.as_ref()).unwrap();

        writer.write(
            format!(
                ":admin TOPIC #{} :{}\r\n",
                self.id,
                metadata.name.unwrap_or(String::from("")),
            ).as_ref()
        ).await.ok();

        self.got_metadata = true;

        self.check_if_warmup_done(writer, nostr_tx, client_data).await;
    }

    pub async fn send_message(&mut self, message: String, my_keys: &Keys, nostr_tx: &UnboundedSender<ClientMessage>) -> Option<()> {
        println!("{}: send_message: {message}", self.id);

        let event: Event = EventBuilder::new(
            Kind::Base(KindBase::ChannelMessage),
            message,
            &[Tag::new(TagData::Generic(
                TagKind::E,
                vec![self.id.to_string()],
            ))],
        ).to_event(&my_keys).unwrap();

        let msg = ClientMessage::new_event(event);
        nostr_tx.send(msg).ok();

        Some(())
    }

    pub async fn join(&mut self, nostr_tx: &UnboundedSender<ClientMessage>, writer: &mut OwnedWriteHalf, client_data: &IRCClientData) -> Option<()> {
        self.warming_up = true;

        let channel_info = ClientMessage::new_req(
            format!("{}-info", self.id),
            vec![SubscriptionFilter::new().id(&self.id)],
        );

        nostr_tx.send(channel_info).ok();

        let channel_messages = ClientMessage::new_req(
            format!("{}-messages", self.id),
            vec![SubscriptionFilter::new().kind(Kind::Base(KindBase::ChannelMessage)).limit(200).event(self.id.parse().unwrap())],
        );

        nostr_tx.send(channel_messages).ok();

        let my_pubkey = client_data.identity.as_ref().unwrap().public_key().to_string();
        let my_nick = client_data.get_nick().unwrap_or(my_pubkey);

        writer.write(
            format!(
                ":{my_nick} JOIN #{}\r\n",
                self.id,
            ).as_ref()
        ).await.ok();

        writer.write(
            format!(
                "332 {my_nick} #{} :Channel is loading...\r\n",
                self.id,
            ).as_ref()
        ).await.ok();

        writer.write(
            format!(
                "353 {my_nick} = #{} :{my_nick}\r\n",
                self.id,
            ).as_ref()
        ).await.ok();

        writer.write(
            format!(
                "366 {my_nick} = #{} :End of NAMES list\r\n",
                self.id,
            ).as_ref()
        ).await.ok();

        Some(())
    }

    pub async fn part(&mut self, nostr_tx: &UnboundedSender<ClientMessage>, writer: &mut OwnedWriteHalf, client_data: &IRCClientData) -> Option<()> {
        let channel_info = ClientMessage::close(format!("{}-info", self.id));

        nostr_tx.send(channel_info).ok();

        let channel_messages = ClientMessage::close(format!("{}-messages", self.id));

        nostr_tx.send(channel_messages).ok();

        let my_pubkey = client_data.identity.as_ref().unwrap().public_key().to_string();
        let my_nick = client_data.get_nick().unwrap_or(my_pubkey);

        writer.write(
            format!(
                ":{my_nick} PART #{}\r\n",
                self.id,
            ).as_ref()
        ).await.ok();

        Some(())
    }
}
