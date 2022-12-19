use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use nostr::url::Url;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::sync::mpsc::error::SendError;
use tokio::task::JoinSet;
use futures::StreamExt;
use futures_util::stream::SplitSink;
use nostr::{Event, EventBuilder, Kind, KindBase, Metadata, RelayMessage};
use nostr::event::TagKind;
use nostr::hashes::hex::ToHex;
use nostr::message::relay::MessageHandleError;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_tungstenite::tungstenite::Message;
use crate::irc::client_data::{ClientDataHolder, IRCClientData};
use crate::irc::message::IRCMessage;

pub struct IRCPeer {
    client_data: ClientDataHolder,
}

impl IRCPeer {
    pub fn new() -> Self {
        Self {
            client_data: ClientDataHolder::new(RwLock::new(IRCClientData::new())),
        }
    }

    pub async fn run(&mut self, socket: TcpStream) -> Result<(), Box<dyn Error>> {
        let (reader, mut writer) = socket.into_split();
        let writer = Arc::new(Mutex::new(writer));
        let reader = BufReader::new(reader);

        let mut set = JoinSet::new();

        let (stream, _) = tokio_tungstenite::connect_async(Url::parse("wss://relay.nostr.ch")?).await?;

        let (mut write_stream, mut read_stream) = stream.split();

        let nostr_client = Arc::new(Mutex::new(
            write_stream,
        ));

        let (mut tx, mut rx) = mpsc::unbounded_channel();

        {
            let client_data = self.client_data.clone();
            let nostr_client = nostr_client.clone();
            let writer = writer.clone();
            set.spawn(async move {
                println!("Listening...");
                let mut subscription_ids = HashMap::new();
                let mut ended_init = vec![];
                loop {
                    if let Some(m) = read_stream.next().await {
                        if let Ok(m) = m {
                            println!("nostr: {m:?}");
                            match m {
                                Message::Text(t) => {
                                    let msg = RelayMessage::from_json(&*t);

                                    match msg {
                                        Ok(m) => {
                                            match m {
                                                RelayMessage::Event { event, subscription_id } => {
                                                    if !ended_init.contains(&subscription_id) {
                                                        if !subscription_ids.contains_key(&subscription_id) {
                                                            subscription_ids.insert(subscription_id.clone(), vec![]);
                                                        }

                                                        let backlog = subscription_ids.get_mut(&subscription_id);

                                                        if let Some(b) = backlog {
                                                            (*b).push(event);
                                                        }
                                                    } else {
                                                        let response = handle_nostr_event(&event, false, &nostr_client, &client_data).await;

                                                        if let Some(response) = response {
                                                            println!("live: sending {response}");

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
                                                }
                                                RelayMessage::Notice { .. } => {}
                                                RelayMessage::EndOfStoredEvents { subscription_id } => {
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
                                                }
                                                RelayMessage::Ok { .. } => {}
                                                RelayMessage::Empty => {}
                                            }
                                        }
                                        Err(_) => {}
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    tokio::task::yield_now().await;
                }
            });
        }
        {
            let client_data = self.client_data.clone();
            let nostr_client = nostr_client.clone();
            set.spawn(async move {
                let mut lines = reader.lines();
                loop {
                    let msg = lines.next_line().await;

                    if let Ok(msg) = msg {
                        if let Some(msg) = msg {
                            println!("recv: {}", msg);

                            let message = IRCMessage::from_string(msg);

                            match tx.send(message) {
                                Ok(_) => {}
                                Err(_) => {}
                            };
                        }
                    } else {
                        break;
                    }
                }
            });
        }
        {
            let client_data = self.client_data.clone();
            let nostr_client = nostr_client.clone();
            let writer = writer.clone();
            set.spawn(async move {
                loop {
                    let incoming = rx.recv().await;

                    if let Some(msg) = incoming {
                        let response = msg.handle_message(&client_data, &nostr_client).await;

                        if let Some(response) = response {
                            if response == "QUIT" {
                                break;
                            }

                            println!("send: {}", response);

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
                }
            });
        }

        while let Some(res) = set.join_next().await {
            // nothing
        }

        Ok(())
    }
}

pub async fn handle_nostr_event(event: &Box<Event>, is_history: bool, nostr_client: &Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>, client_data: &Arc<RwLock<IRCClientData>>) -> Option<String> {
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

                                        return Some(format!(":{} PRIVMSG #{channel} :{}", event.pubkey, event.content));
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
