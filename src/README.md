# src/ — the integration code

> The actual scripts and configuration that run the pipeline. Sanitized of organization-specific paths and credentials; structure and logic are unchanged from production.

## Directory layout

```
src/
├── bin/
│   ├── conv-dss-ds2-to-mp3     bash CLI wrapper (the user-facing entry point)
│   ├── audio_cron.sh           the cron worker with 2 modes + anti-doublon
│   └── http_server.mjs         small Node HTTP daemon for the web UI bridge
├── lib/
│   ├── core.mjs                native chain (current default — spawns dss-decode-native + ffmpeg)
│   └── core-wasm.mjs           WASM fallback (the chain we shipped with first)
├── etc/
│   ├── audio-convert.service   systemd unit for the HTTP daemon
│   └── audio_converter.cron    cron entry (put in /etc/cron.d/)
└── web/
    └── convertisseur.php       admin web UI (PHP 5.6 compatible)
```

## Placeholders to replace

Before deploying, replace these with values appropriate to your environment:

| Placeholder | Meaning | Example |
|---|---|---|
| `/srv/AUDIO_ROOT/` | Your audio storage root | `/srv/e_pipeline/` |
| `/opt/conv-dss-ds2-to-mp3/` | Where you install this project | wherever you `git clone` |
| `/var/log/audio_converter.log` | Where the cron logs | adapt to your log directory |
| `DB_HOST` / `DB_USER` / `DB_PASSWORD` / `DB_NAME` | MySQL credentials for the command database | your pipeline DB |
| `OPS_ALERT_EMAIL` | Where conversion failure alerts go | `ops@example.com` |
| `ADMIN_USER_IDS` | List of user IDs allowed to access the admin web UI | adapt to your auth scheme |

A simple `sed` pass can do all of them at once:

```bash
find src/ -type f \( -name '*.sh' -o -name '*.mjs' -o -name '*.php' -o -name '*.service' -o -name '*.cron' \) \
  -exec sed -i \
    -e 's|/srv/AUDIO_ROOT/|/srv/your_root/|g' \
    -e 's|/opt/conv-dss-ds2-to-mp3/|/opt/conv-dss-ds2-to-mp3/|g' \
    -e 's|DB_USER_PLACEHOLDER|your_db_user|g' \
    -e 's|DB_PASSWORD_PLACEHOLDER|your_db_password|g' \
    -e 's|DB_NAME_PLACEHOLDER|your_db_name|g' \
    -e 's|OPS_ALERT_EMAIL|ops@example.com|g' \
    {} +
```

## Installation walkthrough

Detailed in [docs/02-integration.md](../docs/02-integration.md). Short version:

```bash
# 1. Install Node.js 18+ on the host
apt install nodejs

# 2. Install the native binary
#    Either:
#      cargo build --release in https://github.com/gaspardpetit/dss-codec
#    Or:
#      download a pre-built binary from the releases page
cp dss-decode /usr/local/bin/dss-decode-native

# 3. Drop the project
git clone https://github.com/Guillain-RDCDE/ds2-anywhere /opt/conv-dss-ds2-to-mp3
cd /opt/conv-dss-ds2-to-mp3/src

# 4. Run the sed pass above to replace placeholders

# 5. Install the CLI symlink
ln -sf /opt/conv-dss-ds2-to-mp3/src/bin/conv-dss-ds2-to-mp3 /usr/local/bin/conv-dss-ds2-to-mp3
chmod +x bin/*

# 6. Install the systemd unit + start the daemon
cp etc/audio-convert.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now audio-convert

# 7. Install the cron
cp etc/audio_converter.cron /etc/cron.d/audio_converter
chmod 644 /etc/cron.d/audio_converter

# 8. Drop the PHP page (if you want the web UI)
cp web/convertisseur.php /your/admin/web/root/
```

That's the whole thing.

## Reversibility

Every component is undoable in one command:

| To stop | Command |
|---|---|
| The cron | `rm /etc/cron.d/audio_converter` |
| The daemon | `systemctl disable --now audio-convert` |
| The CLI | `rm /usr/local/bin/conv-dss-ds2-to-mp3` |
| Everything | `rm -rf /opt/conv-dss-ds2-to-mp3/` |

No global state is left behind. Drop-in install, drop-out uninstall.
