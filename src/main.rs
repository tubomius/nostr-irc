use std::error::Error;
use lib::irc::server::IRCServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut server = IRCServer::new();

    server.run().await
}
