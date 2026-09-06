# DS2-Anywhere — reference

[← Back to the README](../README.md)

---

## Contents

- [Pick your way in](#pick-your-way-in) — three doors
- [New here? Three words in plain English](#new-here-three-words-in-plain-english)
- [Try it in 30 seconds](#try-it-in-30-seconds)
- [What it does, in one picture](#what-it-does-in-one-picture)
- [The technical trail](#the-technical-trail) — the story, chapter by chapter
- [Where it stands now](#where-it-stands-now) — giving it back to FFmpeg & preservation
- [Real-world numbers](#real-world-numbers)
- [What's in this repo](#whats-in-this-repo)
- [Credits](#credits--proper-order) · [License](#license)

## Pick your way in

Three doors. Take the one that matches why you are here.

| | |
|---|---|
| 🎧 **I have a dictation file and need the words** | **[The practical guide →](../docs/TRANSCRIBE-A-DSS.md)** · ten minutes, nothing assumed. What your file is, how to open it with nothing installed, how to get a transcript, and the four things that actually go wrong. Or skip straight to the **[in-browser decoder](https://guillain-rdcde.github.io/DS2-Anywhere/)**. |
| 🧠 **I want to understand how it was done** | Three ways, depending on your appetite: **[The Story](../docs/THE-STORY.md)** (no code, ~10 min, reads like a thriller) · **[The paper](../docs/THE-DSS-PAPER.md)** (the reference: container, codec, the defect, method, validation, and what we have *not* proved) · **[How to circle a closed thing](../docs/HOW-TO-CIRCLE-A-CLOSED-THING.md)** (the method, generalised — useful even if you never touch a DSS file). The chapter-by-chapter trail is **[below ↓](#the-technical-trail)**. |
| 🛠 **I want to run it, or improve it** | **[30 seconds ↓](#try-it-in-30-seconds)** to convert a file · **[CONTRIBUTING](../.github/CONTRIBUTING.md)** for setup, scope and how patches get reviewed. A file that fails to decode is genuinely useful to us — two of the supported formats exist because someone turned up with one. |

## New here? Three words in plain English

You don't need any audio or programming background to follow this repo. Three terms cover most of it:

- **Codec** — the secret "recipe" that squeezes a voice recording into a tiny file, and rebuilds it on playback. Without the recipe, the file is just unreadable noise.
- **Decode** (and **demux**) — turning that tiny file back into sound. *Demux* is the first step (split the file into the right little chunks, called *frames*); *decode* is the second (turn frames into audio). Most of our hardest bugs were in the *demux* step — getting the chunks lined up.
- **Reverse-engineering** — working out the secret recipe yourself, by careful observation, because the manufacturer never published it.

That's it. Everything below builds gently from these.

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

Production install (config + cron + systemd + web UI): `sudo ./src/bin/install.sh`, or [docs/02-integration.md](../docs/02-integration.md).

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

1. **[The thirty-year lock](../docs/01-reverse-engineering.md)** — what a DS2 file is, why it
   resisted, and how Kieran Hirpara reverse-engineered the codec from the Olympus DLLs
   (the genius part — not ours).
2. **[Putting it in production](../docs/02-integration.md)** — turning a decoder into a
   real pipeline: CLI, cron, daemon, the encode chain. The engineering part.
3. **[The empty-block bug](../docs/06-the-empty-block-bug.md)** — a decoder that was
   bit-exact on every file we tested and *still* wrong on paused recordings. Ten dead
   ends, a twelve-line fix. A detective story worth reading even if you never touch DS2.
4. **[Cracking the re-sync block](../docs/07-cracking-the-resync-block.md)** — the sequel:
   we ran the closed-source Olympus decoder *inside a debugger we built from its own
   DLLs*, and read the format's last undocumented rule straight off the silicon.
5. **[The re-sync block, again — and into FFmpeg](../docs/13-the-sp-resync-block.md)** — the
   same trick a second time, on Olympus's *other* format (DSS SP). We re-hosted the
   vendor's decoder, read the rule off the live parser, fixed it in one branch — and this
   time **sent the fix to FFmpeg itself** (see [where it stands](#where-it-stands-now)).
6. **[The bug that wasn't](../docs/10-the-reckoning-the-bug-that-wasnt.md)** — the twist,
   and the chapter we're proudest of. A rigorous case for a "last bug" ([the research
   paper](../docs/09-the-resync-excitation-anomaly.md)), then *overturned* — there was no
   bug; it was a person stepping away from the mic. How careful work can be confidently
   wrong, and how to catch it.
7. **[Cracking the Grundig SP codec](../docs/12-cracking-the-grundig-sp-codec.md)** — the
   finale. The Grundig grandfather format that *nobody* decoded — not us, not FFmpeg,
   not even Olympus's own software. We extracted Grundig's decoder, ran it under a
   debugger, patched out the instruction it used to delete its own evidence, and
   rebuilt the codec **bit-exact**. Now a [native Python decoder](../grundig/) and an
   [FFmpeg patch](../ffmpeg-upstream/patches/avcodec-grundig_sp-decoder.patch).

8. **[The framing was wrong all along](../docs/17-the-framing-was-wrong.md)** — the second
   twist, and the one that stings. We diagnosed a codec bug with real rigour, shipped the
   fix to FFmpeg and to the upstream crate, and were wrong about which component was
   broken. What broke it open was giving up on audio metrics: seven pulse positions from
   72 slots is C(72,7) = 1,473,109,704, the field is 31 bits wide, so any larger value is
   a frame that *cannot exist*. A free oracle, no reference decoder required. Read it for
   the trap: **a stabilising fix that damps a symptom is not evidence you found the
   cause.**

**How it was actually done:** **[How to circle a closed thing →](../docs/HOW-TO-CIRCLE-A-CLOSED-THING.md)** — the method rather than the result. How you get unstuck on something that will not explain itself: find the constraint that cannot be violated instead of the signal you can compare, treat a redundant-looking field as a case you have not met, and learn to tell "the symptom moved" from "the cause is gone". Our own wrong turn is the worked example. Useful on your first reverse-engineering project and on your hundredth.

**The whole thing in one document:** **[Reading Digital Speech Standard →](../docs/THE-DSS-PAPER.md)** — the technical paper. Container layout, block headers, the 328-bit frame, the decode pipeline, the sample rate, the framing defect and why it survived everywhere, the method that found it, the validation instruments, the results, and an honest list of what we have *not* proved. Written to be the reference: if you maintain a DSS implementation, this is the page to read.

> Short on time? **5 min** → this page · **20 min** → chapter 1 · **30 min** → the two
> detective stories (3 & 4) · **the twists** → 6 and 8 · **the finale** → 7 ·
> **everything** → [docs/](../docs/) in order, "impossible for thirty years" to
> "production in a weekend."

## Where it stands now

The work didn't stop at our own servers — it's being handed back to the tools everyone else uses:

- **Into FFmpeg.** FFmpeg is the audio/video engine inside VLC, Chrome, OBS and much of
  the internet. The Olympus DS2 decoder + demuxer and the DSS-SP paused-recording fix
  have been **submitted to the `ffmpeg-devel` mailing list and are in review**, joined
  by a two-patch series fixing the DSS SP framing and sample rate for everyone; the
  Grundig SP decoder patch is staged behind them. Once merged, *every* program built on
  FFmpeg reads these files for free, forever — no recipe required.
- **A public specification.** [The first one ever written](../docs/SPEC-grundig-dss-sp.md)
  for the Grundig DSS-SP codec, bit-exact — so nobody has to reverse-engineer it again.
- **Digital preservation.** A [PRONOM submission](../docs/preservation/PRONOM-submission.md)
  so archives and forensic tools can even *recognise* these files in the first place.
- **DSS SP: the one we got wrong first, then got right.** Long recordings blew up into
  noise, so we rebuilt the synthesis filter — published it, and were wrong. The runaway
  was never in the codec. Every 512-byte DSS block *declares its own framing*, and every
  open implementation — ours, FFmpeg's, the npm and PyPI ports — skips those bytes and
  guesses instead. The guess is right until a recording is paused or edited, and wrong
  for the rest of the file after that. Believing the block takes a reference bench from
  **0.5849 to 0.9995**, and leaves healthy files bit-identical. The output rate is also
  **11000 Hz, not 11025** — the container proves it on its own.
  [Chapter 17 →](../docs/17-the-framing-was-wrong.md)

That's the throughline of the project: not just open the lock for ourselves, but leave
the door open for everyone.

## Real-world numbers

The decision to ship rested on **an A/B against the reference Windows implementation
on the same source file**, not a count of successful decodes:

- **A/B vs Switch.exe** (same `.ds2`, both chains, both MP3s through the same Whisper
  API): transcripts **functionally identical** — 16.2 % vs 17.2 % low-confidence words,
  inside Whisper's own run-to-run variance. The chains are interchangeable downstream.
- **1678 / 1685** recordings on the production server decode end to end. The seven that
  do not contain no audio at all: two images renamed `.dss`, one file of 10 bytes, two of
  **0 bytes**, and two truncated to their headers — one of which declares a 7680-byte
  header inside a 1024-byte file. In other words, **every real dictation**.
- **144 049** cron passes since go-live on 23 May 2026, zero errors. The Windows VM has
  been on standby and untouched since 19 May.
- **More accurate than the commercial reference.** A DSS recorder writes the length of
  the recording into the file's own header. On a 23-minute dictation, our chain lands
  **0.1 s** off that declared length; Switch.exe lands **3.1 s** short, because it plays
  DSS SP 0.23 % fast. Same samples — correlation 0.9998 — at the right speed.

For the Grundig codec: **byte-for-byte identical** to Grundig's own decoder on every
sample. [Full methodology →](../docs/03-validation-campaign.md)

## What's in this repo

- 📖 **[docs/](../docs/)** — the full didactic trail above, plus [the benchmarks](../docs/benchmarks/).
- 🛠 **[src/](../src/)** — the integration code: CLI, cron, HTTP daemon, admin web UI. Sanitized; the patterns are reusable as-is.
- 🎙 **[grundig/](../grundig/)** — the native Grundig DSS-SP decoder (pure Python, bit-exact) + its tables.
- 🎬 **[ffmpeg-upstream/](../ffmpeg-upstream/)** — the FFmpeg patches (DS2 decoder + demuxer, the DSS-SP paused-recording fix, and the Grundig SP decoder), the ones submitted to `ffmpeg-devel` plus their test samples and cover notes.
- 🤝 **[.github/CONTRIBUTING.md](../.github/CONTRIBUTING.md)** — local setup, what is in and out of scope, commit and PR conventions. Start here if you want to add a device family, or if a file of yours refuses to decode.
- 📐 **[the formal spec](../docs/SPEC-grundig-dss-sp.md)** — the world's first public specification of the Grundig DSS-SP codec (bit-exact), plus a [PRONOM submission](../docs/preservation/PRONOM-submission.md) so digital-preservation tools can identify these files at all.

## Credits — proper order

The intellectual heavy-lifting belongs to the people who opened the locks:

- **Oleksij Rempel** — `libavcodec/dss_sp.c` in FFmpeg, **2014**. The first open DSS SP decoder, and the only one for twelve years. The bug we found in that corner was in the demuxer around it, not in his codec.
- **Kieran Hirpara** — [hirparak/dss-codec](https://github.com/hirparak/dss-codec) — the reverse-engineering that started all of it. MIT, February 2026.
- **Gaspard Petit** — [dss-codec-wasm](https://github.com/gaspardpetit/dss-codec-wasm) + [dss-codec fork](https://github.com/gaspardpetit/dss-codec) (the Rust crate our Dockerfile uses). MIT.
- **Patrick Domack** — the [FFmpeg C port](https://gist.github.com/patrickdk77/330dd3f593696d103e831c4c1d78d1f9) of the spec. MIT / public domain.
- **JulsRX** — the Grundig Digta owner who reported the file nobody could decode, and supplied the public sample that made cracking the Grundig codec possible.
- **lamejs** (LGPL) and **[FFmpeg](https://ffmpeg.org/)** (LGPL) — the MP3 encoders.

This repo is a recipe; the recipe needs the ingredients above to exist at all. Full breakdown: [CREDITS.md](../CREDITS.md).

