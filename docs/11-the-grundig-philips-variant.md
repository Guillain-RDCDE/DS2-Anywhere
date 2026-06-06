# 11 — The Grundig/Philips variant (the header that wasn't 0x600)

Every file we'd seen — Olympus DS-series — opens with `\x03ds2` (or `\x02dss`/
`\x03dss`) and stores its first **audio** block at offset `0x600`. The decoder
hard-codes that: header = `0x600`, audio right after.

Then a real production dictation came in (a notarial constat) that the chain
refused with:

```
$ dss-decode-native DICT1528_0.ds2 out.wav
Error decoding ...: unsupported DS2 format type: 7
```

`7` is the **first byte** — `0x07`, not `0x03`. ffmpeg's DSS demuxer rejected it
too (`Invalid data found`). But it was not corrupt and not encrypted (bytes 1..3
are still `ds2`, an encrypted file would be `\x03enc`). NCH Switch decoded it
perfectly.

## What it actually is

A hexdump told the story. The header carries a device tag **`GR/PH9607`** —
**Grundig/Philips**, the other two IVA/DSS-consortium vendors alongside Olympus —
and where Olympus puts its first audio block (`0x600`) this file has a chain of
`0xFF`-padded device-id records:

```
0x000–0x600  metadata header (author, dates, GR/PH9607 tag)
0x600        GR___9607_<uuid>   ┐
0x800        GR___0504_000000   │  4 × 512-byte device records
0xa00        GR___0607_...      │
0xc00        GR___1008_...      ┘
0xe00 … EOF  848 audio blocks   ← ordinary DS2-QP (block hdr 0f 03 0a ff 06 ff)
```

The audio starts at `0xe00`. And `0xe00 = 7 × 512`.

That's the whole secret: **the first byte is the header size in 512-byte
blocks.** Olympus `.ds2` = 3 → `0x600`. This recorder = 7 → `0xe00`. The `.dss`
side already worked this way upstream (`header = version * 512`); DS2 had simply
hard-coded version 3. The same recorder's `.dss` files start with `0x06`
(6 × 512 = `0xc00`) — which is exactly issue
[hirparak/dss-codec#11](https://github.com/hirparak/dss-codec/issues/11), a
Grundig Digta 415 reporting `unsupported DS2 format type: 6`. Same device, two
modes.

## The fix

We **don't** touch the codec — the CELP frames are bog-standard DS2-QP /
DSS-SP. We normalize the *container* in front of the decoder
([`src/lib/grph.mjs`](../src/lib/grph.mjs)): keep the `0x600` metadata header,
reset the version byte to `3`, drop the `GR___` records, and concatenate the
audio. The existing decoder then handles it unchanged.

```
keep data[0x000 .. 0x600]   (set byte0 = 0x03)
append data[version*512 ..]  (the audio blocks)
```

## Proof

Decoded the normalized file with the native binary and compared, sample for
sample, against the NCH Switch (licensed Olympus) decode of the same `.ds2`:

| metric | value |
|---|---|
| samples | 1 958 144 / 1 958 144 (identical) |
| correlation | **1.000000** |
| SNR vs Switch | **68.8 dB** |

The normalized bytes are also bit-identical to a hand-built transcode, and all
three conversion paths (native, WASM, bash CLI) now accept the raw file. The
Switch/Windows detour is no longer needed for GR/PH recorders either.

> One caveat inherited from the QP path: correct decoding of *paused* GR/PH
> recordings also relies on the empty-block / `byte1` re-anchoring fix
> ([07](07-cracking-the-resync-block.md)) — the same rule that paused Olympus QP
> files need. With both in place, raw GR/PH files decode bit-exact.
