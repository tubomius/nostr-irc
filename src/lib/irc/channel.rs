use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use async_recursion::async_recursion;
use nostr::{ClientMessage, Event, EventBuilder, Keys, Kind, KindBase, RelayMessage, SubscriptionFilter, Tag};
use nostr::event::{TagData, TagKind};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc::UnboundedSender;
use crate::irc::client_data::ClientDataHolder;
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
        println!("new channel {id}");

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

    pub async fn handle_nostr_metadata(&mut self, message: &RelayMessage, writer: &mut OwnedWriteHalf, nostr_tx: &UnboundedSender<ClientMessage>, client_data: &ClientDataHolder) {
        match message {
            RelayMessage::Event { event, .. } => {
                let user_id = event.pubkey.to_string();

                let metadata = serde_json::from_slice::<Metadata>(event.content.as_ref()).unwrap();

                let user = self.nicknames.get_mut(&user_id);

                let new_nick = metadata.name.unwrap_or(user_id).chars().filter(|c| c.is_ascii_alphanumeric()).collect();

                if let Some(user) = user {
                    if !self.got_nicks {
                        *user = Some(new_nick);
                    } else {
                        if let Some(user) = user {
                            // Changed nick
                            let old_nick = user.clone();
                            if old_nick != new_nick {
                                writer.write(
                                    format!(
                                        ":{old_nick} NICK :{new_nick}\r\n",
                                    ).as_ref()
                                ).await.ok();

                                *user = new_nick;
                            }
                        } else {

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

    pub async fn check_if_warmup_done(&mut self, writer: &mut OwnedWriteHalf, nostr_tx: &UnboundedSender<ClientMessage>, client_data: &ClientDataHolder) {
        println!("{}: check_if_warmup_done: {} {} {}", self.id, self.got_metadata, self.got_messages, self.got_nicks);

        if self.got_metadata && self.got_messages && self.got_nicks {
            println!("{}: warmup done, {} messages, {} nicks", self.id, self.warm_up_events.len(), self.nicknames.len());

            self.warming_up = false;

            let mut messages = vec![];

            for m in self.warm_up_events.drain(..) {
                messages.push(m);
            }

            for m in messages {
                self.handle_nostr_channel_message(m, writer, nostr_tx, &client_data, true).await;
            }
        }
    }

    #[async_recursion]
    pub async fn handle_nostr_channel_message(&mut self, message: RelayMessage, writer: &mut OwnedWriteHalf, nostr_tx: &UnboundedSender<ClientMessage>, client_data: &ClientDataHolder, is_history: bool) {
        match &message {
            RelayMessage::Event { event, .. } => {
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
                    let our_nick = client_data.read().await.identity.as_ref().unwrap().public_key().to_string();
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

    pub async fn handle_nostr_channel_creation(&mut self, message: RelayMessage, writer: &mut OwnedWriteHalf, nostr_tx: &UnboundedSender<ClientMessage>, client_data: &ClientDataHolder) {
        let event = match message {
            RelayMessage::Event { event, .. } => event,
            _ => return,
        };

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

    pub async fn join(&mut self, nostr_tx: &UnboundedSender<ClientMessage>) -> Option<()> {
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

        Some(())
    }

    pub async fn part(&mut self, nostr_tx: &UnboundedSender<ClientMessage>) -> Option<()> {
        let channel_info = ClientMessage::close(format!("{}-info", self.id));

        nostr_tx.send(channel_info).ok();

        let channel_messages = ClientMessage::close(format!("{}-messages", self.id));

        nostr_tx.send(channel_messages).ok();

        Some(())
    }
}
