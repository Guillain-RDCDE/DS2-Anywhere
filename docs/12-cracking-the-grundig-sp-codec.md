# 12 — Cracking the Grundig SP codec (the one nobody decoded)

The `.ds2` side of the Grundig/Philips story closed cleanly: the
[GR/PH container fix](11-the-grundig-philips-variant.md) made the QP audio
decode bit-exact, because QP is the same CELP as Olympus. Then a Grundig **Digta
415** owner ([hirparak/dss-codec#11](https://github.com/hirparak/dss-codec/issues/11))
tried a `.dss` (SP) file from the same device and got **pure noise** — not just
from our pipeline, but from FFmpeg's `dss_sp` and from NCH Switch too.

That ruled out the easy theory. The Grundig **SP** path is a *genuinely different
codec* from Olympus DSS-SP — a separate CELP, at a different rate. No open tool
on earth decoded it. So we reverse-engineered it.

## The oracle

You can't reverse a codec without a reference. Grundig's own `dss2wav.dll`
(shipped in DigtaSoft) is that reference. We extracted it from an archived
DigtaSoft One DVD (no install — `7z` + `cabextract`), ran it under Wine, and
confirmed it decodes the samples to clean 16 kHz speech (the Digta 415 sample
is just *"This is a test."*). That gave us a bit-exact target.

The decisive trick: the DLL writes three internal temp files during a decode and
deletes them at the end. We patched its `DeleteFileA` thunk to `xor eax,eax; ret`,
so the temps survived:

- `*.dwa` — the unpacked CELP bitstream
- `*.dwb` — 12 kHz PCM (after CELP synthesis)
- `*.dwc` — 16 kHz PCM (after resample)

Three exact checkpoints, one per pipeline stage. We could now validate each stage
of our reimplementation **independently** before chaining them.

## The format (Grundig PH9607, `\x06dss`)

**Container.** First byte = header size in 512-byte blocks (`6` → `0xC00`
header). Audio is 512-byte blocks, each a 6-byte header `[b0][b1][b2] ff 00 ff`
followed by 506 payload bytes:

- `b2` = number of CELP frames whose start lies in this block
- `b0` bit 7 = a channel/silence flag
- `(b1<<8 | b0) >> 4` = the **bit offset** at which the first whole frame starts
  in the block — frames are a continuous bitstream and **span block boundaries**

**Stage 1 — unpack.** Concatenate the 506-byte payloads into one bitstream and
cut it into **328-bit (41-byte) frames**, honouring each block's declared start
offset and dropping trailing padding. `nframes = Σ b2`.

**Stage 2 — CELP synthesis** (4 subframes × 72 samples = 288 @ **12 kHz**), per
328-bit frame:

- **14 reflection-coefficient indices**, bit widths `[5,5,4,4,4,4,4,4,3,3,3,3,3,3]`,
  into a 14×32 table of doubles (range ±1).
- A 24-bit field → base-151 / base-48 → **4 differentially-coded pitch lags**.
- Per subframe: 5-bit adaptive-codebook gain index, **31-bit fixed-codebook
  index**, 6-bit fixed gain index, and 7×3-bit pulse signs.
- Excitation = adaptive codebook (past excitation repeated at the pitch lag,
  scaled by a gain table) **+** fixed codebook = **7 pulses decoded
  enumeratively** from the 31-bit index via cumulative binomial tables, with
  amplitude/sign tables. Unvoiced frames use an LCG PRNG (`x = x*0x209 + 0x103`).
- **14th-order lattice (reflection) synthesis filter**, then 1-pole de-emphasis
  `y[n] = x[n] + 0.1·y[n-1]`, rounded `floor(x + 0.5)`, clamped ±32767.

**Stage 3 — resample.** A 3:4 rational **polyphase FIR**, 100 taps per phase,
4 phases, steps the audio from 12 kHz to **16 kHz** (288 → 384 samples/frame).

Every stage is integer + IEEE-754 `double` arithmetic with deterministic
`floor`/clamp rounding — so a faithful transcription is bit-exact, not merely
high-SNR.

## Result

[`grundig/grundig_dss.py`](../grundig/grundig_dss.py) — pure Python, no Wine, no
DLL — decodes Grundig `.dss` to 16 kHz mono WAV. The quantization tables are
extracted to [`grundig/gtables.json`](../grundig/gtables.json).

| sample | samples | max abs diff vs oracle | corr | Whisper |
|---|---|---|---|---|
| Digta 415 test | 57 984 | **0** | 1.0000000000 | "This is a test." |
| welcome (EN) | 453 888 | **0** | 1.0000000000 | "Welcome. This professional speech processing software…" |
| willkommen (DE) | 404 352 | **0** | 1.0000000000 | "Herzlich Willkommen! Diese professionelle Sprachverarbeitungssoftware…" |

The output WAVs are **byte-for-byte identical** to Grundig's own decoder
(`cmp` clean, headers included), on every sample. A genuinely bit-exact native
reimplementation of a codec that, until now, only Grundig's Windows software
could read.

## Upstream

A native FFmpeg decoder followed directly from the spec:
[`ffmpeg-upstream/patches/avcodec-grundig_sp-decoder.patch`](../ffmpeg-upstream/patches/avcodec-grundig_sp-decoder.patch)
— a new `AV_CODEC_ID_GRUNDIG_SP` decoder (`libavcodec/grundig_sp.c`) plus the
`libavformat/dss.c` demuxer wiring (gated on header version 6, Olympus DSS left
untouched), with a FATE test. It emits the codec's native 12 kHz and leaves the
3:4 → 16 kHz device resample to libswresample. Its output is **bit-exact** to
this Python reference (hence to Grundig's own decoder) on every sample; it builds
clean and `git am`s onto FFmpeg master. Sent to ffmpeg-devel.

A Rust port (to sit beside the Olympus codec in `dss-codec`) is the same direct
transcription — `f64` throughout, `floor(x + 0.5)` rounding — and remains the one
open follow-up.
