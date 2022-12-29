# nostr-irc
Acts as an IRC server and a nostr client. Connect with your IRC client using your nostr private key as the password.

Experimental code, use with caution.

# Running

- Build and run main, it will listen to 127.0.0.1:6667
- It will send metadata to change your name to the chosen IRC nick when you connect, so use a throwaway private key for now or else metadata may be overwritten because reasons
- Connect with your regular IRC client, use a nostr private key as the password
- Join a channel with /join #channelkey
- Chat away
- Theoretically possible to handle several IRC connections at the same time

# IRC -> nostr feature mapping

|     | IRC                 | nostr           | Description                                                         |
|-----|---------------------|-----------------|---------------------------------------------------------------------|
| ✅   | NICK                | Metadata        | Sends nostr metadata event when you connect and/or change your nick |
| ✅   | PRIVMSG #channelkey | Channel message | Sends nostr event 42:s for channel messages                         |
| ✅   | JOIN #channelkey    | REQ             | Subscribes to channel messages, and gets last 200                   |
| ✅   | PART #key           | CLOSE           | Closes subscriptions but not informing IRC client yet               |
| ✅   | JOIN #string        | Channel create  | Create channel with that name and send JOIN #channelkey instead     |
| ❌   | PRIVMSG userkey     | Encrypted DM?   |                                                                     |

# nostr -> IRC feature mapping

|     | nostr                     | IRC                 | Description                                                        |
|-----|---------------------------|---------------------|--------------------------------------------------------------------|
| ✅   | Author metadata           | NICK                | Syncs metadata of users seen in channel and renames them if needed |
| ✅   | Channel message           | PRIVMSG #channelkey | Sends IRC messages for channel messages                            |
| ✅   | Channel message           | JOIN #channelkey    | If this is the first message you see from this key, sends a JOIN   |
| ✅   | Channel metadata/creation | TOPIC               | Sets topic from channel metadata name                              |
| ❌   | Encrypted DM?             | PRIVMSG userkey     |                                                                    |

# Features/TODO

✅ Support multiple relays, hard-coded list for now, deduplicates messages and gives a merged stream, writes are sent to all

❌ Support NIP-19 with embedded relay recommendations as #channelkey, and add recommended relays

❌ Publish

❌ Make relay list configurable

❌ Dockerize
