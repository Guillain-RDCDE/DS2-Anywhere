# FATE sample plan

> How we'll produce a publicly-redistributable DS2/DSS sample for FFmpeg's FATE regression test suite.

## The constraint

FFmpeg's FATE test infrastructure needs:

1. A sample file in the format we're adding (one `.ds2` for DS2 QP, ideally one `.dss` for DSS SP too).
2. Reference output (PCM checksums) that the decoder must reproduce bit-for-bit.
3. The sample must be **freely redistributable** — FFmpeg can't ship anything under restrictive copyright.

We don't ship existing samples in our own repo (see [`examples/README.md`](../examples/README.md) for why), but for FATE we *have* to produce one.

## The plan, in order of preference

### Plan A — record a public-domain text on a borrowed Olympus device

- Pick a short, recognizable, unmistakably public-domain text: ~30 seconds of the [Declaration of the Rights of Man and of the Citizen of 1789](https://en.wikipedia.org/wiki/Declaration_of_the_Rights_of_Man_and_of_the_Citizen) (or any equivalent in the language of choice — Lincoln's Gettysburg Address, the preamble of the U.S. Constitution, a Wikipedia paragraph on a neutral subject).
- Borrow an Olympus DS-series, DM-series, or DPM-series recorder (a colleague's, a hardware library, a thrift store one). Record in DS2 QP mode (16 kHz, the dominant format we want covered).
- Optionally record the same text in DS2 SP and DSS SP modes for broader codec coverage.
- License: explicitly **CC0 / public-domain dedication** in the file's accompanying metadata.

The text doesn't need to be artistic. It just needs to be:

- Long enough to exercise the codec (~20-60 seconds, so several frames at each subframe boundary).
- Calm reading voice (not silence, not music, not screaming — typical dictaphone usage).
- Indisputably non-copyrighted.

### Plan B — synthesize from a known public-domain WAV

If Plan A is logistically annoying:

- Take a known public-domain WAV recording (e.g. a clip from [Librivox](https://librivox.org/), CC0).
- Convert it to DS2 QP using the **proprietary Olympus encoder** or Switch.exe (which can encode, not just decode).
- This gives us an authentic DS2 file containing CC0 audio content.

Concern: the *encoder* output is still Olympus's; the audio *content* inside is CC0. Whether that's "freely redistributable" enough for FATE is a question to confirm with FFmpeg maintainers.

### Plan C — synthesize from scratch

If both above fail:

- Generate a synthetic test signal (sine sweep, white noise, dual-tone) in WAV.
- Encode to DS2 via the proprietary tool.
- 100% no copyright issue on the content.

Less useful as a real-world test, but bulletproof on licensing.

## Reference output generation

Once we have a sample DS2:

```bash
# Use Kieran's reference Rust decoder to produce the canonical PCM
dss-decode-native -O sample.wav sample.ds2

# Convert to raw PCM s16le mono for FATE checksum
ffmpeg -i sample.wav -f s16le -ac 1 sample.pcm

# Compute the FATE-style checksum (md5 over the raw PCM)
md5sum sample.pcm
```

These checksums get embedded in `tests/fate/audio.mak`:

```makefile
FATE_AUDIO-$(call DEMDEC, DS2, DS2) += fate-ds2-qp
fate-ds2-qp: CMD = framecrc -i $(TARGET_SAMPLES)/ds2/sample-qp.ds2

FATE_AUDIO-$(call DEMDEC, DS2, DS2) += fate-ds2-sp
fate-ds2-sp: CMD = framecrc -i $(TARGET_SAMPLES)/ds2/sample-sp.ds2

FATE_AUDIO-$(call DEMDEC, DS2, DS2) += fate-dss-sp
fate-dss-sp: CMD = framecrc -i $(TARGET_SAMPLES)/dss/sample.dss
```

## Sample submission

Samples don't ship in the FFmpeg source tree itself — they live in `samples.ffmpeg.org`, fetched separately by people running FATE. The actual submission process for a new sample:

1. Email `ffmpeg-devel@` with the patchset.
2. Coordinate with an FFmpeg admin to upload the sample to `samples.ffmpeg.org/ds2/`.
3. FATE tests reference `$(TARGET_SAMPLES)/ds2/sample.ds2`.

Worth checking the current process in the FFmpeg developer docs before sending the patch.
