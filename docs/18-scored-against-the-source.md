# 18 — Scored against the source

*Or: what happened when someone measured our decoders against the speaker instead of against each other.*

---

[Chapter 17](17-the-framing-was-wrong.md) ended with four patches on `ffmpeg-devel`: the DS2 decoder and demuxer, the QP re-sync fix, and a two-patch series that took DSS SP framing from the block headers and set the rate to 11000 Hz. They drew no review. After a few weeks of silence we stopped following the list.

Two months later, an issue landed on this repository from Patrick Domack — the same Patrick who wrote the C port of the DS2 decoder in the first place ([CREDITS](../CREDITS.md)). He had rebuilt his own FFmpeg fork's DSS/DS2 support on top of our four patches, and then done the one thing nobody in this story had done yet.

## The measurement nobody had made

Every comparison in this repository so far had been decoder against decoder: our output against the Olympus DLL running under a debugger, against NCH Switch, against FFmpeg, against the previous build of ourselves. Those tell you whether two decoders agree. They cannot tell you which one is closer to the person who spoke, because nobody had the person.

Patrick made the person. He took clean speech — nine speakers from a public speech-quality corpus, twenty seconds each — encoded it with **Olympus's own encoder** (ODMS R8, `DssEncoder.dll`), and then decoded the result with every decoder he could find. Then he scored each decode with **PESQ against the original audio**: the ITU's perceptual speech-quality measure, the one telephone networks are tuned with.

| Decoder | PESQ wideband | PESQ narrowband |
|---|--:|--:|
| FFmpeg `dss_sp` (Rempel, 2014, with our framing) | 2.796 | 3.701 |
| NCH Switch (Olympus 2007 `dss32.dll`) | 2.796 | 3.701 |
| Olympus R8 (`DssDecoder.dll`) | 2.784 | 3.617 |
| **Our lattice synthesis** | **2.677** | **3.531** |

Read it slowly. The synthesis Oleksij Rempel wrote for FFmpeg in 2014 ties Olympus's own 2007 decoder on every speaker. It *beats* Olympus's current decoder. And the lattice we rebuilt it into — the fix [the Lattice Hunt](THE-LATTICE-HUNT.md) disassembled 2,500 bytes of x87 code to justify — is the worst of the four.

The lattice's "0.93 correlation with the DLL oracle" was true. It measured how close we were to R8. R8 is not the decoder closest to the speaker. We had optimised, with great care, toward the wrong reference.

This is the lesson of chapter 17 one level up. There, the trap was *a stabilising fix that damps a symptom is not evidence you found the cause*. Here it is: **agreement with a reference is not quality, unless the reference is the source.** When the source can be manufactured — and for a codec whose encoder you can run, it can — manufacture it.

## Three bugs in our patches

Patrick's fork carries thirty commits on top of our series, and each commit message carries its own measurements. Three of them fix bugs in what we posted:

- **An ABI break.** Our v3 patch inserted `AV_CODEC_ID_DS2` *before* `AV_CODEC_ID_APPLE_APAC` in the codec ID list. That moves APAC from its n9.0 value 86125 to 86126, and every program compiled against n9.0 that uses APAC would silently get the wrong codec. New IDs go at the end. Any FFmpeg reviewer would have caught it in a minute; none looked.
- **No licence header** on `libavcodec/ds2.c` and `libavformat/ds2.c`, so FFmpeg's own `fate-source` check fails.
- **`ret < sizeof(hdr)`.** `sizeof` is unsigned, so the comparison is unsigned, so a negative `AVERROR_EOF` becomes an enormous number and *passes*. The demuxer then carried on with an uninitialised block header. On one test file the output changed from build to build. This is the C pitfall every C programmer knows and every C programmer writes anyway.

## What our own decoder was getting wrong

Patrick's issue was about the FFmpeg patches, but most of what he fixed lives in the format, not in FFmpeg. So we did what [chapter 15](15-the-relay-runs-backward.md) taught us: before believing anything, we built his fork, ran it, and bench-marked our production decoder against it — on his public test vectors (the Olympus encodes, whose source audio he publishes), and on every real DSS and DS2 dictation on our production server, 272 files.

What the bench showed about **our** decoder, the one in production since May:

- **DS2 files in SP mode went down a path of their own**, decoded at 12000 Hz by a separate floating-point synthesis. On format 0 that path was merely worse than the DSS one (PESQ 3.21 wideband against 3.30). On **variable rate SP (format 1)** and on **DS2 LP** it produced pure noise: PESQ 1.04. The reason is almost embarrassing: a DS2 file in SP, variable rate SP or LP carries **exactly the same audio blocks as a DSS file**. Only the header is bigger. There was never a "DS2 SP codec" to write.
- **DSS LP and DSS format 1 were refused.** LP is G.723.1, a published ITU standard; we had told users to go and use FFmpeg for it. Format 1 is SP with a type bit per frame: long frames of 41 bytes, and 9-byte frames that carry only a filter and four noise gains for silence.
- **The frames of every pause block were thrown away.** When a recorder pauses or stops, it writes a block with 0 in the frame-count byte and the real count in byte 5. Every open demuxer, ours included, read the 0. Olympus's R8 plays those frames. On our production corpus, that is **up to 25 seconds of audio per dictation** — 1,039 frames over 164 pause blocks in one 24-minute recording. The recovered frames sit at speech level (about half the RMS of the surrounding audio, as you would expect around a pause) and clip *less* than the rest, which is not what misframed garbage looks like. We tried to go further and confirm it with speech recognition; a full-length Whisper transcript is too noisy a measure to isolate a few seconds, so we are not claiming more than the decoder behaviour and the levels show.
- **Our DSS SP synthesis was no longer FFmpeg's.** During the Q15 episode ([chapter 16](16-the-q15-instability.md)) the crate had picked up small changes — a clip inside the error-shaping loop, a sticky saturation, an alternative pulse-position decode — each written to fight a runaway that turned out to be misframing. None of them was ever measured against the source. On clean files they changed nothing, which is why they survived; on one paused recording, 15% of the samples differed from FFmpeg's.

