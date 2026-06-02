# 06 — The empty-block bug: a decoder that was right for 99% of files, and how the other 1% nearly slipped through

> A production decoder can be bit-exact against the reference on every file you tested, ship to production, get submitted upstream to FFmpeg — and still be wrong, in a way that only shows up on the kind of recording nobody happened to test with. This is the story of that bug, told the way it actually happened, because the *method* of finding it is more reusable than the fix itself.

The fix is twelve lines. Getting to those twelve lines took a full day and ten dead ends. The dead ends are the interesting part.

---

## The report

A user flagged a single dictation: *"the second half of the audio is garbled."* Not silent, not truncated — garbled. The kind of broken where you can tell something is there but it's drowning in noise.

First instinct: the file was truncated in transfer. Wrong — the file decoded to its full declared length. Second instinct: the source recording itself is corrupt. Also wrong, and proving *that* was the first real step.

**Lesson 0, before any debugging: get a ground truth.** We still had the licensed Olympus decoder (NCH Switch) on a standby VM — the very thing this whole project replaced. So we decoded the same file two ways: our native Rust decoder, and Switch. Sample-aligned. Then we measured the signal-to-noise ratio between them, windowed across the file.

```
region            SNR (ours vs Olympus)
0 – 52.5 s        +77 dB     ← bit-exact (±2 LSB, just float rounding)
52.6 s → end      −5 dB      ← total decorrelation
```

That table reframed everything. Up to 52.5 seconds we were **bit-exact** to the reference. Then, at one specific instant, we fell off a cliff and never recovered. The source wasn't corrupt — Olympus decoded the whole thing cleanly. *We* broke, at 52.6 seconds, on one file, after being perfect up to that point.

A sharp, permanent divergence at a single timestamp is a *gift*. It means there's a discrete event in the bitstream that throws the decoder off — not a slow drift you'd have to chase statistically. Find the event, find the bug.

---

## Ten things it wasn't

Here's where humility pays. The synthesis filter in a CELP decoder is an IIR filter — feedback, internal state, the works. When the output explodes, the obvious suspect is filter instability. We chased it. We chased a lot of things. Every one of these was a real experiment, run against the Olympus ground truth, and every one failed to reproduce it:

