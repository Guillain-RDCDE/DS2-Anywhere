# 09 — The re-sync excitation anomaly: a research paper

> ## ⚠️ Superseded — read [10 — The reckoning](10-the-reckoning-the-bug-that-wasnt.md) first (or right after)
>
> **This paper's central claim is wrong, and we have kept it standing, unedited below this banner, on purpose.** It documents a rigorous investigation that reached a confident, *false* conclusion: there is **no** residual decoder bug. When we finally built an instrumentable oracle from the closed decoder's own DLLs — and then did the cheapest test of all, *listening to the file* — the "seven-second wound" turned out to be a person stepping away from the microphone, and the "10× excitation collapse" turned out to be a measurement diverging from a reference it should not have trusted. The spectral filter, the demux, and our decoder are all faithful.
>
> Concretely, three things below are now known to be untrue or unfair:
> - the **"spec gap"** this paper infers does **not** exist — Kieran Hirpara's and Patrick Domack's decoders were **complete**, not missing a hidden mode;
> - the **"locked door / unsolved"** framing is resolved: there was nothing behind the door;
> - the **inverse-filter A/B is not a valid regression test for a bug**, because there is no bug to regress against.
>
> We keep this chapter because the *method* (analysis-by-synthesis) is sound and the *trap* (a beautiful internally-consistent analysis chasing its own reference artefact) is the single most useful lesson in this repo. **[Chapter 10](10-the-reckoning-the-bug-that-wasnt.md) is the honest accounting.** Read the paper below as the record of what we believed — and how careful work can still be wrong.

*How we proved a ten-year-locked dictation codec is decoded perfectly except for one acoustic regime, exactly cornered the bug to a single hidden state machine, and hit the one wall we could not climb — told as it happened.*

