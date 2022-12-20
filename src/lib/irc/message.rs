use nostr::{ClientMessage, Event, EventBuilder, Kind, KindBase, Metadata, SubscriptionFilter, Tag};
use nostr::event::{TagData, TagKind};
use tokio::sync::mpsc::UnboundedSender;
use crate::irc::client_data::ClientDataHolder;
use crate::irc::message::IRCMessage::UNKNOWN;

#[derive(Debug, Clone)]
pub enum IRCMessage {
    CAP(String, Option<i32>),
    NICK(String),
    QUIT(String),
    PASS(String),
    JOIN(String),
    LIST(),
    PRIVMSG(String, String),
    USER(String, String, String, String),
    UNKNOWN(Vec<String>),
}

impl IRCMessage {
    pub fn from_string(s: String) -> Self {
        let parts = get_parts(s);

        if parts.len() == 0 {
            return UNKNOWN(vec![]);
        }

        if let Some(first) = parts.get(0) {
            match &first[..] {
                "CAP" => {
                    Self::CAP(
                        parts.get(1).unwrap().to_string(),
                        parts.get(2).map_or(None, |s| s.parse::<i32>().ok()),
                    )
                }
                "NICK" => {
                    Self::NICK(
                        parts.get(1).unwrap().to_string(),
                    )
                }
                "LIST" => {
                    Self::LIST()
                }
                "QUIT" => {
                    Self::QUIT(
                        parts.get(1).unwrap().to_string(),
                    )
                }
                "JOIN" => {
                    Self::JOIN(
                        parts.get(1).unwrap().to_string(),
                    )
                }
                "PASS" => {
                    Self::PASS(
                        parts.get(1).unwrap().to_string(),
                    )
                }
                "PRIVMSG" => {
                    Self::PRIVMSG(
                        parts.get(1).unwrap().to_string(),
                        parts.get(2).unwrap().to_string(),
                    )
                }
                "USER" => {
                    Self::USER(
                        parts.get(1).unwrap().to_string(),
                        parts.get(2).unwrap().to_string(),
                        parts.get(3).unwrap().to_string(),
                        parts.get(4).unwrap().to_string(),
                    )
                }
                _ => UNKNOWN(parts.into_iter().map(|s| s.to_string()).collect()),
            }
        }  else {
            UNKNOWN(parts.into_iter().map(|s| s.to_string()).collect())
        }
    }

    pub async fn handle_message(&self, client_data: &ClientDataHolder, nostr_tx: &UnboundedSender<ClientMessage>) -> Option<Option<String>> {
        match self {
            Self::CAP(s, _) => {
                if s == "LS" {
                    Some(Some(format!("CAP * LS")))
                } else {
                    let nick = client_data.read().await.get_nick();
                    let private_key = client_data.read().await.get_private_key();

                    if !private_key.is_none() {
                        if let Some(nick) = nick {
                            return Some(Some(format!("001 {nick} :Welcome to the Internet Relay Network")))
                        }
                    }

                    Some(Some(format!("ERROR :No nick or no private key, set password to your private key")))
                }
            },
            Self::QUIT(_) => {
                Some(Some(format!("QUIT")))
            }
            Self::LIST() => {
                let channel_list = ClientMessage::new_req(
                    format!("list"),
                    vec![
                        SubscriptionFilter::new()
                            .kind(Kind::Base(KindBase::ChannelCreation))
                    ],
                );

                nostr_tx.send(channel_list).ok();

                Some(None)
            }
            Self::JOIN(channel) => {
                let channels = channel.split(",");

                for channel in channels {
                    let channel = channel.split_once("#").unwrap().1;

                    let channel_info = ClientMessage::new_req(
                        format!("{channel}-info"),
                        vec![SubscriptionFilter::new().id(channel)],
                    );

                    nostr_tx.send(channel_info).ok();

                    let channel_messages = ClientMessage::new_req(
                        format!("{channel}-messages"),
                        vec![SubscriptionFilter::new().kind(Kind::Base(KindBase::ChannelMessage)).limit(200).event(channel.parse().unwrap())],
                    );

                    nostr_tx.send(channel_messages).ok();
                }

                Some(None)
            }
            Self::PRIVMSG(channel, message) => {
                let my_keys = client_data.read().await.identity.as_ref().unwrap().clone();

                let channel = channel.split_once("#").unwrap().1;

                let event: Event = EventBuilder::new(
                    Kind::Base(KindBase::ChannelMessage),
                    message,
                    &[Tag::new(TagData::Generic(
                        TagKind::E,
                        vec![channel.to_string()],
                    ))],
                ).to_event(&my_keys).unwrap();

                let msg = ClientMessage::new_event(event);
                nostr_tx.send(msg).ok();

                Some(None)
            }
            Self::PASS(s) => {
                client_data.write().await.set_private_key(s.clone());

                Some(None)
            }
            Self::NICK(s) => {
                let old_nick = client_data.read().await.get_nick().clone();

                client_data.write().await.set_nick(s.clone());

                if let Some(old_nick) = old_nick {
                    let metadata = Metadata::new()
                        .name(s)
                        .display_name(s);

                    let my_keys = client_data.read().await.identity.as_ref().unwrap().clone();

                    let event: Event = EventBuilder::set_metadata(&my_keys, metadata).unwrap().to_event(&my_keys).unwrap();

                    let msg = ClientMessage::new_event(event);
                    nostr_tx.send(msg).ok();

                    Some(Some(format!(":{old_nick} NICK {s}")))
                } else {
                    Some(None)
                }
            }
            Self::USER(_, _, _, _) => {
                let metadata = Metadata::new()
                    .name(client_data.read().await.get_nick().unwrap())
                    .display_name(client_data.read().await.get_nick().unwrap())
                    .about("description wat");

                let my_keys = client_data.read().await.identity.as_ref().unwrap().clone();

                let event: Event = EventBuilder::set_metadata(&my_keys, metadata).unwrap().to_event(&my_keys).unwrap();

                let msg = ClientMessage::new_event(event);
                nostr_tx.send(msg).ok();

                Some(None)
            }
            _ => Some(None),
        }
    }
}

fn get_parts(s: String) -> Vec<String> {
    let parts = s.split(" ");
    let mut final_parts = vec![];

    let mut last_started = false;
    let mut last = String::from("");
    for p in parts {
        if p.starts_with(":") {
            last_started = true;
            last = p.split_once(":").unwrap().1.to_string();
        } else if last_started {
            last = format!("{last} {p}");
        } else {
            final_parts.push(p.to_string());
        }
    }

    if last_started {
        final_parts.push(last.to_string());
    }

    final_parts
}
