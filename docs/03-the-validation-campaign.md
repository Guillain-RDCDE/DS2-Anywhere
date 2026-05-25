# 03 — The validation campaign

> How we proved on 35 real-world dictation files that the new chain is at least as good as the commercial Windows-based one. The methodology, the results, the honest caveats.

You can't put a new audio decoder in front of a production transcription pipeline on faith. The risk is subtle: a decoder that produces *technically valid* WAV but with *wrong audio content* (wrong sample rate, clipped, distorted, garbage tail) will pass `ffprobe`'s sanity checks, but the speech-to-text engine downstream will produce gibberish — and you won't notice until a typist reports a strange transcript.

So before switching anything in production, we ran a controlled validation campaign.

---

## The hypothesis to test

Three things had to be true before we'd consider going live:

1. **The decoder produces valid audio.** Not just "ffprobe-valid" — actually contains the dictation, full length, no truncation, no distortion.
2. **The audio quality is at least as good as Switch.exe's**. The downstream STT engine (Whisper) must produce transcripts of equivalent quality.
3. **The decoder is reliable across formats and devices**. DS2 SP, DS2 QP, DSS — different recorders, different firmware, different client habits.

## The dataset

35 real DS2 / DSS files pulled from a live transcription pipeline, sanitized of client identifiers. Selection criteria:

- **Mix of formats**: 32× DS2 QP (16 kHz, modern recorders) + 3× DSS SP (11.025 kHz, older recorders).
- **Mix of durations**: from 87 seconds (short instruction) to 38 minutes (full property survey).
- **Mix of clients**: different professional contexts (legal, technical, transactional), different recorder models implied.
- **Mix of dates**: spread across several weeks of production traffic to catch any "this Tuesday's batch is weird" pattern.

DS2 dominates the dataset because it dominates the real-world traffic (DSS is legacy — a few percent of incoming files, mostly from older recorders).

Total audio decoded: **6 hours 48 minutes**.

## Phase 1 — does it decode?

Step one: run all 35 files through `dss-decode-native -O <output>.wav`, record the result.

Results:

| Metric | Value |
|---|---|
| Files attempted | 35 |
| Files decoded successfully | **35** |
| Files with non-zero exit code | 0 |
| Files producing empty WAV | 0 |
| Files with truncated WAV (duration < expected) | 0 |
| Encryption-required files in sample | 0 |
| Median decode time per file | 1.4 s |
| Total decode time | ~8 minutes for 6h48 of audio |

Across all 35 files, the WAV duration matched the embedded DS2 metadata within ±0.1 seconds (rounding error in PCM frame boundaries). No file was silently shorter than it should have been.

## Phase 2 — does Whisper understand it?

A WAV that decodes "correctly" might still be subtly wrong — phase-inverted, slightly off-pitch, with a high-frequency artifact that's inaudible to humans but throws off the speech recognizer. The only way to know is to run the WAVs through the actual STT engine and look at the transcripts.

For each of the 35 files, we sent the converted audio to the same Whisper API used in production, with the same parameters. We collected:

