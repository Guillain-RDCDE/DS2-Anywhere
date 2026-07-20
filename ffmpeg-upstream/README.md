# submission/

> Staging for the upstream FFmpeg patch (`ffmpeg-devel@ffmpeg.org`).
> The **v2 patch** has been sent; two **demuxer corrections** for paused
> recordings (empty-block + `byte1` re-anchoring) have been written up and
> flagged to the thread (2026-06-02); the consolidated **v3 patch** that
> folds them in was **sent to ffmpeg-devel on 2026-07-20** (2-patch v3
> series; built, applies cleanly on master, both FATE tests pass). This folder is the open workbench:
> cover letter, FATE plan, changelog, FATE sample, and the follow-up notes.

## Status

| Item | State |
|---|---|
| Patch by Patrick Domack | Obtained, MIT/PD relicensed via [hirparak/dss-codec#1][reli], applies cleanly on FFmpeg HEAD `69bdb05` (2026-05-25) |
| Decoder verified vs Hirpara's Rust reference on FATE sample | SNR 66.4 dB; 99.98% of 591360 samples differ by at most +/-1 LSB, 1 sample by 3 (max). Rounding noise, inaudible. Full table: [`01-fate-sample-plan.md`](01-fate-sample-plan.md) |
| End-to-end build + native `ffmpeg -i file.ds2 out.wav` | Tested on the FATE sample (37 s, decoded in 38 ms; cold-cache real-time ratio not benchmarked) |
| FATE sample, publicly-redistributable | [`fate/sample-qp.ds2`](fate/sample-qp.ds2), DS2 QP 16 kHz, 37 s, 129 KiB, md5 `23eab82c…` |
| FATE reference output (per-frame CRC) | [`fate/fate-ds2-qp.ref`](fate/fate-ds2-qp.ref) — 2319 lines (6 header + 2313 frame entries) |
| `.patch` artefact for `git am` / `git send-email` | [`patches/v2-0001-avcodec-avformat-add-Olympus-DS2-decoder-and-demu.patch`](patches/v2-0001-avcodec-avformat-add-Olympus-DS2-decoder-and-demu.patch) — applies cleanly on `69bdb05`, verified end-to-end (fresh clone → `git am` → `make` → decode FATE sample → md5 matches `fate-ds2-qp.ref` byte-for-byte) |
| Mail body ready to copy-paste | [`patches/email-body.txt`](patches/email-body.txt) + [`patches/email-subject.txt`](patches/email-subject.txt) (see [`patches/README.md`](patches/README.md) for procedure) |
| Cover letter (long form, for the repo) | [`00-cover-letter.md`](00-cover-letter.md) — body fenced between `BEGIN MAIL BODY` / `END MAIL BODY` |
| Changelog + doc + header entries drafted | [`02-changelog-and-doc.md`](02-changelog-and-doc.md) |
| Patch sent to `ffmpeg-devel` | **v2 sent** — supersedes v1 (2026-05-25 21:35 CEST). v2 brings Patrick Domack EOF + rounding fixes; decoder bit-perfect vs Hirpara reference (zero diff on FATE sample). v1 lore: [archive](https://lists.ffmpeg.org/lore/ffmpeg-devel/20260525193532.1845986-1-guillain@poulpe.us/T/#u). |
| Demuxer corrections for paused recordings | **Written up + flagged to the v2 thread 2026-06-02.** Both turned out to be one rule (per-block `byte1` re-anchoring; empty-block is its zero-fresh-frames case), confirmed byte-for-byte against the live Olympus parser. Notes: [`03-v3-empty-block-fix.md`](03-v3-empty-block-fix.md), [`04-resync-block-byte1.md`](04-resync-block-byte1.md); story: [`docs/07`](../docs/07-cracking-the-resync-block.md); follow-up mail: [`patches/email-body-v3-followup.txt`](patches/email-body-v3-followup.txt). Reference implementation (Rust) is live in production with an 18-file OLD-vs-NEW regression corpus. |
| v3 patch (folds both demux corrections) | **Sent to ffmpeg-devel 2026-07-20** (SMTP 250). 2-patch series: [`patches/v3-0001-*.patch`](patches/v3-0001-avcodec-avformat-add-Olympus-DS2-decoder-and-demu.patch) (Patrick's decoder+demuxer, + ASCII cleanup) and [`patches/v3-0002-*.patch`](patches/v3-0002-avformat-ds2-re-anchor-QP-frames-across-segment-b.patch) (QP re-sync re-anchoring + FATE). `git am` clean on master `c2312363`; builds; `fate-ds2-qp` + `fate-ds2-qp-paused` pass. Paused FATE sample (CC0, scrubbed of all audio+metadata) + refs in [`fate/`](fate/). Cover: [`patches/email-body-v3.txt`](patches/email-body-v3.txt). |

[reli]: https://github.com/hirparak/dss-codec/issues/1

## Why publish this folder before sending

So reviewers on `ffmpeg-devel` (and anyone else interested) can follow
the link back from the cover letter and see the prep work, the
validation methodology, and the chain of credit — and tell us about
mistakes before the patch goes out, while they're trivially fixable.

If you spot something in any draft in here: open an issue or PR on
this repo.

## Files

- [`00-cover-letter.md`](00-cover-letter.md) — the body of `[PATCH 0/2]`.
- [`01-fate-sample-plan.md`](01-fate-sample-plan.md) — the sample we
  ship, the validation against the Rust reference (separated from
  other validation datasets), the FATE wiring.
- [`02-changelog-and-doc.md`](02-changelog-and-doc.md) — `Changelog`
  entry, `doc/general_contents.texi` line, MAINTAINERS guidance, file
  header template.
- [`fate/sample-qp.ds2`](fate/sample-qp.ds2) — the actual FATE sample
  (DS2 QP), ready for upload to
  `samples.ffmpeg.org/A-codecs/DS2/`.
- [`fate/fate-ds2-qp.ref`](fate/fate-ds2-qp.ref) — the FATE reference
  output (framecrc) produced by Patrick's C decoder on the sample
  above, ready to drop into `tests/ref/fate/ds2-qp`.
- [`patches/`](patches/) — the .patch artefact (built via
  `git format-patch`), the body-ready text files, and step-by-step
  send instructions.

## Attribution chain

How credit is allocated in the patch and cover letter:

- **Codec specification** (the underlying intellectual work) — **Kieran
  Hirpara** ([`hirparak/dss-codec`][hcodec], MIT, Feb 2026).
  Reverse-engineered from the Olympus DLLs (`DssDecoder.dll`,
  `dss32.dll`) via clean-room Ghidra analysis, with byte-for-byte
  verification against the official Olympus DirectShow filter.
- **C implementation** (the patch itself: `libavcodec/ds2.c` 982
  lines + `libavformat/ds2.c` 369 lines, plus 12 lines of
  registration boilerplate across 6 existing files; 1363 insertions
  total in 8 files) — **Patrick Domack** ([`patrickdk77` on
  GitHub][pgh]). Originally posted as [gist
  `330dd3f5...`][gist] (2026-03). Explicitly relicensed MIT/public-domain
  for upstream FFmpeg merge in [hirparak/dss-codec#1][reli].
- **Validation, FATE prep, sample sourcing, mailing-list submission**
  — this project ([`Guillain-RDCDE/DS2-Anywhere`][us]).

[hcodec]: https://github.com/hirparak/dss-codec
[pgh]: https://github.com/patrickdk77
[gist]: https://gist.github.com/patrickdk77/330dd3f593696d103e831c4c1d78d1f9
[us]: https://github.com/Guillain-RDCDE/DS2-Anywhere

Patrick has explicitly opted out of `ffmpeg-devel` interactions but
remains available for codec-internals questions via the issue thread
above. The submitter handles mailing-list traffic; Patrick is not on
the loop unless reviewers specifically need him.

## Relationship to FFmpeg's existing DSS support

FFmpeg already ships a DSS SP decoder and demuxer
(`libavcodec/dss_sp.c` + `libavformat/dss.c`, Oleksij Rempel, 2014).
That code is **untouched** by this patch. DSS and DS2 share a similar
0x600 header layout but use different codebooks and frame structures;
the two formats coexist as siblings in FFmpeg after this patch.

The DS2 demuxer's probe only accepts the `\x03ds2` magic, so DSS
files (`\x02dss` / `\x03dss`) continue to route to Rempel's existing
demuxer. No regression to the existing path is possible.
