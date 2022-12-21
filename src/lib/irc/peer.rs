use std::collections::HashMap;
use std::error::Error;
use nostr::url::Url;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinSet;
use nostr::{ClientMessage, Event, EventBuilder, Kind, KindBase, RelayMessage, SubscriptionFilter};
use nostr::event::{TagKind};
use nostr::hashes::hex::ToHex;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc::{UnboundedSender};
use crate::irc::channel::{IRCChannel};
use crate::irc::client_data::{ClientDataHolder, IRCClientData};
use crate::irc::message::IRCMessage;
use crate::nostr::client::NostrClient;

pub struct IRCPeer {
    client_data: ClientDataHolder,
    channels: HashMap<String, IRCChannel>,
    set: JoinSet<()>,
}

impl IRCPeer {
    pub fn new() -> Self {
        Self {
            client_data: ClientDataHolder::new(RwLock::new(IRCClientData::new())),
            channels: HashMap::new(),
            set: JoinSet::new(),
        }
    }

    pub async fn run(&mut self, socket: TcpStream, mut nostr_client: NostrClient) {
        let (reader, mut writer) = socket.into_split();
        let reader = BufReader::new(reader);

        let (nostr_client_tx, mut nostr_rx) = mpsc::unbounded_channel();
        let (nostr_tx, nostr_client_rx) = mpsc::unbounded_channel();

        self.set.spawn(async move {
            nostr_client.run(nostr_client_tx, nostr_client_rx).await;
        });

        let mut lines = reader.lines();

        loop {
            tokio::select!(
                Some((relay_url, message)) = nostr_rx.recv() => {
                    self.handle_nostr_message(relay_url, message, &nostr_tx, &mut writer).await.ok();
                }
                Ok(msg) = lines.next_line() => {
                    if let Some(msg) = msg {
                        self.handle_message(msg, &nostr_tx, &mut writer).await;
                    } else {
                        break;
                    }
                }
                Some(_) = self.set.join_next() => {
                    // nothing
                }
            );
        }
    }

