# The Lattice Hunt

*How we disassembled a codec byte by byte to fix a bug that took exactly 58 seconds to appear.*

---

## The sound of nothing

On 25 August 2026, a transcription worker in Madagascar pressed play on an audio file and heard a human voice for exactly fifty-eight seconds. Then the voice turned into a wall of static. She parked the command. The next day, a different worker tried. Same result. Same file, same wall at the same second. By the third day it was an incident.

The file was a `.DSS` — an Olympus Digital Speech Standard recording, the format that doctors and lawyers have been dictating into for thirty years. Our decoder handled it fine for the first minute. Then it exploded.

## Seven files, four broken

The client used an Olympus DS-7000. Seven files came in that week. Three sounded perfect. Four went haywire between 55 and 62 seconds in. Not random noise: a specific, violent distortion, like the audio had been fed through a broken amplifier. The recording was still there, underneath — you could almost hear words through the static, the way you can almost read a letter through frosted glass.

We measured it. The root-mean-square energy of normal speech from this recorder sat at around 3,000. At second 58, it jumped to 27,000. At second 60, it clipped at 32,768 — the absolute maximum a 16-bit integer can hold.

The LPC synthesis filter had gone unstable.

## Where the code came from

Our DSS SP decoder is derived from FFmpeg's `dss_sp.c`, written in 2014 by Oleksij Rempel — a reverse-engineering effort that cracked the DSS SP codec well enough to produce intelligible audio from every file anyone had tested it on. It was the only open implementation in existence. We ported it, shipped it, and ran thousands of dictations through it without a problem.

Until these seven files from one specific recorder.

## Ruling things out

We checked the obvious culprits.

**The tables.** Our codebook — the lookup table that maps quantized indices to filter coefficients — came from Rempel's reverse-engineering. Maybe the values were slightly off. We extracted the real codebook from the Olympus DLL: 14 rows of 256 double-precision floating-point numbers, at virtual address `0x10050008` in the binary. We plugged them in. The instability got *worse*. The DLL's codebook is calibrated against the DLL's own excitation tables, not the ones in `dss_sp.c`. You can't swap one table from one implementation into another. They're a matched set, like a lock and its key.

**The precision.** The FFmpeg codec uses Q15 fixed-point arithmetic: every multiplication is followed by a right-shift of 15 bits, which throws away the low bits. Maybe the accumulated rounding was the problem. We rewrote the entire synthesis path in IEEE 754 double-precision floating point — 52 bits of mantissa, no rounding. Same instability. Same frame. Precision was not the problem.

**The DLL.** We built a custom program — a DirectShow filter graph harness called `render_to_wav.exe` — that loads the Olympus DssDecoder.dll, feeds it the raw bitstream, and captures the PCM output. The DLL decoded all seven files. Twenty-three minutes of perfectly stable audio. RMS between 2,300 and 4,400. Zero clips.

Whatever the difference was between `dss_sp.c` and the DLL, the DLL had it right.

## Into the DLL

On 27 August, we stopped guessing and started reading. We would disassemble the DLL's synthesis pipeline — every function, every instruction, every floating-point operation — and find out exactly what it does that `dss_sp.c` doesn't.

The DLL is a 32-bit Windows PE binary. Its synthesis lives in the `.text` section, starting at virtual address `0x10001000`. The code uses the x87 floating-point unit: `FLD` to load a double onto the FPU stack, `FMUL` to multiply, `FADD` to add, `FSTP` to store and pop. No SSE. Pure 1990s x87.

We extracted four functions:

| Function | Address | Size | FPU ops | What we expected | What it actually was |
|----------|---------|------|---------|------------------|---------------------|
| `func_18E90` | `0x10018E90` | 462 bytes | 21 | Error correction filter | **Codebook dequantizer** |
| `func_17090` | `0x10017090` | 965 bytes | 56 | Main synthesis filter | **Excitation generator** |
| `lattice_filter` | `0x10019060` | 392 bytes | 45 | IIR synthesis | **IIR lattice (confirmed)** |
| `func_177F0` | `0x100177F0` | 478 bytes | 66 | Post-filter | **De-emphasis + int16 clamp** |

Every one of our initial labels was wrong except the lattice.

## The surprise

We expected to find a complex pipeline. The DLL had to be doing *something* subtle — some sophisticated post-filter, some clever normalization, some formant-shaping trick.

Instead, the DLL's synthesis was almost embarrassingly simple.

The function we thought was an error correction filter (462 bytes) turned out to be a codebook dequantizer — it just reads quantized indices and looks them up in a table. No filtering at all.

The function we thought was the main synthesis filter (965 bytes, the biggest of the four) turned out to be the excitation generator — the adaptive codebook and pulse decoder. Not a filter. It generates the *input* to the filter.

The post-filter we expected to find formant shaping in (478 bytes) turned out to be a one-line de-emphasis: `y[n] = x[n] + 0.1 * y[n-1]`, followed by int16 conversion. That's it.

And the lattice filter — the only function we labeled correctly — is the entire synthesis. Not a piece of a larger pipeline. The whole thing.

**The DLL does five things. `dss_sp.c` does ten. The five extra things were what made it unstable.**

## What `dss_sp.c` does that the DLL doesn't

