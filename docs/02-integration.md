# 02 — The integration

> How we wrapped the Rust codec into a production transcription pipeline with three entry points, two safety nets, and zero downtime. The engineering recipe.

The codec ([Hirpara's work](01-reverse-engineering.md)) is the hard part — but a CLI binary on a disk isn't a production system. You need it to run *automatically*, *recover from failures*, *handle the weird files*, and *fit the existing pipeline* without breaking what was already working.

This chapter is how.

---

## The three entry points

A real transcription pipeline has more than one way of getting audio in. We built three matching ways of converting it:

```
┌────────────────────────┐    ┌────────────────────────┐    ┌────────────────────────┐
│  CLI (bash)            │    │  cron (automatic)      │    │  Admin web UI (PHP)    │
│                        │    │                        │    │                        │
│  Manual one-shot       │    │  Processes new DS2     │    │  Drag-and-drop one     │
│  conversion. Used by   │    │  files as they arrive  │    │  file → download MP3.  │
│  ops, scripts, ad-hoc  │    │  in the pipeline. The  │    │  Or "unblock" a stuck  │
│  pipelines.            │    │  workhorse.            │    │  command by ID.        │
└──────────┬─────────────┘    └──────────┬─────────────┘    └──────────┬─────────────┘
           │                             │                             │
           └─────────────────────────────┴─────────────────────────────┘
                                         │
                              ┌──────────▼──────────┐
                              │  Shared core        │
                              │  decode → encode    │
                              │  (Rust + ffmpeg)    │
                              └─────────────────────┘
```

All three converge on the same core conversion logic. No duplicated decode paths, no risk of "the CLI does X but the cron does Y differently".

## The core: decode → encode

The conversion itself is two phases:

1. **Decode DS2 → PCM WAV** using the native binary `dss-decode-native` (compiled from the upstream Rust crate).
2. **Encode PCM WAV → MP3** using ffmpeg with `libmp3lame`.

Why two binaries instead of one? Because the upstream decoder outputs WAV (Olympus's native interchange format), not MP3. Adding MP3 encoding directly into the Rust would have meant pulling in `libmp3lame` as a Rust dependency — bigger build, more deps. ffmpeg is already on every Linux server (and if it isn't, you have other problems), so the split keeps both pieces small and standard.

```bash
dss-decode-native -O /tmp/audio.wav recording.ds2
ffmpeg -y -i /tmp/audio.wav -ac 1 -c:a libmp3lame -b:a 64k recording.mp3
```

That's the whole core. ~2 commands. ~10 seconds for a 30-minute dictation. The wrapper bash is ~100 lines around this to handle argument parsing, error mapping, encryption detection, and output naming.

## Entry point 1 — the CLI

`conv-dss-ds2-to-mp3` is a pure bash wrapper. No Node, no daemon, no service. It's the simplest thing that exposes the core to humans and scripts.

```bash
# Standard usage
conv-dss-ds2-to-mp3 recording.ds2
# Output: [ds2_qp 16000Hz, 31.8min] recording.mp3  OK  (14.55 Mo en 10.3 s)

# Custom output path
conv-dss-ds2-to-mp3 recording.ds2 /path/to/output.mp3

# Inspect only (no conversion)
conv-dss-ds2-to-mp3 --inspect recording.ds2

# Encrypted DS2
conv-dss-ds2-to-mp3 --password=mypwd recording.ds2

# Different bitrate
conv-dss-ds2-to-mp3 --bitrate=128 recording.ds2
```

### Exit codes that mean something

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Generic error (file not found, decode failure, IO error) |
| 2 | Bad usage (unknown flag, wrong arity) |
| 3 | Encrypted file, password required |

The `3` is important. It lets cron and the web UI distinguish "this file is malformed" from "this file is encrypted and we didn't have the key" — completely different remediation paths.

### Encryption detection without decoding

The bash wrapper detects encryption by reading the first 4 bytes of the file (magic bytes):

| Bytes | Meaning |
|---|---|
| `03 64 73 32` (`\x03ds2`) | Plain DS2 |
| `03 64 73 73` (`\x03dss`) | Plain DSS (older format, also supported) |
| `03 65 6e 63` (`\x03enc`) | **Encrypted DS2** — need `--password=` |

This is faster than attempting decode-and-catch (which would spawn the binary, fail, and we'd have to parse stderr). 4 bytes from disk is enough.

## Entry point 2 — the cron (the workhorse)

This is what runs every minute, in the background, on every server in production. It's the reason the pipeline is now hands-off.

### Two modes in one script

The cron has two scan modes, both running in every tick:

**Mode 1 — drain the legacy queue**

Some upstream intake scripts still copy DS2 files into a folder called `audio_aconv/`. This was the input folder for the old Switch.exe-based pipeline. We didn't want to touch those upstream scripts (battle-tested, owned by other teams, risky), so we just consume from the same folder.

For each `*.ds2` / `*.dss` file in `audio_aconv/`:

1. Stability check (skip if modified <60 s ago — might still be being written).
2. Database check — is the corresponding command still active, or already delivered? If delivered or cancelled, just delete (it's a leftover, no conversion needed).
3. Anti-duplicate check (more on this below).
4. Convert + distribute the MP3.
5. On success: delete the source from `audio_aconv/` (it's a queue).
6. On failure: leave the source in place + send an alert email.

**Mode 2 — scan for active commands with DS2 in their mail folder**

The new pipeline doesn't need the `audio_aconv/` queue at all. Each command has its own mail folder (`mail/m{prefix}/{cmd_id}/`) where the original DS2 lands. Mode 2 queries the database for "active commands with DS2 in the subject line, less than 7 days old, not already delivered", and for each one:

1. Look in the mail folder. Is there a `*.ds2` at the top level?
2. Is there an audio file (`.mp3`/`.wav`/`.m4a`) next to it already? If yes → noop, audio is done. If no → it needs conversion.
3. Convert + distribute. **Do not delete the source DS2** — that's the client's original upload, kept forever.

Mode 1 is the legacy compat layer. Mode 2 is the new way. Both run because some files still come through the legacy path, and we get to them either way.

### The anti-duplicate safeguard

In our setup, the legacy intake script (`conv_fast.sh` in the upstream pipeline) drops the same DS2 into `audio_aconv/` about 30 seconds after it lands in `mail/`. So in any given minute, the same file is sometimes seen by both Mode 1 and Mode 2.

Both conversions write the same MP3 to the same destination (idempotent — bit-for-bit identical output). No data corruption. But it's wasted CPU.

The fix is small. In Mode 1, before converting, check:

```bash
# Does the destination MP3 already exist, fresh (<10 min old)?
# If yes, Mode 2 (or a previous tick) probably did it. Just clean up.
mp3_dest="$MAIL_BASE/m${prefix}/${cmd_id}/${stem}.mp3"
if [ -f "$mp3_dest" ]; then
  age_min=$(( ($(date +%s) - $(stat -c%Y "$mp3_dest")) / 60 ))
  if [ "$age_min" -lt 10 ]; then
    log "doublon: ${stem}.mp3 already produced ${age_min} min ago → cleanup without reconverting"
    rm -f "$f"
    continue
  fi
fi
```

10 minutes is wide enough to absorb any timing skew between Mode 1 and Mode 2 (their delta is usually <2 min). Narrow enough that a genuinely *new* DS2 dropped into `audio_aconv/` later in the day will trigger a real conversion (the old MP3 will be too stale).

### Safety nets, listed

The cron has six safety nets stacked on top of each other:

1. **`flock` on `/tmp/audio_cron.lock`** — only one cron pass at a time. If a long file is still converting when the next tick fires, the second exits immediately.
2. **Stability mtime check** — skip if the source DS2 was touched less than 60 s ago. Avoids race conditions with intake scripts still writing.
3. **Database status filter** — never re-process a command marked `delivered` or `cancelled`. Hands off finalized work.
4. **Anti-duplicate destination check** — described above.
5. **Encryption detection** — skip + alert if the DS2 is encrypted and no password is provided. Never crash.
6. **Conversion failure handling** — on any decode/encode error, leave the source DS2 in place (so a human can investigate) + send an alert email. Never silently lose a file.

### What it looks like in the log

```
[2026-05-25 10:09:01] === audio_cron pass (dryrun=0) ===
[2026-05-25 10:09:01] --- mode 1: audio_aconv ---
[2026-05-25 10:09:01]   audio_aconv empty
[2026-05-25 10:09:01] --- mode 2: scan mail/ ---
[2026-05-25 10:09:05] [2940793 feed] OK -> BPACA_21_05.mp3 (conv 4s)
[2026-05-25 10:09:05] [2940793 feed] source DS2 KEPT in mail/ (client original)
[2026-05-25 10:09:05] === pass done: aconv[ok=0 clean=0] feed[ok=1 noop=40] ===
```

A normal pass on a "nothing to do" minute is two log lines. A pass that converts something is six. Easy to grep for failures, easy to skim for activity.

## Entry point 3 — the admin web UI

For ops and admins, a one-page PHP admin tool. Two cards:

**Card 1 — Convert a file manually**

Drag a `.ds2` or `.dss` into the page → optional password field → click "Convert and download" → MP3 streams back as a download. ~10 seconds for a 30-minute file. Used when:

- Testing a new client's DS2 to check it decodes properly.
- One-off conversion of a file from a non-pipeline source.
- Demonstrating to teammates that the pipeline works.

**Card 2 — Unblock a command by ID**

Type a 7-digit command ID → optional password → click "Unblock". The backend:

1. Queries the database to find the command's status. If already delivered, refuses ("already delivered, nothing to do").
2. Searches for the DS2 in the usual places: `audio_aconv/`, the command's mail folder, the `_audio_remplaces_` archive subfolder.
3. Converts + distributes to all the usual destinations.
4. Returns a success message: "Command 2940793 unblocked, MP3 generated (12.6 min, 5.8 MB), Whisper will resume within the minute."

The unblock card is the safety valve. When something jams (anything jams sometimes), a human can intervene in seconds without SSH.

### The PHP ↔ Linux backend bridge

The PHP runs inside a Docker container. The native binary lives on the host. To bridge them, a small Node.js HTTP daemon listens on `127.0.0.1:8765` on the host, exposes two endpoints, and shells out to `dss-decode-native` + ffmpeg internally.

```
┌────────────────────────────┐       ┌────────────────────────────┐
│ PHP container              │       │ Host                       │
│                            │  HTTP │                            │
│  $ curl -X POST            │ ────► │  Node daemon (systemd)     │
│    127.0.0.1:8765/convert  │       │   listens on localhost     │
│    --data-binary @file.ds2 │       │   spawns dss-decode-native │
│                            │ ◄──── │   pipes to ffmpeg          │
│  receives MP3 binary       │       │   returns MP3 binary       │
└────────────────────────────┘       └────────────────────────────┘
```

Why not run the Node daemon inside the container? Because we don't want Node, Rust binaries, or ffmpeg installed in a PHP container that's purposefully kept minimal. The daemon is a 100-line bridge, runs as a `systemd` unit on the host, restarts on failure, logs to journalctl.

The daemon also serves a `/health` endpoint (for monitoring) and a `/enqueue?cmd=X` endpoint (for the "unblock by command ID" path — same logic as the cron, but on-demand).

## Distribution: where the MP3 goes

After a successful conversion, the MP3 lands in three places:

| Destination | Why |
|---|---|
| `mail/m{prefix}/{cmd_id}/<stem>.mp3` | Where the speech-to-text pipeline (Whisper / ElevenLabs / etc.) looks for audio |
| `ftpsaisieaudio/41/{cmd_id}/<stem>.mp3` | Where the human typist UI looks for the recording (to listen alongside the transcript) |
| `ftpsaisieaudio/tmp_audio_speechtotext/m{prefix}/{cmd_id}/<stem>.mp3` | Legacy staging folder still consumed by some scripts |

All three with permissions `www-data:www-data 666` so the rest of the pipeline can read them. The source DS2 is **never deleted** from `mail/` (it's the original client upload), but it *is* deleted from `audio_aconv/` after a successful conversion (it was a queue staging area).

## What stays the same downstream

After the MP3 lands in `mail/`, the rest of the pipeline is **untouched**. Whisper picks up the audio, transcribes it, the transcript goes to a human typist for review, then to a corrector, then to a layout person, then to delivery. None of that knows or cares whether the MP3 came from Switch or from the new chain.

This is the whole point. The migration is invisible to everyone downstream. The day we switched over, no transcript looked different, no client noticed, no internal user filed a "this is broken" ticket.

## Reversibility

Every component can be rolled back in under 5 minutes:

| Component | Rollback |
|---|---|
| Cron | `rm /etc/cron.d/audio_converter` — the automatic conversion stops. The old Switch.exe pipeline can resume if reactivated. |
| Daemon | `systemctl stop audio-convert && systemctl disable audio-convert` — the web UI stops working, but the CLI and cron continue. |
| CLI / native binary | `mv conv-dss-ds2-to-mp3.wasm.bak conv-dss-ds2-to-mp3` — switches the wrapper back to the older WASM-based chain we used before the native bascule (kept as a backup). |
| Whole project | The Windows VM with Switch.exe is kept on standby. Re-enable it as the converter of last resort. |

We didn't burn the bridges. Every step of the migration could be undone in a single command without losing data.

---

Next: **[03 — The validation campaign](03-validation-campaign.md)** — how we proved on 35 real-world files that the new chain is at least as good as Switch.

---

*Three entry points, one core, six safety nets. 🔧*
