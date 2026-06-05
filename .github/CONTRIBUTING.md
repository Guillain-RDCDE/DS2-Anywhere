# Contributing to DS2-Anywhere

Glad you're here. This project is small but real (running in production daily). Contributions are welcome — bug fixes, doc improvements, new install targets, adaptations to other pipeline shapes.

## Before you start

- **Issues** : check open issues first. If your idea/bug isn't there, open one before sending a PR so we can align early.
- **Big changes** : if your contribution touches the conversion core, the cron, or the daemon, please open a discussion-style issue first. Small fixes can go straight to PR.

## Local setup

```bash
git clone https://github.com/Guillain-RDCDE/DS2-Anywhere
cd DS2-Anywhere
# Lint everything (matches what CI runs)
shellcheck src/bin/audio_cron.sh src/bin/conv-dss-ds2-to-mp3
node --check src/lib/core.mjs src/lib/core-wasm.mjs src/bin/http_server.mjs
docker run --rm -v "$PWD:/app" -w /app php:5.6-cli php -l src/web/convertisseur.php
```

For a quick end-to-end test:

```bash
docker compose up
# Web UI at http://localhost:8080
# HTTP daemon at http://localhost:8765/health
```

## Code style

- **Bash** : passes `shellcheck` clean (CI enforces). Use `set -u`, quote variables, `local` inside functions.
- **JavaScript / Node** : ES modules, no transpilation. Plain `node` runs the code as-is. No bundler, no minifier.
- **PHP** : **must remain PHP 5.6 compatible** (no `??`, `<=>`, typed properties, etc.). Use `isset(...) ? ... : default`.
- **Comments** : explain *why*, not *what*. The code already says what.
- **Languages** : code comments in English, documentation in English. The original team is French-speaking but the project is for everyone.

## What's in / out of scope

**In scope** :
- Fixes for the conversion pipeline (CLI, cron, daemon, web UI).
- New adapters for different pipeline shapes (different DB schemas, no-DB mode, queue-based, etc.).
- Better docs, more examples, more languages.
- Better install / deploy automation (Ansible role, Helm chart, etc.).
- Performance improvements for the wrapping (not the codec itself — that's upstream).

**Out of scope** (please go upstream) :
- Changes to the DS2 / DSS codec logic → [hirparak/dss-codec](https://github.com/hirparak/dss-codec)
- WASM build improvements → [gaspardpetit/dss-codec-wasm](https://github.com/gaspardpetit/dss-codec-wasm)
- MP3 encoder bugs → ffmpeg / lamejs upstream

## Commit messages

Conventional-ish, kept simple. First line is a summary < 72 chars. Body explains the *why* if non-trivial.

```
Add --no-db mode to audio_cron for users without a command DB

Some adopters don't have an email_messages-equivalent schema. The
--no-db flag (or USE_DB=0 in config) skips the SQL scan in mode 2
and instead picks up everything in a configurable input directory.
```

## Pull requests

- Target `main`.
- One topic per PR. Don't bundle unrelated fixes.
- Update docs if your change is user-visible.
- Update `docs/benchmarks/conversion-results.json` only if you actually re-ran the validation campaign (no synthetic numbers — see the [validation chapter](../docs/03-validation-campaign.md) for the philosophy here).
- The CI must pass. If you can't make it pass locally, push anyway and we'll help.

## Credit

Contributors are listed in release notes. Material contributions earn a mention in `CREDITS.md` if you want one.
