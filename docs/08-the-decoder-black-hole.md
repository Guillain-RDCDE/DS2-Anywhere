# 08 — The handoff: one synthesis function between us and 100%

> This is a working document, not a war story. Chapters 06 and 07 closed the demuxer: the native pipeline now turns DS2 bytes into frames exactly the way the Olympus parser does, confirmed against the real DLL. One thing remains — a single acoustic regime where the **decoder** (frames → audio) still diverges. Here is everything the next session needs to finish it, so nobody has to re-derive what we already know.

---

## The symptom, stated precisely

Take a paused QP recording with a loud passage immediately after a pause. Decode it and sample-align against the Olympus reference (NCH Switch, driven headless — see "Getting a reference" below):

```
region                          native vs Olympus
before the resync run            +76…+78 dB   bit-exact (corr 0.99999999)
the resync run (loud passage)    decorrelated AND ~0.29× energy
after it realigns                +78 dB       bit-exact again (lag 0)
```

Two precise facts pin it down:

1. **The bad band is the *under-production of energy*, not noise.** In the band our RMS is ~0.29× Olympus's. The output is both decorrelated *and* too quiet — not a blow-up, not a phase wobble at the right level.
2. **The bad band coincides EXACTLY with the run of re-sync blocks.** It begins at the `count = 19` re-sync block and ends at the very next block whose byte1 anchor returns to 0 (the natural 28-block tiling realignment). Through that run every block carries a non-zero anchor (48, 46, 44 … 2, then 0 = recover). Quiet inside the run, bit-exact the instant alignment returns.

## What it is NOT (settled — do not re-litigate)

- **Not the demuxer.** Frame count and position align with Olympus *before and after* the band at lag 0, so the band has the same frame count too; and decoding the frame bytes captured live from the Olympus parser reproduces the band. Frames are provably correct. (Chapter 07.)
- **Not filter instability / blow-up.** `|k| < 1` throughout (the band's mean `|k|` is actually *lower* than the good regions, ~0.80 vs ~0.85); output is too *quiet*, not exploding.
- **Not decoder state.** Resetting lattice + pitch memory at the re-sync changes the band's correlation by nothing (−0.015 → −0.015).
- **Not fixed-point vs float.** The Olympus lattice (`FUN_10019d40`, reached via the single decode loop `FUN_10016540 → FUN_10018450 → FUN_10017e70`) is `double`, identical in structure to the open `lattice_synthesis`. No integer-rounding cliff.
- **Not short-pitch, not dropped pulses, not codebook clamping.** Pitch distribution is identical across the band (4.1% short everywhere); zero pulses land out of range; every per-coefficient codebook is full-size (no index clamping). Excitation energy is actually *higher* in the band — yet the synthesised output is quieter.

## The two ports are identical — so this is a SPEC gap, not a port bug

Approach "B" settled the most important question. We diffed the two independent open decoders — Kieran Hirpara's Python/Rust and Patrick Domack's FFmpeg C port — function by function: adaptive excitation, fixed (pulse) excitation, `excitation = pitch_gain·adaptive + fixed`, the normalized lattice, the `y[n]=x[n]+0.1·y[n-1]` de-emphasis. **They are byte-for-byte the same algorithm.** Neither implements any postfilter. So both produce the *same* quiet band — which means the residual is **not** a mistake in either port; it is something in the **shared reverse-engineered spec** that does not match the real Olympus decoder. Comparing the ports cannot fix it; only the real decoder can tell us what's missing.

We also fully traced the Olympus decode path: `FUN_10016540 → FUN_10018450 (parse) → FUN_10017e70 (synthesis) → FUN_10018ca0 (de-emphasis)`. The synthesis and de-emphasis are exactly what the ports implement. The **one** thing the ports do NOT implement is the **voiced/unvoiced branch** inside `FUN_10017e70`: when the per-frame voicing flag `param_3` (set by `FUN_10018450`, byte offset `0x6c`, via a rotating-bitmask scheme on a bit register at `0x324`) is **0**, the excitation is *not* pitch+pulses at all — it's **`state = state*0x209 + 0x103` PRNG noise scaled by a gain table** (`DAT_1004ce50`). Kieran's code even has the vestige: a `prng_state = 0` that is initialised and never used.

## The prime live hypothesis (corrected)

**The bad band is unvoiced, and the ports decode it as voiced.** A loud post-pause passage (breath, fricatives, noise burst) would be flagged unvoiced; the real decoder fills it with loud PRNG noise; the ports instead compute a quiet pitch+pulse excitation from bitstream fields that, for an unvoiced frame, the real decoder *ignores*. That single substitution explains **both** symptoms at once — wrong energy (PRNG noise gain ≠ pitch+pulse energy) and decorrelation (noise ≠ tonal). An earlier note here "ruled this out" because the band's pitch-gain indices look normal — that reasoning was **wrong**: voicing is a *separate* signal (the `0x324` bitmask), not derivable from pitch gain, so normal-looking pitch gains say nothing.

What's needed to confirm and fix: the per-frame value of `param_3` across the band (is it 0 there?) and the source of the `0x324` voicing bits.

A static trace got us most of the way: `FUN_10016910` does **not** read the voicing bits from the per-frame audio bitstream at all — it pulls them from a **separate buffer queue** on the decoder object (`+0x69e0`, with `+0x69e4` as its count). That is the smoking gun for why both ports never go unvoiced: a QP audio frame is exactly 448 bits with no room for a voicing bit, so the open decoders — which parse only the audio stream — have **no voicing data and therefore default to "always voiced."** The real decoder consumes a **separate voicing/metadata channel** that the reverse-engineered spec never documented, and that channel is what selects the PRNG-noise excitation for the unvoiced (loud, post-pause) frames.

So the remaining work is a defined RE task, not a mystery: **find where that voicing channel lives in the DS2 container** (which header field or interleaved block feeds the `0x69e0` queue), parse it, drive the voiced/unvoiced switch, and add the `state*0x209+0x103` PRNG-noise excitation (`× DAT_1004ce50[gain]`) for unvoiced frames — then validate against the Switch reference. Running the real decoder hookably (the blocker below) would shortcut the discovery by letting you log `param_3` and the queue contents directly.

## The exact next steps (the tooling already exists)

**The blocker to clear first.** Hooking the decoder's internals needs the real decoder to actually *run* on our chosen file. That is the wall: the Olympus `DssParser` and `DssDecoder` filters **refuse to connect** in a hand-built graph (`ConnectDirect` parser-out → decoder-in returns `VFW_E_NO_ACCEPTABLE_TYPES`), and `Render`-ing the source builds a graph in which `DssDecoder` is *loaded but never driven* (the decode loop `FUN_10016540` never fires — verified by hooking it). So we can get the parser to run and emit frames (that's how chapter 07 was cracked), but we have **not** yet made the Olympus *decoder* execute inside a process we can instrument. Until that's solved, its intermediates are out of reach.

