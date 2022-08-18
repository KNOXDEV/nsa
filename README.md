
# nsa
> a discord bot that does nothing
> but store every message sent in
> a database

### progress

- [x] Stage 1 - all messages sent are recorded
  - [ ] Stage 1.5 - including attachments and reactions
- [ ] Stage 2 - channels will be scraped for their historical messages as well
- [ ] Stage 3 - make pretty dashboards and metrics using this data
- [ ] Stage 4 - ???

### how do I add this discord bot to my server?

Well, it's private for obvious reasons.
But you can make your own and follow
[this guide](https://discordjs.guide/preparations/adding-your-bot-to-servers.html#bot-invite-links).

### quick start
```bash
docker compose up
```

### build release images
```bash
docker build . -t nsa --build-arg profile=release
```
