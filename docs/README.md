# The DS2-Anywhere docs

A long-form, didactic walkthrough of how a format that was locked for thirty years got decoded on Linux, put into production, and debugged in public — mistakes and all. You can read these in order as one story, or jump to the chapter you need.

New here? Start with the [README](../README.md) for the 30-second demo and the install. Then come back for the story.

## The chapters

| # | Chapter | What it's about |
|---|---------|-----------------|
| 00 | [The operational pain](00-operational-pain.md) | What a Windows-VM DS2 pipeline actually costs you in production, and why it kept breaking. ⚙️ |
| 01 | [The reverse engineering](01-reverse-engineering.md) | How the codec was cracked from the Olympus DLLs with Ghidra — **the genius part, and it isn't ours.** |
| 02 | [Integration](02-integration.md) | Turning a decoder into a service: CLI, cron, HTTP daemon, web UI. The reusable patterns. |
| 03 | [The validation campaign](03-validation-campaign.md) | How we decided it was safe to ship — an A/B against the commercial reference, not a count of green checks. |
| 04 | [WASM vs native](04-wasm-vs-native.md) | Why the first build was WebAssembly, why production runs a native binary, and the 3–5× that bought. |
| 05 | [Lessons learned](05-lessons-learned.md) | The transferable lessons — including the one about reporting your own mistakes. 🐛 |
| 06 | [The empty-block bug](06-the-empty-block-bug.md) | Bit-exact on every file we tested, **still** wrong on paused recordings. Ten dead ends, a twelve-line fix. 🧱 |
| 07 | [Cracking the re-sync block](07-cracking-the-resync-block.md) | We ran the closed Olympus decoder inside a debugger we built from its own DLLs, and read the format's last demux rule off the silicon. 🔓 |
| 08 | [The decoder black hole](08-the-decoder-black-hole.md) | The terse engineer's handoff for the "last bug." ⚠️ **Superseded by 10 — kept as honest record.** |
| 09 | [The re-sync excitation anomaly](09-the-resync-excitation-anomaly.md) | The research-paper write-up of that same "last bug": analysis-by-synthesis, nine falsified hypotheses, a hidden state machine. ⚠️ **Its central claim is wrong — read 10.** |
| 10 | [The reckoning: the bug that wasn't](10-the-reckoning-the-bug-that-wasnt.md) | The twist. We built an instrumentable oracle from the vendor's DLLs, watched a reference lie to us, and settled it by **listening**. There was no bug. **The most honest chapter here.** 👂 |
| 11 | [The Grundig/Philips variant](11-the-grundig-philips-variant.md) | A real dictation whose header wasn't at `0x600` — a Grundig/Philips container the chain refused. The header-size fix that opened a whole second family of devices. 📼 |
| 12 | [Cracking the Grundig SP codec](12-cracking-the-grundig-sp-codec.md) | A Grundig Digta `.dss` that decoded to pure noise — in our pipeline, in FFmpeg, and in NCH Switch. The one nobody had decoded, cracked from the DLL. 🔊 |
| 13 | [The SP re-sync block](13-the-sp-resync-block.md) | The chapter-07 re-sync rule, never ported to the SP demuxer, comes due. Same coin: re-host the parser, read the law off the silicon, port it. 🔁 |
| 14 | [The compact block](14-the-compact-block-pause.md) | A third way DSS hides a pause — and the only one we didn't find ourselves. It arrived as a pull request. We measured, gated, shipped, and declined the half we couldn't verify. 🤝 |
| 15 | [The relay runs backward](15-the-relay-runs-backward.md) | A PR from the same contributor: a good fix with a broken half. A shadow bench caught the regression before it merged, scrubbed repros proved it without shipping anyone's voice, and the cause was a golden master that lied — chapter 10 from the other chair. Plus a pin on the door we ship through. 🪞 |
| 16 | [The Q15 instability](16-the-q15-instability.md) | A codec that correlates at 0.998 for 58 seconds then blows up. The cause: Q15 integer truncation vs the DLL double-precision arithmetic. The fix: a six-line AGC. The discovery: the DLL codebook is 256 doubles in log-space, not 32 integers. 🔊 |

## How to read it

- **The integration story (00 → 05):** if you want to take an open codec into production, this is the recipe — pain, RE, integration, validation, the WASM→native call, and the lessons.
- **The detective trilogy-plus (06 → 10):** if you reverse-engineer for a living, start here. Two real bugs hunted to ground (06, 07), then a rigorous investigation into a third (08, 09) that **08 and 09 get confidently wrong** — and 10, the reckoning, which is the single most useful read in the repo: how careful work fools itself, and the cheapest test that breaks the spell.
- **The relay (11 → 15):** the format keeps handing over new locks — a second device family (Grundig/Philips), a codec nobody had decoded, a pause encoding a contributor found before we did, and — the other direction at last — a regression *we* caught in a contributor's pull request before it shipped. This is what an open format looks like when strangers keep pulling the thread, in both directions.

A note on chapters 08 and 09: we did not delete them when their conclusion fell. They're preserved, each behind a banner, because the *method* in them is sound and the *trap* they fell into is the lesson. A reverse-engineering log is only worth something if its dead ends stay marked.

---

*Sixteen chapters from "impossible for thirty years" to "production in a weekend" — including the wrong turn we're proudest of having written down. 🔓*
