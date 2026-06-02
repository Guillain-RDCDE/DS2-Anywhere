# 08 — The handoff: one synthesis function between us and 100%

> This is a working document, not a war story. **For the narrative, polished write-up of this same investigation, read the research paper: [09 — The re-sync excitation anomaly](09-the-resync-excitation-anomaly.md).** This page is the terse engineer's handoff. Chapters 06 and 07 closed the demuxer: the native pipeline now turns DS2 bytes into frames exactly the way the Olympus parser does, confirmed against the real DLL. One thing remains — a single acoustic regime where the **decoder** (frames → audio) still diverges. Here is everything the next session needs to finish it, so nobody has to re-derive what we already know.

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

## The unvoiced hypothesis — investigated, then RULED OUT for QP

We chased the voiced/unvoiced branch hard, localised its exact mechanism, and in doing so **eliminated it as the QP cause** — a result worth recording so nobody re-chases it.

The mechanism: the decoder reads ONE continuous bitstream. A rotating 16-bit mask (`0x326`) walks a word register (`0x324`); when the mask wraps, `FUN_10016910` refills `0x324` with the next word from an input-buffer queue (`+0x69e0`). **Both** the audio reader (`FUN_10017460`) and the voicing read in `FUN_10018450` draw from this same register — so a voicing bit, when taken, is simply the next bit of the same stream, gated by config byte `0x6c`: if `0x6c == 0`, `FUN_10018450` sets `param_3 = 1` (always voiced) and consumes **no** bit; if `0x6c != 0`, it consumes one voicing bit per frame.

The decisive observation: if QP consumed one voicing bit per frame, the open decoders — which read 448 audio bits per frame with **no** voicing bit — would desync within two frames. They are instead **bit-exact across 2310 continuous QP frames** (the FATE sample). Therefore **QP consumes no voicing bit → `0x6c == 0` → QP is always voiced**, and the PRNG-noise path is SP-only. It cannot be the QP residual. (The earlier "it's unvoiced" hypothesis here was wrong; so was the even-earlier "not unvoiced because pitch gains look normal" — both are now moot for QP.)

## Where that leaves it

QP's decode path is *almost* reproduced by the open ports: parse → voiced excitation → lattice → de-emphasis, no postfilter, no unvoiced branch. But "almost" is the new lead. The voiced-excitation builder `FUN_10018800` — the one decode-path function nobody had read line-by-line — is **substantially richer than the ports' one-liner** `excitation = pitch_gain·adaptive + fixed`:

- it keeps a **persistent excitation buffer** across subframes (`+0x36b8`, sized `pitch_range + 1`), not a fresh copy from pitch memory each time;
- it applies a **pitch-indexed gain factor** read from a table (`dVar2 = table[(pitch + DAT_1006ecc8[mode@0x1b0]) ]`), i.e. the adaptive contribution is scaled by a coefficient that depends on the pitch lag and a mode index — the signature of **fractional-pitch interpolation / a pitch-prediction gain** that the ports drop.

That is the prime suspect now: the ports' integer-pitch `pitch_memory[end - pitch + i]` with a flat `pitch_gain` omits whatever `FUN_10018800` does with its persistent buffer and pitch-indexed factor. It would bite hardest exactly where the adaptive (pitch) contribution dominates and the buffer state matters — a sustained voiced passage right after a re-sync, where the persistent buffer was just disturbed. (Caveat: the band's pitch *distribution* matches the good regions, so it's the buffer-state/gain handling, not the raw lag, that differs.)

### Analysis-by-synthesis nailed the layer (decoder-free)

We then did the test that finally splits coeffs from excitation **without** running the decoder. Our reflection coeffs are bit-verified correct (the frame bits match the real parser, refl region included). So inverse-filter the Switch reference WAV through the *analysis* lattice with **our** coeffs (and inverse de-emphasis), and you recover **Switch's own excitation**. Compare it to the excitation our decoder synthesises:

```
region        corr(our exc, Switch exc)   RMS ours   RMS Switch   ratio
before band            1.000                 438         438       1.00
the re-sync band       0.002                 313        3281      10.5x
after band             1.000                 254         254       1.00
```

This is conclusive: **coeffs and the lattice are perfect** (1.000 corr outside the band proves the inverse-filter + coeffs are exact), and **the residual is entirely in the excitation** — in the band, Switch's excitation is **10× louder and completely decorrelated** from ours. Characterising it further: Switch's band excitation is **more *pitched*** (autocorr periodicity 0.24 vs 0.05 outside), not noisier — so it is the **adaptive (pitch) contribution** that is wrong/too weak in our decoder, not a noise path.

### The mechanism (evidence-backed)

The adaptive codebook is a feedback loop: `excitation = pitch_gain·(past excitation, repeated at the pitch lag) + fixed`, and that excitation is fed back into the pitch memory for the next subframe. Everything entering the band matches (frames, coeffs, pitch memory all verified). So a **small excitation difference introduced at the `count=19` re-sync block** is then **amplified by the pitch-feedback loop across the sustained high-pitch-gain passage** — ours decays to ~1/10th, Switch's sustains loud — and it self-heals when the pitch gain drops at ~60 s. The exact thing the real decoder does to the excitation/pitch-memory at the re-sync (in `FUN_10018800`, the rich excitation builder with its persistent buffer and pitch-indexed gain factor `dVar2 = table[pitch + DAT_1006ecc8[mode]]`) is the missing piece. It is NOT a static gain-table error (those match outside the band) — it is a re-sync-triggered state/gain difference.

### Root cause (the dissection that nailed it)

We located the FIRST diverging subframe and dissected it against Switch's recovered excitation. The result is unambiguous:

