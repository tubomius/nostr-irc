use std::error::Error;
use tokio::net::TcpListener;
use crate::irc::peer::IRCPeer;

pub struct IRCServer {
    clients: Vec<IRCPeer>
}

impl IRCServer {
    pub fn new() -> Self {
        Self {
            clients: vec![],
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        let addr = "127.0.0.1:6667".to_string();

        let listener = TcpListener::bind(&addr).await?;

        println!("Listening on: {}", addr);

        loop {
            // Asynchronously wait for an inbound socket.
            let (mut socket, _) = listener.accept().await?;

            let mut peer = IRCPeer::new();

            tokio::spawn(async move {
                peer.run(socket).await.expect("client died");
            });
        }

        return Ok(());
    }
}