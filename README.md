# DS2-Anywhere

![DS2-Anywhere — decode Olympus DSS / DS2 dictation files on Linux, pure CLI, no GUI](docs/assets/social-preview.png)

> Open the dictation formats that stayed locked for thirty years — Olympus **DS2/DSS**
> and now **Grundig DSS** — on any Linux box. No Windows, no GUI, no commercial
> software. A production recipe *and* the reverse-engineering trail behind it. 🔓

[![CI](https://github.com/Guillain-RDCDE/DS2-Anywhere/actions/workflows/ci.yml/badge.svg)](https://github.com/Guillain-RDCDE/DS2-Anywhere/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE) [![Latest release](https://img.shields.io/github/v/release/Guillain-RDCDE/DS2-Anywhere)](https://github.com/Guillain-RDCDE/DS2-Anywhere/releases) ![Status](https://img.shields.io/badge/status-production-green) ![FFmpeg](https://img.shields.io/badge/FFmpeg-patches%20in%20review-orange) ![Platform](https://img.shields.io/badge/platform-linux-blue)

**In one sentence:** doctors, lawyers and police dictate into small voice recorders;
the file format those recorders produce was kept secret for thirty years; this project
opens it on any Linux machine — and gives the fix back to the open-source tools everyone
uses.

---

A handful of strangers who never met picked a thirty-year-old lock — a proprietary
voice codec that doctors, lawyers and police dictated billions of seconds into, and
that no open tool on Earth could read. One person reverse-engineered the first piece.
Others made it portable, then universal. We put it in production over a weekend — and
then a lawyer in Germany dug an old recorder out of a drawer and handed us a codec
**even the commercial software couldn't decode.** So we cracked that one too, in an
afternoon, by interrogating the manufacturer's own decoder inside a debugger we built
from its DLLs.

All of it is in here: the working code, and exactly how it was done.

## Contents

- [New here? Three words in plain English](#new-here-three-words-in-plain-english)
- [Pick your way in](#pick-your-way-in) — four doors, choose your depth
- [Try it in 30 seconds](#try-it-in-30-seconds)
- [What it does, in one picture](#what-it-does-in-one-picture)
- [The technical trail](#the-technical-trail) — the story, chapter by chapter
- [Where it stands now](#where-it-stands-now) — giving it back to FFmpeg & preservation
- [Real-world numbers](#real-world-numbers)
- [What's in this repo](#whats-in-this-repo)
- [Credits](#credits--proper-order) · [License](#license)

## New here? Three words in plain English

You don't need any audio or programming background to follow this repo. Three terms cover most of it:

- **Codec** — the secret "recipe" that squeezes a voice recording into a tiny file, and rebuilds it on playback. Without the recipe, the file is just unreadable noise.
- **Decode** (and **demux**) — turning that tiny file back into sound. *Demux* is the first step (split the file into the right little chunks, called *frames*); *decode* is the second (turn frames into audio). Most of our hardest bugs were in the *demux* step — getting the chunks lined up.
- **Reverse-engineering** — working out the secret recipe yourself, by careful observation, because the manufacturer never published it.

That's it. Everything below builds gently from these.

## Pick your way in

| | |
|---|---|
| 📖 **Read it like a thriller** | **[The Story →](docs/THE-STORY.md)** — no code, ~10 minutes. A locked format, a relay of strangers, a bug that turned out to be a human being, and a German lawyer's drawer. It really happened, and every twist links to the chapter that proves it. |
| 🔧 **Follow the technical trail** | **[Go deeper ↓](#the-technical-trail)** — from "what even *is* a `.ds2` file" up to running a closed-source decoder under a debugger. Built to be readable if you've never reverse-engineered anything. |
| 🌐 **Decode one right now** | **[Open the in-browser decoder →](https://guillain-rdcde.github.io/DS2-Anywhere/)** — drop a `.ds2`/`.dss` (Olympus, Grundig, even encrypted) and get audio back. Nothing uploaded, nothing installed. |
| 🛠 **Run it yourself** | **[30 seconds ↓](#try-it-in-30-seconds)** — drop a file in, get an MP3 out. |

---

## Try it in 30 seconds

```bash
git clone https://github.com/Guillain-RDCDE/DS2-Anywhere
cd DS2-Anywhere
docker compose up --build
# Web UI: http://localhost:8080/convertisseur.php
# HTTP API: http://localhost:8765/health
```

Drop a `.ds2` or `.dss` into `examples/` and convert it from the web UI. Or one file from the CLI:

```bash
conv-dss-ds2-to-mp3 recording.ds2
# [ds2_qp 16000Hz, 31.8min] recording.mp3  OK  (14.55 Mo en 10.3 s)
```

Production install (config + cron + systemd + web UI): `sudo ./src/bin/install.sh`, or [docs/02-integration.md](docs/02-integration.md).

## What it does, in one picture

The whole point in one diagram: a Windows VM running commercial software, replaced by a small local binary.

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

   GUI app + Windows VM + SSHFS round-trip      bash + native binary, all local, ~10s/file
```

A Windows VM with commercial software in the loop, replaced by a bash wrapper, a cron, and a native binary — all local, ~10 s per file.

---

## The technical trail

**Start from zero.** A `.dss`/`.ds2` file is a voice recording squeezed *tiny* by a
secret algorithm (see [the three words above](#new-here-three-words-in-plain-english)).
"Decoding" it means rebuilding the original sound — and to do that you need the
algorithm, which the manufacturers never published. The chapters below are the story of
getting it anyway. Each one starts from the ground; you can stop at any rung.

1. **[The thirty-year lock](docs/01-reverse-engineering.md)** — what a DS2 file is, why it
   resisted, and how Kieran Hirpara reverse-engineered the codec from the Olympus DLLs
   (the genius part — not ours).
2. **[Putting it in production](docs/02-integration.md)** — turning a decoder into a
   real pipeline: CLI, cron, daemon, the encode chain. The engineering part.
3. **[The empty-block bug](docs/06-the-empty-block-bug.md)** — a decoder that was
   bit-exact on every file we tested and *still* wrong on paused recordings. Ten dead
   ends, a twelve-line fix. A detective story worth reading even if you never touch DS2.
4. **[Cracking the re-sync block](docs/07-cracking-the-resync-block.md)** — the sequel:
   we ran the closed-source Olympus decoder *inside a debugger we built from its own
   DLLs*, and read the format's last undocumented rule straight off the silicon.
5. **[The re-sync block, again — and into FFmpeg](docs/13-the-sp-resync-block.md)** — the
   same trick a second time, on Olympus's *other* format (DSS SP). We re-hosted the
   vendor's decoder, read the rule off the live parser, fixed it in one branch — and this
   time **sent the fix to FFmpeg itself** (see [where it stands](#where-it-stands-now)).
6. **[The bug that wasn't](docs/10-the-reckoning-the-bug-that-wasnt.md)** — the twist,
   and the chapter we're proudest of. A rigorous case for a "last bug" ([the research
   paper](docs/09-the-resync-excitation-anomaly.md)), then *overturned* — there was no
   bug; it was a person stepping away from the mic. How careful work can be confidently
   wrong, and how to catch it.
7. **[Cracking the Grundig SP codec](docs/12-cracking-the-grundig-sp-codec.md)** — the
   finale. The Grundig grandfather format that *nobody* decoded — not us, not FFmpeg,
   not even Olympus's own software. We extracted Grundig's decoder, ran it under a
   debugger, patched out the instruction it used to delete its own evidence, and
   rebuilt the codec **bit-exact**. Now a [native Python decoder](grundig/) and an
   [FFmpeg patch](ffmpeg-upstream/patches/avcodec-grundig_sp-decoder.patch).

> Short on time? **5 min** → this page · **20 min** → chapter 1 · **30 min** → the two
> detective stories (3 & 4) · **the twist** → 6 · **the finale** → 7 · **everything** →
> [docs/](docs/) in order, "impossible for thirty years" to "production in a weekend."

## Where it stands now

The work didn't stop at our own servers — it's being handed back to the tools everyone else uses:

- **Into FFmpeg.** FFmpeg is the audio/video engine inside VLC, Chrome, OBS and much of
  the internet. The Olympus DS2 decoder + demuxer and the DSS-SP paused-recording fix
  have been **submitted to the `ffmpeg-devel` mailing list and are in review**; the
  Grundig SP decoder patch is staged behind them. Once merged, *every* program built on
  FFmpeg reads these files for free, forever — no recipe required.
- **A public specification.** [The first one ever written](docs/SPEC-grundig-dss-sp.md)
  for the Grundig DSS-SP codec, bit-exact — so nobody has to reverse-engineer it again.
- **Digital preservation.** A [PRONOM submission](docs/preservation/PRONOM-submission.md)
  so archives and forensic tools can even *recognise* these files in the first place.

That's the throughline of the project: not just open the lock for ourselves, but leave
the door open for everyone.

## Real-world numbers

The decision to ship rested on **an A/B against the reference Windows implementation
on the same source file**, not a count of successful decodes:

- **A/B vs Switch.exe** (same `.ds2`, both chains, both MP3s through the same Whisper
  API): transcripts **functionally identical** — 16.2 % vs 17.2 % low-confidence words,
  inside Whisper's own run-to-run variance. The chains are interchangeable downstream.
- **35 / 35** real production dictations decoded (6 h 48 of audio), zero failures.
- **~3 200** cron passes since go-live, zero errors. The Windows VM stays on standby,
  untouched.

For the Grundig codec: **byte-for-byte identical** to Grundig's own decoder on every
sample. [Full methodology →](docs/03-validation-campaign.md)

## What's in this repo

- 📖 **[docs/](docs/)** — the full didactic trail above, plus [the benchmarks](docs/benchmarks/).
- 🛠 **[src/](src/)** — the integration code: CLI, cron, HTTP daemon, admin web UI. Sanitized; the patterns are reusable as-is.
- 🎙 **[grundig/](grundig/)** — the native Grundig DSS-SP decoder (pure Python, bit-exact) + its tables.
- 🎬 **[ffmpeg-upstream/](ffmpeg-upstream/)** — the FFmpeg patches (DS2 decoder + demuxer, the DSS-SP paused-recording fix, and the Grundig SP decoder), the ones submitted to `ffmpeg-devel` plus their test samples and cover notes.
- 📐 **[the formal spec](docs/SPEC-grundig-dss-sp.md)** — the world's first public specification of the Grundig DSS-SP codec (bit-exact), plus a [PRONOM submission](docs/preservation/PRONOM-submission.md) so digital-preservation tools can identify these files at all.

## Credits — proper order

The intellectual heavy-lifting belongs to the people who opened the locks:

- **Kieran Hirpara** — [hirparak/dss-codec](https://github.com/hirparak/dss-codec) — the reverse-engineering that started all of it. MIT, February 2026.
- **Gaspard Petit** — [dss-codec-wasm](https://github.com/gaspardpetit/dss-codec-wasm) + [dss-codec fork](https://github.com/gaspardpetit/dss-codec) (the Rust crate our Dockerfile uses). MIT.
- **Patrick Domack** — the [FFmpeg C port](https://gist.github.com/patrickdk77/330dd3f593696d103e831c4c1d78d1f9) of the spec. MIT / public domain.
- **JulsRX** — the Grundig Digta owner who reported the file nobody could decode, and supplied the public sample that made cracking the Grundig codec possible.
- **lamejs** (LGPL) and **[FFmpeg](https://ffmpeg.org/)** (LGPL) — the MP3 encoders.

This repo is a recipe; the recipe needs the ingredients above to exist at all. Full breakdown: [CREDITS.md](CREDITS.md).

## License

MIT, same as the upstream codec. Fork, adapt, deploy — please keep attribution to the codec authors. We publish the clean reimplementations and the recovered specs, never the vendors' proprietary code.

---

*Thirty years of locked, one bash command later. The chain has to keep going. 🔓*
