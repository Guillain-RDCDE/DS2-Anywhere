# FATE sample — what we ship and why

> One DS2 QP sample, sourced from a public vendor test page, validated
> byte-near-perfect against the spec author's reference Rust decoder.
> SP coverage and DSS-SP coverage are out of scope for this series (see
> bottom of file).

## The sample

**`Sample_DS2_Audio_File_-_No_Encryption.ds2`**, hosted on Shopify CDN by
[dictate.com.au][src] as part of a public download page intended for
transcription-software vendors to test their DSS Pro support.

[src]: https://dictate.com.au/blogs/news/download-ds2-audio-file-samples-dss-pro

| Property | Value |
|---|---|
| URL | `https://cdn.shopify.com/s/files/1/0075/3642/files/Sample_DS2_Audio_File_-_No_Encryption.ds2` |
| Size | 132 608 bytes (129 KiB) |
| MD5 | `23eab82c3fc093c44ef4eb45ac35ba20` |
| Magic | `\x03ds2` (unencrypted DS2) |
| Mode | QP — 16 000 Hz, mono, CELP, 16 reflection coefficients |
| Duration | 37.01 s |
| Author metadata | `DICTATE` (vendor test content) |
| Date metadata | `2018-06-13T11:20:29` |
| Frames decoded | 2313 |

A copy is included in this folder as [`fate/sample-qp.ds2`](fate/sample-qp.ds2)
and the corresponding reference output as
[`fate/fate-ds2-qp.ref`](fate/fate-ds2-qp.ref) (framecrc, 2319 lines =
6 header lines + 2313 frame entries).

## Why this sample

1. **It's a real DS2** — not a re-muxed DSS file, not a synthetic test
   signal. Magic byte `\x03ds2`, QP format byte `0x06` at offset
   `0x604`, parsed cleanly by both Hirpara's Rust reference and
   Patrick's C port.
2. **QP is the format we want covered first.** In the in-the-wild
   corpus we have access to (see [`../benchmarks/conversion-results.json`][bench]),
   QP dominates DS2 traffic; SP is a long-tail mode. A FATE rule for
   QP covers the high-leverage case.
3. **Public, persistent URL.** dictate.com.au has hosted this file on
   a stable Shopify CDN since at least 2018. The bus factor on the
   original host is low.
4. **No content concerns.** Author metadata says `DICTATE`, no client
   information, no identifiable speech subject — clearly a sample the
   vendor produced specifically as a public artefact for codec
   testing.

[bench]: ../benchmarks/conversion-results.json

## Licensing position

The host page describes the file as "created to demonstrate the DSS
Pro `.ds2` audio file format for testing with your favourite
transcription software". There is no explicit CC0 dedication, but the
file is hosted on a public CDN, with no restrictive notice attached,
and its stated purpose is precisely the use we make of it: regression
testing of a transcription decoder. We submit it to
`samples.ffmpeg.org` under that read of intent.

If FFmpeg maintainers prefer an explicit CC0 grant on file, we will
obtain one from dictate.com.au before the sample is uploaded — flag
it during review and we will handle it pre-upload.

## Validation: C decoder vs Rust reference

This is the comparison the FATE checksum will exercise. The C decoder
(Patrick Domack's patch) and the Rust decoder (Kieran Hirpara's
reference for the published specification) are two independent ports
of the same spec. Both decode the FATE sample successfully; the delta
between them is rounding noise.

**Dataset: FATE sample (`fate/sample-qp.ds2`), 37.0 s @ 16 kHz**

```
PCM samples compared:  591360
  exact match:           49.59 %
  diff = +/-1 LSB:       50.39 %  (float-to-int16 ordering noise)
  diff = +/-2 LSB:        0.02 %
  diff >= 3:              1 sample
  max abs diff:           3
RMS error:                0.71
RMS signal:               1484
SNR:                      66.4 dB
```

The +/-1 LSB delta is inaudible by construction: each differing sample
is off by at most one int16 quantization step. This is the expected
shape of disagreement between two floating-point CELP implementations
that follow the same specification — neither is "more correct" than
the other; both are within rounding noise of the mathematical ideal.

## Other validation, for context (not exercised by FATE)

