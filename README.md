# nostr-irc
Acts as an IRC server and a nostr client. Connect with your IRC client using your nostr private key as the password.

Experimental code, use with caution.

# Running

- Build and run main, it will listen to 127.0.0.1:6667
- Connect with your regular IRC client, use a nostr private key as the password
- Join a channel with /join #<pubkey>
- Chat away

# Features

| Supported |  IRC              | nostr           | Description                                                           |
|-----------|-------------------|-----------------|-----------------------------------------------------------------------|
| ✅         | NICK              | Metadata        | Sends nostr metadata event when you connect and/or change your nick   |
| ✅         | PRIVMSG #channel  | Channel message | Sends nostr event 42:s for channel messages                           |
| ✅         | JOIN #channel     | Subscribe       | Subscribes to channel messages, and gets last 200                     |
| ❌         | PART #channel     | Unsubscribe     |                                                                       |
| ❌         | PRIVMSG nick      | Encrypted DM?   |                                                                       |
|-----------|-------------------|-----------------|-----------------------------------------------------------------------|
