# dss-codec

Native Rust decoder for Olympus DSS and DS2 proprietary dictation audio formats. Produces WAV output with configurable sample rate, bit depth, and channel count, and can also normalize encrypted DS2 files back to plain container bytes.

Replaces the Wine + Olympus DirectShow + NCH Switch pipeline with a standalone binary — no runtime dependencies, ~140x faster than the Python reference decoders.

## Supported Formats

| Format | Extension | Sample Rate | Quality | Bit Rate |
|--------|-----------|-------------|---------|----------|
| DSS / DS2 SP | `.dss`, `.ds2` (format 0) | 11000 Hz | Standard | ~13.7 kbps |
| DSS / DS2 SP, variable rate | `.dss`, `.ds2` (format 1) | 11000 Hz | Standard | variable |
| DSS / DS2 LP (G.723.1) | `.dss`, `.ds2` (format 2) | 8000 Hz | Long play | 6.3 kbps |
| DS2 QP | `.ds2` (format 6) | 16000 Hz | Quality | ~28 kbps |
| DS2 QP7 | `.ds2` (format 7) | 16000 Hz | Quality, noise frames | variable |
| Grundig DSS SP | `.dss` (byte 0 = 6) | 16000 Hz | Grundig CELP | ~13.7 kbps |

A DS2 file in SP, variable rate SP or LP carries exactly the audio blocks of a DSS
file; only the header is bigger, so both go through the same block walk.

Format is auto-detected from the file header.

## Installation

```bash
cd dss-codec
cargo build --release
# Binary at target/release/dss-decode
```

## CLI Usage

```bash
# Basic conversion (outputs .wav next to input)
dss-decode recording.DS2

# Specify output file
dss-decode -O output.wav recording.DS2

# Batch convert a directory
dss-decode -o /output/dir *.DS2 *.DSS

# Custom sample rate and bit depth
dss-decode -r 16000 -b 24 recording.DSS

# Stereo output (duplicated mono)
dss-decode -c 2 recording.DS2

# File info only
dss-decode --info recording.DS2

# Decrypt an encrypted DS2 file to a plain .ds2 container
dss-decode --decrypt --password 1234 recording.DS2
```

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `-O, --output-file` | Output file path (single input) | `<input>.wav` |
| `-f, --format` | Output format | `wav` |
| `-r, --rate` | Output sample rate in Hz | Native rate |
| `-b, --bits` | Bit depth: 16, 24, or 32 | 16 |
| `-c, --channels` | 1 (mono) or 2 (stereo) | 1 |
| `-o, --output-dir` | Output directory for batch mode | Same as input |
| `-q, --quiet` | Suppress progress output | Off |
| `--info` | Print file metadata and exit | Off |
| `--decrypt` | Save decrypted/plain container bytes instead of WAV | Off |
| `--password` | Password for encrypted DS2 input | Unset |

## Library Usage

```rust
use dss_codec::{
    decode_file,
    decode_file_with_password,
    decode_to_buffer,
    decode_to_buffer_with_password,
    decode_and_write,
    decrypt_file,
    decrypt_to_bytes,
    AudioBuffer,
};
use dss_codec::output::OutputConfig;
use std::path::Path;

// Decode to in-memory buffer
let buf: AudioBuffer = decode_file(Path::new("recording.ds2"))?;
println!("Format: {:?}, {} samples at {} Hz", buf.format, buf.samples.len(), buf.native_rate);

// Decode from raw bytes
let data = std::fs::read("recording.dss")?;
let buf = decode_to_buffer(&data)?;

// Decode encrypted DS2 with a password
let encrypted = std::fs::read("encrypted.ds2")?;
let buf = decode_to_buffer_with_password(&encrypted, Some(b"1234"))?;

// Normalize to plain container bytes (plain input passes through unchanged)
let plain_ds2 = decrypt_file(Path::new("encrypted.ds2"), Some(b"1234"))?;
let plain_bytes = decrypt_to_bytes(&encrypted, Some(b"1234"))?;

// Decode and write WAV with custom settings
let config = OutputConfig {
    sample_rate: Some(16000),  // Resample to 16 kHz
    bit_depth: 16,
    channels: 1,
};
decode_and_write(
    Path::new("input.ds2"),
    Path::new("output.wav"),
    &config,
)?;
```