## What we changed

The rule we set ourselves: our decoder must give **the same samples** as the FFmpeg fork Patrick measured, so that his measurements apply to it unchanged.

- The DSS block walk is now a port of the fork's `libavformat/dss.c`: SP, variable rate SP and LP, in DSS or DS2 files, pause-block frames included, the swap byte read from the file rather than carried, a `1` in byte 0 meaning a 3-block header (SpeechExec writes that in files it splits).
- The SP decoder is a straight port of FFmpeg's `dss_sp.c`, 32-bit wrapping arithmetic exactly where the C wraps, plus Patrick's format 1 tables.
- LP gets a port of FFmpeg's G.723.1 decoder, checked against it on 12 million samples, synthetic frames of every type included.
- QP and QP7 follow the fork's demuxer and decoder: pause-block frames counted the same way, a frame the file ends inside kept and zero-filled, no decoder reset at re-sync blocks, and QP7's last reflection coefficient read from its own 4-entry codebook (it had been borrowing the first four entries of another table, which was most of the QP7 error).
- The separate DS2 SP decoder is deleted.

The result, on Patrick's public vectors and our own samples:

| File | Before | After |
|---|---|---|
| `olympus-sp.dss` | identical | identical |
| `olympus-sp.ds2` | 12000 Hz, other synthesis | **identical** |
| `olympus-format1.dss` | refused | **identical** |
| `olympus-format1.ds2` | noise (PESQ 1.04) | **identical** |
| `olympus-lp.dss` | refused | **identical** |
| `olympus-lp.ds2` | noise (PESQ 1.04) | **identical** |
| `sp-paused.dss` | 2 frames short, 73% identical | **identical** |
| `sample-qp.ds2`, encrypted AES-128 / AES-256 | identical | identical |
| `paused-qp.ds2` | 10 frames too many, 6% identical | **identical** |
| `olympus-qp7.ds2` | 2% within 1 LSB | **identical** |

"Identical" means every sample, and [`tests/fate_vectors.rs`](../vendor/dss-codec/tests/fate_vectors.rs) pins it. The in-browser decoder is built from the same crate and CI checks that it gives the same samples as the native one.

On the 272 production recordings (252 QP, 19 DSS SP, one empty file every decoder refuses): **all 252 QP files are identical to the fork** — the decoder we had in production matched it on only 173 — and **all 16 DSS files the fork can open are identical to it**. The other three DSS files are the subject of the next section. Every one of the 271 real recordings decodes; the new decoder went into production the same evening, behind a kept backup.

## What we could send back

A review like Patrick's deserves more than thanks, so we looked for anything his fork still missed. The production bench found one thing:

**Three of our nineteen real DSS files leave the header's codec byte at `0xff`.** They come from recorders that also write unusual header sizes (3, 8 and 19 blocks). The audio blocks are ordinary SP and say so in their own format byte. The fork refuses them with "Not yet implemented in FFmpeg, patches welcome"; our previous decoder decoded them, and the new one does too, by falling back to the first block's format byte when the header's is not a known codec. That goes back to him with this chapter.

The bench also turned up one bug of our own that had nothing to do with the codec. Our production wrapper rewrote any `.dss` file whose first byte was between 4 and 16 to a 3-block Olympus header, to help a Grundig/Philips DS2 container variant through. A Grundig DSS file starts with **6**. It was being rewritten into an Olympus header and decoded as Olympus SP — into noise, reported as a success. The decoder itself had always recognised it; the wrapper in front of it undid that. The rewrite now applies to DS2 containers only.

## What this chapter is about

Strip the codec away and this is a story about references.

For five months every check in this repository was a comparison against another implementation, and the implementations were close enough to each other that the comparisons looked like proof. They were proof of agreement. Patrick's contribution was not a cleverer decoder; it was a better question — *closer to what?* — and the patience to build the one reference that answers it: the speaker, re-encoded by the vendor's own encoder, scored by the measure built for exactly this.

And the relay keeps running in both directions. Patrick wrote the first C port of DS2; we put it on the mailing list; the list was silent; he picked our patches back up, measured them, fixed them, and handed them back better. The decoder you run on this page is the result of that loop, and so is every sample in it that now matches.

*Next time something looks fixed, ask what it was compared against.*
