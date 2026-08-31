# Reading Digital Speech Standard: container, codec, and a framing bug that outlived every open implementation

*A technical account of how DSS and DS2 dictation files are structured, how they are decoded, and how a defect that silently corrupted a subset of recordings survived in every open decoder for years — including ours, after we published a confident and wrong diagnosis of it.*

> **Which of these should you read?** This is the reference document: precise, dense, meant to be cited. The same story told as it happened is **[chapter 17](17-the-framing-was-wrong.md)**; the reasoning generalised is **[How to circle a closed thing](HOW-TO-CIRCLE-A-CLOSED-THING.md)**; and if you only want to open a file, **[start here](TRANSCRIBE-A-DSS.md)**.

---

## Abstract

Digital Speech Standard (DSS) and its successor DS2 are the recording formats of handheld dictation machines — Olympus, Philips, Grundig — used for three decades by clinicians, lawyers, surveyors and police. Both were proprietary. Between 2024 and 2026 a loose relay of independent contributors reverse-engineered them from vendor binaries, producing the first open decoders.

This paper documents the formats as we now understand them, and reports a defect present in **every** open implementation we examined — FFmpeg's `dss_sp.c` and `dss.c`, the Rust `dss-codec` crate, and the npm and PyPI ports derived from it. Each 512-byte audio block declares its own framing in three header fields; all of them discarded those fields and derived framing by running on from the first frame to the last. The two agree on any recording captured without interruption, which is why the defect went unnoticed. They stop agreeing at the block where a paused or edited recording resumes, after which every remaining frame is read one byte out of phase and decoded into unbounded noise.

We also correct the declared output rate, 11025 Hz since the FFmpeg decoder was written in 2014, to its true value of 11000 Hz.

Measured against a commercial reference decoder on seven recordings, mean sample correlation rises from 0.5849 to 0.9995; the three affected files move from ~0.03 to ~0.999 and their clipped-sample fraction falls from ~0.6% to 0.001%. Across 1685 production recordings, every file containing audio now decodes. The corrected chain is closer to the recorders' own declared durations than the commercial reference is.

We report the investigation honestly, including a published wrong diagnosis, because the failure mode is instructive and general: **a stabilising change applied to a system fed corrupt input will damp the symptom while leaving the cause untouched.**

---

## 1. Why this format was a problem

A dictation machine is an unusual audio device. It optimises for hours of intelligible speech on small storage, from a single close-microphone speaker, with hardware-button editing — insert, overwrite, resume — performed on the device itself. The formats reflect that: aggressive speech-specific compression, and a container designed to be appended to and edited in place by firmware with little memory.

For thirty years the only way to read one was the vendor's Windows software. That is a preservation problem (archives hold recordings they cannot open), an operational problem (a transcription pipeline forced through a Windows VM), and an access problem (the person who dictated cannot read their own file without a licence).

The open work proceeded in stages, and credit for the hard part is not ours (§11). What follows is the state of knowledge as of August 2026, plus our contribution: the container's framing rules, the output rate, and the fix.

---

## 2. The container

### 2.1 File layout

A DSS file opens with a header whose size is given by **byte 0, counted in 512-byte blocks**. Two and three are what common recorders write; larger headers exist, and refusing them was itself a bug we had to fix — files with a 6- or 7-block header (Grundig/Philips) were being rejected as unrecognised.

Bytes 1–3 are the ASCII magic `dss` (or `ds2`). The header carries metadata that turns out to be more useful than it looks:

| Offset | Content |
|---|---|
| `0x0C` | author / device identifier |
| `0x26` | recording start time, ASCII `YYMMDDhhmmss` |
| `0x32` | recording end time, same encoding |
| `0x3E` | **recording length, ASCII `HHMMSS`** |
| `0x2A4` | audio codec byte |
| `0x31E` | free-text comment |

The field at `0x3E` matters for §4: the recorder writes down how long it recorded. That gives an external check on any decoder, requiring no reference implementation.

After the header, the file is a flat sequence of **512-byte blocks**. Each begins with a **six-byte header**, leaving 506 bytes of payload. Audio frames are laid end to end across the *concatenation of payloads* — a frame may begin in one block and finish in the next, and the six header bytes in between are not part of it.

### 2.2 The block header

Three of the six bytes describe framing, and they are the subject of this paper:

| Byte | Meaning |
|---|---|
| 0, bit 7 | **byte-swap parity** of the first whole frame beginning in this block |
| 1 | with the parity bit, gives the **continuation length** — how many leading payload bytes still belong to the frame that began in the previous block |
| 2 | **frame count** — how many frames this block contains |
| 4 | frame mode (selects the codec; see §2.4) |