The C decoder has also been run on a second, larger dataset that does
not ship with this patch but is documented for reviewers who want
broader evidence:

**Dataset: 31-minute real-world DS2 QP recording**

```
Whisper transcription, run with deterministic config (greedy decoding,
temperature=0, fixed seed), produces transcripts that are textually
equivalent between the C-decoded MP3 and the MP3 produced by the
proprietary NCH Switch Windows decoder fed the same input:
  Switch.exe: 16.2% low-confidence words.
  C decoder:  17.2% low-confidence words.
The 1-point difference sits inside Whisper's run-to-run variance on
this length of audio; the two chains are interchangeable for any
downstream STT consumer.

SNR vs the Rust reference, same metric as the FATE-sample table
above (20*log10(RMS_signal / RMS_error) on s16le PCM):  ~67 dB.
RMS error 0.78, RMS signal 1577.
```

**Dataset: 35-file in-the-wild DS2/DSS corpus** (production dictation
pipeline)

```
Decode success: 35/35.
Mean conversion ratio: 1.5x real time on a single core (Rust path).
Full per-file results: ../benchmarks/conversion-results.json.
Note: this dataset measures decode success and timing, not byte-for-byte
diff vs Rust. The byte-for-byte argument is the FATE-sample analysis
above.
```

These two extra datasets are upstream-of-the-mailing-list evidence,
visible to reviewers who follow the project link in the cover letter.
FATE itself exercises only the QP sample and the per-frame CRC32 the
C decoder produces.

## Reference checksums

```
sample.ds2  md5  = 23eab82c3fc093c44ef4eb45ac35ba20
sample.ds2  size = 132608 bytes
output PCM  md5  = 2f46c9c35c606cbcf2bbd8bb55d366a9   (Patrick C decoder)
                   352d08d75cb44c1fba30f7e01c1e3730   (Hirpara Rust)
framecrc lines   = 2319 (6 header + 2313 frame entries)
```

The two PCM md5s differ because of the +/-1 LSB rounding noise
documented above. FATE compares the per-frame CRC32 against the C
decoder's reference (`fate-ds2-qp.ref`) — those CRCs are stable across
runs on any platform since the C decoder is deterministic.

## FATE wiring

To go into `tests/fate/audio.mak`:

```makefile
FATE_AUDIO-$(call DEMDEC, DS2, DS2) += fate-ds2-qp
fate-ds2-qp: CMD = framecrc -i $(TARGET_SAMPLES)/ds2/sample-qp.ds2
```

Reference output (`tests/ref/fate/ds2-qp`) is the file
[`fate-ds2-qp.ref`](fate/fate-ds2-qp.ref) in this submission folder.

## Scope: what's NOT in this patch

- **DS2 SP (12 kHz).** The decoder supports it (the C code has the
  full SP path with 14 reflection coefficients), but we don't ship a
  SP FATE sample yet. The public test files we located are all QP. A
  follow-up patch will add SP coverage when a SP sample is sourced.

- **DSS SP (11.025 kHz).** Already supported in FFmpeg by Oleksij
  Rempel's `libavcodec/dss_sp.c` + `libavformat/dss.c` (2014). This
  patch does not touch that code and does not duplicate it. DS2 and
  DSS are sibling formats with different codebooks; the DS2 decoder
  refuses DSS files at the demuxer probe.

- **G.723.1 LP mode in DSS containers.** Same as above: existing
  `libavformat/dss.c` already routes those files to FFmpeg's existing
  G.723.1 decoder. No new code needed.

## Sample upload procedure

Samples don't live in the FFmpeg source tree — they go to
`samples.ffmpeg.org`, fetched separately by people running FATE.
Procedure (to confirm against the current FFmpeg dev docs before
sending):

1. Send the patchset to `ffmpeg-devel@`.
2. Once reviewers ack the patch, request an upload slot from a
   `samples.ffmpeg.org` admin (typically posted on the same list).
3. Provide them [`fate/sample-qp.ds2`](fate/sample-qp.ds2) for upload
   to `samples.ffmpeg.org/A-codecs/DS2/sample-qp.ds2`.
4. The FATE rule above then resolves against
   `$(TARGET_SAMPLES)/ds2/sample-qp.ds2`.
