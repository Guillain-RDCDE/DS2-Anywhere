# The lattice hunt

_How we disassembled a codec byte by byte to fix a bug that wasn't supposed to exist._

---

## The sound of nothing

On 25 August 2026, a transcription worker in Madagascar pressed play on an audio file and heard a human voice for exactly fifty-eight seconds. Then the voice turned into a wall of static, like someone had pressed a microphone into a waterfall. She parked the command. The next day, a different worker tried. Same result. Same file, same wall at the same second. By the third day it was an incident.

The file was a `.DSS` — an Olympus Digital Speech Standard recording, the codec that doctors and lawyers have been dictating into for thirty years. Our decoder handled it fine for the first minute. Then it exploded.

## Seven files, four broken

The client used an Olympus DS-7000. Seven files came in that week. We decoded all seven. Three sounded perfect. Four went haywire at roughly the same point — between 55 and 62 seconds in. Not random noise: a specific, violent distortion, like the audio had been fed through a broken amplifier. The recording was still there, underneath. You could almost hear words through the static, the way you can almost read a letter through frosted glass.

We measured it. The root-mean-square energy of normal speech from this recorder sat at around 3,000. At second 58, it jumped to 27,000. At second 60, it clipped at 32,768 — the absolute maximum a 16-bit integer can hold.

The filter had gone unstable.

## Ruling things out

We did what you do when a filter blows up: we checked the obvious culprits.

**The tables.** Our codebook — the lookup table that maps quantized indices to filter coefficients — came from FFmpeg. Maybe the values were wrong. We extracted the real codebook from the Olympus DLL: 14 rows of 256 double-precision floating-point numbers, at virtual address 0x10050008 in the binary. We plugged them in. The instability got *worse* — because the DLL's codebook is calibrated against the DLL's own excitation tables, not FFmpeg's.

**The precision.** Our original codec used Q15 fixed-point arithmetic: every multiplication is followed by a right-shift of 15 bits, which throws away the low bits. Over millions of operations, those tiny errors accumulate. We rewrote the entire synthesis path in IEEE 754 double-precision floating point. Same instability. Same frame. Precision was not the problem.

**The DLL.** We built a custom program — a DirectShow filter graph harness — that loads the Olympus DssDecoder.dll, feeds it the raw bitstream, and captures the PCM output. The DLL decoded all seven files. Twenty-three minutes of perfectly stable audio. RMS between 2,300 and 4,400. Zero clips.

Whatever we were doing wrong, the DLL was doing right.

## Into the DLL

On 27 August, we decided to stop guessing and start reading. We would disassemble the DLL's synthesis pipeline — every function, every instruction, every floating-point operation — and find out exactly what it does that we don't.

The DLL is a 32-bit Windows PE binary. Its synthesis lives in the `.text` section, starting at virtual address 0x10001000. The code uses the x87 floating-point unit: `FLD` to load a double onto the FPU stack, `FMUL` to multiply, `FADD` to add, `FSTP` to store and pop. No SSE. Pure 1990s x87.

We extracted four functions:

| Function | Address | Size | FPU ops | What we expected | What it actually was |
|----------|---------|------|---------|------------------|---------------------|
| func_18E90 | 0x10018E90 | 462 bytes | 21 | Error correction filter | **Codebook dequantizer** |
| func_17090 | 0x10017090 | 965 bytes | 56 | Main synthesis filter | **Excitation generator** |
| lattice_filter | 0x10019060 | 392 bytes | 45 | IIR synthesis | IIR lattice (confirmed) |
| func_177F0 | 0x100177F0 | 478 bytes | 66 | Post-filter | **De-emphasis + int16 clamp** |

Every one of our initial labels was wrong except the lattice.

## The surprise

We expected to find a complex pipeline. The DLL had to be doing *something* we weren't — some sophisticated post-filter, some clever normalization, some formant-shaping trick.

Instead, the DLL's synthesis pipeline was almost embarrassingly simple:

1. Look up reflection coefficients from the codebook. (func_18E90 — not a filter, just a table lookup.)
2. Generate the excitation signal: repeat the previous pitch period scaled by a gain factor, add seven sparse pulses at combinatorially-decoded positions. (func_17090 — not a filter either.)
3. Push the excitation through a **14-stage lattice filter** using the raw reflection coefficients. No polynomial conversion. No bandwidth expansion. No normalization. (lattice_filter.)
4. Apply a gentle first-order de-emphasis: `y[n] = x[n] + 0.1 * y[n-1]`. Convert to int16 with symmetric saturation at +/-32,767. (func_177F0.)
5. Sinc-resample from 12,000 Hz to 11,025 Hz.