1. **Clamp the long-term-predictor (pitch) memory to int16.** No effect.
2. **Clamp the lattice synthesis accumulator per sample.** No effect.
3. **Reset the entire filter state** (lattice + pitch memory + de-emphasis) right after the divergence. No effect — and *this one was decisive*: if resetting the state doesn't help, the state isn't the problem. The *decoded parameters themselves* are wrong from that point on. That's a **bitstream-level desync**, not a numerical blow-up.
4. **Integer truncation / rounding of the synthesis output** (emulating the DLL's `cvttsd2si`). Made the *first* half slightly worse. Wrong direction.
5. **Per-block bit-reader reset.** Broke everything, including the parts that worked.
6. **Skipping N frames of bytes at the divergence point** (manual realign, several N). Best case crawled to 0 dB. No.
7. **Excluding the suspicious block's payload entirely.** No.
8. **Global constant time-shift realignment** (cross-correlation to find a lag). No single lag recovered it.

Diagnostics we *did* trust, that narrowed the search:

- Reflection coefficients stayed `|k| < 1` throughout (max ≈ 0.98). The lattice filter was nominally stable. **Not** an unstable-filter problem.
- Excitation energy stayed bounded (≈ 5000). **Not** a runaway pitch loop.
- The first half matched to ±2 LSB. So the tables, the synthesis math, the de-emphasis — all correct.

By elimination, the bug had to be in **how bytes become frames** — the demuxer — not in how frames become audio — the codec. We'd been staring at the wrong half of the pipeline.

---

## Looking at the container instead of the codec

DS2 audio lives in 512-byte blocks: a 6-byte header, then a 506-byte payload. The QP demuxer concatenates every block's payload into one continuous byte stream and reads fixed-size frames (56 bytes = 448 bits) sequentially. Simple.

So we dumped the block headers around the divergence. Byte 2 of each header is the per-block frame count. Normal files show a tidy rhythm: `9` frames per block, with a `10` every 28th block (because `28 × 506 bytes = 253 × 56 bytes` exactly — the frame grid tiles perfectly over a 28-block cycle).

And there, right at the divergence, sat an anomaly: **a block whose frame count was not 9 or 10.**

We checked a second, much longer recording — eighteen minutes — that the user said had the same problem. Different anomaly, same family: **33 blocks with a frame count of `0`**, scattered through the file. The first one landed at 46 seconds. The decode was bit-exact until 46 seconds, then garbage. Every single time, the divergence began at the first non-standard block.

What do these recordings have in common? **Pauses.** Both were voice-activated dictation — someone talking, pausing to look at a document, talking again. A continuous read-through recording has none of these blocks. A real-world dictation is full of them.

---

## The answer was in the spec all along

The reverse-engineered spec (`CODEC_SPECIFICATION.md` in `hirparak/dss-codec`) has a section we'd skimmed past. For the *older DSS* format it says, almost word for word:

> DSS files have a critical complication: **empty blocks** (`frame_count = 0`). [...] Empty block payloads contain only continuation data (partial frame bytes), not full frames. [...] Remaining payload bytes in empty blocks are garbage and must be discarded. [...] **FFmpeg's `dss.c` demuxer does NOT handle empty blocks — it reads straight through, producing corrupt output.**

There it was. Documented. Including the *exact* size of the valid data: `cont_size = 2 × byte1 + 2 × swap − 6` bytes, where `byte1` is the block header's continuation-offset field. An empty block is a pause marker: it carries only the handful of bytes that finish the frame straddling the block boundary, and then padding that must be thrown away.

The catch: this handling lived only in the **DSS** path. The same spec says of the newer format: *"DS2 QP mode is simpler: the payload from all blocks is concatenated into a continuous bitstream... 1.0000 correlation (perfect match)."*

That sentence is true — **for gap-free recordings**. The QP decoder was verified against continuous read-throughs, which have no empty blocks, so it scored a perfect match and the empty-block case was reasonably assumed not to exist for QP. It does. Every pause makes one. The QP demuxer was concatenating each pause marker's full 506-byte payload straight into the frame stream, shifting every subsequent frame and desyncing the rest of the file.

It is *the same bug the spec explicitly warns about for `dss.c`* — transplanted to a path nobody thought needed the warning.

---

## The fix, and the proof

Apply the DSS empty-block rule to the QP demuxer. In QP there's no byte-swap, so `swap = 0` and `cont_size = 2 × byte1 − 6`. For a `frame_count == 0` block, keep only the continuation bytes and discard the rest:

```rust
let frame_count = block[2] as usize;
self.pending_frames += frame_count;
if frame_count == 0 {
    // Empty block = pause / segment-boundary marker. Keep only the cont_size
    // continuation bytes that finish the straddling frame; the rest is garbage.
    let cont = (2 * block[1] as usize)
        .saturating_sub(HEADER_SIZE)
        .min(PAYLOAD_SIZE);
    self.stream_buf.extend_from_slice(&block[HEADER_SIZE..HEADER_SIZE + cont]);
} else {
    self.stream_buf.extend_from_slice(&block[HEADER_SIZE..BLOCK_SIZE]);
}
```

The result, on the eighteen-minute recording, measured against the Olympus reference:

```
region          before fix        after fix
0 – 45 s        +76 dB            +76 dB     (pre-pause, unchanged)
100 – 130 s     −8.5 dB           +72 dB
400 – 430 s     −7.3 dB           +74 dB
900 – 1090 s    −6.9 dB           +74 dB
```

Bit-exact to the reference across the whole file. And — the property that made it safe to ship — the fix only ever runs on `frame_count == 0` blocks, so any file *without* empty blocks decodes byte-for-byte identically to before. We verified that too: a recording with no empty blocks produced an MD5-identical WAV before and after the change. No regression possible on the 99% of files that never hit this path.

---

## Corroboration: a nine-year-old ticket, suddenly explained

Two independent confirmations made us confident this wasn't a local quirk:

- **FFmpeg's `libavformat` DSS demuxer** ignores the per-block frame count entirely — it reads payloads continuously, skipping headers, tracking leftover bytes in a counter. Exactly the structure that *cannot* handle empty blocks, exactly as the spec predicted. The same blind spot, in the most battle-tested DSS demuxer in existence.
- **FFmpeg trac #6091** — the original 2017 request for DS2 support — opens with the reporter's complaint: *"distorted, and the duration is doubled."* That is this bug. It has been sitting in plain sight for nine years, mis-filed as "codec not supported yet," when at least part of it was a demuxer that walked straight through pause markers.

When your local bug reproduces a stranger's nine-year-old bug report, you've usually found something real.

---

## The one we haven't cracked

Honesty, per the house style. The short recording's anomaly was not an empty block — it was an **over-count** block: a frame count of `19` (a block can physically hold at most ~9 frames of payload), sitting exactly on a 28-block group boundary (the divergence frame was `13 × 253`, dead on a group edge). The empty-block rule doesn't touch it, and none of the realignment tricks recover it. It isn't in the spec, it isn't in `dss.c`, it isn't in the DSS reference. It appears to be a rarer segment-boundary event — one occurrence, versus thirty-three empty blocks in a comparable file.

So in production we do the responsible thing: fix the common case in the decoder, and **detect the exotic case** (`frame_count ∉ {0, 9, 10}`) to fall back to the reference decoder for those rare files, rather than emit something subtly wrong. The over-count case gets its own reverse-engineering session another day. A known, contained gap beats a silent one.

> **Update.** That "another day" came. We cracked the over-count block by running the Olympus decoder inside a debugger we built from its own DLLs — it turned out to be a per-block re-anchoring rule (`2 × byte1 − 6`) that no spec ever wrote down. The detector and the Windows fallback described above have since been **deleted**: the native demuxer now handles every structural case, including segment-boundary files the count detector never even flagged. The full story is **[07 — Cracking the re-sync block](07-cracking-the-resync-block.md)**.

---

## What to take away

- **Get a ground truth before you theorize.** The whole investigation pivoted on having the reference decoder to A/B against. Without it we'd have been arguing about whether the file was "just corrupt."
- **A sharp, reproducible failure point is a feature.** Localize *when* it breaks to the sample, map that to a position in the container, and look at what's structurally special there.
- **"Resetting state doesn't help" is a powerful negative result.** It cleanly separated a codec bug from a demux bug and saved us from polishing the wrong half of the pipeline for another day.
- **Read the whole spec, including the part for the format you think you're not using.** The answer had been written down, for the sibling format, complete with the exact formula and a warning naming the exact tool that gets it wrong.
- **A perfect correlation score only certifies the inputs you fed it.** "1.0000 match" was true and honest — and it covered every case except the one real users hit most.

---

*Twelve lines, one day, ten dead ends. The bug was always in the blocks we thought were empty. 🧱*