    pub async fn handle_nostr_message(&mut self, _relay_url: Url, message: RelayMessage, nostr_tx: &UnboundedSender<ClientMessage>, writer: &mut OwnedWriteHalf) -> Result<(), Box<dyn Error>> {
        match &message {
            RelayMessage::Event { event, .. } => {
                match &event.kind {
                    Kind::Base(k) => {
                        match k {
                            KindBase::ChannelMetadata => {
                                // println!("ChannelMetadata {event:?}");
                            },
                            KindBase::ChannelMessage => {
                                for tag in &event.tags {
                                    if let Ok(tk) = tag.kind() {
                                        match tk {
                                            TagKind::E => {
                                                let channel = tag.content();

                                                if let Some(channel) = channel {
                                                    self.get_or_add_channel(channel.to_string());

                                                    let channel = self.channels.get_mut(channel).unwrap();

                                                    channel.handle_nostr_channel_message(message.clone(), writer, nostr_tx, &self.client_data, false).await;
                                                }

                                                break;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            },
                            KindBase::ChannelCreation => {
                                // println!("ChannelCreation {event:?}");

                                let channel = event.id.to_hex();

                                self.get_or_add_channel(channel.to_string());

                                let channel = self.channels.get_mut(&channel).unwrap();

                                channel.handle_nostr_channel_creation(message, writer, nostr_tx, &self.client_data).await;
                            },
                            KindBase::Metadata => {
                                // println!("Metadata {event:?}");

                                for (_, channel) in self.channels.iter_mut() {
                                    channel.handle_nostr_metadata(&message, writer, nostr_tx, &self.client_data).await;
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            RelayMessage::Notice { .. } => {}
            RelayMessage::EndOfStoredEvents { subscription_id } => {
                if subscription_id.contains("-messages") || subscription_id.contains("-info") {
                    let channel = subscription_id.split_once("-").unwrap().0;

                    let channel = self.channels.get_mut(channel);

                    if let Some(channel) = channel {
                        channel.handle_nostr_channel_message(message.clone(), writer, nostr_tx, &self.client_data, false).await;
                    }
                } else if subscription_id.contains("-user-metadata") {
                    for (_, channel) in self.channels.iter_mut() {
                        channel.handle_nostr_metadata(&message, writer, nostr_tx, &self.client_data).await;
                    }
                }
            }
            RelayMessage::Ok { .. } => {}
            RelayMessage::Empty => {}
        }

        Ok(())
    }

    pub async fn handle_message(&mut self, msg: String, nostr_tx: &UnboundedSender<ClientMessage>, writer: &mut OwnedWriteHalf) -> Option<()> {
        println!("irc client sent: {msg}");

        let msg = IRCMessage::from_string(msg);

        let response = match msg {
            IRCMessage::CAP(s, _) => {
                if s == "LS" {
                    Some(Some(format!("CAP * LS")))
                } else {
                    let nick = self.client_data.read().await.get_nick();
                    let private_key = self.client_data.read().await.get_private_key();

                    if !private_key.is_none() {
                        if let Some(nick) = nick {
                            Some(Some(format!("001 {nick} :Welcome to the Internet Relay Network")))
                        } else {
                            Some(Some(format!("ERROR :No nick or no private key, set password to your private key")))
                        }
                    } else {
                        Some(Some(format!("ERROR :No nick or no private key, set password to your private key")))
                    }
                }
            },
            IRCMessage::QUIT(_) => {
                Some(Some(format!("QUIT")))
            }
            IRCMessage::WHO(_) => {
                Some(None)
            }
            IRCMessage::LIST() => {
                let channel_list = ClientMessage::new_req(
                    format!("list"),
                    vec![
                        SubscriptionFilter::new()
                            .kind(Kind::Base(KindBase::ChannelCreation))
                    ],
                );

                nostr_tx.send(channel_list).ok();

                Some(None)
            }
            IRCMessage::JOIN(channel) => {
                let channels = channel.split(",");

                for channel in channels {
                    let channel = channel.split_once("#").unwrap().1;

                    let irc_channel = {
                        self.get_or_add_channel(channel.to_string());

                        self.channels.get_mut(channel).unwrap()
                    };

                    irc_channel.join(&nostr_tx, writer, &self.client_data).await;
                }

                Some(None)
            }
            IRCMessage::PART(channel) => {
                let channels = channel.split(",");

                for channel in channels {
                    let channel = channel.split_once("#").unwrap().1;

                    let irc_channel = {
                        self.get_or_add_channel(channel.to_string());

                        self.channels.get_mut(channel).unwrap()
                    };

                    irc_channel.part(&nostr_tx, writer, &self.client_data).await;

                    self.channels.remove(channel);
                }

                Some(None)
            }
            IRCMessage::PRIVMSG(channel, message) => {
                println!("PRIVMSG {channel} {message}");

                let my_keys = self.client_data.read().await.identity.as_ref().unwrap().clone();

                if channel.contains("#") {
                    let channel = channel.split_once("#").unwrap().1;

                    let irc_channel = {
                        self.get_or_add_channel(channel.to_string());

                        self.channels.get_mut(channel).unwrap()
                    };

                    irc_channel.send_message(message, &my_keys, nostr_tx).await;
                } else {
                    // @todo encrypted DM
                }

                Some(None)
            }
            IRCMessage::PASS(s) => {
                self.client_data.write().await.set_private_key(s.clone());

                Some(None)
            }
            IRCMessage::NICK(s) => {
                let old_nick = self.client_data.read().await.get_nick().clone();

                self.client_data.write().await.set_nick(s.clone());

                if let Some(old_nick) = old_nick {
                    let metadata = nostr::Metadata::new()
                        .name(&s);

                    let my_keys = self.client_data.read().await.identity.as_ref().unwrap().clone();

                    let event: Event = EventBuilder::set_metadata(&my_keys, metadata).unwrap().to_event(&my_keys).unwrap();

                    let msg = ClientMessage::new_event(event);
                    nostr_tx.send(msg).ok();

                    Some(Some(format!(":{old_nick} NICK {s}")))
                } else {
                    Some(None)
                }
            }
            IRCMessage::USER(_, _, _, _) => {
                let metadata = nostr::Metadata::new()
                    .name(self.client_data.read().await.get_nick().unwrap())
                    .about("description wat");

                let my_keys = self.client_data.read().await.identity.as_ref().unwrap().clone();

                let event: Event = EventBuilder::set_metadata(&my_keys, metadata).unwrap().to_event(&my_keys).unwrap();

                let msg = ClientMessage::new_event(event);
                nostr_tx.send(msg).ok();

                Some(None)
            }
            _ => Some(None),
        };

        if let Some(response) = response {
            if let Some(response) = response {
                if response == "QUIT" {
                    return None;
                }

                match writer.write(response.as_ref()).await {
                    Ok(_) => {}
                    Err(_) => {}
                }

                match writer.write("\r\n".as_ref()).await {
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        }

        Some(())
    }

    pub fn get_or_add_channel(&mut self, channel_id: String) {
        let channels = &mut self.channels;

        if let Some(_) = channels.get_mut(&channel_id) {
            return;
        }

        channels.insert(channel_id.to_string(), IRCChannel::new(channel_id.to_string()));
    }
}