The continuation length is computed as:

```
cont = 2 × byte1 + 2 × parity − 6
```

so the first whole frame of the block starts at file offset `block_start + 6 + cont`. A block that begins exactly on a frame boundary declares `cont = 0`.

**Every block re-states all three.** They are, on an undisturbed recording, redundant with simply walking forward from the previous frame. That redundancy is the trap (§5).

### 2.3 Byte-swap framing

DSS SP frames are 41 bytes. They are not stored as 41-byte units. The stream alternates between reads of **42 and 40 bytes**, carrying one byte across the boundary — a parity that flips frame to frame, averaging the required 41. Concretely, with parity 0 the decoder consumes 42 bytes and remembers byte 40 as the *swap byte*; with parity 1 it consumes 40 bytes, shifts them, and re-inserts the remembered byte at position 1.

This is why the parity bit exists in the block header, and why losing it is not a recoverable rounding error but a permanent one-byte phase shift.

### 2.4 Frame modes: DSS LP is not a mystery

Byte 4 of each block header selects a frame size from a table we located at `DAT_1002ce20` in the vendor's `DssParser.dll`. Mode 0 is 328 bits — the 41-byte DSS SP frame. **Mode 2 is 192 bits, which is G.723.1**; modes 3 and 5 add its 4-byte silence-insertion frame.

So "DSS LP", written by older recorders such as the Olympus DS-4000, is not a proprietary codec awaiting reverse engineering. It is a published ITU standard in a different wrapper, and any recent `ffmpeg` decodes it. Recognising this converted an open problem into a routing decision.

### 2.5 Headerless files

Recordings exist whose file header was lost — transfer accidents, partial recovery — leaving a file that begins directly with audio blocks. These are recoverable, because the container is self-describing: on each block, byte 3 is `0xFF`, byte 4 is the mode, and crucially **the continuation a block leaves must be exactly the one the next block announces**. That chain does not hold by chance. Twenty-four consecutive blocks are enough to identify such a file with confidence, and a legitimate recording cut breaks the chain in a way random data does not imitate.

One 1.5 MB file in our corpus was being rejected as encrypted on the strength of its first four bytes. It contained fifteen minutes of perfectly readable dictation.

---

## 3. The DSS SP codec

DSS SP is a CELP-family speech codec: it transmits filter coefficients and an excitation description rather than waveform samples, and rebuilds speech by driving a synthesis filter with that excitation.

### 3.1 Frame layout

A frame is exactly **328 bits**, and every bit is accounted for:

| Field | Bits | Notes |
|---|---|---|
| Reflection coefficients | **52** | 14 coefficients: 2×5 + 6×4 + 6×3 bits |
| Per subframe, ×4 | **63 each = 252** | see below |
| Combined pitch | **24** | delta-coded across subframes |
| **Total** | **328** | = 41 bytes |

Each of the four subframes carries:

| Field | Bits |
|---|---|
| adaptive gain index | 5 |
| combined pulse position | **31** |
| fixed gain index | 6 |
| pulse amplitudes, 7 × 3 | 21 |

### 3.2 Decode pipeline

Per frame, in order:

1. **Unpack** the bitstream into coefficient indices, per-subframe parameters and the combined pitch.
2. **Unpack the filter** — map the 14 quantised indices to reflection coefficients through the codebook.
3. **Convert coefficients** — reflection coefficients to direct-form LPC.
4. Per subframe (4 × 72 samples):
   - **`gen_exc`** — build the adaptive (pitch) excitation from history at the decoded lag.
   - **`add_pulses`** — add the 7 fixed-codebook pulses at their decoded positions and amplitudes.
   - **`sf_synthesis`** — run the direct-form LPC synthesis filter.
   - **`shift_sq_sub`** and the error-buffer updates — maintain the filter's state, including the clamped error feedback that keeps it stable.
5. **`update_state`** — 11:12 sinc decimation, and history maintenance.

288 samples in (4 × 72 at 12000 Hz), **264 out**. That ratio is the whole of §4.

### 3.3 The combined pulse position, and why it is a free oracle

The seven fixed-codebook pulse positions are transmitted as a single combined index over 31 bits. Seven positions chosen from 72 slots gives

```
C(72,7) = 1 473 109 704
```

while a 31-bit field holds up to 2 147 483 647. **There is a gap of 674 373 943 values that encode no valid combination.**

