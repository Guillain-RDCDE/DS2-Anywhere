# 15 — The relay runs backward: a regression, caught before it shipped

> Chapter 14 ended on a line: *you hand someone a piece, and one day, without asking,
> they hand one back.* This is the chapter where we hand one back. A pull request from the
> same contributor — a genuine improvement with a broken half — caught by a shadow bench
> before it reached anyone, and the very human reason it broke. 🪞

## The pull request, again

Two chapters after the compact block, another PR from Patrick Domack landed on the Rust
crate we build from — [hirparak/dss-codec#16](https://github.com/hirparak/dss-codec/pull/16),
*"Ported the two annotation fixes."* He'd been deep in a different codec family for weeks
(Sony's MSV/DVF), and on the way through he folded two changes from his FFmpeg tree back
into the Rust decoder: a combinatorial-index overflow fix, and a rewrite of the QP demux —
a *block-quantized walk* that replaced the byte1 re-sync anchor from chapters 07 and 13
with a self-contained per-block frame count.

The anchor it removed is the one rule in this whole story we were most sure of. We didn't
read it from a spec; we read it off the silicon — chapter 07, the real Olympus decoder
hosted in a debugger built from its own DLLs, `poff = 2·byte1 − 6`, byte-for-byte. A pull
request that removes *that* is one you don't merge on trust.

## The bench that runs before the merge

So we didn't trust it — in either direction. We built four binaries: our production
decoder, upstream master, and master plus each half of the PR on its own. Then we ran all
316 real DSS/DS2 recordings — the same production traffic chapter 14's census drew from —
through every one, comparing not file sizes but samples.

The split verdict came out clean and uncomfortable:

- The **combinatorial-index fix** — the half that touches the codec — degraded **nothing**.
  Kept it.
- The **block-quantized walk** — the half that removed the anchor — **garbled 16 of our 290
  QP recordings.** Not subtly: roughness doubling, clipping appearing where a real dictation
  never clips, the unmistakable signature of a demux that has lost its place.

Every file that broke had the same shape on disk. A re-sync block — `frame_count = 19`,
byte1 announcing that *the next frame starts a few bytes over* — after which the walk sat
six bytes behind the anchor and never recovered. On file `2943690`, block 364 is that block:
up to 363 the walk and the anchor agree byte-for-byte, and from 365 on they part by six. That
six-byte drift is the noise a human hears.

And it made sense the instant we looked at *whose* files these were. **His clips are straight
runs; ours are full of pauses.** A doctor, a bailiff, a police officer at the roadside — they
stop, they think, they start again. Every pause is a re-sync block. Patrick had one sample
with pauses in it; we have thousands. The same format, tested from its two opposite ends —
and the anchor is exactly the rule you only need when the recording breathes.

## Samples you can actually share

We wanted to hand him the proof. But our proof is people's confidential dictations: names,
addresses, medical and legal facts. You cannot email that to a contributor. Ever.

So we built reproducers that keep the bug and throw away the person. A DS2 file is block
headers — six bytes, the framing — and payloads — 506 bytes, the voice. The demux bug lives
*entirely* in the headers. So we kept every block header byte-for-byte, overwrote all 506
payload bytes of every block with a position-varying pattern, and zeroed the metadata header.
What's left decodes to noise — no speech, nothing recoverable — but still drifts at block 364
exactly as the real file did. Two files: one reproducing the walk drift, one reproducing an
unrelated panic (below). Both safe to post in public, both usable as permanent regression
fixtures. We put the block-by-block framing table in the reproducer's own README, so the bug is
legible without decoding a thing.

As it turned out, we never had to send them. The written diagnosis was enough, and Patrick
found the rest himself (next section). We kept the files anyway — a scrubbed regression fixture
is worth having whether or not it's ever needed.

## The panic we found on the way

The bench turned up something the PR had nothing to do with. Upstream master **panicked
outright** on 3 of our real SP recordings: a `usize` underflow in the SP demux, `end − spos`
going negative on the last frame when the declared frame count overshoots the built stream.
Our own decoder passes those files fine — a different code path — so we'd never have seen it
in production. But anyone building from master would. One line — `end.saturating_sub(spos)` —
and the files decode full-length. We put it in the same report.

## The very human root cause

Here is the part that belongs in *this* repo specifically, right next to chapter 10.

Patrick took the report and, instead of defending the patch, went and *looked*. He'd already
hit the same overrun in his FFmpeg C and fixed it there — he'd simply never looped back to
check the Rust carried it too. He agreed with the two-ends reading of the pause data on the
spot. And then he found the thing underneath it:

> *"My golden masters I made for the annotations got corrupted along the way and I did not
> notice — likely overwrote them by accident from a test decode."*

The walk had *validated*. It passed his tests. It passed them because the reference files it
was checked against were quietly broken — overwritten, at some point, by the output of the
very thing they were meant to check. That is chapter 10, from the other side of the table: a
reference that lies, and a careful person trusting it because careful people trust their
references. The only reason it surfaced here and not in production is that two of us were
testing the same format from opposite ends, and the ends disagreed.

He pulled the walk, kept the codec fix, took the panic patch, and merged. Master today carries
the good half and the panic fix, and the anchor is back. We re-ran all 316 files against the
merged result: **zero garbling, and the 3 files that used to panic now decode.** Our own
production decoder never changed — we run our own build — but the public build *this* repo
ships, which clones that fork fresh, is now safe by construction.

## Pinning the door we ship through

Which exposed a loose thread of our own, unrelated to Patrick. Our Dockerfile built the native
decoder with `git clone --depth=1` of the fork's default branch — *whatever it happened to be*
at build time. That is precisely how a future bad merge would reach our public image with
nobody in the loop: silently, on the next `docker build`. The regression we caught by hand
this time is the exact class of thing a floating clone ships by accident.

So the build now pins the fork to the specific commit we shadow-benched (`e16b71c5`), fetched
by SHA. It's a one-line discipline with a real payoff: the public image is reproducible, and
the codec only moves when someone bumps the pin *and re-runs the bench* — on purpose, never
overnight. An open relay still needs a latch on the door you ship through.

## What to take away

- **Shadow-bench upstream before you adopt it — even from someone you trust; especially the
  half that removes a rule you're sure of.** A four-binary A/B on real traffic caught a
  regression a green test suite waved through.
- **You can share a bug without sharing your data.** Keep the bytes that carry the defect
  (here, block headers); destroy the bytes that carry the person (payloads, metadata). What's
  left is a fixture anyone can commit.
- **A reference can lie to you the same way twice.** Chapter 10 was *our* golden master
  misleading us; this was a contributor's, corrupted by a stray test decode. The defence is
  identical both times: a second, independent oracle — here, two people testing the format
  from opposite ends.
- **Don't ship from a moving target.** Pin the upstream you build from to a commit you
  verified. Bump it deliberately, with the bench — not on whatever landed overnight.
