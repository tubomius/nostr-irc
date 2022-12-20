use std::error::Error;
use std::sync::Arc;
use nostr::url::Url;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinSet;
use nostr::{ClientMessage, Event, Kind, KindBase, Metadata, RelayMessage};
use nostr::event::TagKind;
use nostr::hashes::hex::ToHex;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc::UnboundedSender;
use crate::irc::client_data::{ClientDataHolder, IRCClientData};
use crate::irc::message::IRCMessage;
use crate::nostr::client::NostrClient;

pub struct IRCPeer {
    client_data: ClientDataHolder,
}

impl IRCPeer {
    pub fn new() -> Self {
        Self {
            client_data: ClientDataHolder::new(RwLock::new(IRCClientData::new())),
        }
    }

    pub async fn handle_nostr_message(&mut self, _relay_url: Url, message: RelayMessage, nostr_tx: &UnboundedSender<ClientMessage>, writer: &mut OwnedWriteHalf) -> Result<(), Box<dyn Error>> {
        match message {
            RelayMessage::Event { event, .. } => {
                let response = handle_nostr_event(&event, false, &nostr_tx, &self.client_data).await;

                if let Some(response) = response {
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
            RelayMessage::Notice { .. } => {}
            RelayMessage::EndOfStoredEvents { .. } => {
                /*
                @todo re-implement
                let backlog = subscription_ids.get_mut(&subscription_id);

                if let Some(b) = backlog {
                    b.sort_by(|a, b| a.created_at.cmp(&b.created_at));

                    for m in b.iter() {
                        let response = handle_nostr_event(m, true, &nostr_client, &client_data).await;

                        if let Some(response) = response {
                            println!("history: sending {response}");

                            let mut writer = writer.lock().await;

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

                    *b = vec![];

                    ended_init.push(subscription_id);
                }
                */
            }
            RelayMessage::Ok { .. } => {}
            RelayMessage::Empty => {}
        }

        Ok(())
    }

    pub async fn handle_message(&mut self, msg: String, nostr_tx: &UnboundedSender<ClientMessage>, writer: &mut OwnedWriteHalf) -> Option<()> {
        let msg = IRCMessage::from_string(msg);

        let response = msg.handle_message(&self.client_data, &nostr_tx).await;

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

    pub async fn run(&mut self, socket: TcpStream, mut nostr_client: NostrClient) {
        let (reader, mut writer) = socket.into_split();
        let reader = BufReader::new(reader);

        let mut set = JoinSet::new();

        let (nostr_client_tx, mut nostr_rx) = mpsc::unbounded_channel();
        let (nostr_tx, nostr_client_rx) = mpsc::unbounded_channel();

        set.spawn(async move {
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
                Some(_) = set.join_next() => {
                    // nothing
                }
            );
        }
    }
}

pub async fn handle_nostr_event(event: &Box<Event>, is_history: bool, _nostr_tx: &UnboundedSender<ClientMessage>, client_data: &Arc<RwLock<IRCClientData>>) -> Option<String> {
    match event.kind {
        Kind::Base(k) => {
            match k {
                KindBase::ChannelMessage => {
                    for tag in &event.tags {
                        if let Ok(tk) = tag.kind() {
                            match tk {
                                TagKind::E => {
                                    let channel = tag.content();

                                    if let Some(channel) = channel {
                                        let p = client_data.read().await.identity.as_ref().unwrap().public_key();
                                        if !is_history && event.pubkey == p {
                                            return None;
                                        }

                                        return Some(format!(":{} PRIVMSG #{channel} :{}", &event.pubkey.to_string()[..16], event.content));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                KindBase::ChannelCreation => {
                    let channel = event.id.to_hex();
                    let metadata = serde_json::from_slice::<Metadata>(event.content.as_ref()).unwrap();

                    return Some(
                        format!(
                            "322 #{channel} 0 :{} ({})",
                            metadata.name.unwrap_or(String::from("")),
                            metadata.about.unwrap_or(String::from("")),
                        )
                    );
                }
                _ => {}
            }
        }
        _ => {}
    }

    return None;
}
