use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::time::{Duration, Instant};
use nostr::{ClientMessage, RelayMessage};
use nostr::url::Url;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;
use tokio_stream::{Stream, StreamExt, StreamMap};
use crate::nostr::relay_connection::NostrRelayConnection;
use crate::nostr::subscription::NostrSubscription;

pub struct NostrClient {
    relays: Vec<Url>,
    subscriptions: HashMap<String, NostrSubscription>,
    seen_ids: HashSet<String>,
}

impl NostrClient {
    pub fn new(relays: Vec<String>) -> Self {
        Self {
            relays: relays.into_iter().map(|r| r.parse().unwrap()).collect(),
            subscriptions: HashMap::new(),
            seen_ids: HashSet::new(),
        }
    }

    pub async fn run(&mut self, tx: UnboundedSender<(Url, RelayMessage)>, mut rx: UnboundedReceiver<ClientMessage>) -> Option<()> {
        let mut client_messages_txs = HashMap::new();
        let mut relay_messages_rxs = StreamMap::new();

        let mut set = JoinSet::new();

        for relay_url in self.relays.iter() {
            let relay_url = relay_url.clone();

            let (relay_messages_tx, mut relay_messages_rx) = mpsc::unbounded_channel();
            let (client_messages_tx, client_messages_rx) = mpsc::unbounded_channel();

            client_messages_txs.insert(relay_url.clone(), client_messages_tx);
            relay_messages_rxs.insert(relay_url.clone(), Box::pin(async_stream::stream! {
                  while let Some(item) = relay_messages_rx.recv().await {
                      yield item;
                  }
            }) as Pin<Box<dyn Stream<Item = RelayMessage> + Send>>);

            set.spawn(async move {
                let mut relay_connection = NostrRelayConnection::new();
                match relay_connection.connect(relay_url, relay_messages_tx, client_messages_rx).await {
                    Ok(_) => {}
                    Err(_) => {}
                }
            });
        }

        let mut checker = tokio::time::interval(Duration::from_millis(500));

        loop {
            tokio::select!(
                Some((relay_url, message)) = relay_messages_rxs.next() => {
                    self.handle_relay_message(relay_url, message, &tx).await;
                }
                Some(message) = rx.recv() => {
                    self.handle_client_message(message, &client_messages_txs).await;
                }
                Some(_) = set.join_next() => {
                    // nothing
                }
                _ = checker.tick() => {
                    self.check_subscriptions(&tx).await;
                }
            );
        }
    }

    async fn check_subscriptions(&mut self, tx: &UnboundedSender<(Url, RelayMessage)>) -> Option<()> {
        let now = Instant::now();
        let max_age = Duration::from_secs(5);

        for (_, sub) in self.subscriptions.iter_mut() {
            if sub.done {
                sub.data.clear();

                continue;
            }

            if now - sub.started > max_age {
                sub.done = true;

                // println!("subscription initial events ended: {}", sub.name);

                let mut messages = sub.data.drain().map(|(_, y)| y).collect::<Vec<_>>();

                messages.sort_by(|a, b| {
                    if let RelayMessage::Event { event: a, .. } = a {
                        if let RelayMessage::Event { event: b, .. } = b {
                            return a.created_at.cmp(&b.created_at);
                        }
                    }

                    Ordering::Equal
                });

                if sub.responded_relays.len() > 0 {
                    let first_relay = sub.responded_relays.get(0).unwrap().clone();
                    for msg in messages.into_iter() {
                        let mut message_id = None;

                        match &msg {
                            RelayMessage::Event { event, .. } => message_id = Some(event.id.clone()),
                            _ => {}
                        }

                        if let Some(message_id) = message_id {
                            let message_id = message_id.to_string();
                            let was_seen = self.seen_ids.contains(&message_id);

                            if was_seen {
                                continue;
                            }

                            self.seen_ids.insert(message_id);
                        }

                        tx.send((first_relay.clone(), msg)).ok();
                    }
                }
                if sub.end_of_stored_events_message.is_some() {
                    let first_relay = sub.responded_relays.get(0).unwrap().clone();
                    tx.send((first_relay, sub.end_of_stored_events_message.take().unwrap())).ok();
                } else {
                    tx.send((self.relays.get(0).unwrap().clone(), RelayMessage::new_eose(sub.name.clone()))).ok();
                }
            }
        }

        Some(())
    }

    async fn handle_relay_message(&mut self, relay_url: Url, message: RelayMessage, tx: &UnboundedSender<(Url, RelayMessage)>) -> Option<()> {
        // println!("NostrClient: handle_relay_message: {message:?}");

        let mut subscription = None;
        let mut message_id = None;
        let mut sub_ended = false;

        match &message {
            RelayMessage::Event { subscription_id, event } => {
                message_id = Some(event.id.clone());
                subscription = Some(subscription_id.clone())
            },
            RelayMessage::Notice { .. } => {}
            RelayMessage::EndOfStoredEvents { subscription_id } => {
                subscription = Some(subscription_id.clone());
                sub_ended = true;
            },
            RelayMessage::Ok { .. } => {}
            RelayMessage::Empty => {}
        }

        if let Some(subscription_id) = &subscription {
            let sub = self.subscriptions.get_mut(subscription_id);

            if let Some(sub) = sub {
                if sub_ended {
                    sub.end_of_stored_events_message = Some(message);

                    sub.responded_relays.push(relay_url.clone());

                    return Some(());
                } else {
                    let already_ended = sub.responded_relays.contains(&relay_url);

                    if !already_ended {
                        let message_id = message_id.unwrap();
                        sub.data.insert(message_id, message);

                        return Some(());
                    }
                }
            }
        }

        if let Some(message_id) = message_id {
            let message_id = message_id.to_string();
            let was_seen = self.seen_ids.contains(&message_id);

            if was_seen {
                return Some(());
            }

            self.seen_ids.insert(message_id);
        }

        if sub_ended {
            return Some(());
        }

        tx.send((relay_url, message)).ok()
    }

    async fn handle_client_message(&mut self, message: ClientMessage, client_messages_txs: &HashMap<Url, UnboundedSender<String>>) -> Option<()> {
        // println!("NostrClient: handle_client_message: {message:?}");

        let mut subscription = None;

        match &message {
            ClientMessage::Event { .. } => {}
            ClientMessage::Req { subscription_id, .. } => subscription = Some(subscription_id.clone()),
            ClientMessage::Close { .. } => {}
        }

        let json = message.to_json();

        if let Some(subscription_id) = &subscription {
            if !self.subscriptions.contains_key(subscription_id) {
                // println!("started subscription: {subscription_id}");

                self.subscriptions.insert(subscription_id.clone(), NostrSubscription::new(subscription_id.clone()));

                // Send to all relays
                for (relay, tx) in client_messages_txs.iter() {
                    if let Some(subscription_id) = &subscription {
                        let sub = self.subscriptions.get_mut(subscription_id).unwrap();
                        sub.asked_relays.push(relay.clone());
                    }

                    match tx.send(json.clone()) {
                        Ok(_) => {}
                        Err(_) => {}
                    }
                }
            }
        } else {
            // Probably write, send to all relays
            for (_, tx) in client_messages_txs.iter() {
                tx.send(json.clone()).ok();
            }
        }

        Some(())
    }
}
