# submission/

> Pre-submission staging for the upstream FFmpeg patch (`ffmpeg-devel@ffmpeg.org`). **Nothing here has been sent yet.** This folder is the workbench where the cover letter, the FATE plan, and the changelog entries are being prepared, in the open, for review by anyone interested.

## Status

| Item | State |
|---|---|
| Patch by Patrick Domack | ✅ obtained, MIT/PD licensed, applies cleanly on FFmpeg HEAD |
| Decoder byte-for-byte verified vs reference Rust | ✅ SNR 67 dB, ±1 LSB rounding noise (see [docs/03-validation-campaign.md](../docs/03-validation-campaign.md) and [docs/04-wasm-vs-native.md](../docs/04-wasm-vs-native.md)) |
| End-to-end build + native `ffmpeg -i file.ds2 out.wav` | ✅ tested, 981× real time |
| **FATE sample** (publicly-redistributable `.ds2`) | ⏳ being recorded |
| **Cover letter** for `ffmpeg-devel` | 📝 draft in [00-cover-letter.md](00-cover-letter.md) |
| **Changelog + doc entries** in the patch | ⏳ pending |
| **Patch sent** to `ffmpeg-devel` | ⏳ pending |

## Why publish this folder before sending

Two reasons:

1. **Transparency for the upstream review process.** When the patch lands on `ffmpeg-devel`, reviewers can follow the link in the cover letter back to this folder and see the prep work, the validation methodology, and the chain of credit. Saves everyone time on context questions.
2. **Open invitation for feedback.** Anyone — Patrick, Kieran, FFmpeg contributors, random readers — can spot a flaw in the cover letter, the FATE plan, or the attribution chain *before* it goes out, when it's still trivially fixable.

If you have feedback on any draft in here, open an issue or PR on this repo.

## Files

- [`00-cover-letter.md`](00-cover-letter.md) — the email that will accompany `[PATCH 1/2]` and `[PATCH 2/2]` to `ffmpeg-devel`.
- [`01-fate-sample-plan.md`](01-fate-sample-plan.md) — how we'll produce a publicly-redistributable DS2/DSS sample and what FATE entries we'll add.
- [`02-changelog-and-doc.md`](02-changelog-and-doc.md) — the exact lines we'll add to `Changelog`, `doc/general_contents.texi`, and (if needed) `MAINTAINERS`.

## Attribution chain

For the record, here's how credit will be allocated in the patch and cover letter:

- **Codec specification** (the underlying intellectual work) — **Kieran Hirpara** ([`hirparak/dss-codec`](https://github.com/hirparak/dss-codec), MIT, Feb 2026). Reverse-engineered from Olympus DLLs via Ghidra, with byte-for-byte verification against the official DirectShow filter.
- **C implementation** (the patch itself: `libavcodec/ds2.c` + `libavformat/ds2.c`, ~1,400 lines) — **Patrick Domack** ([`patrickdk77` on GitHub](https://github.com/patrickdk77)). Originally posted as [gist `330dd3f5...`](https://gist.github.com/patrickdk77/330dd3f593696d103e831c4c1d78d1f9) on March 5, 2026. Explicitly relicensed MIT/PD for upstream merge (see [hirparak/dss-codec#1](https://github.com/hirparak/dss-codec/issues/1)).
- **Validation, FATE prep, mailing-list submission, integration patterns** — this project ([`Guillain-RDCDE/DS2-Anywhere`](https://github.com/Guillain-RDCDE/DS2-Anywhere)).

Patrick has explicitly opted out of `ffmpeg-devel` interactions but remains available for technical questions via [issue #1](https://github.com/hirparak/dss-codec/issues/1) on the upstream codec repo.