- The divergence begins **exactly at subframe 13156 = 52.62 s = the `count=19` re-sync block.** Not before, not after — at the re-sync.
- The smoking gun, the very next subframes: e.g. one has gain index → `gc = 7` (essentially **zero** fixed-codebook gain) and a small pitch gain, so the ports synthesise an excitation of RMS ≈ 57. **Switch's excitation for that same subframe has RMS ≈ 3779 — ~66× larger.** Across the band, Switch's excitation is huge *regardless of how small the bitstream's gain indices are.*

So the real decoder does **not** decode the re-sync region as ordinary CELP. The `count=19` block triggers a **special decode mode** in which the excitation is generated loud, independent of the per-subframe gain indices, and that mode persists until alignment returns at the next anchor-0 block (≈ 60 s). The open ports have no such mode: they decode those frames as normal CELP, get near-zero gains, produce a 10× too-quiet excitation, and the pitch-feedback loop then smears it into the decorrelated band we measured.

A natural hypothesis was the PRNG-noise branch (`FUN_10017e70`'s `param_3 == 0` path: `state = state*0x209 + 0x103` × a gain table), re-sync-armed by the `FUN_10018450` counter (`0x6c`/`0x328`/`0x32c`/`0x330`). **A spectral-flatness test refuted it.** Inverse-filter Switch's band excitation and measure its spectral flatness (SFM): outside the band SFM ≈ 0.52 (normal CELP excitation), **inside the band SFM ≈ 0.11 — strongly *structured/periodic*, not white.** PRNG noise would be flat (SFM → 1). So the loud band excitation is **not** noise; it is a **strongly pitched** excitation, ~10× louder than ours.

So the mechanism is in the **adaptive (pitch) path**, not a noise path: in the re-sync region the real decoder's pitch excitation is much louder and self-reinforcing (consistent with a pitch gain pushed near/over 1 so the adaptive feedback resonates — our measured per-subframe `gp` even reaches 1.067 in the band). The divergence starts at the `count=19` block and compounds through the pitch-memory feedback loop (once our excitation is quieter, our pitch memory is quieter, so the next adaptive contribution is quieter still), then heals when the gain settles at ~60 s. Whatever the `count=19` block changes about the pitch-excitation gain/state is the missing piece — and it is **not** visible in the per-frame decode functions, which match the open ports exactly. It is a `count=19`-triggered behaviour that only shows in the running decoder's evolving excitation/pitch-memory state.

### What's left

The layer is pinned to certainty: **the excitation (adaptive/pitch path), `count=19`-triggered, ~10× too quiet in our decode, compounding through the pitch-feedback loop.** Coeffs, lattice and de-emphasis are bit-proven correct; the band excitation is loud and *pitched* (not noise). What is NOT yet pinned is the exact thing the real decoder changes at the `count=19` block — and crucially, **it is not in any per-frame decode function** (all of `FUN_10018450/10017e70/10018800` match the open ports). It is a `count=19`-triggered change to the evolving excitation/pitch state that only manifests in the running decoder's accumulated state.

That makes the remaining step decoder-bound: **hook `FUN_10018800`'s output (or the excitation buffer `+0x36b8`) on the running decoder across the `count=19` block** and read what it does to the pitch gain / pitch memory there that the ports don't. The blocker (below) is that the Olympus decoder still won't run inside an instrumentable graph. The inverse-filter A/B is the decoder-free regression test ready to validate any candidate fix (it already gives the exact per-subframe target excitation). Implementing blind — without seeing what `count=19` does to the pitch state — would be guessing, and two such guesses (PRNG noise; a static gain-table swap) have already been falsified here.

## The exact next steps (the tooling already exists)

**The blocker — and why it's architectural, not effort.** Hooking the decoder's internals needs the real decoder to actually *run* on our chosen file in a process we control. A dedicated push proved this is blocked by the filters' design, not by a missing trick. Every standard DirectShow way to wire `DssParser` → `DssDecoder` fails:

- `ConnectDirect(parser_out, decoder_in, NULL)` → **`VFW_E_NO_ACCEPTABLE_TYPES`** — the two pins share **no** media type at all.
- `Connect(parser_out, grabber_in)` with the grabber pinned to PCM (intelligent connect, which would auto-insert the decoder) → **`VFW_E_CANNOT_CONNECT`** — no chain of intermediate filters bridges them.
- `Render(parser_out)` with only the parser present → fails to build a chain; it only "succeeds" when a `NullRenderer` is pre-added, in which case `Render` connects parser→null directly and the decoder is **loaded but never driven** (`FUN_10016540` never fires).

So the Olympus filters do not interconnect through any public DirectShow mechanism — they're wired by the Olympus application's own code with proprietary media-type negotiation. That's the wall. (`SetSyncSource(NULL)` to defeat headless audio-renderer starvation was tried too; moot, since the chain never builds.)

The remaining ways through are all **multi-day sub-projects**, not quick hooks:

- **A. DLL-proxy instrumentation.** Build a 32-bit proxy that forwards every COM export to the real decoder DLL and inline-hooks its synthesis to log the excitation buffer (`+0x36b8`). Needs a Windows cross-compiler (mingw) + COM export forwarding + an inline-hook trampoline — *and* it must target whichever DLL the host actually runs (NCH Switch appears to decode via `dss32.dll`, not the `DssDecoder.dll` we disassembled, so the hook RVAs would need re-deriving on `dss32.dll`).
- **B. Drive + hook the Olympus app (ODMS).** ODMS builds the graph correctly (the filters work in it). Needs GUI automation to make it play the file headless, and ODMS may also resist instrumentation. (NCH Switch is confirmed anti-debug — frida spawn and attach both fail.)
- **C. A/B two independent open ports.** Already done and refuted: Patrick's C port and Kieran's Rust/Python port are byte-identical, so there's no divergence to mine.

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