That's it. Five steps. No magic.

## The five ghosts

Our decoder — derived from FFmpeg's implementation, which was itself reverse-engineered years ago — had five additional components that the DLL does not use:

1. An **error correction IIR filter**: a 14th-order recursive filter using the raw polynomial coefficients, with no damping. This filter can resonate when coefficients are near the unit circle.

2. An **FIR pre-filter** with bandwidth expansion at gamma = 0.5 (each coefficient multiplied by 0.5^i).

3. The **IIR synthesis in polynomial direct-form** with bandwidth expansion at gamma = 0.8 — instead of the DLL's lattice with raw coefficients.

4. A **noise modulation** step: compute the ratio of input energy to output energy, build a first-order exponential envelope from that ratio, and multiply the output by the envelope.

5. A **dynamic normalization** step: scale the signal up by a power of 2 before filtering and back down after, to maximize the use of the integer range.

Five components. All absent from the DLL. All inherited from FFmpeg's reverse-engineering of the codec, which approximated the DLL's behavior with a more complex pipeline that happened to produce similar output — until it didn't.

## Why the lattice matters

A lattice filter and a polynomial direct-form filter can compute the same transfer function — but they compute it differently, and the difference matters for stability.

The polynomial form converts reflection coefficients to polynomial coefficients through a Levinson recursion, then filters with those polynomial coefficients. If the polynomial has roots close to the unit circle (which happens when the original reflection coefficients are close to +/-1, which is common in speech), tiny errors can grow over time.

The lattice form uses the reflection coefficients directly. It is **mathematically guaranteed** to be stable when every |k_i| < 1 — which is guaranteed by any valid speech codebook. No error can accumulate. No resonance can build up. It is unconditionally stable by construction.

That is why the DLL never blows up. Not because of better precision. Not because of a clever trick. Because it uses the right filter structure.

## The codebook gap

Even with the correct lattice structure, our decoder still slowly drifted from the DLL's output over thousands of frames. The correlation started at 0.93 at frame 10 (excellent), held at 0.88 at frame 1,000, and then collapsed to zero at frame 2,500. The reason: FFmpeg's codebook tables are approximations. They differ from the DLL's real values by 1-5%. Over 2,400 frames, those small differences compound through the pitch-adaptive codebook feedback loop until the accumulated error pushes the system into resonance.

Using the DLL's exact codebook values would eliminate this drift — but only if we also used the DLL's excitation codebooks, gain tables, and pulse value tables. The codebooks form a calibrated system. Mixing one source's reflection coefficients with another source's excitation tables makes the imbalance worse, not better. We tested this. The RMS went from 27,000 to 32,768 — full saturation from the first minute onward.

## The fix that ships

We replaced the FFmpeg-derived synthesis with the DLL's actual pipeline: a pure lattice filter with raw reflection coefficients, a de-emphasis post-filter, and int16 conversion with symmetric clamping. To compensate for the FFmpeg codebook approximation, we kept a simple energy limiter (AGC) at a threshold of 0.15 RMS per subframe — the top of the DLL's observed output range.

The result: 13 out of 13 DSS SP files decode stably over their full duration (up to 23 minutes), with a maximum RMS of 4,998. The correlation with the DLL oracle is 0.93 — up from 0.80 with the old pipeline. The code is 174 lines shorter.

## What it took

Three days, four functions, 2,500 bytes of machine code. A DirectShow harness built from scratch to capture the DLL's output. A Wine installation on a GPU server to run the DLL under a debugger. Three failed hypotheses (precision, codebook, filter structure alone). Seven experimental Rust builds. Four parallel reverse-engineering agents disassembling x87 FPU instructions. And the realization, at the very end, that the answer was not in what the DLL does differently — but in what it doesn't do at all.

The five components we removed were not bugs. They were a faithful implementation of someone's best guess at how the codec worked, made years ago with fewer tools and less information than we had. They produced correct-sounding output on short files. They only failed when the accumulated approximation error exceeded the polynomial filter's stability margin — which took exactly 58 seconds.

Sometimes the fix is not adding something. Sometimes it's taking five things away.
