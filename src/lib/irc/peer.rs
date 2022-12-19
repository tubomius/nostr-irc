use std::error::Error;
use std::sync::Arc;
use nostr::url::Url;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::sync::mpsc::error::SendError;
use tokio::task::JoinSet;
use futures::StreamExt;
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
        let reader = BufReader::new(reader);

        let mut set = JoinSet::new();

        let (stream, _) = tokio_tungstenite::connect_async(Url::parse("wss://relay.nostr.ch")?).await?;

        let (mut write_stream, mut read_stream) = stream.split();

        let nostr_client = Arc::new(Mutex::new(
            write_stream,
        ));

        let (mut tx, mut rx) = mpsc::unbounded_channel();

        {
            let nostr_client = nostr_client.clone();
            set.spawn(async move {
                println!("Listening...");
                loop {
                    if let Some(m) = read_stream.next().await {
                        if let Ok(m) = m {
                            println!("nostr: {m:?}");
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
            set.spawn(async move {
                loop {
                    let incoming = rx.recv().await;

                    if let Some(msg) = incoming {
                        match &msg {
                            IRCMessage::NICK(nick) => client_data.write().await.set_nick(nick.clone()),
                            _ => {}
                        }

                        let response = msg.handle_message(&client_data, &nostr_client).await;

                        if let Some(response) = response {
                            if response == "QUIT" {
                                break;
                            }

                            println!("send: {}", response);

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
