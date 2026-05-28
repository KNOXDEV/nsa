
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

### quick start (local dev)

Uses a Nix devshell with a throwaway local Postgres managed by process-compose:

```bash
nix develop                # rust toolchain + postgres + process-compose
process-compose            # starts postgres (init + create db on first run)
# in a second `nix develop` shell:
DISCORD_TOKEN=... cargo run
```

Quitting process-compose tears the database down — no stale cluster. Postgres
listens on a project-local unix socket (`.pgdata/`), so there are no port
conflicts and no TCP exposure.

### deploy

The bot ships as a flake package (`nix build`) and a NixOS module
(`nixosModules.default`, see `module.nix`).

```bash
nix build                  # build the binary
```