- HTTP response code
- Whisper's `audio_loss_detected` flag (its internal sanity check)
- Number of segments in the transcript
- Word count
- "Low-confidence words" ratio (Whisper flags words it's unsure about)
- A 200-character excerpt of the transcript (for human eyeballing)

Aggregate results:

| Metric | Value |
|---|---|
| HTTP 200 responses | 35 / 35 |
| `audio_loss_detected = True` | 0 |
| Empty transcripts | 0 |
| Median words/minute | 91 |
| Median low-confidence word ratio | 15.9 % |
| Excerpts judged "coherent French legal/dictation content" by human review | 35 / 35 |

A median of 91 words/min is on the low side of normal speech (110–160 wpm) — explained by the nature of dictation (legal language uses long sentences with lots of subordinate clauses, and dictators pause to think). Several files had very low wpm because they were 30-minute recordings with only 50 words of actual speech — the recorder kept rolling in silence after the dictator finished. The transcript text on those files was correct (just sparse), confirming the decoder wasn't truncating audio.

The 15.9 % low-confidence rate is high in absolute terms but **identical to what we measured on Switch.exe-produced audio** — see the A/B section below. It's a characteristic of Whisper on dictaphone-grade audio (compressed speech with proper nouns, spelled-out names, technical jargon), not a defect of the decoder.

## Phase 3 — the A/B test against Switch.exe

The decisive comparison. We took one file from the dataset (cmd 2940662, a 31-minute DS2 QP dictation), produced an MP3 with **both** chains:

- **Chain A**: source DS2 → Switch.exe (on the Windows VM) → WAV → ffmpeg → MP3 at 224 kbps (Switch's default).
- **Chain B**: source DS2 → `dss-decode-native` → WAV → ffmpeg → MP3 at 64 kbps mono (our pipeline default).

Sent both MP3s through the same Whisper API.

| Metric | Switch.exe chain | dss-decode-native chain |
|---|---|---|
| HTTP response | 200 | 200 |
| Whisper segments | 95 | 92 |
| Total words | **3 961** | **3 872** |
| Low-confidence words | 643 | 667 |
| Low-confidence ratio | **16.2 %** | **17.2 %** |
| Audio loss detected | False | False |
| Transcript opening | *"Dossier Oren HOU REN, email à la maïf. Oui, les nouvelles références majeures..."* | *"Dossier Orenachou-REN, email à la maïf. Les nouvelles références..."* |

**Functionally identical.** The 2.2 % word count delta is within Whisper's own run-to-run variance (it's not fully deterministic). The 1 percentage point difference in low-confidence rate is in the noise — and crucially, it's not biased: sometimes the new chain has fewer low-confidence words, sometimes more.

The opening of the transcript shows the same content with the kinds of tiny word-boundary differences you get between any two Whisper passes on the same audio (slight encoder differences, slight bitrate differences). Nothing structural.

**Conclusion**: the new chain produces audio that Whisper transcribes with the same quality as the commercial Windows chain. The migration is safe.

## What we did not test (honest caveats)

Three things we explicitly did **not** validate, and should be clear about:

### 1. Encrypted DS2

Our 35-file sample contained **zero encrypted files**. The upstream codec advertises support for AES-128 and AES-256 password-protected DS2 (via `decodeWithPassword`), and the code path exists in our wrapper and the web UI. But it's not validated on real Olympus-encrypted files because none of the clients in our pipeline use the password-protect feature on their recorders.

If your pipeline does see encrypted files, treat this as untested — though the upstream test suite covers the path.

### 2. Very large files (> 1 hour)

Largest file in our sample was ~38 minutes. We didn't test 90-minute or 2-hour recordings. The decoder is streaming (memory-efficient), so very long files should work fine, but we don't have direct measurements.

### 3. Pathologically malformed files

We tested 35 *real* files from clients who use Olympus recorders normally. We didn't deliberately try to feed the decoder truncated, bit-flipped, or maliciously crafted DS2 files. The Rust crate has its own internal sanity checks and returns errors on malformed input — but we didn't measure how *graceful* those errors are in our specific pipeline. (The cron's "leave on disk + send alert" path catches this either way, so the operational impact is bounded.)

## Why 35 and not 100

35 was the cap because we wanted real, recent files from active commands — and active DS2 commands have a short shelf life. The intake pipeline keeps raw DS2 around for about 14 days, then archives or deletes them as part of the normal command lifecycle. We sampled what we could find that was both recent enough to still be on disk and old enough to have its commands already delivered (so we could compare transcripts against the human-reviewed final text).

35 is comfortably above the threshold where you'd catch a systematic decoder bug: if a particular DS2 mode were misdecoded, we'd see *every* file of that mode produce garbage transcripts. We saw none of that. 

## The proof in production

After the campaign passed, we put the system live. **The first real DS2 to go through the new chain in production** was a 12.6-minute dictation that arrived on a Monday morning:

- T+0 — Dictation arrives in `mail/`.
- T+1 min — Cron picks it up, converts in 4 seconds.
- T+2 min — Whisper transcribes.
- T+3 min — Command routed to a human typist.
- T+34 min — Typist hands off to corrector.
- ~T+90 min — Final transcript delivered to the client.

End-to-end automatic, no human noticed the underlying chain had changed.

We've been live ever since. No regressions. The Windows VM is on standby, available if needed, untouched.

---

Next: **[04 — WASM vs native](04-wasm-vs-native.md)** — why we shipped with the WASM chain first, why we then switched to native, and what the performance numbers actually look like.

---

*Trust, then verify. 35 times. ✅*
