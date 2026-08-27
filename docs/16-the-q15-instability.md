# 16 - The Q15 instability

_Or: why a codec that is 99.9% right can still blow up your speakers after one minute._

## The symptom

A batch of seven DSS files from the same Olympus DS-7000 recorder. Three decoded fine. Four came out distorted -- our transcription workers parked the command three times in three days, each time a different person wrote "audios illisibles."

## The investigation

### 1. The first 58 seconds are perfect

Our decoder and the DLL oracle correlate at **0.93** for the first 58 seconds. Then, at exactly frame 2,420 (t = 58.0s), the correlation drops to zero. The RMS jumps from 3,000 to 27,000.

### 2. It is not the tables

We extracted the DLL codebook tables (14 x 256 doubles at VA 0x10050008) and replaced ours. The instability got *worse* -- the DLL tables are calibrated against the DLL's own excitation codebooks, not FFmpeg's.

### 3. It is not the arithmetic precision

We rewrote the entire codec in f64 floating-point. Same instability at the same frame.

### 4. The DLL is perfectly stable

We built a DirectShow harness (render_to_wav.exe) that loads the real DssDecoder.dll from the Olympus ODMS installation. The DLL produces 1,404 seconds of perfectly stable audio. RMS stays between 2,300 and 4,400.

### 5. NCH Switch uses a completely different codec

Correlation between the DLL output and NCH Switch is 0.04 (random noise). Both produce intelligible audio, but from different algorithms.

## The full DLL reverse engineering

On 27 August 2026, we disassembled the entire synthesis pipeline of the DLL -- four functions, 2,500 bytes of x86-32 machine code with x87 FPU instructions:

| Function | VA | Size | Role |
|----------|------|------|------|
| synthesis_pipeline | 0x100175B0 | 564 | Orchestrator: dequantize, loop over subframes |
| func_18E90 | 0x10018E90 | 462 | Codebook dequantizer: quantized indices to f64 reflection coefficients |
| func_17090 | 0x10017090 | 965 | Excitation generator: adaptive CB + combinatorial pulse decoder |
| lattice_filter | 0x10019060 | 392 | IIR lattice synthesis: 14-stage Burg form, raw reflection coefficients |
| func_177F0 | 0x100177F0 | 478 | De-emphasis (alpha=0.1) + int16 conversion with +/-32767 clamping |

### What the DLL does

The DLL's per-frame pipeline is strikingly simple:

1. **Dequantize** reflection coefficients from the codebook (14 doubles)
2. **Per subframe (x4):**
   - Generate excitation: pitch-periodic adaptive codebook + 7 sparse algebraic codebook pulses
   - **Lattice IIR 1/A(z)** with raw reflection coefficients
3. **De-emphasis:** `y[n] = x[n] + 0.1 * y[n-1]`
4. **Int16 conversion** with symmetric +/-32767 clamping
5. **Sinc resampling** 12000 to 11025 Hz

### What FFmpeg's decoder does that the DLL does not

| Component | Effect |
|-----------|--------|
| Error correction IIR filter (err_buf2) | IIR with raw polynomial -- can resonate |
| FIR pre-filter (shift_sq_add, gamma=0.5) | Bandwidth expansion numerator |
| IIR synthesis (shift_sq_sub, gamma=0.8) | Different transfer function from lattice |
| Noise modulation (vsum1/vsum2, multiplicative) | Energy ratio envelope |
| Normalization (normalize_bits scaling) | Dynamic range management from Q15 era |

### The lattice filter

The lattice uses reflection coefficients directly without converting to polynomial form:

```
for each sample n:
    f = input[n] - k[13] * b[13]
    for i = 12 down to 0:
        f_new = f - k[i] * b[i]
        b[i+1] = b[i] + k[i] * f_new
        f = f_new
    b[0] = f; output[n] = f
```

A lattice filter is **intrinsically stable** when |k_i| < 1 -- guaranteed by any valid speech codebook.

## The root cause

The instability is the accumulated effect of FFmpeg codebook approximation: the tables differ from the DLL's by 1-5%, causing a slight energy imbalance per frame. Over ~2,400 frames, the cumulative error pushes the synthesis into resonance.

## The fix

We replaced the FFmpeg synthesis with a clean pipeline matching the DLL:

1. **Lattice IIR** with raw reflection coefficients (no FIR, no polynomial, no bandwidth expansion, no noise modulation)
2. **AGC** (threshold 0.15): compensates for the FFmpeg codebook approximation
3. **De-emphasis:** `y[n] = x[n] + 0.1 * y[n-1]`
4. **Int16 quantization** with +/-32767 clamping

Result: **13/13 DSS SP files stable**, max RMS 4,998, correlation with DLL **0.93** (up from 0.80). The code is 174 lines shorter.
