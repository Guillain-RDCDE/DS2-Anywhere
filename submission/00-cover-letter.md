# Cover letter — `ffmpeg-devel` submission

> Draft, not sent. Will be the body of the `[PATCH 0/N]` email accompanying the patch series to `ffmpeg-devel@ffmpeg.org`. Plain text, no Markdown, ~80 char lines (mailing-list convention).

---

```
Subject: [PATCH 0/2] avcodec, avformat: add Olympus DSS/DS2 codec and demuxer

Hi,

This patchset adds support for the Olympus Digital Speech Standard (DSS)
and DSS Pro (DS2) proprietary audio formats — the formats used by Olympus
dictaphone devices (DS-, DM-, DPM-series, used heavily in legal, medical,
and journalistic dictation workflows).

Trac ticket #6091 ("Add DS2 codec support") has been open since
October 2017. This series closes it.

  Patch 1/2: libavformat/ds2.c
             demuxer for the .ds2 container, ~370 lines
  Patch 2/2: libavcodec/ds2.c
             CELP decoder, ~980 lines, handles both SP (12 kHz)
             and QP (16 kHz) modes plus the older DSS SP format

Authorship and licensing
------------------------

The C implementation is by Patrick Domack <patrickdk77>, originally
posted as a github gist in March 2026, and explicitly relicensed under
MIT / public-domain terms for inclusion in FFmpeg (see attribution chain
at the link below). I am submitting on Patrick's behalf with his explicit
permission; he is happy to answer codec-internals questions if reviewers
ping him but prefers to stay off the mailing list.

The codec specification used as the basis for the implementation was
reverse-engineered from the Olympus DLLs (DssDecoder.dll + dss32.dll)
using Ghidra, by Kieran Hirpara <hirparak>, released February 2026 at
https://github.com/hirparak/dss-codec (MIT). The specification document
is at:

  https://github.com/hirparak/dss-codec/blob/master/dss-codec/CODEC_SPECIFICATION.md

It includes byte-for-byte verification against the output of the
official Olympus DirectShow filter, a reference Python decoder, and a
reference Rust decoder. Both Patrick's C and Kieran's Rust are
independent implementations of the same specification.

Validation
----------

  - The patch applies cleanly to current master (last tested against
    commit XXXXXXX on YYYY-MM-DD, no rebase needed).

  - The C decoder is byte-for-byte equivalent to Kieran's reference
    Rust within ±1 LSB rounding noise on a 31-minute real-world DS2 QP
    recording. Specifically: 49.8% of samples differ by exactly ±1 LSB
    (float-to-int16 ordering noise), 0.003% by ±2, none above. RMS
    error 0.71 against an RMS signal of 1577 (SNR 67 dB), well below
    the audibility threshold for speech codecs.

  - End-to-end transcription quality verified against the commercial
    NCH Switch decoder (the existing Windows-only reference): identical
    speech-to-text output from Whisper on identical input, measured
    across 35 real-world production dictation files. Full methodology
    at https://github.com/Guillain-RDCDE/DS2-Anywhere

FATE
----

  - One sample added to fate-suite: a 30-second public-domain reading
    recorded on an Olympus DS-series device, encoded in DS2 QP and
    DSS SP modes.
  - Reference PCM checksums for both modes added to fate/audio.mak.

Changelog and doc
-----------------

  - Changelog entry added under "version <NEXT>" -> "audio decoders".
  - doc/general_contents.texi: DS2 added to the supported audio codecs
    table.

Testing instructions
--------------------

  git am 0001-libavformat-add-ds2-demuxer.patch
  git am 0002-libavcodec-add-ds2-decoder.patch
  ./configure --enable-decoder=ds2 --enable-demuxer=ds2
  make -j$(nproc)
  ./ffmpeg -i sample.ds2 sample.wav

Background, integration patterns, additional validation data, and the
production-deployment story for one user of this codec are at:

  https://github.com/Guillain-RDCDE/DS2-Anywhere

Comments and review welcome. Happy to iterate on style, naming,
testing scope, anything.

Thanks,
Guillain d'Erceville
```

---

## Open questions to settle before sending

- **Subject line convention** — FFmpeg uses `avcodec/<topic>:` or `avcodec, avformat:` prefixes. The latter is appropriate here since the patchset touches both. To confirm by reading the last 100 `[PATCH]` subjects on the list archive.
- **Patch series ordering** — demuxer first (1/2) since the decoder is useless without it for the `ffmpeg -i` use case. Open to reordering if reviewers prefer.
- **MAINTAINERS entry** — should we add a `libavcodec/ds2.c` entry? Patrick is opted out of the list; we don't want to volunteer him as the listed maintainer. Possibly leave unowned and let it land where it lands.
- **Sample file location** — FFmpeg samples go into the `fate-suite` git submodule (`samples.ffmpeg.org`). Coordination with one of the FFmpeg admins to push the sample once produced.

## Etiquette reminders

- Plain text only, no HTML, no attachments (patches sent via `git send-email` are inlined).
- Subject line ≤ 78 chars.
- Body lines wrapped at ~75 chars.
- Reply by quoting (`> `) and below the quoted text.
- One topic per thread; if a side question comes up, start a fresh thread.
- Patience: review cadence on `ffmpeg-devel` is typically days, sometimes weeks. No bumping the thread before 7 days.

## When to send

After:

1. ✅ Patch byte-for-byte validated (done).
2. ⏳ FATE sample produced and checksums computed.
3. ⏳ Changelog + doc entries added to the patch.
4. ⏳ Final dry-run: clean checkout of master, apply both patches, build, FATE pass.
5. ⏳ One last re-read of the cover letter by a second pair of eyes.

Then `git format-patch -2 --cover-letter --subject-prefix='PATCH'` and `git send-email --to=ffmpeg-devel@ffmpeg.org`.
