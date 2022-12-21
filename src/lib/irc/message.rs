use crate::irc::message::IRCMessage::UNKNOWN;

#[derive(Debug, Clone)]
pub enum IRCMessage {
    CAP(String, Option<i32>),
    NICK(String),
    QUIT(String),
    PASS(String),
    JOIN(String),
    PART(String),
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
                "PART" => {
                    Self::PART(
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