Two ways through:

- **A. Make the Olympus decoder run hookably.** Options: insert a SampleGrabber and let `Render` negotiate the full chain (parser→decoder→grabber→null) so the decoder is actually pulled; or feed the decoder its input media type explicitly (capture the type `Render` negotiates, pass it to `ConnectDirect`); or inject frida into NCH Switch itself (it *does* decode — it resisted injection before, retry with a gadget/early-attach). Once the decode loop fires, hook the per-frame synthesis and **A/B its intermediates against the open `f64` decoder on one bad-band frame** to localise synthesis-vs-upstream, then read the disassembly.
- **B. A/B two independent open decoders.** Build Patrick Domack's FFmpeg C port and decode the same paused file. If its bad band differs from Kieran's Rust/Python port, the divergence between the two *is* the bug (one of them does the missing step). Cheaper than RE — no decoder hooking needed.

**Getting a reference (this works headless).** NCH Switch on the VM produces the ground-truth WAV even from a disconnected RDP session: `switch.exe -convert <in.ds2> -outfolder <dir> -format .wav -overwrite` via `Start-Process` (see `swconv.ps1`). With that WAV, the whole windowed-SNR / per-frame-correlation / energy-ratio A/B in this chapter is reproducible in minutes.

## Assets left in place (don't rebuild these)

- **Registered Olympus filters** on the conversion VM (`DssParser.dll`, `DssDecoder.dll`, COM-registered 32-bit). CLSIDs in chapter 07.
- **`C:\temp\graph2.py` / `graph3.py`** — hand-built DirectShow graphs (Render-based, and explicit-ConnectDirect). The parser runs; the decoder does **not** yet (see blocker above).
- **`runner_frida.py` + a `hook.js`** — frida spawn/attach harness; the **parser** hook (`DssParser.dll!+0x9890`, frame ptr in `args[0]`) is what works today. (`Process.findModuleByName(...).base` — `Module.findBaseAddress` is gone in frida 17.)
- **Ghidra project for `DssDecoder.dll`** under `/opt/ghidra-re`, decompiled to `decomp.c`. The single decode loop is `FUN_10016540 → FUN_10018450 (parse + voicing) → FUN_10017e70 (synthesis, calls the `double` lattice `FUN_10019d40`)`. `FUN_10003a90` (formant/adaptive postfilter cascade) is the prime unexamined suspect.

## Why this isn't urgent (but is worth doing)

In the production context this repo came from, the recorded `.ds2` is the source of truth and the typists work from it on their own Olympus players — the server-side MP3 is a convenience copy, so a rare loud-passage artifact in that copy is tolerable, and the demuxer fix (which fixes the *structural*, whole-file-after-here corruption) was the part that actually mattered. For a *general-purpose* DS2 decoder — and for an FFmpeg merge that aims to be byte-exact — this last function matters. It is the difference between "bit-exact except one acoustic regime" and "bit-exact, full stop."

We've cornered it: correct frames in, too-quiet output out, only across the re-sync run, all the easy explanations ruled out. The last mile is making the closed decoder run where we can watch it — then it's a diff, not a mystery.

---

*The demuxer is done. The decoder's last secret is behind one locked door — running the original where we can instrument it. We know exactly which door, and exactly what to read once it opens. 🔍*
