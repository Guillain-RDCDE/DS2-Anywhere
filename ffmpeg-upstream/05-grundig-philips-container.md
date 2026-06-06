# 05 — Grundig/Philips container support (`libavformat/ds2.c`)

Status: **patch ready** —
[`patches/avformat-ds2-grundig-philips-header-sizes.patch`](patches/avformat-ds2-grundig-philips-header-sizes.patch)
(`From:`/`Subject:`/`Signed-off-by:` set, `git apply --check` clean on top of the
v2 demuxer). Needs a GR/PH FATE sample before it goes to ffmpeg-devel.

This is a **standalone** demuxer follow-up, independent of the empty-block /
`byte1` re-sync v3 work (`03`, `04`, `patches/email-body-v3-followup.txt`) — that
series is about *paused-recording* decoding, this one is about *container header
size*. Both apply on top of v2; fold whichever you re-roll first. It deliberately
carries **no** version-series number so it doesn't claim a slot in your v3 thread.

## Why

The demuxer assumes the Olympus layout: magic `\x03ds2`, header fixed at `0x600`,
first audio block right after. Grundig/Philips recorders (header tag `GR/PH9607`,
e.g. the Grundig Digta — see hirparak/dss-codec#11 and
[docs/11](../docs/11-the-grundig-philips-variant.md)) write the **same** DS2-QP /
DSS-SP audio but with a larger header: the **first byte is the header size in
512-byte blocks** (7 for `.ds2`, 6 for `.dss`) and the extra blocks hold
`GR___`-tagged device-id records before the audio.

This is not a new codec and not a new container — it's the same rule the DSS
demuxer already applies (`header = version * 512`). DS2 just hard-coded version 3.
Generalizing the header size makes `ds2.c` accept Olympus **and** Grundig/Philips
**and** any future header size, with no codec change.

## The change

Add a `header_size` to the demux context and derive it from the first byte;
replace every `DS2_HEADER_SIZE` use with it. Broaden the probe to match `ds2`
under any version byte.

**1. Probe — accept any version byte (the `ds2` magic is bytes 1..3):**

```c
static int ds2_probe(const AVProbeData *p) {
    /* First byte = header size in 512-byte blocks (Olympus 2/3, GR/PH 6/7);
     * bytes 1..3 are the "ds2" tag. */
    if (p->buf_size < 4 || p->buf[1] != 'd' || p->buf[2] != 's' || p->buf[3] != '2')
        return 0;
    if (p->buf[0] < 2 || p->buf[0] > 16)
        return 0;
    return AVPROBE_SCORE_MAX;
}
```

**2. Context — add the field:**

```c
typedef struct DS2DemuxContext {
    int header_size;      /* first_byte * 512 (0x600 for Olympus, 0xe00 for GR/PH) */
    int format_type;
    ...
} DS2DemuxContext;
```

**3. `ds2_read_header` — set it first, then use it.** Read the version byte at
the very top, *before* `ds2_count_total_frames()` (which depends on it):

```c
    {
        uint8_t version = avio_r8(pb);           /* file is positioned at 0 here */
        if (version < 2 || version > 16)
            return AVERROR_INVALIDDATA;
        ctx->header_size = version * DS2_BLOCK_SIZE;
    }
    ...
    ret = ds2_count_total_frames(s);             /* now uses ctx->header_size */
    ...
    if ((ret64 = avio_seek(pb, ctx->header_size, SEEK_SET)) < 0)   /* was DS2_HEADER_SIZE */
        return (int)ret64;
    ...
    if (file_size > ctx->header_size && s->duration > 0)          /* was DS2_HEADER_SIZE */
```

**4. `ds2_count_total_frames`, `ds2_find_next_nonempty_swap`, and the packet
reader** — replace the macro with the context value:

```c
    int header_size = ((DS2DemuxContext *)s->priv_data)->header_size;
    ...
    blocks = (size - header_size) / DS2_BLOCK_SIZE;
    avio_seek(pb, header_size + (int64_t)i * DS2_BLOCK_SIZE + 2, SEEK_SET);
```

`DS2_HEADER_SIZE` can stay as a documented default (`0x600`) but is no longer
used for offset math.

The format-type / SP-vs-QP branch is unchanged: `ctx->format_type =
block_header[4]` is read from the *first audio block*, which is now correctly
located via `header_size`.

## Validation done on the reference (Rust) side

The identical generalization in the Rust reference decodes a real GR/PH DS2-QP
dictation **bit-exact vs the licensed Olympus decoder** (corr 1.000000, 68.8 dB,
1 958 144 samples). The C change mirrors it one-for-one.

## FATE

Needs a public GR/PH sample. The Grundig `.dss` attached to hirparak/dss-codec#11
is public and exercises the DSS-SP path (version 6); a short synthetic/redacted
`.ds2` (version 7) covers the QP path. Reference framecrc generated from the Rust
decoder, same as the existing `fate-ds2-qp`.
