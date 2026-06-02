# 07 — Cracking the re-sync block: how we ran the closed-source decoder against itself

> The previous chapter ended on a confession. One file had an anomaly the empty-block rule didn't fix — a block whose frame count was `19`, impossible for a 506-byte payload — and we punted: detect it, fall back to the Olympus reference, reverse-engineer it "another day." This is that day. It took running the real Olympus decoder inside a debugger we built ourselves, hooking it at the instruction level, and reading its disassembly — and it ended with a one-condition fix, a retracted theory, and a smaller, sharper black hole than the one we started with.

The empty-block bug (chapter 06) was a *missing rule we found written down*. This one was a rule **nobody had ever written down**, for either format, anywhere. So we had to get it from the only thing that knew it: the binary.

---

## Where we left off

The short recording — call it **File A** — diverged from the Olympus reference at exactly 52.6 s, the same razor-sharp cliff as the empty-block files. But its anomaly wasn't a `frame_count = 0` pause marker. It was the opposite: one block, number 364, claiming **19** frames. A 506-byte payload holds at most nine 56-byte frames. Nineteen is nonsense.

We had eliminated the codec already (chapter 06: resetting filter state changed nothing → the *decoded parameters* were wrong → bitstream desync → demuxer). So the bug was in how block 364 turned into frames. We just had no idea what the rule was. The spec was silent. `dss.c` was silent. Kieran's reference handled it by... reading straight through, same as us, and being wrong in the same way (it just never got tested on a file like this).

The only system on Earth that decoded File A correctly was the closed-source Olympus decoder. So the plan was blunt: **make the Olympus decoder tell us what it does.**

---

## Dead end #1 — hooking the GUI

The obvious move: attach a debugger to NCH Switch (which wraps the Olympus filters) and watch it parse File A. We tried frida against `Switch.exe`. Both spawn-and-attach and late-attach failed — the process is 32-bit, packed, and actively unfriendly to instrumentation. You can drive Switch from the *outside* (feed it a file, get a WAV) but you cannot see *inside* it.

That failure was the useful pivot. If we couldn't instrument *their* host process, we'd put *their* decoder inside *ours*.

---

## Building a debugger's decoder from COM parts

The Olympus DS2 codec ships as two DirectShow filters inside the free **Olympus Dictation Management System** (ODMS): `DssParser.dll` (the demuxer — the half we needed) and `DssDecoder.dll` (the CELP synthesis). We pulled both out of an ODMS install, registered them as 32-bit COM servers (`regsvr32`, landing under `WOW6432Node`), and wrote down their CLSIDs:

```
DssParser   {801AB45B-DE83-4EA6-8493-B6EA002FF1F9}
DssDecoder  {9F1642AE-1C3B-4400-9AA4-AD140A44E836}
```

Then, in a **32-bit Python** (you cannot load 32-bit COM DLLs into a 64-bit process), we built a DirectShow graph by hand with `comtypes`: instantiate `CLSID_FilterGraph`, `AddSourceFilter` on `fileA.ds2`, `AddFilter` the parser, the decoder, and a `NullRenderer`, then `Render` the source's output pin and `IMediaControl::Run`. `RenderFile` and `IMediaDet` had both refused the stream as "unsupported / video-only," which is why the graph is wired by hand pin-by-pin.

It ran. The real Olympus parser and decoder, executing in a process we owned, with no anti-debug, decoding the exact file we cared about. Now we could attach frida to *that*.

> **Method note.** When a vendor's application resists instrumentation, you rarely need the application — you need the *library* it loads. Re-host the library in a process you control and the entire surface opens up.

---

## Listening to the parser

