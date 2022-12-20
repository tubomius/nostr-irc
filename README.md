# nostr-irc
Acts as an IRC server and a nostr client. Connect with your IRC client using your nostr private key as the password.

Experimental code, use with caution.

# Running

- Build and run main, it will listen to 127.0.0.1:6667
- Connect with your regular IRC client, use a nostr private key as the password
- Join a channel with /join #pubkey
- Chat away

# IRC-nostr feature mapping

| Supported |  IRC              | nostr           | Description                                                           |
|-----------|-------------------|-----------------|-----------------------------------------------------------------------|
| ✅         | NICK              | Metadata        | Sends nostr metadata event when you connect and/or change your nick   |
| ✅         | PRIVMSG #channel  | Channel message | Sends nostr event 42:s for channel messages                           |
| ✅         | JOIN #channel     | REQ             | Subscribes to channel messages, and gets last 200                     |
| ❌         | JOIN #user        | REQ             | Should this follow as well?                                           |
| ❌         | JOIN #thread      | REQ             |                                                                       |
| ❌         | PART #channel     | CLOSE           |                                                                       |
| ❌         | PRIVMSG user      | Encrypted DM?   |                                                                       |
| ❌         | PRIVMSG #user     | Regular DM?     |                                                                       |
| ❌         | PRIVMSG #thread   | Reply           |                                                                       |
| ❌         | TOPIC             | Metadata        | Indicate type and name of chat, i e if it's a group/thread/user       |

# Features

- Support multiple relays
- Make relay list configurable