### `AudioBuffer`

```rust
pub struct AudioBuffer {
    pub samples: Vec<f64>,     // Mono samples (f64 internally)
    pub native_rate: u32,      // Native sample rate before resampling
    pub format: AudioFormat,   // Detected format (DssSp, Ds2Sp, Ds2Qp)
}
```

## Architecture

```
Input File
    |
    v
[Demuxer] ─── detect_format() → AudioFormat
    |
    ├── DSS / DS2 SP, format 1, LP: DSS block walk (port of FFmpeg's dss.c)
    │     → 42-byte SP packets, or G.723.1 frames
    └── DS2 QP / QP7: continuous bitstream concatenation
    |
    v
[Decoder] ─── per-format CELP decoder
    |
    ├── SP: Q15 integer, port of FFmpeg's dss_sp.c (format 1 included)
    ├── LP: G.723.1, port of FFmpeg's g723_1 decoder
    └── DS2 QP: f64, lattice synthesis, C(64,11) codebook, de-emphasis
    |
    v
AudioBuffer (f64 samples at native rate)
    |
    v
[Output] ─── optional resample (rubato) → WAV (hound)
```

All three decoders implement CELP (Code-Excited Linear Prediction) with:
- Reflection coefficient codebooks for spectral envelope
- Pitch-adaptive excitation (long-term prediction)
- Combinatorial fixed codebook excitation (short-term)
- Synthesis filter (lattice for DS2, LPC polynomial for DSS)

## Codec Details

Full technical specification including all algorithms, tables, bit allocations, encrypted DS2 notes, and DLL function addresses: see [CODEC_SPECIFICATION.md](CODEC_SPECIFICATION.md).

### Key Parameters

| Parameter | SP (DSS and DS2) | DS2 QP |
|-----------|--------|--------|
| Arithmetic | Q15 integer | f64 float |
| Reflection coefficients | 14 | 16 |
| Subframes per frame | 4 | 4 |
| Subframe size | 72 | 64 |
| Excitation pulses | 7 | 11 |
| Codebook | C(72,7) = 31 bits | C(64,11) = 40 bits |
| Pitch encoding | Combined 24-bit | Per-subframe 8-bit |
| Pitch range | 36-186 | 45-300 |
| Frame bits | 328 (72 for a format 1 noise frame) | 448 |
| Post-processing | Noise modulation + 11:12 sinc decimation | De-emphasis (a=0.1) |

## Verification

SP, variable rate SP, LP and QP give the same samples as Patrick Domack's FFmpeg fork
(release n9.0.2b) on its public test vectors; `tests/fate_vectors.rs` pins them.
Earlier verification of the original crate, against the Python reference decoders and
Olympus DirectShow reference WAVs:

| Format | vs Python | vs DirectShow |
|--------|-----------|---------------|
| DSS SP | 1.0000 (bit-exact) | 0.99999 |
| DS2 SP | 1.0000 (+-1 rounding) | 0.995 |
| DS2 QP | 1.0000 (bit-exact) | 0.99999997 |

## Why Not FFmpeg?

FFmpeg's built-in `dss_sp` decoder does **not** work for DS2 files. The Olympus codec uses completely different parameters, tables, and sample rates than what FFmpeg implements. Correlation between FFmpeg output and correct audio is ~0.01 (effectively random). See [FFmpeg Trac #6091](https://trac.ffmpeg.org/ticket/6091). Patrick Domack's [FFmpeg fork](https://github.com/patrickdk77/FFmpeg/releases) (n9.0.2b) reads every one of these formats; this crate gives the same samples.

## Dependencies

- [clap](https://crates.io/crates/clap) 4 — CLI argument parsing
- [thiserror](https://crates.io/crates/thiserror) 2 — Error types
- [hound](https://crates.io/crates/hound) 3 — WAV file I/O
- [rubato](https://crates.io/crates/rubato) 0.16 — Sample rate conversion

## License

MIT — see [LICENSE](LICENSE).