A correctly framed bitstream can never produce one. That makes the test a *hard constraint* rather than a similarity measure, and it needs no reference decoder, no listening, and no time alignment. It is the single most useful instrument in this investigation (§6.2).

### 3.4 Direct form, not lattice

The synthesis filter is a 14th-order **direct-form** recursion with clamped error feedback. We say this explicitly because we previously argued, in public, that it should be a lattice. It should not; see §6.1.

---

## 4. The output rate is 11000 Hz

FFmpeg has declared DSS SP as 11025 Hz since the decoder was contributed in 2014, and every port inherited it. It is wrong.

**From the codec.** Synthesis runs at 12000 Hz and the output stage decimates by 11:12, taking 288 samples per frame to 264. Therefore

```
12000 × 11 / 12 = 11000 Hz exactly
```

and a frame is 264 / 11000 = **24.0 ms**. At 11025 Hz the same frame would be 23.9456 ms, a figure nothing in the format accounts for.

**From the container.** Independently: divide the decoded sample count by the length the recorder wrote at offset `0x3E`. Over **124 recordings longer than ten minutes**, the median implied rate is **11001.6 Hz**. The declared length is truncated to the second, so a ten-minute file constrains the rate to ±9 Hz; 11025 sits 23 Hz away, outside the margin across the great majority of the corpus.

Use the median, not the mean: recordings with missing blocks drag the mean toward 10900 and tell you nothing about the rate.

**Consequence.** 11025 plays every DSS SP file **0.23% fast** — about three seconds of drift over a 23-minute dictation, enough to pull a transcript's timestamps away from its audio. On one 18-minute recording, the commercial reference decoder produced a file 3.1 s shorter than the recorder's own declared length; our corrected chain lands 0.1 s away.

---

## 5. The defect

### 5.1 Statement

Every open implementation reads the six block-header bytes, discards them, and derives framing by running on from the previous frame — from the first block of the file to the last.

On a recording captured without interruption, the running walk agrees with the declared framing at **every single block**. The header fields are redundant, the walk is correct, and the file decodes perfectly. That is the overwhelming majority of files, and it is precisely why this survived years of use.

On a recording **paused, resumed, or edited on the device**, the block where recording resumes re-states the framing — a different continuation length, a different parity. The running walk does not look. From that block onward, every frame is read one byte out of phase for the remainder of the file.

### 5.2 Why it sounds the way it does

A one-byte shift does not degrade the audio gracefully. It re-partitions the 328-bit frame: reflection-coefficient indices land in pulse-position fields, pulse positions land in gain fields, pitch lags become arbitrary. The synthesis filter is then asked to make speech from coefficients that were never written.

It does what any resonant filter does with noise in its coefficients: it rings, and the energy grows. In our corpus the affected files reached ~0.6% of samples clipped against the 16-bit rails, worsening with duration — which is exactly the profile of an accumulating numerical instability, and exactly why we misdiagnosed it (§6.1).

### 5.3 A second, milder effect

Ignoring the frame count has a separate consequence, present on *all* files: the walk reads past the last recorded frame of a block and decodes whatever padding follows, manufacturing audio that was never captured. This is why some files decode noticeably longer than their own declared duration, and why honouring the frame count makes some outputs shorter — correctly so.

### 5.4 The fix

Believe the block. For each audio block: start at the offset its header declares, with the parity its header gives, and emit exactly the number of frames it declares — letting frames straddle boundaries by stepping over the six header bytes they meet.

Where headers and walk agree, this costs nothing and produces bit-identical output. Where they disagree, it keeps the walk in step. In FFmpeg it is about a hundred lines in `libavformat/dss.c`.

---

## 6. Method

### 6.1 The wrong answer, and why it was convincing

Unbounded energy growth in a CELP decoder points at the synthesis filter, and direct form is the structure textbooks warn about: quantisation error feeds back through the recursion and can compound. The hypothesis wrote itself — accumulated Q15 error, worse on longer files, matching the observation exactly. So did the remedy: rebuild the synthesis as a lattice, unconditionally stable for reflection coefficients inside the unit circle, and a natural fit for coefficients the bitstream already carries in that form.

It worked, in the sense that the symptom improved substantially. Energy stopped running away; the energy ratio against the reference fell from 2.6–5.3× to 1.3–1.5×. We published it: a patch to `ffmpeg-devel`, a correction on the upstream issue tracker, and a pull request that was merged into the shared crate.

It was wrong about which component was broken.

> **A stabilising change applied to a system fed corrupt input will damp the symptom without touching the cause.**

