# 01 — The reverse-engineering

> How a single developer broke a ten-year stalemate on a proprietary audio format, using a free decompiler and a lot of patience.

This chapter is about **Kieran Hirpara's work**, not ours. We didn't reverse-engineer the codec — we only integrated it. But to integrate it well, we had to understand what was actually in our hands. This is what we learned.

---

## 1. Why was DS2 closed?

The Digital Speech Standard 2 (DS2) is Olympus's proprietary container + codec for their dictation recorders (DS-series, DM-series, DPM-series — gear used massively by lawyers, doctors, court reporters). The format dates back to **2007** and was Olympus's answer to the older DSS Standard Play.

It was never standardized. The spec was never published. The only decoders were:

- Olympus's own `DssDecoder.dll` (bundled with the DSS Player software, Windows-only)
- NCH Software's `dss32.dll` (bundled with their commercial Switch product)
- A handful of forensic-software vendors who'd licensed the codec under NDA

Two practical consequences:

1. **You couldn't decode DS2 on Linux, macOS, BSD, or in a browser.** Period. If your workflow handled dictations from these devices, you had to keep a Windows machine in the loop somewhere.
2. **FFmpeg, the universal audio swiss-army-knife, had no support.** [Ticket #6091](https://trac.ffmpeg.org/ticket/6091) ("Add DS2 codec support") was opened in October 2017 and sat unimplemented for nine years. Comments came and went. Nobody had the time to do the work.

FFmpeg has support for `dss_sp` (the older, simpler DSS Standard Play). But DS2 SP and DS2 QP use **completely different lookup tables** and a different frame structure — what works for DSS doesn't work at all for DS2.

So for nine years, if you needed `.ds2` on Linux, you had two options: pay for forensic software, or run a Windows VM. Most production pipelines (ours included, before this work) ended up with the second.

## 2. Then someone did the work

In February 2026, Kieran Hirpara published [hirparak/dss-codec](https://github.com/hirparak/dss-codec). The README is modest. The work behind it is not.

What's in the repo:

- A complete spec of the DS2 (and DSS SP) bitstream — every field, every constant, every table.
- A Python reference decoder (`ds2decode.py`, `dss_decode.py`) you can read in an afternoon.
- A Rust crate with a CLI binary, a streaming decoder, and AES decryption for password-protected DS2.
- A WAV/PCM output path verified **byte-for-byte** against the output of Olympus's own DirectShow filter.

For nine years, the FFmpeg ticket sat. Then one person wrote ~4 400 lines of Rust and changed the answer.

## 3. The reverse-engineering toolkit

If you've never reverse-engineered a binary format, here's the 60-second primer.

### Ghidra

[Ghidra](https://ghidra-sre.org/) is a free, open-source software reverse-engineering toolkit released by the **NSA** in 2019 (yes, that NSA). It does what until then required expensive commercial tools (IDA Pro): it takes a compiled binary — a `.dll`, a `.exe`, an `.so` — and turns it back into **pseudo-C code** you can read.

The pseudo-C isn't pretty. Functions are named `FUN_10017460`. Variables are `iVar3`, `puVar7`, `local_18`. Loops look like `do { ... } while (cVar2 != '\0');`. But it's **readable** — and once you stare at it long enough, the original algorithm emerges.

### What you start with

For DS2, Hirpara worked from two binaries:

- **`DssDecoder.dll`** — 465 KB, from Olympus's AudioSDK. The official Olympus decoder, distributed with the DSS Player software.
- **`dss32.dll`** — 215 KB, from NCH Switch. NCH licensed the codec from Olympus and shipped their own wrapper.

Why two? Because **comparing two implementations of the same codec** is gold. If both DLLs do the same thing in slightly different code, you can triangulate what's essential (the codec) vs what's incidental (each vendor's wrapping).

### What you do with it

Roughly, the loop:

1. Open the DLL in Ghidra.
2. Identify the entry points (the exported functions: `Decode`, `Init`, `GetSampleRate`, etc.).
3. Follow the call graph. Read the pseudo-C. Rename `FUN_xxxxx` to `decode_frame` as you figure out what they do.
4. Spot the **patterns** of a known algorithm family. CELP codecs have a very recognizable shape: a synthesis filter loop, codebook lookups, pitch prediction. Once you see those shapes, you know what you're looking for.
5. Extract the constants: the **codebooks**. These are usually large arrays of numbers in the binary's `.rdata` section. Dump them, organize them.
6. Write a from-scratch decoder in a language you control (Python first, for clarity), feed it the codebooks, and check the output.

It is slow, careful work. The reward is that you no longer depend on anyone else's binary.

## 4. The methodology, step by step

This is reconstructed from reading Hirpara's [CODEC_SPECIFICATION.md](https://github.com/hirparak/dss-codec/blob/main/dss-codec/CODEC_SPECIFICATION.md), the code itself, and standard reverse-engineering practice.

### Step 1 — Identify the right binaries

Not every DLL on a Windows machine is interesting. Hirpara zeroed in on the two that actually contained the decoder. The references in the spec — `FUN_10017460` for the bitstream reader, `FUN_100180c0` for the DSS SP decoder parameters — are Ghidra-assigned names in those exact DLLs. That tells you the work was done with these specific binaries open in Ghidra.

### Step 2 — Decompile and triage

A 465 KB DLL contains hundreds of functions. Most are uninteresting (Windows COM boilerplate, string handling, registry access). The decoder itself is maybe 20-30 functions. The triage is tedious but mechanical.

### Step 3 — Recognize CELP

The DS2 codec is a variant of **CELP (Code-Excited Linear Prediction)** — the same family as G.729, the codec that powered most digital telephony from the late 1990s onward. CELP models human speech in a clever way:

- Speech is what you get when an **excitation signal** (the vocal cords vibrating, or the breath for fricatives) passes through a **filter** (the vocal tract shape).
- A CELP codec stores, for each 10–20 ms frame of audio: the filter parameters (a few numbers describing the vocal tract shape) and the excitation parameters (which "shape" from a predefined codebook + a gain).
- To decode, you do the reverse: rebuild the filter, regenerate the excitation, run it through, get audio back.

The bit savings come from the codebook lookup: instead of storing the raw audio, you store a 7-bit index into a codebook of 128 pre-defined excitation shapes. Multiply that by every frame and you compress speech down to ~13–28 kbps.

Recognizing CELP early was the key. Once you know "this is CELP", you know what to look for in the binary: synthesis filter taps, pitch predictor, codebook indexing, gain scalars.

### Step 4 — Write a Python reference decoder

Python is slow for audio, but it's **readable**. Hirpara's `ds2decode.py` exists not to be fast, but to prove "I understand this codec well enough to rewrite it from scratch". It uses the codebooks extracted from the DLL (shipped as `.npz` files in the repo) and runs the decode logic in plain Python.

Once it works on a single file, you have a **reference implementation** you can trust.

### Step 5 — Verify byte-for-byte against the official decoder

This is the most important step, and the one that separates "I think it works" from "it works".

Olympus ships a **DirectShow filter** on Windows that decodes DS2 to PCM WAV. You feed it a DS2 file, get a WAV. You feed the same DS2 to your Python decoder, get a WAV. You compare the two WAVs **sample by sample**.

If they match — every PCM sample, every offset — you're done. You've successfully reverse-engineered the algorithm.

If they don't match, you have a bug somewhere. Find it. Iterate. This is how you build confidence that your decoder is faithful, not "close enough".

### Step 6 — Rewrite in Rust

Once the Python prototype is verified, the Rust port is engineering, not research. Rust gives you:

- Speed (compiled, with SIMD vectorization).
- A CLI binary you can ship as a single file.
- A WASM target (via wasm-bindgen) usable from browsers and Node.js.
- Memory safety without garbage collection (important for embedded use).

The result is what we use in production.

## 5. What's actually in a DS2 file

To make the rest concrete, here's the file structure (simplified). Magic bytes first:

| Bytes 0-3 | Meaning |
|---|---|
| `03 64 73 32` (`\x03ds2`) | Plain DS2 |
| `03 64 73 73` (`\x03dss`) | Plain DSS (older format) |
| `03 65 6e 63` (`\x03enc`) | **Encrypted** DS2 (AES-128 or AES-256) |

Then the layout:

```
┌────────────────────────────────────────────────────────────┐
│ HEADER  (0x600 bytes / 1536 bytes)                         │
│                                                            │
│   Magic + file metadata:                                   │
│     - format type (DS2 SP / DS2 QP / DSS SP)              │
│     - sample rate (12000 or 16000 Hz)                     │
│     - duration                                             │
│     - recording date                                       │
│     - device serial                                        │
│     - encryption descriptor (if encrypted, at 0x146)      │
└────────────────────────────────────────────────────────────┘
┌────────────────────────────────────────────────────────────┐
│ AUDIO BLOCKS — each 0x200 bytes (512)                      │
│                                                            │
│   Each block:                                              │
│     - 6 bytes block header                                 │
│     - 506 bytes packed CELP frames                         │
│                                                            │
│   One frame =                                              │
│     - 328 bits (DS2 SP) or 448 bits (DS2 QP)              │
│     - Reflection coefficients (filter shape)               │
│     - Pitch lag                                            │
│     - Codebook indices + gains                             │
│     - Per-subframe parameters                              │
└────────────────────────────────────────────────────────────┘
┌────────────────────────────────────────────────────────────┐
│ AUDIO BLOCK                                                │
└────────────────────────────────────────────────────────────┘
        ... (one block every ~24 ms or ~16 ms of audio) ...
```

For a typical 30-minute dictation:

- DS2 QP (16 kHz): ~112 500 frames, ~6.3 MB compressed
- Decompressed PCM: ~58 MB (16-bit mono at 16 kHz)
- ~9× compression ratio over PCM, ~3× over a typical 64 kbps MP3 of the same speech

## 6. The decode loop, in plain English

For each frame in each block:

1. **Read the bitstream** — pull the packed bits out of the byte stream, in the order the encoder wrote them.
2. **Dequantize the reflection coefficients** — these describe the shape of the synthesis filter for this frame (what your vocal tract "looks like" right now).
3. **For each subframe** (a DS2 QP frame has 4 subframes of 4 ms each):
   - Decode the **pitch lag** — how often the vocal cords are vibrating.
   - Look up the **adaptive excitation** in the pitch memory (the recently-decoded signal).
   - Look up the **fixed excitation** in the codebook (a small dictionary of pre-defined excitation patterns).
   - Combine them: `excitation = pitch_gain × adaptive + code_gain × fixed`.
   - Run the excitation through the **synthesis filter** (the reverse of the analysis filter the encoder used).
   - Update the pitch memory with what you just produced.
4. **Post-process** — noise modulation, de-emphasis, optional resampling to a standard rate.

That's it. ~4 400 lines of Rust to do this for three codec variants, with proper error handling, AES decryption for protected files, and a streaming API.

## 7. The AES layer (the bonus complication)

DS2 supports password-protected files. Olympus calls this "secure" recording — used in medical and legal contexts where the dictation must not be readable by anyone without the password.

The encrypted file looks identical from outside (a `.ds2` file) but starts with the magic `\x03enc` instead of `\x03ds2`. Inside, the header still has metadata in clear, but starting at offset `0x146` there's a 22-byte **encryption descriptor**, and the audio body (from offset `0x600` onward, ~0x1f0 bytes per block) is encrypted with **AES-128 or AES-256**.

Reverse-engineering this layer was a separate puzzle from the codec itself. AES is a well-known public standard — the unknown was **where in the file** the key/IV/salt sat, and **how the user's password was transformed into the AES key**.

The `crypto/ds2_encrypted.rs` file in the upstream repo is **772 lines** — the single largest file in the codec source. That size is the tell: this layer was the most painful to figure out. There's no algorithmic mystery (AES is standard), but every implementation detail had to be matched exactly to the Olympus convention.

Once decrypted, the audio body looks identical to a plain DS2 — you re-attach the unmodified header, swap the magic back to `\x03ds2`, and the regular decoder works on it.

## 8. Is this legal?

Yes. Two layers to the answer:

### EU: explicit allowance

EU Directive 2009/24/EC, Article 6, explicitly authorizes **decompilation for interoperability purposes**. You can disassemble proprietary code to understand its file formats and protocols, in order to write software that works with them. Olympus has no legal grounds to object to a clean-room decoder.

### US: still fine, under DMCA §1201(f)

The DMCA carves out an exemption for "reverse engineering for interoperability" — same idea. As long as no Olympus code is redistributed (and Hirpara's repo redistributes none — only the spec + a from-scratch Rust implementation), it's protected.

### What matters in practice

The upstream codec is **MIT licensed**. The spec is published. Olympus has been aware of the project (it's been on GitHub since February) and has taken no action — because there's nothing to take action on. The work is, legally and ethically, in the clear.

## 9. Sources

- Upstream codec: <https://github.com/hirparak/dss-codec> (MIT, Kieran Hirpara, Feb 2026)
- WASM build: <https://github.com/gaspardpetit/dss-codec-wasm> (MIT, Gaspard Petit)
- Full codec spec: [`CODEC_SPECIFICATION.md`](https://github.com/hirparak/dss-codec/blob/main/dss-codec/CODEC_SPECIFICATION.md) in the upstream repo
- Ghidra: <https://ghidra-sre.org/>
- FFmpeg ticket #6091 (the nine-year wait): <https://trac.ffmpeg.org/ticket/6091>
- CELP background: ITU-T G.729 specification, freely available
- EU reverse-engineering law: [Directive 2009/24/EC, Article 6](https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=CELEX%3A32009L0024)

---

Next chapter: **[02 — The integration](02-integration.md)**. How we wrapped this Rust codec into a production transcription pipeline that processes real-world dictations daily, with three entry points (CLI, cron, web), and what we learned in the process.

---

*Reading the binary so you don't have to. 🔍*
