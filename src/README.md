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

### Recommended: the interactive installer

```bash
sudo ./src/bin/install.sh
```

Asks the questions, generates `/etc/conv-dss-ds2-to-mp3/audio-cron.conf`, installs systemd unit + cron + CLI symlink. Idempotent — safe to re-run if you want to change a value.

### Manual install (if you prefer)

Detailed in [docs/02-integration.md](../docs/02-integration.md). Short version:

```bash
# 1. Prerequisites
apt install nodejs ffmpeg mariadb-client

# 2. Install the native binary
#    Either:  cargo build --release in https://github.com/gaspardpetit/dss-codec
#    Or:      download from the project's GitHub Releases
cp dss-decode /usr/local/bin/dss-decode-native
chmod +x /usr/local/bin/dss-decode-native

# 3. Drop the project
git clone https://github.com/Guillain-RDCDE/DS2-Anywhere /opt/conv-dss-ds2-to-mp3

# 4. Create a config
sudo mkdir -p /etc/conv-dss-ds2-to-mp3
sudo cp /opt/conv-dss-ds2-to-mp3/src/etc/audio-cron.conf.example /etc/conv-dss-ds2-to-mp3/audio-cron.conf
sudo nano /etc/conv-dss-ds2-to-mp3/audio-cron.conf   # edit the placeholders

# 5. CLI symlink + perms
sudo ln -sf /opt/conv-dss-ds2-to-mp3/src/bin/conv-dss-ds2-to-mp3 /usr/local/bin/
sudo chmod +x /opt/conv-dss-ds2-to-mp3/src/bin/*

# 6. systemd + cron
sudo cp /opt/conv-dss-ds2-to-mp3/src/etc/audio-convert.service /etc/systemd/system/
sudo cp /opt/conv-dss-ds2-to-mp3/src/etc/audio_converter.cron /etc/cron.d/audio_converter
sudo systemctl daemon-reload && sudo systemctl enable --now audio-convert

# 7. (optional) drop the PHP page in your admin area
cp /opt/conv-dss-ds2-to-mp3/src/web/convertisseur.php /your/admin/web/root/
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
