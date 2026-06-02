# 08 — The handoff: one synthesis function between us and 100%

> This is a working document, not a war story. Chapters 06 and 07 closed the demuxer: the native pipeline now turns DS2 bytes into frames exactly the way the Olympus parser does, confirmed against the real DLL. One thing remains — a single acoustic regime where the **decoder** (frames → audio) still diverges. Here is everything the next session needs to finish it, so nobody has to re-derive what we already know.

---

## The symptom, stated precisely

Take a paused QP recording with a loud passage immediately after a pause. Decode it three ways and sample-align against the Olympus reference:

```
region                 native vs Olympus
quiet / normal speech   +76…+78 dB   bit-exact
loud passage post-pause  ~0 dB       decorrelated
after the loud passage  +78 dB       bit-exact again
```

The divergence is **content-dependent and self-healing**: it starts when the loud passage starts and ends when it ends. It is not a permanent desync (those were all demuxer bugs, now fixed).

## What it is NOT (these are settled, do not re-litigate)

- **Not the demuxer.** Feed the decoder the *exact frame bytes captured from the live Olympus parser* (not our reconstruction) and the same band still diverges. The frames are provably correct. (Chapter 07.)
- **Not filter instability.** Reflection coefficients stay `|k| < 1` (max ≈ 0.98) through the band; excitation energy stays bounded.
- **Not decoder state.** Resetting lattice + pitch + de-emphasis at the divergence changes nothing — the per-frame output is wrong, not the carried state.
- **Not fixed-point vs float.** This was the tempting theory and it is **wrong**. The Olympus lattice synthesis (`DssDecoder.dll!FUN_10019d40`) is `double`, same precision as the open implementation. There is no integer-rounding cliff.

## The single live hypothesis

The QP decode path in `DssDecoder.dll` does something in the synthesis (or in excitation/pitch reconstruction) on these frames that the open-source `f64` implementation does not replicate — but it is **not** `FUN_10019d40`. We hooked that routine during an actual DS2 decode and it **never fired**: it's the DSS/SP lattice. The DS2/QP path runs a *different* synthesis function we have not yet located among the ~600 in `DssDecoder.dll`.

So the entire remaining problem is: **find the QP synthesis function, and diff its math against the open implementation on a frame from the bad band.**

## The exact next steps (the tooling already exists)

Everything below reuses the rig built in chapter 07, which is still on the conversion VM.

1. **Identify the QP synthesis entry point.** With the manual DirectShow graph running a DS2 file (`graph2.py`), use frida's `Stalker` or a coarse `Interceptor` sweep over `DssDecoder.dll`'s exported/internal functions to find which routine is called once per QP frame (256 output samples). Candidates worth hooking first (high `double`-density routines found by static scan): `FUN_10019060`, `FUN_10018ca0`, `FUN_100177f0`, `FUN_10018800`. The one that fires ~`total_frames` times during a QP decode is the synthesis.
2. **Capture inputs + outputs for one bad-band frame.** Hook that function; dump its argument arrays (reflection coeffs, excitation/pitch buffers) and its output PCM for a frame inside the loud post-pause band.
3. **Run the open decoder on the identical inputs.** Feed those exact captured inputs to the `f64` reference synthesis. If the outputs differ for identical inputs → the divergence is *in the synthesis math* (compare the disassembly of the QP routine to the reference). If the outputs match → the divergence is *upstream* (excitation/pitch/gain reconstruction for these frames), so walk one stage back and repeat.
4. **Decompile the winning function** (`analyzeHeadless` is already set up under `/opt/ghidra-re`; the project for `DssDecoder.dll` is built — the decompiled C is at `decomp.c`). Read the routine, find the operation the open port omits or does differently, port it, and re-validate with the SNR-windowed A/B against Olympus.

## Assets left in place (don't rebuild these)

- **Registered Olympus filters** on the conversion VM (`DssParser.dll`, `DssDecoder.dll`, COM-registered 32-bit). CLSIDs in chapter 07.
- **`C:\temp\graph2.py`** — the hand-built DirectShow graph that runs the real decoder on a chosen file. Point it at the target `.ds2`.
- **`runner_frida.py` + a `hook.js`** — frida spawn/attach harness; swap the hook body for whichever `DssDecoder` routine you're probing. (`Process.findModuleByName("DssDecoder.dll").base` — `Module.findBaseAddress` is gone in frida 17.)
- **Ghidra project for `DssDecoder.dll`** under `/opt/ghidra-re`, decompiled to `decomp.c`. `FUN_10019d40` (the SP lattice, `double`) is already read and ruled out.

## Why this isn't urgent (but is worth doing)

In the production context this repo came from, the recorded `.ds2` is the source of truth and the typists work from it on their own Olympus players — the server-side MP3 is a convenience copy, so a rare loud-passage artifact in that copy is tolerable, and the demuxer fix (which fixes the *structural*, whole-file-after-here corruption) was the part that actually mattered. For a *general-purpose* DS2 decoder — and for an FFmpeg merge that aims to be byte-exact — this last function matters. It is the difference between "bit-exact except one acoustic regime" and "bit-exact, full stop."

It is one function. We know how to find it. The rig is warm.

---

*The demuxer is done. What's left is a needle in a 600-function haystack — but we have the magnet, and we know which barn. 🧲*
