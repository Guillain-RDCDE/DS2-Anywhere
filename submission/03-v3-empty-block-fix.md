# Submission follow-up — v3 must add empty-block handling to the QP demuxer

**Status:** v2 (sent 2026-05-26, message-id `<20260526151029.449720-1-guillain@poulpe.us>`) is
bit-exact against the Rust reference and clean on continuous recordings, but a production bug
surfaced afterward that affects **paused / voice-activated DS2 QP recordings**. Full story and
proof: [`docs/06-the-empty-block-bug.md`](../docs/06-the-empty-block-bug.md).

## What's wrong in v2

The QP path treats the block stream as a plain concatenation of 506-byte payloads. Real
dictation contains **empty blocks** (`frame_count == 0`) at every pause — pause/segment
markers whose payload is only a few continuation bytes plus padding. Concatenating their full
payload desyncs the frame stream from the first pause onward (garbage for the rest of the
file). This is the *same* defect the codec spec explicitly attributes to the existing
`libavformat/dss.c` ("does NOT handle empty blocks — produces corrupt output"); it was simply
never carried into the DS2 QP path because the QP path was validated only on gap-free files.

It is almost certainly also the root of the "distorted, duration doubled" symptom in the
original FFmpeg trac **#6091**.

## The v3 fix

In the DS2 demuxer, for a `frame_count == 0` block, emit only the continuation bytes and
discard the rest of the payload:

```
cont_size = 2 * header[1] + 2 * swap - 6      // swap == 0 for QP
```

Keep `cont_size` bytes from the payload, drop the remaining ~500. Blocks with a non-zero
frame count are unchanged, so continuous recordings are byte-identical to v2 (no regression).
Verified bit-exact against the licensed Olympus decoder across an 18-minute paused recording
(see docs/06 for the SNR table).

## What v3 still needs before sending

1. **A FATE sample that exercises a pause.** The current `fate/sample-qp.ds2` is a continuous
   read-through (no empty blocks) — it would pass with *or* without the fix, so it can't guard
   this regression. v3 needs a short, permissively-licensed recording with at least one
   deliberate pause, plus its regenerated `framecrc` reference.
2. **The over-count edge** (`frame_count` larger than a block can hold, seen once at a 28-block
   group boundary) is **not** covered by this fix and is not in any reference. It is rare; until
   it's reverse-engineered it should be detected and routed to a fallback rather than mis-decoded.
   Not a blocker for v3, but worth a comment in the demuxer.

A heads-up flagging all of this was sent to the ffmpeg-devel thread as a reply to v2.
