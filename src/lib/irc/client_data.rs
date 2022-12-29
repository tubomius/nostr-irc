use std::str::FromStr;
use nostr::key::{SecretKey, XOnlyPublicKey};
use nostr::Keys;

pub struct IRCClientData {
    pub private_key: Option<SecretKey>,
    pub public_key: Option<XOnlyPublicKey>,
    pub identity: Option<Keys>,
}

impl IRCClientData {
    pub fn new() -> Self {
        Self {
            private_key: None,
            public_key: None,
            identity: None,
        }
    }

    pub fn get_public_key(&self) -> Option<XOnlyPublicKey> {
        return self.public_key.clone();
    }

    pub fn get_private_key(&self) -> Option<SecretKey> {
        return self.private_key.clone();
    }

    pub fn set_private_key(&mut self, private_key: String) {
        let my_identity = Keys::from_str(&*private_key).unwrap();

        self.public_key = Some(my_identity.public_key());
        self.private_key = Some(my_identity.secret_key().as_ref().unwrap().clone());
        self.identity = Some(my_identity);
    }
}
