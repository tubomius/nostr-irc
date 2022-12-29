use nostr::key::XOnlyPublicKey;

pub enum IRCNickname {
    Nick(String),
    Key(XOnlyPublicKey),
}

impl IRCNickname {
    pub fn get_displayed_name(&self) -> String {
        match self {
            IRCNickname::Nick(s) => s.clone(),
            IRCNickname::Key(s) => s.to_string(),
        }
    }
}