> **Abstract.** After open-sourcing a production DS2 decoder (chapters 01–07), one defect remained: on recordings that contain a *pause*, a few seconds of audio right after the pause come out wrong — too quiet and garbled — while the entire rest of the file is bit-perfect against the original Olympus software. This paper documents the full investigation that followed. Using an *analysis-by-synthesis* technique (inverse-filtering the reference decoder's own output), we proved the error lives **entirely in the excitation signal, not the spectral filter** — the filter coefficients are bit-exact. We then falsified, one by one, every plausible cause (noise/PRNG substitution, gain-table error, a voicing channel, fractional pitch, codebook overflow, filter instability, fixed-vs-float arithmetic, decoder-state carryover). The surviving explanation, supported by a frame-level dissection: the anomalous **`count=19` re-synchronisation block arms a hidden frame-counter state machine in the decoder** that changes how excitation is generated for ~120 frames, and the pitch-prediction feedback loop then amplifies the difference until it heals. The final confirmation requires watching the closed-source decoder's internal state evolve — and that is blocked by a hard architectural wall: the Olympus DirectShow filters refuse to interconnect through any public mechanism, and the commercial host (NCH Switch) is anti-debug. We close with the exact open problem and a decoder-free regression test that will validate any future fix.

---

## Part 0 — For the newcomer: what is even happening here

Skip to Part 1 if you already know CELP. Otherwise, ninety seconds of background.

A **dictaphone** (Olympus DS-/DM- series, used by lawyers, doctors, bailiffs) records voice into a proprietary file called **`.ds2`**. It does not store the sound wave directly — that would be huge. It stores a *recipe* for re-creating the voice, using a technique called **CELP** (Code-Excited Linear Prediction), the same family that powered GSM phone calls.

CELP splits speech into two parts, every 16 milliseconds:

1. **The filter** — a short list of numbers (here, 16 "reflection coefficients") that describe the *shape of the mouth*: where the tongue and lips are, which vowel is being formed. Feed any sound through this filter and it takes on that vowel's colour (its **formants**).
2. **The excitation** — the *buzz from the vocal cords* (or the *hiss* of a consonant) that you pour into the filter. It carries the pitch and the energy.

`filter( excitation ) = speech`. Decode = read the recipe, rebuild the excitation, run it through the filter. That's it.

For ten years there was no open decoder; you needed Windows + commercial software (NCH Switch) to turn `.ds2` into something a transcription pipeline could use. Chapters 01–07 of this repo changed that — a pure-CLI, no-Windows decoder, byte-exact against Olympus. Except for the bug in this paper.

**Why a 7-second glitch is not "minor".** These are *legal* dictations transcribed word-for-word. Seven seconds of garbled audio in a deed or a witness statement is not cosmetic — it is potentially a missing clause. "Mostly perfect" is not good enough. This paper is the hunt for the last 1%.

---

## Part 1 — The crime scene

One paused recording. Decode it with our open decoder, decode it with Olympus, line the two up sample-for-sample, and measure how close they are, second by second:

```
region                       our decoder vs Olympus
0 – 50 s                      +69 dB    bit-exact (correlation 1.0000)
50 – 53 s                     +5 dB     degrading
53 – 60 s                     −0.4 dB   garbage (correlation −0.02, energy 0.30×)
60 – 66 s                     +1 dB     recovering
66 – 75 s                     +71 dB    bit-exact again (correlation 1.0000)
```

A perfect decode, then a cliff at ~52.6 s, seven seconds of ruin, then perfect again. **A sharp, self-healing wound.** That shape is a gift: a slow drift you have to chase statistically; a sharp on/off says *there is a discrete event in the file that flips the decoder into a bad state and a later event flips it back.* Find the two events, find the bug.

We already knew from chapter 06 that the demuxer (the part that cuts the file into frames) was correct here — the frame boundaries match the real Olympus parser byte-for-byte, confirmed by hooking the live parser. So this is **not** a framing problem. It is the decoder turning correct frames into wrong sound.

---

## Part 2 — You cannot debug what you cannot measure: the ground truth

Every step that follows depends on one thing: a **reference**. The exact PCM the official Olympus decoder produces for this file. We get it from NCH Switch on a Windows VM — and, usefully, **headless**: Switch will convert a file to WAV from a command line even on a disconnected remote session (`switch.exe -convert in.ds2 -outfolder dir -format .wav`). Two minutes, repeatable. Without this reference we'd be arguing about whether the file is "just corrupt." It isn't — Olympus decodes it cleanly. *We* break.

---

## Part 3 — The masterstroke: splitting the pipeline without opening the decoder

Here is the central trick of the whole investigation, and it needs no access to Olympus's source.

Recall `speech = filter(excitation)`. A filter is invertible. If we run Olympus's *output* **backwards** through the filter (the "analysis lattice", the mathematical inverse of synthesis), we recover **the excitation Olympus must have used**. And we can do this using **our** filter coefficients.

Why does that split the problem? Because there are only two things the decoder produces — the filter coefficients and the excitation — and this lets us test them *separately*:

- If our coefficients are wrong, inverse-filtering Olympus's output with them yields garbage **everywhere**, including the parts that sound fine.
- If our coefficients are right, the recovered excitation will match *our* excitation exactly wherever we already agree — and any disagreement is, by elimination, **purely in the excitation**.

We ran it. The result is the hinge of the paper:

```
region            corr(our excitation, Olympus excitation)   RMS ours   RMS Olympus
before the band              1.000                              438          438
the bad band                 0.002                              313         3281   (10.5×)
after the band               1.000                              254          254
```

Read that carefully. Outside the band the two excitations are **identical** (correlation 1.000) — which proves our coefficients *and* the lattice math are bit-exact (otherwise the inverse-filter couldn't reproduce them). Inside the band the excitations are **completely uncorrelated**, and Olympus's is **ten times louder** than ours.

**Verdict, beyond doubt: the filter is perfect; the residual is 100% in the excitation.** We had cut the suspect list in half without ever reading a line of Olympus's decoder.

---

## Part 4 — The gallery of dead ends (every one falsified, not hand-waved)

Good reverse-engineering is mostly *killing* hypotheses. Each of these was a real experiment with a real falsifier.

1. **"It's a noise/PRNG excitation."** CELP decoders have an *unvoiced* mode that fills the excitation with pseudo-random noise instead of pitch pulses; the Olympus binary has exactly such a branch (`state = state*0x209 + 0x103`). Loud + decorrelated certainly *smells* like noise. **Falsifier — spectral flatness.** White noise has a flat spectrum (flatness → 1.0); a pitched buzz has a peaky one (→ 0). We measured the *flatness* of Olympus's band excitation: **0.11**, versus 0.52 outside. Not noise — **more strongly pitched** than normal. Killed.

2. **"The gain table is wrong."** Maybe our excitation-gain lookup is off. **Falsifier:** the excitations match at correlation 1.000 *outside* the band, where the same gain table is used. A static table error would show everywhere. Killed.

3. **"There's a hidden per-frame voicing bit we're not reading."** The binary reads a voicing flag from a bit register; if QP frames carried one and we ignored it, the bitstream would shift. **Falsifier:** our decoder is bit-exact across 2310 consecutive frames of a *continuous* file — impossible if a voicing bit were being consumed each frame. So continuous QP consumes none; the voicing machine is dormant there. Killed (for the continuous case — hold that thought).

4. **"Fractional/short pitch handling."** The adaptive codebook is fiddly when the pitch period is shorter than a subframe. **Falsifier:** the distribution of pitch values inside the band is identical to outside (4.1% short, both). Killed.

5. **"Codebook index overflow / dropped pulses."** The fixed-codebook index can exceed the combinatorial limit. **Falsifier:** we counted — **zero** pulses land out of range in the band; the per-coefficient codebooks are all full size, so no index is clamped. Killed.

6. **"Filter instability / blow-up."** A reflection coefficient ≥ 1 makes the synthesis filter explode. **Falsifier:** all `|k| < 1` through the band (its mean is actually *lower* than outside), and the output is too **quiet**, not exploding. Killed.

7. **"Fixed-point vs floating-point rounding."** The classic codec-port divergence. **Falsifier:** the Olympus lattice (`FUN_10019d40`) is `double` — same precision as ours; and rounding noise can't produce a 10× energy gap. Killed.

8. **"Decoder state carryover at the pause."** Maybe the pitch memory / lattice state needs resetting at the re-sync. **Falsifier:** resetting it changes the band correlation by *nothing* (−0.015 → −0.015). Killed.

9. **"It's a bug in our Python harness, not the real decoder."** **Falsifier:** the deployed Rust binary — independent code — shows the identical band (corr −0.02, energy 0.30×) and the exact same output length as Olympus. Real.

10. **"Patrick's independent C port does it differently — diff the two."** **Falsifier:** Patrick Domack's FFmpeg C decoder and Kieran Hirpara's Rust/Python decoder are **byte-for-byte the same algorithm** (excitation, lattice, de-emphasis; neither has a postfilter). Two faithful ports of the same spec can't reveal a gap *in* that spec. Killed — but it proves something big: **this is a gap in the reverse-engineered specification itself, not a coding mistake.**

By the end of the gallery, the survivors are: *the excitation, in the adaptive (pitch) path, doing something the published spec does not describe, only in the re-sync region.*

---

## Part 5 — The smoking gun

We found the **first** subframe where our excitation diverges from Olympus's, and dissected it.

It is **exactly** subframe 13156 = **52.62 s = the `count=19` block.** Not a millisecond before. The wound begins at one specific, anomalous structural element of the file.

And the dissection of the frames just after it is the smoking gun. Take one where the bitstream's excitation-gain index decodes to essentially **zero** (`gc = 7`): the ports synthesise an excitation of RMS ≈ 57 — correctly, that's what a near-zero gain gives. **Olympus's excitation for that same frame has RMS ≈ 3779.** Sixty-six times larger, out of a frame the bitstream says should be nearly silent.

The real decoder is **ignoring the per-subframe gains** in this region and generating a loud, pitched excitation from somewhere else. The re-sync region is simply **not decoded as ordinary CELP.**

---

## Part 6 — The mechanism

Two facts from the disassembly, finally connected:

- `FUN_10017e70` (synthesis) chooses between a normal pitch+pulse excitation and an alternate branch, based on a flag `param_3`.
- `param_3` is driven by a **frame-counter state machine** in `FUN_10018450`: a control byte (`0x6c`) enables it; counters (`0x328`, `0x32c`) tick frames; a flag (`0x330`) marks the mode "active"; and it's fed from a separate buffer queue (`0x69e0`).

Here is the synthesis:

- On a **continuous** recording the machine never arms → always ordinary CELP → bit-exact (this is precisely why hypothesis #3 was right *for continuous files*).
- The **`count=19` re-sync block arms the counter** for a run of frames. Through that run the decoder builds excitation differently — loud, pitched, independent of the per-subframe gains — exactly the "RMS 3779 from a gc=7 frame" we measured.
- The adaptive codebook is a **feedback loop**: each subframe's excitation is fed back as the pitch memory for the next. Once our excitation is 10× too quiet, our pitch memory is 10× too quiet, so the next adaptive contribution is quieter still — the gap *compounds* across the sustained high-pitch-gain passage (our own per-subframe pitch gain even reaches 1.067 in the band, a near-resonant value). The wound **heals** when the gain settles and the loop drains, ~60 s.

So the earlier unvoiced hypothesis was not entirely wrong — there *is* an alternate excitation mode — it is just **re-sync-triggered by a state machine, not selected by a per-frame bit.** That distinction is the entire reason it never showed on the continuous test files everyone validated against, and why it is absent from the published spec.

---

## Part 7 — The wall

To finish, we need to see *exactly* what that state machine switches the excitation to. The per-frame decode functions don't tell us — they're identical to the open ports. We need to watch the **running** Olympus decoder's evolving excitation/pitch state across the `count=19` block. And that is where a hard, architectural wall stands.

We must make the closed-source Olympus decoder execute inside a process we can instrument. Every public way to wire its two DirectShow filters (`DssParser` → `DssDecoder`) fails:

- `ConnectDirect(parser_out, decoder_in, NULL)` → **`VFW_E_NO_ACCEPTABLE_TYPES`**: the two pins share *no* media type.
- `Connect(parser_out, PCM_sink)` (intelligent connect, which would auto-insert the decoder) → **`VFW_E_CANNOT_CONNECT`**: no chain of intermediate filters bridges them.
- `Render(parser_out)` → cannot build a chain; it only "succeeds" with a `NullRenderer` pre-added, in which case it wires parser→null directly and the decoder is *loaded but never run* (its decode loop never fires — verified by hooking it).

The conclusion is not "we missed a trick." It is structural: **the Olympus filters interconnect only through the Olympus application's own code, with proprietary media-type negotiation, not through any documented DirectShow API.** And the obvious alternative — instrument NCH Switch, which decodes fine — is blocked too: Switch is **anti-debug** (frida spawn and attach both fail) and it appears to decode through a *different* DLL (`dss32.dll`) than the one we disassembled.

That is the honest edge of the map.

---

## Part 8 — The open problem (and what's already built for whoever solves it)

**The question, stated precisely:** what does the `count=19`-armed state machine (`0x6c`/`0x328`/`0x32c`/`0x330`) cause `FUN_10017e70`/`FUN_10018800` to do to the excitation for the frames it covers, such that the excitation becomes ~10× louder and pitched, independent of the per-subframe gains?

**Two routes to the answer, both multi-day:**

- **Instrument a decoder that runs.** A 32-bit DLL-proxy that forwards every COM export to the real decoder and inline-hooks its synthesis to log the excitation buffer (`+0x36b8`) — built with a Windows cross-compiler, and targeted at whichever DLL the host actually executes (`dss32.dll` for Switch, with hook offsets re-derived there). Or drive the Olympus app (ODMS) — which builds the graph correctly — under an instrument it doesn't resist.
- **Pure static RE** of the state machine plus the mode branches of the excitation builder, reading exactly what the counter switches on.

**What is already in place** — so the next attempt starts at the wall, not the beginning:
- A **decoder-free regression test**: the inverse-filter A/B. It already pins the exact diverging subframes and the *target* excitation RMS for each. Any candidate fix is validated in minutes against the Olympus reference, no decoder hooking needed.
- A **headless reference generator** (Switch from a disconnected session).
- A full Ghidra map of the decode path (`FUN_10016540 → FUN_10018450 → FUN_10017e70 → FUN_10018800 → FUN_10019d40`), with the state-machine offsets and the PRNG/gain tables located.
- Every dead end above, falsified, so no one re-runs them.

---

## Coda

We did not fix it. But we turned "the decoder is wrong somewhere on some files" into a single, sharp, fully-evidenced sentence: *a structural re-sync marker arms a hidden state machine that changes excitation generation, and the pitch-feedback loop smears it into a few seconds of ruined audio.* We proved the filter is flawless, proved the framing is correct, falsified nine other explanations, found the exact trigger to the subframe, and identified the precise state machine responsible. What stops the last inch is not a missing idea — it is a decade-old design decision to lock the codec inside one application, and a commercial tool that bites the hand that probes it.

The format gave up almost all of its secrets. This is the one it's still holding.

---

*Coefficients perfect, framing perfect, nine ghosts laid to rest — and the last door is locked from the inside. 🔒*
