use std::error::Error;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::sync::{mpsc, RwLock};
use tokio::sync::mpsc::error::SendError;
use tokio::task::JoinSet;
use crate::irc::message::IRCMessage;

pub type ClientDataHolder = Arc<RwLock<IRCClientData>>;

pub struct IRCClientData {
    nick: Option<String>,
    private_key: Option<String>,
    public_key: Option<String>,
}

impl IRCClientData {
    pub fn new() -> Self {
        Self {
            nick: None,
            private_key: None,
            public_key: None,
        }
    }

    pub fn get_nick(&self) -> Option<String> {
        return self.nick.clone();
    }

    pub fn set_nick(&mut self, nick: String) {
        self.nick = Some(nick);
    }

    pub fn get_private_key(&self) -> Option<String> {
        return self.private_key.clone();
    }

    pub fn set_private_key(&mut self, private_key: String) {
        self.private_key = Some(private_key);
    }
}
