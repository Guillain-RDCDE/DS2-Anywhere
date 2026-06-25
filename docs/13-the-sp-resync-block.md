# 13 — The re-sync block strikes back, in SP this time

> Chapter 07 cracked the DS2 **re-sync block**: the demuxer re-anchors every block at the
> `byte1` offset, and a block whose carry doesn't line up with that anchor is a re-sync block —
> drop the stale partial, restart at the anchor. We confirmed it byte-for-byte against the real
> Olympus `DssParser.dll` and shipped it. What we never did was port that fix to the **DSS SP**
> demuxer. This chapter is the bill coming due — and, pleasingly, it's paid with the exact same
> coin: re-host the parser, hook it, read what it actually does, port the law.

## The field report

Ma Dactylo cmd `2954847` — `DPM_0266.DSS`, 16:10, a real voice-activated dictation. The typist
heard garble at **3m48**, the rest clean. Bytes intact, decode deterministic, source sound. NCH
Switch (which wraps the real Olympus codec) decoded the same file **cleanly** — so, exactly as
in chapters 06–07, the ground truth existed and our native decoder disagreed with it at one
razor-sharp point. We delivered the command the boring way (drop in Switch's MP3, re-run the
pipeline). This chapter is about the decoder.

## Localising it, and a couple of wrong turns

The garble starts right after a VOX pause — block index **773** (a run of three consecutive
`0f` header bytes). The empirical probing was instructive precisely because it kept being almost
right:

- changing the empty-block continuation size at 773 (every value 8…18) — **no effect**;
- skipping the swap-parity reset at that pause — **3m48 goes clean**, but five other regions
  break (1m58, 4m11, 5m04, 9m53, 13m00).

That second result is the tell. The old SP demuxer reaches frames through a
`stream_pos` + scheduled-reset + compaction path, and one parity change at one pause ends up
rippling across multiple, temporally-scattered regions (the buffer runs thousands of bytes ahead
of the audio, so reset-firing order ≠ time order). Per-pause patching cannot win. And the
byte-swap defeats curve-fitting: instrumenting the first ~25 gap-free blocks shows the clean QP
relation `leftover + anchor == FRAME_SIZE` does **not** hold for SP (the anchor that keeps the
stream continuous shifts by 2 with the running parity). Fitting a predicate to "make every
gap-free block aligned" is circular — it would also make a genuine re-sync block look aligned.

Same conclusion as chapter 07, reached the same way: **observed behaviour gets you the
hypothesis; only the live parser gets you the law.**

## Re-hosting the parser, headless, on Linux-driven Windows

Chapter 07 hooked the parser on Windows with frida on a 32-bit Python `comtypes` DirectShow
graph. We rebuilt that rig from scratch, driven entirely over SSH:

- **32-bit Python** (the embeddable zip — the MSI fought us in Session 0) + `comtypes` + a
  32-bit `frida` (cross-arch injection 64→32 fails — `ProcessNotRespondingError`; same-arch is
  the fix).
- The Olympus DLLs (`DssParser.dll`/`DssDecoder.dll`) re-registered, the file staged.
- A hand-wired graph in `comtypes`: `CreateObject(FilterGraph)`, define `IGraphBuilder` /
  `IBaseFilter` / `IPin` by vtable (no typelib for those), `AddSourceFilter`, `AddFilter` the
  parser + decoder + `NullRenderer`, `Render(source_out)`. Three traps re-learned the hard way:
  `Render` only succeeds with the renderer **pre-added**; `IMediaControl` is `IDispatch`-derived
  (its `Run` is at vtable slot 7, not 3 — a null-write access violation if you get it wrong);
  and the whole thing has to run in the **interactive desktop session** (COM is forbidden in
  Session 0 — `tscon` the disconnected session back to console, then `schtasks /IT`).
- `runner_frida.py` spawns the graph under frida and loads `hook.js`; the hook polls for
  `DssParser.dll`, attaches at **`base + 0x9890`** (the frame-sizer; **frame ptr in `args[0]`**,
  noted in chapter 08), and `send()`s 64 bytes per frame back to the runner.

It ran. **40 620 frames** captured from the real Olympus parser — and the parser streams the
**whole** file (the 76-second stalls we'd seen earlier were the *decoder* being loaded-but-never-
run behind a `NullRenderer`, not the parser).

