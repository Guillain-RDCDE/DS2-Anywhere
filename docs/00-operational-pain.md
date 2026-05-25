# 00 — The operational pain

> What running a DS2 pipeline on Windows GUI software actually looks like in production. Not the codec problem — the *operations* problem.

The previous chapter ([01 — the reverse engineering](01-reverse-engineering.md)) tells you why no one could decode DS2 on Linux for ten years. This chapter tells you why that mattered *operationally* — what the duct-tape solution looked like, and why it kept breaking.

If your production system ingests DS2 files and you're not on Windows, you'll recognize this. If you've never had to wrangle a Windows VM in a Linux server farm, this is a glimpse of what it looked like before February 2026.

---

## The recipe before

A typical pre-2026 DS2-to-transcript pipeline on a Linux backend looked like this:

```
   client uploads .ds2
          │
          ▼
   ┌──────────────────────────────────────────┐
   │ Linux server                             │
   │   intake script copies .ds2 into a       │
   │   "to-convert" folder mounted via SSHFS  │
   │   on a Windows VM                        │
   └──────────────────────────────────────────┘
          │
          ▼  (SSHFS, hopefully alive)
   ┌──────────────────────────────────────────┐
   │ Windows VM (Win10 or Server)             │
   │   - RDP session must stay logged in      │
   │   - Switch.exe (NCH, commercial) running │
   │     in foreground, watching folder       │
   │   - DSS Player as fallback               │
   │   - decodes .ds2 → .wav, drops in        │
   │     "converted" output folder            │
   └──────────────────────────────────────────┘
          │
          ▼  (SSHFS again, fingers crossed)
   ┌──────────────────────────────────────────┐
   │ Linux server                             │
   │   cron picks up .wav, ffmpeg re-encodes  │
   │   to mp3/ogg, drops in mail/ folder      │
   │   for the speech-to-text pipeline        │
   └──────────────────────────────────────────┘
          │
          ▼
   Whisper / ElevenLabs / whatever STT
```

It works. Sort of. Until any of these things happen:

## The failure modes (a non-exhaustive list)

### 1. The RDP session logs out

Switch.exe is a **foreground GUI application**. It needs an active Windows session to run. If the RDP session times out (Windows defaults to 1-2 hours of inactivity), Switch goes idle. Files pile up in the to-convert folder. Nothing tells you. The next time someone notices, there are 50 unconverted files and a stack of "audio not received" alerts from your STT pipeline.

Workaround: never let the RDP session disconnect. Workaround for the workaround: a watchdog that pokes the session every 30 minutes. Workaround for the watchdog: someone has to remember to set it up after every VM reboot.

### 2. Switch.exe hangs on a malformed file

DS2 files can be subtly malformed — short recordings, files truncated by a flaky USB transfer, files with weird metadata from old recorder firmwares. Switch handles most gracefully. Some it doesn't — it pops a modal dialog ("Cannot read file") and **waits for someone to click OK**. While it waits, every other file in the queue waits too.

If your Linux backend doesn't have eyes on the Windows desktop, you don't know. Files pile up. Eventually someone connects via RDP, dismisses the modal, and the queue drains. The intervening hour or two of latency was invisible.

### 3. SSHFS breaks

SSHFS is delicate on long uptimes, especially when the Linux ↔ Windows link has any flakiness. The mount goes stale. Switch sees the folder as empty. Files in the upstream still arrive, but the bridge to the converter is broken. The cron on the other side picks up nothing because nothing comes back.

Detecting this requires monitoring the mount itself, not just the files. Most pipelines don't.

### 4. The Windows VM eats CPU, eats RAM, costs money

A V2Cloud Windows VM with 8 GB RAM and 2 vCPU runs ~€60-130/month depending on the provider. Switch.exe itself isn't heavy, but Windows isn't free either: the OS background activity, the updates, the antivirus, the audit logs all consume resources you're not using. You're paying for an OS to host one GUI app.

### 5. Switch.exe licensing

NCH Switch is **commercial software**. It's not expensive (~$50/seat), but if you scale to multiple converters, you scale the licensing. And every Switch install has its own activation, its own update cycle, its own "your license expires in 14 days" popup that someone needs to click through.

### 6. No CLI, no exit codes, no log discipline

Switch is GUI-first. Yes, there's a command-line mode, but the error reporting is inconsistent (sometimes a popup, sometimes a return code, sometimes silent). You can't build robust pipeline error handling around it. Failures appear as "the file didn't show up in the output folder eventually" — diagnosed by humans reading log timestamps.

### 7. The codec being closed means you can't test

If your pipeline integrator wants to write unit tests around DS2 handling, they can't. They can't write test fixtures, mock decoders, or property-based tests. Every test of the DS2 path requires the actual Windows binary to run.

## What we observed, concretely

We ran this exact architecture for years. Some real production samples from our logs in May 2026, right before the migration:

- **22 DS2 files** sat unconverted in `audio_aconv/` between May 18 and May 23, 2026 (the moment Switch stopped processing — we still don't know exactly why). Nobody noticed for ~5 days, because the symptom is *absence* of output, not *presence* of error.
- The most recent stuck file at the time was a 6.4 MB DS2 with a 31-minute legal dictation. The client thought it had been delivered. Our pipeline thought we were converting it. Switch had given up silently.
- The downstream STT pipeline (Whisper) had a daemon checking every minute for new audio. It kept finding none. Hours of audio silently stuck somewhere in the middle.

The pipeline didn't *fail*. It went *quiet*. Quiet failures are the worst kind, because no alert fires.

## The math that changed in February 2026

Before:

- 1 Windows VM (€60-130/mo)
- 1 Switch.exe license (annual or one-time)
- 1 SSHFS bridge to monitor
- 1 cron to copy files in
- 1 cron to copy files out
- 1 watchdog to keep the RDP alive
- N hours/month spent investigating "where did this file go" tickets

After [hirparak/dss-codec](https://github.com/hirparak/dss-codec) shipped:

- 1 native binary (5 MB, free, MIT)
- 1 cron (the one we'd already need anyway)
- 0 VMs
- 0 licenses
- 0 GUI to keep alive

This isn't a story about saving €130/month. It's a story about removing **an entire class of failure modes** — silent ones, dependent on a GUI app's mood, with no error reporting — from a system that was supposed to be reliable.

The next chapters show how we did it.

---

Next: **[01 — The reverse engineering](01-reverse-engineering.md)** (how Hirpara made this possible) or jump to **[02 — The integration](02-integration.md)** (how we wired it into production).

---

*The quiet failures are the worst kind. ⚙️*
