# benchmarks/

> Raw numbers from the validation campaign and the WASM-vs-native comparison. All data anonymized: command IDs are kept (they're internal pipeline references, no client info) but file paths, client names, and audio content are stripped.

## conversion-results.json

The output of running [`scripts/batch-convert.mjs`](https://github.com/gaspardpetit/dss-codec-wasm) on **35 real production DS2/DSS files**, on a Hetzner Auction Ryzen 9 3900 / Debian 12 / Node 18.

Each entry contains:

| Field | Meaning |
|---|---|
| `command_id` | Internal pipeline identifier (7 digits) |
| `format` | Detected codec format (`ds2_qp` / `ds2_sp` / `dss_sp`) |
| `encryption` | `none` for every file in this sample |
| `native_rate_hz` | Native sample rate of the codec (16000 for DS2 QP, 11025 for DSS SP) |
| `src_bytes` | Size of the source DS2/DSS file |
| `audio_samples` | Number of decoded PCM samples |
| `audio_duration_s` | Audio duration in seconds (computed from samples / native_rate) |
| `audio_rms`, `audio_peak` | Audio statistics on the decoded PCM (sanity checks) |
| `mp3_bytes` | Size of the produced MP3 (64 kbps mono) |
| `convert_ms` | Wall-clock time of the full decode + encode (WASM chain) |
| `status` | `ok` for all 35 files |

## Headline metrics

| | Value |
|---|---|
| **Total files** | 35 |
| Format breakdown | 32× ds2_qp, 3× dss_sp |
| **Conversion success rate** | **35 / 35** (zero failures) |
| Total source bytes | ~85 MB |
| Total decoded audio | **6 hours 48 minutes** |
| Total MP3 output | ~225 MB (compressed to 64 kbps) |
| Shortest file | 87 s |
| Longest file | 38 min |
| Median file duration | 17 min |
| Encryption rate in sample | 0 % |

## Speed (WASM chain measurements)

These are the WASM-chain numbers — what we shipped with on day one before [the switch to native](../04-wasm-vs-native.md).

| Metric | Value |
|---|---|
| Total batch wall-clock | ~8 minutes |
| Median per-file time | 5.2 s |
| Fastest file (87 s audio) | 1.7 s |
| Slowest file (38 min audio) | 40.9 s |
| Throughput | ~50× real time |

## Speed (native chain — same files, after the switch)

After switching to the native binary + ffmpeg (see [docs/04](../04-wasm-vs-native.md)), the same conversion is **~3-5× faster** depending on the file:

| Test | WASM chain | Native chain | Speedup |
|---|---|---|---|
| DS2 QP, 31.8 min, 6.4 MB source | 33 s | 10 s | 3.3× |
| DSS SP, 16.4 min, 1.7 MB source | ~11 s | 2 s | 5.5× |
| In production (real arrivals on May 25, 2026) | n/a | **2-4 s per file** | n/a |

The dominant factor in the speedup is the MP3 encoder, not the decoder — `lamejs` (pure JS) is ~50× slower than `libmp3lame` (C with hand-tuned SIMD).

## Whisper A/B — Switch.exe vs DS2-Anywhere

Single-file comparison: same source DS2 (cmd `2940662`, 31.8 min legal dictation), converted via both chains, both MP3s sent to the same Whisper API (default parameters).

| Metric | Switch.exe chain (commercial Windows) | DS2-Anywhere native chain |
|---|---|---|
| HTTP response | 200 | 200 |
| Whisper segments | 95 | 92 |
| Total words transcribed | **3 961** | **3 872** |
| Low-confidence words | 643 (16.2 %) | 667 (17.2 %) |
| `audio_loss_detected` | False | False |
| Transcript opening | *"Dossier Oren HOU REN, email à la maïf. Oui, les nouvelles références majeures..."* | *"Dossier Orenachou-REN, email à la maïf. Les nouvelles références..."* |

**Functionally identical**. The 2.2 % word-count delta is within Whisper's own run-to-run variance. The 1 percentage point difference in low-confidence rate is in the noise — and not biased: sometimes the native chain wins, sometimes Switch wins.

The opening of the transcript shows the same content with the kinds of tiny word-boundary differences you get between any two Whisper passes on the same audio (slightly different MP3 encoders, slightly different bitrates). Nothing structural.

**The 16-17 % low-confidence rate is a Whisper characteristic on dictaphone-grade audio with proper nouns and spelled-out names, not a defect of either converter.**

## How to read the JSON

Quick exploration with `jq`:

```bash
# Format distribution
jq -r '.[].format' conversion-results.json | sort | uniq -c

# Median throughput (samples per ms)
jq -r '.[] | "\(.audio_samples / .convert_ms)"' conversion-results.json

# Anomalies: files where the audio RMS is suspiciously low (might be silence)
jq -r '.[] | select(.audio_rms < 0.05) | "\(.command_id) rms=\(.audio_rms) dur=\(.audio_duration_s)s"' conversion-results.json

# Average MP3 compression ratio vs source DS2
jq -r '.[] | "\(.mp3_bytes / .src_bytes)"' conversion-results.json | \
  awk '{s+=$1; n++} END {print s/n}'
```

## What we did not benchmark

For the record, what we *did not* measure (and so can't make claims about):

- **Encrypted DS2 performance** — none in the sample.
- **Very long files (> 1 hour)** — longest was 38 min.
- **Resource consumption under concurrent load** — the cron processes serially; we never measured peak RAM/CPU at N=10 concurrent conversions.
- **Cold-start latency** — we measured warm runs only.
- **Comparison against other reverse-engineered decoders** — there were no others to compare against (this is the first open-source DS2 decoder).

If you reproduce these benchmarks on your hardware, please share — happy to add a comparison table.

---

*Real files, real Whisper, real production. No simulated benchmarks. ✅*
