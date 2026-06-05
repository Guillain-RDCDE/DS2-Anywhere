# Submission follow-up — the QP demuxer also needs per-block byte1 re-anchoring

**Status:** complementary to [`03-v3-empty-block-fix.md`](03-v3-empty-block-fix.md). The empty-block
(`frame_count == 0`) handling is necessary but **not sufficient**. A second class of
segment-boundary block desyncs the QP frame stream, and it is not gated by `frame_count`.
Both fixes are the same underlying rule. Full story and proof:
[`docs/07-cracking-the-resync-block.md`](../docs/07-cracking-the-resync-block.md).

## The finding

The QP demuxer must **re-anchor every block's frames at payload offset `2 * byte1 - 6`**
(`byte1 = block_header[1]`, `swap == 0` in QP), not read the payload as a flat concatenation.

On gap-free audio the anchor always coincides with the running read position, so a continuous
read is accidentally correct — which is why every continuous-recording validation passed. At a
segment boundary the anchor jumps (we observed `+2` and `+48` byte jumps), the continuation
bytes at the block start are an orphaned straddle tail to be skipped, and a continuous reader
desyncs from there to end-of-file.

This was confirmed **byte-for-byte against the live Olympus `DssParser.dll`** (hooked at
runtime) and read directly from its decompiled block routine `FUN_10009910`. The empty-block
case is just the special instance where the anchor leaves zero fresh frames.

## The rule, exactly as the Olympus parser implements it

Per block, the parser carries the **straddle tail** `t` = bytes the previous block's last frame
spilled past the 512-byte block end. Then:

```
anchor = 2 * byte1 - 6                       # payload offset of this block's first fresh frame
if t == anchor:                              # aligned: continuous read is correct
    read frames continuously
else:                                        # re-sync block: drop stale partial + orphan tail
    discard the buffered partial frame
    start fresh frames at `anchor`
# either way: read up to `frame_count` frames from the anchor, but STOP at the block end
# (frame_count over-counts on resync blocks — e.g. 19 — so the block-end cap is the real limit;
#  the count field is meaningless except for the value 0)
```

`frame_count ∈ {0, 9, 10}` is **not** a reliable discriminator: we found ordinary
`frame_count = 9/10` blocks that still carry a non-trivial `byte1` and re-anchor. Any heuristic
on the count misses them; only the `t == anchor` test is correct.

## Impact on the C patch (`libavformat/ds2.c`)

The current `ds2_load_block` / `ds2_qp_read_packet` read the payload sequentially via a
`counter` of available bytes, starting at payload offset 0 for `frame_count != 0`. To implement
the rule:

- In `ds2_load_block`, for a non-empty block, compute `anchor = FFMAX(0, 2*hdr[1] - 6)` and the
  expected straddle tail (bytes still owed to the in-progress frame). If they disagree, the
  partial frame in flight must be dropped and reading must restart at `anchor` (skip
  `anchor` payload bytes; do not feed the pre-anchor bytes into the current frame).
- The empty-block path already does the right thing (`counter = cont_size`) — it is the
  `anchor`-leaves-no-fresh-frames special case.

This wants a FATE sample that contains a real re-sync block (a paused recording), and an A/B
against the Olympus reference across the boundary. We have not shipped a C diff for this because
it must be validated against FATE first; the rule above is exact and verified, and the
reference Rust implementation in this repo (`Ds2QpStreamDemuxer::process_block`) carries the
working version with an 18-file OLD-vs-NEW regression corpus behind it.
