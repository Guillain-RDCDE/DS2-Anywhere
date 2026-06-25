# 06 — DSS SP re-sync block (upstream `libavformat/dss.c`)

> Companion to `03-v3-empty-block-fix.md` and `04-resync-block-byte1.md`, but for the
> **existing** DSS support in FFmpeg, not the new DS2 patch. `libavformat/dss.c`
> (Oleksij Rempel, 2014) has the same paused-recording defect as the DS2 QP path, for
> the same reason, and the same one-rule fix applies. Confirmed byte-for-byte against
> the live Olympus `DssParser.dll` (40,620 frames; see `docs/13`).

## The defect

`dss_sp_read_packet()` reads the audio area as a flat concatenation of 506-byte block
payloads (`dss_skip_audio_header()` skips the 6-byte block header and credits
`counter += 506`). Correct only for continuous, gap-free recordings — which is what
exists today and why nobody hit it. Real Olympus dictation is voice-activated: a pause
emits a block with `frame_count == 0` (header `byte[2]`), and the **next** block
restarts the frame grid at its own anchor. The current demuxer reads straight through
both, so the SP stream desyncs from the first pause to end-of-file: audible garble
after the pause, the rest fine.

## The rule (same as the DS2 follow-up)

Every block re-anchors its frames at payload offset `2*byte1 - 6` (header `byte[1]`).
The block **following** an empty (`frame_count == 0`) block drops the in-flight
straddle tail and restarts at that anchor. `frame_count` alone is not a reliable
discriminator (it over-counts on resync blocks); the empty marker on the **preceding**
block is the trigger. On gap-free audio the anchor always coincides with the running
read position, so the change is a no-op there (and on every DS2 file — different
demuxer). This is the exact rule the codec spec already attributes to `dss.c`'s empty
blocks, never actually implemented.

## The change (sketch, against `libavformat/dss.c`)

`DSSDemuxContext` gains the previous block's `frame_count` (or a `prev_empty` flag) and
the current block's `byte1`. `dss_skip_audio_header()` reads `byte[1]`/`byte[2]` instead
of a blind `avio_skip`. In `dss_sp_read_packet()`, when the block just entered follows an
empty block: discard the partial frame in flight, reset the byte-swap state, and seek the
read position to the block payload start + `2*byte1 - 6` before resuming. Everything else
is unchanged.

The reference implementation (Rust) is the one-branch `DssSpStreamDemuxer::process_block`
fix now live in production, with the same OLD-vs-NEW byte-identity regression property as
the empty-block and DS2 work.

## FATE sample (obtained)

`fate/sample-dss-sp-paused.dss` — a 15.7 s, 54-block excerpt of a real paused DSS SP
recording, **header fully anonymised** (author, dates, comment and codec fields zeroed;
`strings` returns nothing) and the audio content confirmed neutral (no names, addresses
or case detail). It starts on a re-sync block (clean cold start) and takes one VOX pause
(empty block) mid-clip, with clean audio before and after — so it exercises exactly the
empty-block + re-sync path upstream `dss.c` currently mishandles, and shows the recovery.
Ready to upload to `samples.ffmpeg.org/A-codecs/DSS/`. Our Rust reference decodes its PCM
to md5 `f05fb3bb8a50150c60b72d8249b8a511` (sanity anchor; the FATE `.ref` is regenerated
from the patched FFmpeg build).

## What's left before the patch goes out

- **Build FFmpeg HEAD + apply the SP demux change + regenerate the FATE reference**, and
  confirm unpatched `dss.c` garbles this sample while the patched build matches the
  Olympus ground truth. (Unlike our Rust baseline — which already had the empty-block
  continuation handling — stock `dss.c` has *no* pause handling at all, so it desyncs on
  this sample; that is what makes it a meaningful regression test.)
- **`git send-email`** to ffmpeg-devel (no SMTP configured on this machine yet).

The mail body is drafted in `patches/email-body-v4-dss-sp-followup.txt`. This rides with
the pending v3 DS2 follow-up rather than going out untested.