A lattice filter fed reflection coefficients that were never written still produces nonsense — merely *bounded* nonsense. The number that should have governed the decision was sample correlation against the reference, and it read **0.03 before the change and 0.03 after**. 0.03 is not a decoder that is nearly working; it is a decoder emitting an unrelated signal. Both numbers were available throughout. We watched the one that was moving.

### 6.2 What actually broke it open

Abandoning audio metrics. Correlation is a *similarity* measure and degrades gracefully, which makes it poor at distinguishing "nearly right" from "completely unrelated" — the exact discrimination we needed.

The combined-pulse-position constraint (§3.3) is not a similarity measure. It cannot be nearly satisfied. Applied frame by frame it reported impossible indices scattered through the bad files and none at all in the good ones, in minutes.

That relocated the problem decisively: the frames were wrong before they ever reached the filter. Every hypothesis about synthesis had been irrelevant.

### 6.3 Reading the container rather than guessing

With the search narrowed to framing, the remaining question was what the correct framing *is* — and the container is undocumented. Two vendor libraries, `DssDecoder.dll` and `DssParser.dll`, went through Ghidra; the parser decompiled to 436 functions. That yielded the frame-size table (§2.4), the meaning of the block header bytes (§2.2), and the 12000 Hz / 11:12 pipeline that settles the rate (§4).

We note the asymmetry honestly: the decompilation told us what the fields *are*. It did not tell us they were being ignored. That came from the oracle.

### 6.4 Two modelling errors worth recording

Both cost hours, and both are easy to repeat:

**The header field is a continuation length, not an offset.** When a frame straddles a boundary, the *next* block declares `frame_size − bytes_already_read`, not the bytes already read. Comparing against the wrong quantity produced ~2000 spurious "disagreements" on files that were decoding perfectly, which nearly discredited the whole line of enquiry.

**Detecting a disagreement and discarding the frame does not work.** It repairs the phase but loses a frame each time, shifting everything after it. On healthy files this converted a correct decode into a correct-but-displaced one: clipping fell to zero while correlation collapsed to 0.04. That pairing — *no clipping, no correlation* — is the signature of audio that is right but misaligned, and it is worth recognising on sight. The correct structure re-seeds position **and** parity per block and emits exactly the declared frame count, losing nothing.

---

## 7. Validation

We used four independent instruments, deliberately chosen so that no two share a failure mode.

**1. Reference correlation.** Sample-level correlation against a commercial decoder's output on the same source files, measured in three windows with a local lag search. Detects gross divergence; insensitive to constant gain; poor at distinguishing degrees of wrongness (§6.1).

**2. The impossible-index constraint** (§3.3). Binary, needs no reference, no alignment. Detects misframing directly.

**3. Declared duration** (§2.1, `0x3E`). Compares decoded length against what the recorder itself wrote. Independent of any decoder, ours or the vendor's — the instrument that settled the sample rate and later proved that production had been delivering truncated audio.

**4. Regression against the existing test suite.** FFmpeg's `fate-dss-sp` and `fate-dss-lp` must remain bit-identical, since the test sample is an undisturbed recording where the declared framing and the running walk agree. A change that alters that output is wrong by construction.

Corpus: 1685 production recordings from Olympus and Philips devices, 5 to 68 minutes, on a server we control. Recordings were never copied off it.

---

## 8. Results

### 8.1 Against a reference decoder, seven recordings

| | before | after |
|---|---|---|
| mean sample correlation | 0.5849 | **0.9995** |
| the three misframed files | 0.03 / 0.03 / 0.04 | 0.9998 / 0.9990 / 0.9997 |
| clipped samples, those three | ~0.6% | 0.001% |

Of the four that already decoded well, **three change by not a single sample**; two of those also shed a short tail (2112 and 1320 samples) of padding read past the last declared frame. The fourth is the instructive one: it looked healthy, and held a misframed stretch that the sampled measurement windows had walked straight past. Two percent of its samples change and its clipped fraction falls from 0.016% to 0.001%.

**A file can be damaged without ever sounding obviously broken.** Spot checks are not coverage.

### 8.2 Regression tests

`fate-dss-sp` and `fate-dss-lp` pass unchanged. The framing patch alters the reference by nothing at all; the rate patch alters exactly two lines, the time base and the declared rate, leaving all thirty frame CRCs and all thirty timestamps untouched.

### 8.3 Corpus coverage

Of 1685 files, **1678 decode**. The seven that do not contain no audio: two images renamed (`.dss` holding PNG and JPEG magic), one file of 10 bytes, two of **0 bytes**, and two truncated to their headers — one of which declares a 7680-byte header in a 1024-byte file.

