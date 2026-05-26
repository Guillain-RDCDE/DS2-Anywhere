# Cover letter — `ffmpeg-devel` submission

> Draft, not sent. Will be the body of the `[PATCH]` email accompanying
> the patch to `ffmpeg-devel@ffmpeg.org`. Plain text, no Markdown,
> ASCII only, lines wrapped at 75 cols (mailing-list convention).
>
> **The mail body starts at the `>8` marker below and ends at the next one.**

```
----8<-------- BEGIN MAIL BODY --------8<----
Subject: [PATCH v2] avcodec, avformat: add Olympus DS2 decoder and demuxer

Hi,

This patch adds support for the Olympus Digital Speech Standard Pro
(DS2) audio format, used by Olympus DS-, DM-, and DPM-series dictation
recorders. It is the successor to the older DSS format already supported
in FFmpeg via libavcodec/dss_sp.c and libavformat/dss.c (Oleksij Rempel,
2014). The two formats share a similar 0x600 header layout but use
different codebooks and frame structures; this patch is complementary
to the existing DSS support, not a replacement.

Trac ticket #6091 ("support ds2 audio (dss pro audio) file format") has
been open since October 2017. This patch closes it.

The patch is shipped as a single atomic commit (decoder + demuxer
together) rather than split. The two pieces are functionally
inseparable: the demuxer hardcodes AV_CODEC_ID_DS2 from the decoder
patch, so splitting would create an intermediate state where the tree
does not build. Combined size:

  - libavcodec/ds2.c          982 lines (new file, CELP decoder
                              handling both DS2 modes: SP 12 kHz and
                              QP 16 kHz)
  - libavformat/ds2.c         369 lines (new file, demuxer for the
                              .ds2 container)
  - registration boilerplate   12 lines across Makefile / allcodecs.c
                              / allformats.c / codec_desc.c /
                              codec_id.h

Total: 1506 insertions across 9 files.

v2 changes: see commit message at end (rounding + EOF fixes from
upstream author Patrick Domack, decoder now bit-perfect vs Hirpara
reference; doc/general_contents.texi added).

The patch applies cleanly to master HEAD (commit 69bdb05, 2026-05-25);
make passes; the new decoder + demuxer build out of the box with
--enable-decoder=ds2 --enable-demuxer=ds2.

Authorship and licensing
------------------------

The C implementation is by Patrick Domack (@patrickdk77 on GitHub),
originally posted as a gist on 2026-03 and updated for current master.
Gist URL:

  https://gist.github.com/patrickdk77/330dd3f593696d103e831c4c1d78d1f9

Patrick has explicitly relicensed the code under MIT / public-domain
terms for inclusion in FFmpeg. The relicensing grant is recorded
publicly in:

  https://github.com/hirparak/dss-codec/issues/1

He has asked to stay off the mailing list but remains available for
technical questions via that same issue thread. I am submitting on his
behalf.

The codec specification used as the basis for the implementation was
reverse-engineered from the Olympus DLLs (DssDecoder.dll and dss32.dll)
via Ghidra by Kieran Hirpara, released February 2026 as MIT-licensed:

  https://github.com/hirparak/dss-codec

The specification document is at:

https://github.com/hirparak/dss-codec/blob/master/dss-codec/CODEC_SPECIFICATION.md

It includes byte-for-byte verification against the output of the
official Olympus DirectShow filter, a reference Python decoder, and a
reference Rust decoder.

The CELP algorithm in libavcodec/ds2.c (decode loops, pitch synthesis
filter, reflection-to-LPC conversion, frame parsing) was implemented
from the FFmpeg trac #6091 specification text by Patrick Domack. The
numerical quantization tables (SP and QP reflection codebooks, pitch
and excitation gain tables, pulse amplitude tables — ~4400 values
total) are sourced from Hirpara's reference Rust implementation,
which originally extracted them from the Olympus DssDecoder.dll via
Ghidra. Both the algorithm and the tables are MIT-licensed.

Validation
----------

Decoder correctness was verified by comparing C output against
Hirpara's reference Rust on the FATE sample shipped with this series:

  PCM samples compared:  591,360 (37.0 s @ 16 kHz)
  exact match:           100.00 %
  any difference:          0 samples
  max abs diff:            0
  SNR:                     infinite (no error)

The two implementations produce bit-identical PCM output.

Beyond the FATE sample, the decoder has been exercised on a 31-minute
production DS2 QP recording and on a 35-file corpus of in-the-wild DS2
and DSS files from a production dictation pipeline. Decode succeeds
on 35/35; Whisper transcription run on the C-decoded output is
coherent with transcription run on the output of the reference
proprietary Windows decoder (NCH Switch). Full methodology and the
sanitized per-file results are at:

  https://github.com/Guillain-RDCDE/DS2-Anywhere

FATE
----

  - One DS2 QP sample is included for upload to
    samples.ffmpeg.org/A-codecs/DS2/: a 37-second file (132,608 bytes,
    md5 23eab82c3fc093c44ef4eb45ac35ba20) published as a public test
    artefact on dictate.com.au's Shopify CDN. The author metadata
    inside the file reads "DICTATE" (vendor-provided test content, no
    third-party identification).

  - Reference framecrc shipped: 2319 lines (6 header + 2313 frame
    entries), generated by the C decoder on the sample above. Drops
    into tests/ref/fate/ds2-qp.

  - One FATE rule added to tests/fate/audio.mak:
      FATE_AUDIO-$(call DEMDEC, DS2, DS2) += fate-ds2-qp
      fate-ds2-qp: CMD = framecrc -i $(TARGET_SAMPLES)/ds2/sample-qp.ds2

  - DS2 SP support is in the decoder code (12 kHz mode) but not
    covered by FATE in this series. A DS2 SP sample will follow when
    one is sourced; the QP sample is the highest-leverage starting
    point since QP is the dominant format in the field.

  - DSS SP coverage remains with the existing dss_sp.c decoder and
    dss.c demuxer; this series does not touch them.

Changelog and doc
-----------------

  - Changelog entry added under "version <NEXT>" -> "audio decoders".
  - doc/general_contents.texi: DS2 added to the supported audio
    codecs table (the existing DSS row is left as-is).

Background, validation data in full, and the production-deployment
context for one user of this codec are at:

  https://github.com/Guillain-RDCDE/DS2-Anywhere

Review and comments welcome.

Thanks,
Guillain d'Erceville
----8<-------- END MAIL BODY --------8<----
```

