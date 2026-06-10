# Grundig DSS-SP (PH9607) — format specification

**Status:** clean-room specification, derived by reverse-engineering and verified
**bit-exact** against Grundig's own decoder (`dss2wav.dll`, shipped in DigtaSoft)
on every available sample. This document is descriptive; the **normative
reference** is the implementation [`grundig/grundig_dss.py`](../grundig/grundig_dss.py)
together with the extracted quantization tables
[`grundig/gtables.json`](../grundig/gtables.json). Where prose and code disagree,
the code wins — it is the artefact proven byte-for-byte identical to the vendor.

This is, to our knowledge, the first public specification of this codec. It is not
endorsed by or affiliated with Grundig. No vendor code is reproduced here; only the
recovered behaviour is described. See
[docs/12 — Cracking the Grundig SP codec](12-cracking-the-grundig-sp-codec.md) for
how it was obtained.

---

## 1. Scope

Grundig's *Digital Speech Standard – Standard Play* ("DSS-SP", internal tag
**PH9607**) is the speech codec used by Grundig **Digta** professional
dictation recorders (e.g. Digta 415) in their `.dss` files. It is **not** the same
codec as Olympus DSS-SP, despite the shared `.dss` extension and `dss` magic —
it is a distinct CELP at a distinct rate, and no Olympus/FFmpeg/NCH decoder
produces intelligible output from it.

This spec covers the **Standard Play** (SP) variant only. Grundig devices can also
emit a TrueSpeech variant (`CDssTrueSpeechCodec` in the vendor library) and a
Quality Play (QP) variant; QP is the same CELP family as Olympus and is handled by
the existing DS2 path. They are out of scope here.

Decoded output is **16 kHz, mono, 16-bit PCM**.

## 2. Identification

| Bytes at offset 0 | Meaning |
|---|---|
| `06 64 73 73` (`\x06` `d` `s` `s`) | Grundig DSS-SP container (this spec) |
| `02 64 73 73` / `03 64 73 73` | Olympus DSS family (different codec) |
| `03 64 73 32` (`\x03` `d` `s` `2`) | Olympus DS2 |
| `03 65 6e 63` (`\x03` `e` `n` `c`) | Encrypted DS2/DSS (out of scope) |

The first byte is **not** part of an ASCII tag: it is the **header size in
512-byte blocks**. For the Grundig SP files observed it is `0x06`, giving a
`6 × 512 = 3072`-byte (`0xC00`) header. A robust identifier therefore checks
`byte[1..4] == "dss"` **and** routes on the device/codec tag inside the header;
the practical discriminator used by this project is the leading
`06 64 73 73` sequence.

## 3. Container structure

```
+----------------------------------+  offset 0
|  header :  (byte[0]) * 512 bytes  |   e.g. 0xC00
+----------------------------------+  offset header_size
|  audio  :  N x 512-byte blocks    |
+----------------------------------+
```

The header carries device metadata and is not required for audio reconstruction
beyond its size (`byte[0] * 512`). Audio begins immediately after it.

### 3.1 Audio block (512 bytes)

Each audio block is a 6-byte block header followed by 506 payload bytes:

```
 byte 0 : b0
 byte 1 : b1
 byte 2 : b2
 byte 3 : 0xFF
 byte 4 : 0x00
 byte 5 : 0xFF
 byte 6..511 : 506 payload bytes (bitstream fragment)
```

| Field | Definition |
|---|---|
| `b2` | number of CELP frames whose **start** falls inside this block |
| `b0` bit 7 (`0x80`) | channel / silence flag |
| `(b1 << 8 \| b0) >> 4` | **bit offset**, within this block's payload, at which the first *whole* frame begins |

The bytes `FF 00 FF` at offsets 3–5 are a fixed block marker.

### 3.2 Bitstream

The 506-byte payloads of all audio blocks form **one continuous MSB-first
bitstream**. CELP frames are **328 bits (41 bytes)** each and **span block
boundaries** freely. Each block declares, via its `(b1<<8|b0)>>4` field, the bit
offset at which the first frame *starting in that block* begins; this is used to
align to frame boundaries and to skip inter-block padding. The total frame count
is `nframes = Σ b2` over all blocks. Trailing padding after the last whole frame
is discarded.

## 4. Frame decoding — CELP synthesis (12 kHz)

