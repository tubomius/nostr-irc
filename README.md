# nostr-irc
Acts as an IRC server and a nostr client. Connect with your IRC client using your nostr private key as the password.

Experimental code, use with caution.

# Running

- Build and run main, it will listen to 127.0.0.1:6667
- It will send metadata to change your name to the chosen IRC nick when you connect, so use a throwaway private key for now or else metadata may be overwritten because reasons
- Connect with your regular IRC client, use a nostr private key as the password
- Join a channel with /join #channelkey
- Chat away

# IRC-nostr feature mapping

| Supported | IRC                 | nostr           | Description                                                         |
|-----------|---------------------|-----------------|---------------------------------------------------------------------|
| ✅         | NICK                | Metadata        | Sends nostr metadata event when you connect and/or change your nick |
| ✅         | NICK                | Metadata        | Syncs metadata of users seen in channel and renames them if needed  |
| ✅         | PRIVMSG #channelkey | Channel message | Sends nostr event 42:s for channel messages                         |
| ✅         | JOIN #channelkey    | REQ             | Subscribes to channel messages, and gets last 200                   |
| ✅         | JOIN                | REQ             | Sends a JOIN for users that were seen                               |
| ✅         | TOPIC               | Metadata        | Gets topic from channel metadata name                               |
| ❌         | PART #key           | CLOSE           | Closes subscriptions but not informing IRC client yet               |
| ❌         | PRIVMSG userkey     | Encrypted DM?   |                                                                     |
| ❌         | JOIN #string        | Channel create  | Create channel with that name and join #channelkey instead          |

# Features/TODO

✅ Support multiple relays, hard-coded list for now, deduplicates messages and gives a merged stream, writes are sent to all

❌ Publish

❌ Make relay list configurable
