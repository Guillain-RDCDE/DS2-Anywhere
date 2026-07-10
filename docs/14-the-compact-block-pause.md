# 14 — The compact block: a pause we'd never seen, found by someone else

> DSS hides a pause three different ways, and this repo now has a chapter for each. Chapter 06:
> the **empty block** (`frame_count = 0`, continuation bytes only). Chapters 07 and 13: the
> **re-sync block** (the next block re-anchors at `2·byte1 − 6`, drop the stale carry). This one
> is the third — the **compact block** — and the only one we didn't find ourselves. It arrived
> as a pull request, from someone already in our credits.

## The pull request

Patrick Domack is [in this repo's CREDITS](../CREDITS.md) for the hand-written FFmpeg C port of
the DS2 decoder — the work that, once it lands upstream, makes `.ds2` a first-class FFmpeg format.
So when a PR from him showed up on the Rust crate we build from
([hirparak/dss-codec#14](https://github.com/hirparak/dss-codec/pull/14)), titled *"Fix DSS SP
decode: compact blocks, repack pulse path"*, it had our attention.

Two chapters ago we shipped a DSS SP decoder and thought the demux was closed. We had never heard
the term **compact block**. That's the uncomfortable, useful thing about a relay of strangers
picking one lock: someone hands you a piece of the mechanism you didn't know was still moving.

## What a compact block is

A DSS block is 512 bytes: a 6-byte header, then payload. Normally a block is *full* — its last
frame spills across the boundary into the next block, so the demuxer just concatenates payloads
and lets frames straddle. The header's `byte1` gives the carry offset (`poff = 2·byte1 − 6`) that
keeps the running stream continuous. This is the machinery chapters 07 and 13 are about.

A **compact block** breaks that pattern. It declares `fc` frames that fit *entirely* inside its
own payload — `fc·42 + poff ≤ 506` — and pads the rest of the 512 bytes with `0xFF`. Nothing
straddles. That padding is not audio; it's a full stop. The recorder wrote it because the dictation
paused and the block closed early. The block that follows starts a fresh segment, so *its* leading
carry offset is spurious and must be skipped.

Our decoder didn't know any of that. It read the `0xFF` padding as if it were frame bytes, fed it
to the CELP synthesis as a burst of nonsense, and — worse — every frame after the pause was now
mis-aligned, because the byte cursor had swallowed 200 bytes that weren't there. Same audible
symptom as the empty-block bug in chapter 06 (clean, then garble after a pause, then desynced to
the end), a completely different shape on disk. Our re-sync fix from chapter 13 never fired,
because a compact block isn't empty — it has a real `frame_count`.

Patrick's fix: at a compact block, take only the declared frames, skip the `0xFF` tail, and
restart framing on the next block (skipping its spurious carry). Preserve the swap-parity byte
across the realignment — only true empty blocks reset it, or the first `swap = 1` frame of the
resumed segment loses its high byte and pops. It's the same species of rule as chapters 07 and 13,
for a block type we'd simply never triaged.

## Does it even matter to us? — a census

Before touching a decoder that already serves production, we did the thing chapters 03 and 10
insist on: **measure, don't assume.** We pulled every DSS/DS2 recording that reached the pipeline
over 90 days — **327 files** — and ran each through the production decoder, classified by codec and
scanned for compact blocks.

- **88.7 %** were **DS2-QP** (16 kHz). Patrick's fix is DSS **SP**; it doesn't touch a single one
  of them.
- **8 %** were DSS SP. Within those, files with a genuine *mid-stream* compact pause came to
  **about four recordings a quarter**. Real, but the long tail of a long tail.
- The only **hard decode failures in the whole set — four of 327 — were 0-byte or
  truncated-at-upload files.** Dead on arrival. No decoder recovers those; they get flagged back to
  the sender.

The census settled the strategy more than the fix did. Our decoder already served ~99 % of real
traffic correctly. The volume lever, if we ever wanted one, is DS2-QP (ten times the files), not
this. And it put a number on something worth saying out loud: **bit-exactness against the Olympus
DLL was never our metric.** A dictation that a human typist can hear, complete and in order, is the
product. A sample that differs by a few LSB is not a defect; a noise burst that desyncs the back
half of a 20-minute *état des lieux* is. Compact blocks caused the second kind. That's why we took
the fix — not for four recordings, but because those four were genuinely broken.

## Shipping it without touching the 99 %

The one rule for changing a decoder that runs in production: **the files that work today must keep
producing the same bytes tomorrow.** So the fix went in gated. A file takes the block-aware batch
demuxer *only if* it actually contains a mid-stream compact block — detected conservatively:
declared frames that fit, a tail that is entirely `0xFF`, and a block after it (a *trailing*
compact block is just a short final block, a normal partial end, not a pause). Every other file —
all the QP, every SP recording with an empty-block or re-sync pause — keeps the streaming demuxer
and the chapter-13 re-sync untouched, byte for byte.

We proved it, not asserted it. On the 26 real DSS SP files from the census:

- **23 of 26 decoded byte-identical**, old binary vs new.
- The **3 that changed were exactly the mid-stream-compact ones** (one command — five back-to-back
  property-inspection dictations from a single bailiff's study, cmd `2946368`, whose blocks carried
  **202 and 168 bytes of pure `0xFF`**). Their **duration didn't change** — the fix doesn't add
  audio, it re-aligns the audio after the pause, which is the whole point.
- The three other census files the scan flagged as "compact" did **not** move: their compact block
  was the *last* one, a normal partial end, and the gate correctly left them alone.
- **25 of 25** sampled DS2-QP files: identical.

Then it went to the production binary the boring way — backup, atomic replace, a real end-to-end
conversion of a QP file (unchanged) and the five repaired SP files (clean MP3s out).

## The box we didn't get to open

Patrick's PR carried a **second** change we did *not* take, and the reason is a chapter-08 story in
miniature.

Besides the demux fix, the PR reworks the decoder itself — a *repack* pulse-position path he'd made
bit-exact against the Olympus DLL. To decide whether it was worth adopting, we wanted to see it the
way chapters 07 and 10 taught us to: feed frames straight into the real Olympus decoder, hosted in
the debugger we built from its own DLLs, and read the PCM it produces. We rebuilt the harness to
push demuxed frames directly at the decoder's input pin.

It didn't yield. The decoder's input frame is the manufacturer's shape, not ours — roughly **56
bytes where our demuxer carries 42** — and whatever we fed it came back decorrelated from every
known-good reference, capping after about **25 seconds** like a trial limit we hadn't fully lifted.
We could host the decoder; we could not feed it faithfully. So we could not certify the repack —
and, per the census, it changed nothing a human ear would catch on a real file anyway. Inaudible is
inaudible. We took the demux fix and left the repack on the shelf.

Chapter 08 would recognise the shape of that: sometimes you get the box open, and sometimes you
just learn exactly which screw you don't have. Both are worth writing down.

## What to take away

- **The best fix in a chapter doesn't have to be yours.** An open lock stays open because people
  keep handing each other pieces. Two chapters after we thought SP demux was finished, a
  contributor we already credited handed us the piece we'd skipped.
- **Measure the traffic before you chase the tail.** Four recordings a quarter is worth fixing
  *because they were broken*, not because they were common — and the census is what told us which.
- **A surgical gate beats a brave rewrite.** Divert only the files that are actually broken; prove
  everything else byte-identical. A decoder in production earns changes by not changing anything it
  already gets right.
- **Pick the metric that's yours.** Bit-exactness against the DLL is the reference implementation's
  goal. Ours is a complete, intelligible dictation. Knowing the difference is what let us take one
  half of a pull request and confidently decline the other.