---

## Notes for the submitter (not in the mail)

### Pre-send checklist

1. **Re-confirm the commit hash.** `git -C ffmpeg log -1 --format='%h on %ad' --date=short master`
   and replace `69bdb05, 2026-05-25` in the mail body if newer.
2. **Re-confirm the patch still applies.** `git -C ffmpeg apply --check
   ../patches/0001-libavformat-ds2.patch ../patches/0002-libavcodec-ds2.patch`.
3. **Confirm `git format-patch` produces a clean single patch.**
   Patrick's original gist is a single combined diff; we keep it as a
   single commit (decoder + demuxer atomic), use `git am` to create
   the commit with proper `Author:` (Patrick) and `Signed-off-by:`
   (submitter), then `git format-patch -1 --stdout` for the wire
   artefact.
4. **Check subject convention** against last 100 patches on
   `ffmpeg-devel`. The form `avformat/ds2, avcodec/ds2:` mirrors what
   FFmpeg uses today for series spanning both libs. If the dominant
   form on the archive is different, adjust.
5. **Plain-text dry run.** `git send-email --dry-run --to=guillain@...
   <patches>` to inspect the wire format before the real send.

### Why the scope is QP-decoded-only for FATE

Patrick's decoder supports both DS2 SP and DS2 QP. The FATE sample we
ship is QP only because (a) QP is the format in real-world use today,
(b) the dictate.com.au public sample we located is QP, and (c) shipping
a single high-confidence sample is better than shipping a synthetic SP
sample with no real-world validation. SP coverage follows in a second
patch once a sample exists.

### MAINTAINERS

Suggested: leave the new files unowned. Patrick has explicitly opted
out of ffmpeg-devel interactions, so we do not list him as the
listed maintainer. The submitter is not an FFmpeg regular either.
Reviewers who want a name on file: option is to add a `R:` (Reviewer)
line if FFmpeg's MAINTAINERS spec supports it (to verify), otherwise
leave unowned.

### After-send etiquette

  - Patience: review cadence on `ffmpeg-devel` is days to weeks. No
    bumping before 7 days have elapsed.
  - Reply with quote (`> `) and below the quoted text.
  - One topic per thread; spin a fresh thread for side questions.
  - If a reviewer requests changes, post v2 as a separate thread
    titled `[PATCH v2 0/2] ...` (not as a reply to v1).
