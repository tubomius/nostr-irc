use std::str::FromStr;
use nostr::Keys;

pub struct IRCClientData {
    nick: Option<String>,
    private_key: Option<String>,
    public_key: Option<String>,
    pub identity: Option<Keys>,
}

impl IRCClientData {
    pub fn new() -> Self {
        Self {
            nick: None,
            private_key: None,
            public_key: None,
            identity: None,
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
        let my_identity = Keys::from_str(&*private_key).unwrap();

        self.public_key = Some(my_identity.public_key_as_str());
        self.private_key = Some(private_key);
        self.identity = Some(my_identity);
    }
}
