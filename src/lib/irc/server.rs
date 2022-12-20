use std::error::Error;
use tokio::net::TcpListener;
use crate::irc::peer::IRCPeer;
use crate::nostr::client::NostrClient;

pub struct IRCServer {
}

impl IRCServer {
    pub fn new() -> Self {
        Self {
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        let addr = "127.0.0.1:6667".to_string();
        let relays = vec![
            "wss://relay.nostr.ch".to_string(),
            "wss://nostr.rocks".to_string(),
            "wss://no.str.cr".to_string(),
            "wss://relay.damus.io".to_string(),
        ];

        let listener = TcpListener::bind(&addr).await?;

        println!("IRC server listening on: {}", addr);
        println!("Connect using your IRC client with a nostr private key as password");

        loop {
            let (socket, _) = listener.accept().await?;

            let mut peer = IRCPeer::new();

            let relays = relays.clone();

            tokio::spawn(async move {
                let nostr_client = NostrClient::new(relays);

                peer.run(socket, nostr_client).await;
            });
        }
    }
}