Nothing that decoded before stopped decoding.

### 8.4 Against the recorders themselves

On a 23-minute dictation whose header declares 1404 s: the commercial reference produces 1400.9 s (**−3.1 s**); the corrected chain produces 1404.1 s (**+0.1 s**). Sample-for-sample the two agree at 0.9998 with identical sample counts — the same audio, at the right speed.

The open implementation is now closer to the recorder's own account of itself than the commercial one is.

---

## 9. Implications for other implementations

The defect is in the demuxing layer and is therefore inherited by anything built on it:

- **FFmpeg** — `libavformat/dss.c`. A two-patch series (framing, rate) is on `ffmpeg-devel`.
- **`dss-codec`** (Rust, upstream) — fixed and merged.
- **npm `dss-codec`, PyPI `pydsscodec`** — ports of the crate; they inherit the demuxer and carry the defect until they re-vendor.
- **Anything else reading DSS** — the test is cheap: decode a recording that was paused mid-capture and look for clipping in the second half, or run the constraint of §3.3 over the frames.

If you maintain such an implementation: the fields are in the block header, they are authoritative, and they are free.

---

## 10. Limits and open questions

We state these plainly rather than let the results imply more than they support.

**The reference is not ground truth.** The commercial decoder is a comparison point, not an oracle — §8.4 shows it is measurably wrong about duration. Agreement with it is evidence, not proof.

**"Bit-exact" is claimed only where measured.** For the Grundig SP codec we verified byte-for-byte identity against the vendor decoder. For DSS SP we have not; we have 0.999 correlation, matching durations, and no impossible indices. That is strong, and it is not the same claim.

**Truncated files are not recovered.** A file cut mid-block is not reconstructed; we detect and report rather than guess. Reconstructing partial frames is possible in principle and we have not attempted it.

**We use FFmpeg's codec tables, not vendor-extracted ones.** Earlier in this project we claimed those tables deviate by 1–5% from the vendor's and proposed compensating for it. That claim was made during the period when we were misdiagnosing the whole problem, and the present results — 0.999 correlation with FFmpeg's tables unchanged — indicate it was not the limiting factor, if it was ever real. We have not re-measured it. Treat any table-deviation figure in this project's earlier chapters as unverified.

**Frame modes 3 and 5** (G.723.1 with silence-insertion frames) are routed to `ffmpeg` rather than handled natively, and we have exercised them on few files.

**In-browser DSS LP** is deliberately unsupported: shipping a second codec into a page whose purpose is the codec nobody else could read is a poor trade. The page names the format and gives the `ffmpeg` command.

---

## 11. Provenance and credit

The hard part is not ours.

- **Oleksij Rempel** wrote the DSS SP decoder in FFmpeg in 2014 — for twelve years the only open implementation, and correct on every file anyone tested. The defect described here is in the demuxer, not his codec.
- **Kieran Hirpara** reverse-engineered DS2 and produced the first modern open decoder.
- **Patrick Domack** and **Gaspard Petit** made it portable — FFmpeg patches, WASM, npm and PyPI ports.
- **heer-gielisch** and **wkochFPV** contributed format work; a German lawyer's forgotten recorder produced the Grundig variant that neither we nor the commercial software could read.

Our contribution: the container's framing rules and the fix, the output rate, the Grundig SP codec, two further device families, a production pipeline, and the report of our own wrong diagnosis.

That last item is not modesty. A reverse-engineering record whose dead ends have been tidied away is worth less than one where they are marked, because the next person is standing at the same fork.

---

## Appendix: reproducing the measurements

**The impossible-index test.** Decode the 31-bit combined pulse position of each subframe and compare against `C(72,7) = 1473109704`. Any greater value means the frame is misaligned. No reference required.

**The declared-duration test.** Read the ASCII `HHMMSS` at offset `0x3E`; divide decoded samples by that length. Median over files longer than ten minutes; the truncation margin is `5500 / T` Hz for a file of `T` seconds.

**The regression test.** `make fate-dss-sp fate-dss-lp SAMPLES=…` in an FFmpeg tree. The framing change must leave the reference untouched.

**A misframing reproducer.** Any recording paused and resumed on the device. Decode with and without the block-driven walk, and measure the clipped-sample fraction in the second half.

---

*Written up in [chapter 17](17-the-framing-was-wrong.md) as a narrative; the wrong turn is preserved in [chapter 16](16-the-q15-instability.md) and [The Lattice Hunt](THE-LATTICE-HUNT.md), each behind a banner, on purpose.*