## The law, byte-for-byte from the silicon

Each emitted frame's bytes search straight back into the raw payload (Olympus emits the raw
chunk; the swap is the decoder's business). Plotting positions across the file: a clean +41
stride, the expected +82 at each block boundary (the header-straddling frame), and **19 big
jumps**. Every one of those jumps lands on the block **immediately after an empty (`frame_count =
0`) block**, restarting at that block's anchor `2·byte1 − 6` (which is `0` there — `byte1 = 3`):

```
empty block 773  →  block 774 restarts at offset 0, discarding block 773's tail (Δ = 369)
```

That's it. That's the SP re-sync rule, and it is exactly chapter 07's DS2 rule wearing a
different hat: **the block after a pause re-syncs.** A model that does just that — read
continuously, and after an empty block restart the next block at its anchor — reproduces the
real parser's positions to **~99%** (the residual is the header-straddling frames, which by
construction aren't in a header-stripped payload). Law confirmed.

## The fix, and the same safety property as last time

One branch in `DssSpStreamDemuxer::process_block`: if the previous block was empty, drop the
buffer, reset the swap to this block's `blk_swap`, and restart extraction at `2·byte1 − 6`.

```rust
if self.prev_empty && frame_count != 0 {
    let anchor = (2 * byte1).saturating_sub(DSS_BLOCK_HEADER_SIZE).min(payload.len());
    self.stream_buf.clear();
    self.stream_pos = 0;
    self.stream_end_pos = 0;
    self.scheduled_resets.clear();
    self.pending_reset_positions.clear();
    self.swap = blk_swap;
    self.swap_byte = 0;
    self.stream_buf.extend_from_slice(&payload[anchor..]);
    self.stream_end_pos += payload.len() - anchor;
    self.pending_frames += frame_count;
    self.prev_empty = false;
    self.emit_available_frames(frames);
    return;
}
self.prev_empty = frame_count == 0;
```

The safety property is the one that made 06 and 07 shippable: **`prev_empty` is only ever set by
an empty block, so a file with no pauses never enters the branch.** Verified — old binary vs new
on cmd 2954847 is byte-for-byte identical up to the first empty block (0.60 s) and only diverges
after it; every DS2 file is untouched (different demuxer). Shipped to the production decoder
(`dss-decode-native`, backup kept, rollback is one `mv`). The typist confirmed the full file by
ear against the Olympus reference.

## Format references (Olympus patents — the only public documentation)

DSS is proprietary (International Voice Association: Olympus / Philips / Grundig). The container
structure used throughout this repo is corroborated by Olympus patents:

- **US 6,665,248 B1** — playback of voice data files: file = 0.5 MB blocks of **1000 sectors**
  (sector = **512 bytes**); each sector header **SH** carries **NF (frame count)** and **CI**
  (SP vs LP); playback time = **NF × 24 ms** (SP) / **× 30 ms** (LP), accumulated. Matches our
  23.949 ms/frame and the `frame_count`/`byte0`/`byte1` header layout.
- **US 5,218,640** — voice recording/reproducing apparatus (silence / VOX coding).
- **US 6,522,695** — transmitting a signal alternately encoded / non-encoded (the VOX "record
  only during voice" behaviour that produces the empty and re-sync blocks).

## What to take away

- **A bug that's "almost fixed" by five different one-off patches is a bug you don't understand
  yet.** The skip-the-parity-reset experiment cleaned 3m48 and broke five other spots — that
  ripple was the whole diagnosis: stop patching, go read the parser.
- **Re-host the library, don't fight the app.** The entire law came from running Olympus's own
  `DssParser.dll` in a process we controlled, headless, over SSH — no GUI, no Session 0.
- **The byte-swap is why you can't curve-fit SP.** Half your positions match at any fixed stride;
  the silicon is the only honest oracle.
- **Cross-arch frida injection fails silently — match the bitness.** 32-bit frida into 32-bit
  Python; that one line cost an hour.
- **Port the fix you already wrote.** This was chapter 07's re-sync rule, sitting on the wrong
  arm of a `match`. The format had no new secret — just an un-crossed border.

---

*One pause, one un-ported branch, one frida hook at `+0x9890`. The DSS SP demuxer now does what
the real parser does. 🔓*