| Component in `dss_sp.c` | Present in DLL |
|--------------------------|:--------------:|
| Error correction IIR (`shift_sq_sub` with `err_buf2`) | **No** |
| Levinson recursion (`convert_coeffs`) | **No** |
| FIR pre-filter (`shift_sq_add` with `binary_decreasing`) | **No** |
| IIR synthesis in polynomial direct-form (`shift_sq_sub` with `unc_decreasing`) | Replaced by lattice |
| Noise modulation (energy ratio, multiplicative envelope) | **No** |
| Dynamic normalization (`normalize_bits` scaling) | **No** |

These components were Rempel's best reconstruction of the DLL's behavior in 2014. He didn't have a way to capture the DLL's output or disassemble its internals at the time. His implementation produced correct-sounding output on every file it was tested on — and it was the *only* open DSS SP decoder in the world. That's not nothing.

But the synthesis structure was an approximation. The FIR + polynomial IIR with bandwidth expansion was his equivalent of the DLL's lattice. The noise modulation was his equivalent of... nothing. The error correction filter was his equivalent of a codebook dequantizer. They all produced similar-enough results on short files. They only diverged when the accumulated approximation error exceeded the polynomial filter's stability margin — which took exactly 58 seconds.

## The lattice

A lattice filter and a polynomial direct-form filter can compute the same transfer function — but they compute it differently, and the difference matters for stability.

The polynomial form converts reflection coefficients to polynomial coefficients through a Levinson recursion, then filters with those polynomial coefficients. If the polynomial has roots close to the unit circle — which happens when the original reflection coefficients are close to \u00b11, which is common in speech — tiny errors can grow over time.

The lattice form uses the reflection coefficients directly, without converting them:

```
for each sample n:
    f = input[n] - k[13] * b[13]
    for i = 12 down to 0:
        f_new = f - k[i] * b[i]
        b[i+1] = b[i] + k[i] * f_new
        f = f_new
    b[0] = f
    output[n] = f
```

A lattice filter is **mathematically guaranteed** to be stable when |k_i| < 1 — which is guaranteed by any valid speech codebook. No error can accumulate. No resonance can build.

That is why the DLL never blows up. Not because of better precision. Not because of a clever trick. Because it uses the right filter structure for the job.

## The codebook gap

Even with the correct lattice structure, our decoder still drifted from the DLL's output over thousands of frames. The correlation started at 0.93 at frame 10 — excellent — held at 0.88 at frame 1,000, and collapsed to zero at frame 2,500.

The reason: the codebook tables in `dss_sp.c` are approximations of the DLL's real values, off by 1–5%. Over 2,400 frames, those small differences compound through the pitch-adaptive codebook feedback loop until the accumulated error overwhelms the filter.

Using the DLL's exact codebook would fix this — but only if we also replaced the excitation codebooks, gain tables, and pulse value tables. The codebooks are a calibrated system. We tested mixing one source's coefficients with another's excitation tables. The instability went from bad to *immediate*: full saturation from the first minute.

## The fix

We replaced `dss_sp_sf_synthesis()` — with its FIR, polynomial IIR, noise modulation, and normalization — with the DLL's actual pipeline:

1. **Lattice IIR** with raw reflection coefficients (14 stages, Burg form)
2. **AGC** at 6,000 RMS per subframe — compensates for the codebook approximation gap
3. **De-emphasis:** `y[n] = x[n] + 0.1 * y[n-1]`
4. **Int16 conversion** with symmetric \u00b132,767 clamping

The removed functions: `dss_sp_convert_coeffs`, `dss_sp_shift_sq_add`, `dss_sp_shift_sq_sub`, `dss_sp_vec_mult`, `dss_sp_get_normalize_bits`, `dss_sp_scale_vector`, `dss_sp_vector_sum`, and the noise modulation block inside `dss_sp_sf_synthesis`. The tables `binary_decreasing_array` and `dss_sp_unc_decreasing_array` are also gone.

**Result: 13/13 DSS SP files stable.** Maximum RMS 4,998, correlation with the DLL oracle 0.93 (up from 0.80 with the old pipeline), and 174 fewer lines of code.

## What it took

Three days. Four functions. 2,500 bytes of machine code. A DirectShow harness built from scratch to capture the DLL's output. A Wine installation on a GPU server to load the DLL under controlled conditions. Three failed hypotheses. Seven experimental builds. And the realization, at the end, that the answer was not in what the DLL does differently — but in what it doesn't do at all.

The five components we removed were not bugs. They were a working approximation made twelve years ago with fewer tools and less information. They decoded thousands of files without complaint. They only failed on long recordings from specific hardware, when the cumulative drift from the real codebook values tipped the polynomial filter past its stability margin.

Sometimes the fix is not adding something. Sometimes it's taking five things away.

---

*Technical reference: [Chapter 16](16-the-q15-instability.md). Source code: [dss_sp.rs](../vendor/dss-codec/src/codec/dss_sp.rs). FFmpeg C patch: [ffmpeg-upstream/](../ffmpeg-upstream/).*

*This is part of [DS2-Anywhere](https://github.com/Guillain-RDCDE/DS2-Anywhere) — the project to open Olympus and Grundig dictation formats on any machine.*