frida 17 (note: `Module.findBaseAddress` is gone in v17 — it's `Process.findModuleByName(...).base` now) let us hook `DssParser.dll` at the frame-sizer routine and capture, for every frame the parser emitted, its bytes. We searched those bytes back into the file to recover each frame's **exact position** in the concatenated payload stream.

The numbers were unambiguous. Around the count=19 block:

```
frame  real position    where a continuous read would put it
3289   block 364 + 2    block 364 + 0      ← +2 byte jump
3298   block 365 + 48   block 365 + 0      ← +48 byte jump
```

Every block re-based its frames at a payload offset of **`2 × byte1 − 6`**, where `byte1` is that same header field the empty-block formula used. The continuation bytes at the start of each block — the tail of the frame straddling in from the previous block — were *skipped*, and fresh frames began at the anchor. On a gap-free recording the anchor always lands exactly where a continuous read already is, which is why this was invisible for a decade. At a segment boundary it jumps, and a continuous reader desyncs by a few bytes — forever.

We replayed our own demuxer with this rule. **Zero position divergences** against the real parser across the whole captured region. We had the demux law of the format, confirmed byte-for-byte against the silicon.

And the frame size question — was block 364 hiding "short" 96-bit frames, as we'd once guessed? No. The parser's own size table (`DAT_1002ce20`) gives format-6 frames a flat **448 bits**, always. The `19` is not a frame count at all; the parser's extraction loop reads frames from the anchor *until the block ends*, and `19` is just an upper bound it never reaches (~9 frames fit). The count field, outside of the special value `0`, doesn't mean what its name says — a conclusion Kieran reached independently from the other side.

---

## Dead end #2 — a theory I had to retract

So the demux was solved. We rebuilt the audio with the corrected frame positions and measured against Olympus:

```
0 – 52 s     +77 dB   bit-exact
53 – 65 s    ~0 dB    still wrong
65 – 75 s    +78 dB   bit-exact
```

The end of the file, previously garbage, was now **perfect**. But a band in the middle — a loud, sustained passage right after the pause — was still decorrelated, *even though we were now feeding the decoder the exact same frame bytes the real parser produces.* (We proved that last point the hard way: we decoded the literal bytes captured from the Olympus parser, not our reconstruction, and got the same wrong band. The demux was no longer the variable.)

The reflexive explanation: **fixed-point.** Kieran's QP synthesis runs in `f64`; the DLL, we assumed, must use integer arithmetic, and a loud near-unstable passage would amplify the divergence. Clean theory. Wrong.

We decompiled `DssDecoder.dll` and read the lattice synthesis routine (`FUN_10019d40`). It is **`double` throughout** — same precision as the open implementation. There is no fixed-point cliff to fall off. And when we hooked that routine during an actual DS2 decode, it **never fired** — meaning it's the *DSS/SP* synthesis path, and the DS2/QP path runs through a *different*, as-yet-unidentified synthesis function. The theory didn't just fail; it pointed at the wrong function entirely. Retracted, in writing, the way the house style demands.

---

## The disassembly settles the demux for good

Before shipping a demuxer change to production we wanted the rule from the *code*, not just from observed positions. `DssParser.dll`'s block routine, `FUN_10009910`, decompiled, says it plainly:

- The first frame of a block starts at offset `2 × byte1` (i.e. `2 × byte1 − 6` into the 506-byte payload).
- It carries a value between blocks — the number of bytes the last frame spilled past the block end (the **straddle tail**).
- At each block it checks: **does the carried straddle tail equal this block's anchor?** If yes, the stream is continuous — read on. If **no**, it *resets the carry to zero and restarts at the anchor*, dropping the stale partial frame.
- It then reads up to `count` frames from the anchor, but stops at the block end — so an over-count like `19` is silently capped by geometry.

That "carry == anchor, else resync" test is the whole secret. It's a demuxer that **re-anchors on every block** and only *appears* continuous because, on normal audio, the anchor and the running position coincide. Translate the straddle tail into our terms (we track the straddle *head*, which is `56 − tail`) and the parser's condition becomes exactly:

```
aligned  ⟺  (leftover == 0 && anchor == 0) || (leftover + anchor == 56)
```

---

## The fix — one condition, in the same demuxer

```rust
let anchor = (2 * block[1] as usize)
    .saturating_sub(HEADER_SIZE)
    .min(PAYLOAD_SIZE);
let leftover = self.stream_buf.len();
let aligned = (leftover == 0 && anchor == 0)
    || (leftover != 0 && leftover + anchor == FRAME_SIZE);

if aligned {
    // continuous — byte-for-byte the original behaviour
    self.stream_buf.extend_from_slice(&block[HEADER_SIZE..BLOCK_SIZE]);
    self.pending_frames += frame_count;
} else {
    // re-sync block: drop the stale partial + the orphan tail, restart at the
    // byte1 anchor, read up to `count` whole frames capped at the block end,
    // and keep any final straddle head for the next block.
    self.stream_buf.clear();
    let mut off = HEADER_SIZE + anchor;
    let mut emitted = 0;
    while emitted < frame_count && off < BLOCK_SIZE {
        if off + FRAME_SIZE <= BLOCK_SIZE {
            frames.push(block[off..off + FRAME_SIZE].to_vec());
            off += FRAME_SIZE; emitted += 1;
        } else {
            self.stream_buf.extend_from_slice(&block[off..BLOCK_SIZE]);
            break;
        }
    }
    self.pending_frames = if self.stream_buf.is_empty() { 0 } else { 1 };
}
```

The safety property is the same one that made the empty-block fix shippable: **on an aligned block the code is the original code.** Any file without a re-sync block decodes byte-for-byte as before.

---

## The proof, and a bonus we didn't expect

Four independent checks, because removing the Windows fallback meant the native path had to be *right*, not just *better*:

1. **The decompiled parser** (`FUN_10009910`) — the rule, from the source of truth.
2. **frida on File A** — the count=19 block re-anchors at offset 2. Confirmed.
3. **frida on a second file** — three consecutive blocks re-anchoring at offsets 0, 4, 2, each exactly `2 × byte1 − 6`. Confirmed.
4. **An 18-file corpus, OLD binary vs NEW** — byte-for-byte identical WAVs on every normal recording *and* every empty-block recording (up to 58 pauses in one file). Differences appeared **only** on files with a genuine re-sync block.

That fourth test surfaced the bonus. One of the "differs" files had **no** count anomaly at all — every block was 9 or 10 — yet it changed. Its segment boundaries were encoded with a non-trivial `byte1` on ordinary-count blocks, so the *old* decoder had been mis-reading it **silently**, and the count-based detector from chapter 06 would never have flagged it. The byte1 rule is strictly more correct than the count-anomaly heuristic it replaces: it fixes files we didn't even know were broken.

So the production system lost a moving part. The `frame_count ∉ {0,9,10}` detector and its "reconvert on the Windows box" alert — the last tether to the licensed decoder — were **deleted**. The native decoder now handles every structural case the real parser does.

---

## The black hole that's left (smaller, and better lit)

Honesty, per the house style. That middle band — the loud passage right after a pause — is still wrong, and it is **not** the demux: feed the decoder the real parser's exact frames and it still diverges there, while the rest of the file is bit-exact. It is a **decoder** discrepancy, isolated to the DS2/QP synthesis path, and we've ruled out the easy answer (it isn't fixed-point; the lattice is `double`). The QP synthesis routine is one of ~600 functions in `DssDecoder.dll` we have not yet mapped.

But notice how much the hole shrank. Chapter 06 left an entire *file class* falling back to Windows. This chapter hands the demuxer back to the native code completely and leaves a single, well-characterised symptom — *one synthesis function, one acoustic regime* — for a future session to hook the same way we hooked the parser. And in our production context it's a non-issue: the typists work from the original `.ds2` on their own Olympus players; the server MP3 is a convenience copy, not the deliverable. A contained, understood gap beats a silent one. (See **08 — the handoff** for the exact next steps.)

---

## What to take away

- **If you can't instrument the app, re-host the library.** The entire breakthrough came from running the vendor's own DLLs inside a process we controlled, after their GUI refused the debugger.
- **Observed behaviour gets you the hypothesis; the disassembly gets you the law.** frida told us *where* the frames were; `FUN_10009910` told us *why*, and only the "why" is safe to ship.
- **Retract loudly.** The fixed-point theory was clean, plausible, and wrong, and saying so in writing is what kept the next person from wasting a day on it.
- **A fix that's identical on the common path is a fix you can ship on a Friday.** Same lesson as the empty-block bug, earned twice.
- **Replacing a heuristic with the real rule fixes bugs you couldn't see.** The count detector was a good proxy; `byte1` is the actual law, and the law caught silent corruption the proxy missed.

---

Next: **[08 — The decoder black hole](08-the-decoder-black-hole.md)** — the one divergence left (a loud passage right after a pause), why it's a *codec* not a demux problem, and the exact warm-rig next steps to close it.

---

*Two files, one invented debugger, one retracted theory. The format's last secret was a number that wasn't a count, hiding in a byte everyone read and nobody used. 🔓*
