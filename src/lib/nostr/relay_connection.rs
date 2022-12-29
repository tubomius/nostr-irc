use std::error::Error;
use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use futures_util::stream::SplitSink;
use nostr::RelayMessage;
use nostr::url::Url;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_tungstenite::tungstenite::Message;

pub struct NostrRelayConnection {
}

impl NostrRelayConnection {
    pub fn new() -> Self {
        Self {
        }
    }

    pub async fn connect(&mut self, relay_url: Url, mut tx: UnboundedSender<RelayMessage>, mut rx: UnboundedReceiver<String>) -> Result<(), Box<dyn Error>> {
        loop {
            // println!("NostrRelayConnection: connecting: {relay_url}");

            match tokio_tungstenite::connect_async(&relay_url).await {
                Ok((stream, _)) => {
                    // println!("NostrRelayConnection: connected: {relay_url}");

                    let (mut write_stream, mut read_stream) = stream.split();

                    loop {
                        tokio::select! {
                            Some(m) = rx.recv() => {
                                self.send_client_message(&mut write_stream, m).await.ok();
                            }
                            Some(m) = read_stream.next() => {
                                if let Ok(m) = m {
                                    self.forward_relay_message(&mut tx, m).await.ok();
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(_e) => {
                    // println!("NostrRelayConnection: error: {e:?}");
                }
            }

            // println!("NostrRelayConnection: disconnected: {relay_url}");

            // Reconnect?
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }

    pub async fn send_client_message(&mut self, write_stream: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>, message: String) -> Result<(), Box<dyn Error>> {
        // println!("NostrRelayConnection: handle_client_message: {message}");

        write_stream.send(tokio_tungstenite::tungstenite::Message::Text(message)).await.map_err(|e| e.into())
    }

    pub async fn forward_relay_message(&mut self, tx: &mut UnboundedSender<RelayMessage>, message: tokio_tungstenite::tungstenite::Message) -> Result<(), Box<dyn Error>> {
        match message {
            Message::Text(text) => {
                // println!("NostrRelayConnection: handle_relay_message: {text}");

                tx.send(RelayMessage::from_json(&*text)?).map_err(|e| e.into())
            }
            Message::Ping(_) => {
                // println!("ping");

                Ok(())
            }
            _ => Ok(()),
        }
    }
}
