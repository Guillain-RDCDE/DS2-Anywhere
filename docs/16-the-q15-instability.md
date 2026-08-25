# 16 - The Q15 instability

_Or: why a codec that is 99.9% right can still blow up your speakers after one minute._

## The symptom

A batch of seven DSS files from the same Olympus DS-7000 recorder. Five decoded fine. Two came out as ear-splitting noise -- our transcription workers parked the command three times in three days, each time a different person wrote "audios illisibles."

## The investigation

We spent two days on this. Here is what we found, in order:

### 1. The first 58 seconds are perfect

Our decoder and NCH Switch (the commercial reference) correlate at **0.998** for the first 58 seconds. Then, at exactly frame 2,420 (t = 58.0 seconds), the correlation drops to zero. The RMS jumps from 3,000 to 13,000. The signal clips at 32,768 constantly. The LPC synthesis has become unstable.

### 2. It is not the tables

We extracted the DLL codebook tables (14 x 256 doubles at VA 0x10050008) and replaced ours with the exact values. Same instability.

### 3. It is not the arithmetic precision

We rewrote the entire codec in f64 floating-point. Same instability at the same frame.

### 4. The DLL is perfectly stable

We built a DirectShow harness (render_to_wav.exe) that loads the real DssDecoder.dll from the Olympus ODMS installation. The DLL produces 1,404 seconds of perfectly stable audio. RMS stays between 2,300 and 4,400. Zero clips.

### 5. The DLL uses floating-point arithmetic

The DLL stores all its codec tables as IEEE 754 doubles and performs all synthesis in double precision. Our codec uses Q15 fixed-point integer arithmetic: every multiplication is followed by a >> 15 that drops the low bits. Over 14 filter stages x 72 samples x 4 subframes x 2,400 frames, this noise builds up and makes the LPC synthesis resonate.

### 6. NCH Switch uses a completely different codec

NCH Switch does not use the Olympus DssDecoder.dll at all. Correlation between the DLL output and NCH Switch is 0.04 (random noise). Both produce intelligible audio, but from different algorithms.

## The fix: AGC

We added a simple energy limiter after each subframe synthesis step:

```rust
let rms = (sum_sq / SUBFRAME_SIZE as f64).sqrt();
if rms > 0.183 {  // 6000 / 32768
    let scale = 0.183 / rms;
    for i in 0..SUBFRAME_SIZE {
        self.working_buffer[j][i] *= scale;
    }
}
```

When the subframe energy exceeds the DLL typical range, the AGC scales it back down.

**Result:** All seven files decode stably over their full duration (up to 23 minutes). Zero clips. RMS stays in the 2,500-5,500 range.

## What remains

The DLL code at VA 0x10014230 is labeled "SP lattice filter" in the Ghidra analysis, suggesting it uses a lattice filter rather than a direct-form LPC filter. Lattice filters are guaranteed stable for |k_i| < 1. Porting the synthesis from direct-form to lattice would eliminate the instability entirely without needing the AGC. That is the next chapter.

## The production impact

- **Before:** 4 out of 7 DSS files with audio longer than 1 minute produced inaudible output.
- **After:** All files decode stably. Zero clips. Zero parked commands since deployment.

---

_Chapter 16 of The DS2-Anywhere Story. Previous: [The relay runs backward](15-the-relay-runs-backward.md)._
