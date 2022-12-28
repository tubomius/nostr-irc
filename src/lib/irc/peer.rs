use log::*;
use std::collections::HashMap;
use std::error::Error;
use nostr::url::Url;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::task::JoinSet;
use nostr::{ClientMessage, Event, EventBuilder, Kind, KindBase, RelayMessage, SubscriptionFilter};
use nostr::event::{TagKind};
use nostr::hashes::hex::ToHex;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use crate::irc::channel::{IRCChannel};
use crate::irc::client_data::IRCClientData;
use crate::irc::message::IRCMessage;
use crate::irc::server::{PeerMessage, ServerMessage};

pub struct IRCPeer {
    peer_id: u64,
    client_data: IRCClientData,
    tasks: JoinSet<()>,
    socket_reader: Lines<BufReader<OwnedReadHalf>>,
    socket_writer: OwnedWriteHalf,
    tx: UnboundedSender<PeerMessage>,
    rx: UnboundedReceiver<ServerMessage>,
    sent_welcome: bool,
}

impl IRCPeer {
    pub fn new(socket: TcpStream, tx: UnboundedSender<PeerMessage>, rx: UnboundedReceiver<ServerMessage>, peer_id: u64) -> Self {
        let (socket_reader, socket_writer) = socket.into_split();

        let socket_reader = BufReader::new(socket_reader).lines();

        Self {
            peer_id,
            client_data: IRCClientData::new(),
            tasks: JoinSet::new(),
            socket_reader,
            socket_writer,
            tx,
            rx,
            sent_welcome: false,
        }
    }

    pub async fn run(&mut self) {
        loop {
            tokio::select!(
                Some(message) = self.rx.recv() => {
                    self.handle_server_message(message).await;
                }
                Ok(message) = self.socket_reader.next_line() => {
                    if let Some(message) = message {
                        self.handle_irc_message(message).await;
                    } else {
                        break;
                    }
                }
                Some(_) = self.tasks.join_next() => {
                    // nothing
                }
            );
        }
    }

    pub async fn handle_server_message(&mut self, message: ServerMessage) -> Option<()> {
        info!("{message:?}");

        match message {
            ServerMessage::SendMessage { channel, message, nickname } => {
                self.socket_writer.write(format!(":{nickname} PRIVMSG #{channel} :{message}\r\n").as_ref()).await.ok();
            }
            ServerMessage::ChangeNickname { public_key, old_nickname, new_nickname } => {
                let our_public_key = self.client_data.get_public_key();

                if let Some(our_public_key) = our_public_key {
                    if public_key == our_public_key {
                        if !self.sent_welcome {
                            self.sent_welcome = true;

                            self.socket_writer.write(format!("001 {new_nickname} :Welcome to the Internet Relay Network").as_ref()).await.ok();
                        }

                        self.socket_writer.write(format!("NICK {new_nickname}\r\n").as_ref()).await.ok();
                    } else {
                        self.socket_writer.write(format!(":{old_nickname} NICK {new_nickname}\r\n").as_ref()).await.ok();
                    }
                }
            }
            ServerMessage::ChangeTopic { channel, topic } => {
                self.socket_writer.write(format!(":admin TOPIC #{channel} :{topic}\r\n").as_ref()).await.ok();
            }
            ServerMessage::PeerJoined { channel, nickname } => {
                self.socket_writer.write(format!(":{nickname} JOIN #{channel}\r\n").as_ref()).await.ok();
                self.socket_writer.write(format!("353 {nickname} = #{channel} :{nickname}\r\n").as_ref()).await.ok();
                self.socket_writer.write(format!("366 {nickname} = #{channel} :End of NAMES list\r\n").as_ref()).await.ok();
            }
            ServerMessage::PeerLeft { channel, nickname } => {
                self.socket_writer.write(format!(":{nickname} PART #{channel}\r\n").as_ref()).await.ok();
            }
            ServerMessage::Join { channel, nickname } => {
                self.socket_writer.write(format!(":{nickname} JOIN #{channel}\r\n").as_ref()).await.ok();
            }
            ServerMessage::CreatedChannel { channel } => {
                self.tx.send(PeerMessage::AddChannel { peer_id: self.peer_id, public_key: channel }).ok();
            }
        }

        None
    }

    pub async fn handle_irc_message(&mut self, message: String) -> Option<()> {
        info!("{message:?}");

        let message = IRCMessage::from_string(message);

        match message {
            IRCMessage::CAP(s, _) => {
                if s == "LS" {
                    self.socket_writer.write(format!("CAP * LS\r\n").as_ref()).await.ok();
                }
            },
            IRCMessage::QUIT(_) => {
                self.socket_writer.write(format!("QUIT\r\n").as_ref()).await.ok();
            }
            IRCMessage::WHO(_) => {}
            IRCMessage::LIST() => {}
            IRCMessage::JOIN(channel) => {
                let channels = channel.split(",");

                for channel in channels {
                    let channel = channel.split_once("#").unwrap().1;

                    if let Ok(channel_key) = channel.parse() {
                        self.tx.send(PeerMessage::AddChannel { peer_id: self.peer_id, public_key: channel_key }).ok();
                    } else {
                        // Create channel
                        self.tx.send(PeerMessage::CreateChannel { peer_id: self.peer_id, name: channel.to_string() }).ok();
                    }
                }
            }
            IRCMessage::PART(channel) => {
                let channels = channel.split(",");

                for channel in channels {
                    let channel = channel.split_once("#").unwrap().1;

                    self.tx.send(PeerMessage::LeaveChannel { peer_id: self.peer_id, public_key: channel.parse().unwrap() }).ok();
                }
            }
            IRCMessage::PRIVMSG(channel, message) => {
                let channel = channel.split_once("#").unwrap().1;

                self.tx.send(PeerMessage::SendMessage { peer_id: self.peer_id, channel: channel.parse().unwrap(), message }).ok();
            }
            IRCMessage::PASS(s) => {
                self.client_data.set_private_key(s.clone());

                let public_key = self.client_data.get_public_key();

                if let Some(public_key) = public_key {
                    self.tx.send(PeerMessage::AddNickname { peer_id: self.peer_id, public_key }).ok();
                    self.tx.send(PeerMessage::SetKeys { peer_id: self.peer_id, keys: self.client_data.identity.as_ref().unwrap().clone() }).ok();
                }
            }
            IRCMessage::NICK(s) => {
                let public_key = self.client_data.get_public_key();

                if let Some(public_key) = public_key {
                    self.tx.send(PeerMessage::ChangeNickname { peer_id: self.peer_id, public_key, nickname: s }).ok();
                }
            }
            IRCMessage::USER(_, _, _, _) => {}
            _ => {},
        };

        Some(())
    }
}
