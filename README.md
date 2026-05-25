# DS2-Anywhere

> Decode Olympus DSS/DS2 dictation files anywhere — pure CLI, no Windows, no GUI, no commercial software. A production integration recipe for a format that was locked for ten years. 🔓

[![CI](https://github.com/Guillain-RDCDE/DS2-Anywhere/actions/workflows/ci.yml/badge.svg)](https://github.com/Guillain-RDCDE/DS2-Anywhere/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE) [![Latest release](https://img.shields.io/github/v/release/Guillain-RDCDE/DS2-Anywhere)](https://github.com/Guillain-RDCDE/DS2-Anywhere/releases) ![Status](https://img.shields.io/badge/status-production-green) ![Platform](https://img.shields.io/badge/platform-linux-blue)

## Try it in 30 seconds

```bash
git clone https://github.com/Guillain-RDCDE/DS2-Anywhere
cd DS2-Anywhere
docker compose up --build
# Web UI: http://localhost:8080/convertisseur.php
# HTTP API: http://localhost:8765/health
```

Drop a `.ds2` or `.dss` into `examples/` (the folder is mounted into both containers), then go to the web UI and convert.

## Production install

```bash
sudo ./src/bin/install.sh
```

Interactive — asks the few questions that matter (install dir, audio root, DB creds if you use mode 2, alert email), generates a config, installs the cron + systemd unit + CLI symlink. Idempotent.

---

## The ten-year lock

In 2007, Olympus released the Digital Speech Standard 2 (`.ds2`) format for their professional dictation recorders. Proprietary codec, no public spec, no open decoder. Anyone who needed to process `.ds2` files on Linux or macOS had exactly one path: run Olympus DSS Player or NCH Switch through a Windows VM.

In 2017, [FFmpeg ticket #6091](https://trac.ffmpeg.org/ticket/6091) was opened: *"Add DS2 codec support"*. It sat unimplemented for **nine years**.

In **February 2026**, Kieran Hirpara published [hirparak/dss-codec](https://github.com/hirparak/dss-codec) — the first open-source DS2 decoder, reverse-engineered from the Olympus DLLs using Ghidra, and verified byte-for-byte against the official Olympus DirectShow filter.

Three months later, this repo shows how to take that work and put it in production: replacing a fragile Windows VM + commercial GUI software with **a bash wrapper, a cron, and ~150 lines of glue**.

## What's in this repo

- 📖 **[docs/](docs/)** — a long-form, didactic walkthrough of:
  - **(1)** how the codec was reverse-engineered (the genius part — not ours);
  - **(2)** how we integrated it into a production transcription pipeline that processes real-world dictations daily (the engineering part — ours).
- 🛠 **[src/](src/)** — the actual integration code: CLI, cron job, HTTP daemon, admin web UI. Sanitized of organization-specific bits; the patterns are reusable as-is.
- 📊 **[benchmarks/](benchmarks/)** — performance comparison (WASM vs native, the chain we use vs the commercial Windows chain), and the validation campaign run on 35 real-world files.

## Pipeline at a glance

```
                   BEFORE                                            AFTER
                                                                    
   .ds2 ─► SSHFS ─► Windows VM ─► Switch.exe                 .ds2 ─► cron (Linux) 
                       │                                              │
                       ▼                                              ▼
                  .wav (mono)                                   .mp3 (mono 64k)
                       │                                              │
                       ▼                                              ▼
                  SSHFS back                                  Whisper API
                       │                                              │
                       ▼                                              ▼
                  glue script                              ready for transcription
                       │                                              
                       ▼                                              
                  Whisper API                                         

   GUI app + Windows VM + SSHFS round-trip      bash + native binary, all local, ~10s/file
```

## Quick start (CLI)

Convert one file:

```bash
conv-dss-ds2-to-mp3 recording.ds2
# [ds2_qp 16000Hz, 31.8min] recording.mp3  OK  (14.55 Mo en 10.3 s)
```

Inspect a file without decoding:

```bash
conv-dss-ds2-to-mp3 --inspect recording.ds2
# format      : ds2_qp
# chiffrement : none
# freq. nat.  : 16000 Hz
# taille      : 6754304 octets
```

Encrypted DS2 with password:

```bash
conv-dss-ds2-to-mp3 --password=mypwd recording.ds2
```

Full install + cron + web UI setup: [docs/02-integration.md](docs/02-integration.md).

## Real-world numbers

The decision to ship was based on **an A/B against the reference Windows implementation on the same source file**, not just a count of successful decodes. The full validation, in order of weight:

1. **A/B vs Switch.exe** (same `.ds2`, both chains, both MP3s through the same Whisper API): transcripts are **functionally identical**. Switch.exe: 16.2 % low-confidence words. Our chain: 17.2 %. Within Whisper's own run-to-run variance. The two chains are interchangeable for any downstream pipeline.
2. **Sample**: 35 real production dictations (32× DS2 QP + 3× DSS SP, 6 h 48 of audio total). **35 / 35** decoded successfully, zero failures. Sample is intentionally tight — DS2 files don't survive long in our pipeline (raw uploads archived after ~2 weeks), and the A/B against the reference was what carried the call, not the headcount.
3. **Production**: ~3 200 cron passes since go-live, zero errors logged. Every new DS2 entering the system now goes through this chain. The Switch VM stays on standby, untouched.

[Full methodology and results →](docs/03-validation-campaign.md)

## The story, in three reads

If you have…

- **5 minutes** → just read this README.
- **20 minutes** → [docs/01-reverse-engineering.md](docs/01-reverse-engineering.md) — the genius part.
- **60 minutes** → all of [docs/](docs/), in order. From "impossible for ten years" to "production in a weekend".

## Credits — proper order

The intellectual heavy-lifting belongs entirely to:

- **Kieran Hirpara** — [hirparak/dss-codec](https://github.com/hirparak/dss-codec) — the reverse-engineering that made all of this possible. MIT, February 2026.
- **Gaspard Petit** — [gaspardpetit/dss-codec-wasm](https://github.com/gaspardpetit/dss-codec-wasm) — the WASM build and npm packaging on top. MIT.
- **lamejs** ([breezystack fork](https://github.com/breezystack/lamejs)) — pure-JS MP3 encoder. LGPL.
- **[FFmpeg](https://ffmpeg.org/)** — the encoder we use in the native chain. LGPL.

This repo is a recipe. The recipe needs the ingredients above to exist at all.

## License

MIT. Same as the upstream codec. Fork, adapt, deploy, integrate — only please keep proper attribution to the codec authors.

---

*A decade of impossible, one bash command later. 🔓*