Each 328-bit frame synthesises **4 subframes × 72 samples = 288 samples at
12 kHz**. All arithmetic is integer plus IEEE-754 `double`; rounding is
`floor(x + 0.5)` and the final sample is clamped to `[-32768, 32767]`. The fields,
in bitstream order:

### 4.1 Spectral envelope — 14 reflection coefficients

14 indices are read with bit widths

```
[5, 5, 4, 4, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3]   (total 52 bits)
```

Each index selects a `double` in `[-1, 1]` from a per-coefficient quantization
table (a 14×32 table; rows for the 3-bit coefficients use 8 entries). The 14
values are the reflection (PARCOR) coefficients `k[0..13]` of the synthesis
lattice. Tables are in `gtables.json`.

### 4.2 Pitch lags

A 24-bit field is decoded as a mixed radix **base-151 / base-48** number into
**4 differentially-coded pitch lags**, one per subframe (the first is absolute,
the rest are differential).

### 4.3 Per-subframe excitation (× 4)

Each subframe carries:

| Field | Bits | Use |
|---|---|---|
| adaptive-codebook gain index | 5 | indexes a gain table |
| fixed-codebook index | 31 | enumerative index of 7 pulses |
| fixed-codebook gain index | 6 | indexes a gain table |
| pulse signs | 7 × 3 | sign/amplitude selectors for the 7 pulses |

**Adaptive (pitch) contribution.** The past excitation buffer is repeated at the
subframe's pitch lag and scaled by the decoded adaptive gain.

**Fixed (innovation) contribution.** The 31-bit index is expanded
**enumeratively** into **7 pulses** over the 72-sample subframe using cumulative
**binomial** tables (the index is decoded as a combinatorial rank), each pulse
given an amplitude/sign from the 7×3-bit sign field, scaled by the decoded fixed
gain.

**Unvoiced frames** replace the structured innovation with a linear-congruential
PRNG: `x = (x * 0x209 + 0x103) mod 2^32`.

Total excitation = adaptive + fixed contribution.

### 4.4 Synthesis filter and post-processing

1. **14th-order lattice synthesis** driven by the reflection coefficients
   `k[0..13]` and the excitation.
2. **1-pole de-emphasis:** `y[n] = x[n] + 0.1 · y[n-1]` (state carried across
   subframes and frames).
3. **Round** `floor(y + 0.5)`, **clamp** to `[-32768, 32767]` → 16-bit PCM @ 12 kHz.

## 5. Resampling — 12 kHz → 16 kHz

A **3:4 rational polyphase FIR**: 4 phases, **100 taps per phase**, upsamples each
288-sample (12 kHz) frame to **384 samples (16 kHz)**. Filter coefficients are in
the reference implementation. The de-emphasis/filter state and the resampler state
are continuous across the whole stream.

## 6. Output

PCM **16-bit signed, mono, 16 000 Hz**, little-endian, wrapped in a canonical
44-byte WAV header. The reference decoder's WAV output is byte-for-byte identical
to Grundig's `dss2wav.dll` output, header included.

## 7. Conformance / test vectors

A decoder conforms if its WAV output is byte-for-byte identical to the reference
on the published samples:

| Sample | Samples (16 kHz) | Whisper transcript |
|---|---|---|
| Digta 415 "test" | 57 984 | "This is a test." |
| welcome (EN) | 453 888 | "Welcome. This professional speech processing software…" |
| willkommen (DE) | 404 352 | "Herzlich Willkommen! Diese professionelle Sprach­verarbeitungs­software…" |

Max absolute sample difference vs the vendor decoder: **0**. Correlation: **1.0**.

## 8. Reference implementation

- Python (normative): [`grundig/grundig_dss.py`](../grundig/grundig_dss.py) + [`grundig/gtables.json`](../grundig/gtables.json)
- C / FFmpeg: [`ffmpeg-upstream/patches/avcodec-grundig_sp-decoder.patch`](../ffmpeg-upstream/) (`AV_CODEC_ID_GRUNDIG_SP`)
- Rust: [hirparak/dss-codec PR #12](https://github.com/hirparak/dss-codec/pull/12)

All three are independently verified bit-exact against the same reference.

---

*Specification authored from black-box reverse-engineering of the vendor decoder,
cross-checked at three internal pipeline stages (bitstream → 12 kHz PCM → 16 kHz
PCM) against checkpoints recovered from the decoder itself. No proprietary code is
reproduced. Corrections and additional device samples welcome via issues.*
