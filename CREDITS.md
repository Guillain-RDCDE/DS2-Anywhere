# Credits

> Where the work in this project actually came from. The honest version.

This repo is an integration recipe. It would not exist — *could not* exist — without the work of others. Crediting them clearly, and in the right order, is part of the deal.

---

## The codec — the genius part

### Kieran Hirpara — [hirparak/dss-codec](https://github.com/hirparak/dss-codec)

The reverse-engineering of the Olympus DSS / DS2 codec, published **February 2026**, MIT licensed. ~4 400 lines of Rust + a Python reference decoder + a complete written specification + byte-for-byte verification against the official Olympus DirectShow filter.

Before this work, DS2 was undecodable on any open-source stack. After this work, anyone can integrate DS2 audio into a Linux pipeline. The entire premise of this repo rests on it.

The full story of how this work was done is in [docs/01-reverse-engineering.md](docs/01-reverse-engineering.md).

### Oleksij Rempel — the DSS SP decoder in FFmpeg, 2014

Twelve years before any of the rest of this, Oleksij Rempel contributed `libavcodec/dss_sp.c`
and `libavformat/dss.c` to FFmpeg — the first and, until 2026, the only open decoder for
Olympus DSS SP. Anyone who has ever opened a `.dss` with VLC, ffmpeg, or anything built on
them has used his work without knowing it.

We spent August 2026 finding a bug in that corner and were, for a while, quite pleased with
ourselves. It is worth being precise about what we actually found: **the defect was in the
demuxer's framing walk, not in his codec**, and it only ever bit on recordings that had been
paused or edited on the device. His decoder was correct, and had been correct on every file
anyone tested for twelve years. Our contribution there is a hundred lines on top of a
foundation somebody else laid and maintained for free.

### Gaspard Petit — [gaspardpetit/dss-codec-wasm](https://github.com/gaspardpetit/dss-codec-wasm) and [gaspardpetit/dss-codec](https://github.com/gaspardpetit/dss-codec)

The WASM build of the codec (`dss-codec-wasm`), the npm packaging, the JavaScript bindings, the streaming decoder API, the wasm-bindgen wrapper. MIT licensed. This is what made the codec usable from any JS/TS project (Node, browser, edge worker) without needing a Rust toolchain.

Gaspard also maintains a [fork of Hirpara's Rust crate](https://github.com/gaspardpetit/dss-codec) with CI workflows, streaming decode, and 128/256-bit decryption support. That work has since been merged into Hirpara's upstream, and our native `dss-decode` binary (Dockerfile) and the in-browser decoder are now both built from the same vendored crate, [`vendor/dss-codec`](vendor/dss-codec/) — a copy of [our fork](https://github.com/Guillain-RDCDE/dss-codec) of Hirpara's upstream, which carries Gaspard's work plus our DSS fixes.

The WASM build was our first integration target. We later switched to the native binary for performance reasons (see [docs/04-wasm-vs-native.md](docs/04-wasm-vs-native.md)), but the WASM is what got us off the ground in a few hours, and it's the chain we kept as a documented fallback.

### Patrick Domack — FFmpeg C port ([gist `330dd3f5…`](https://gist.github.com/patrickdk77/330dd3f593696d103e831c4c1d78d1f9))

The hand-written C port of the DS2 decoder + demuxer for FFmpeg — `libavcodec/ds2.c` (982 lines) + `libavformat/ds2.c` (369 lines). The CELP algorithm (decode loops, pitch synthesis filter, frame parsing, demuxer) was implemented from the specification text in [FFmpeg trac #6091](https://trac.ffmpeg.org/ticket/6091); the numerical quantization tables (reflection codebooks, pitch and excitation gains, pulse amplitudes — ~4400 values) are sourced from Hirpara's reference Rust implementation, which originally extracted them from the Olympus `DssDecoder.dll` via Ghidra. Both the algorithm and the tables are MIT-licensed. Posted as a gist in 2026-03, explicitly relicensed under MIT / public-domain terms for upstream FFmpeg merge in [hirparak/dss-codec#1](https://github.com/hirparak/dss-codec/issues/1).

This is the work that closes the loop: once Patrick's C lands upstream, **DS2 / DS2 Pro becomes a first-class audio format in any FFmpeg build, anywhere**. No more decoder bundling, no more native binary in our Dockerfile, no more "first install this dependency". Just `ffmpeg -i recording.ds2 out.wav`.

Patrick has asked to stay off the `ffmpeg-devel` mailing list. The mailing-list submission (cover letter, FATE sample, validation campaign) is being prepared in this repo at [`ffmpeg-upstream/`](ffmpeg-upstream/) and will go out under the submitter's name, on Patrick's behalf, with his explicit consent. See [ffmpeg-upstream/README.md](ffmpeg-upstream/README.md) for the chain of credit going to FFmpeg.

**Also — the DSS SP compact-block fix ([hirparak/dss-codec#14](https://github.com/hirparak/dss-codec/pull/14)).** A third way a DSS SP recording encodes a pause: a short block padded with `0xFF`, which our own decoder read as audio and desynced the rest of the file on. Patrick found it and fixed the demux; we validated it against 90 days of production traffic and adopted it, gated so it touches only the files that were actually broken. The story is [chapter 14](docs/14-the-compact-block-pause.md).

**And the review that closed the loop — [issue #9](https://github.com/Guillain-RDCDE/DS2-Anywhere/issues/9), October 2026.** When `ffmpeg-devel` went quiet on our patches, Patrick rebuilt his own fork's DSS/DS2 support on top of them and scored every decoder against the *source* audio with PESQ — nine speakers, Olympus's own encoder, Olympus's two DLLs as reference points. He showed our lattice synthesis was worse than FFmpeg's original, found three real bugs in our patches (an ABI break, missing licence headers, an unsigned comparison that let EOF through), and fixed thirty more things on top: DS2 SP and LP decoded to noise, the frames of every pause block were dropped, variable rate SP and SpeechExec-split files did not open. Our decoder took all of it, sample for sample. The story is [chapter 18](docs/18-scored-against-the-source.md).

**And once more, the other direction — [PR #16](https://github.com/hirparak/dss-codec/pull/16).** Patrick folded two fixes from his FFmpeg work back into the Rust crate. One half — a combinatorial-index overflow fix — was clean and we kept it. The other rewrote the QP demux and removed the `byte1` re-sync anchor, garbling paused recordings. We shadow-benched the PR against 316 real files, wrote up the regression alongside a one-line fix for a separate `usize` panic we hit in the SP demux (`end.saturating_sub(spos)`), and built structure-only reproducers to hand over — block framing kept, every trace of voice erased. We never had to send them: Patrick traced his own side to a set of golden masters that had been corrupted by a stray test decode, pulled the demux rewrite, kept the codec fix, took the panic patch, and merged. The story is [chapter 15](docs/15-the-relay-runs-backward.md).

---

### ITU-T — Recommendation G.723.1, and the clean room behind our LP decoder

LP recordings carry **G.723.1** frames, an ITU-T standard: the Recommendation, its fixed-point reference and its test vectors are published by the ITU. Our LP decoder ([`vendor/dss-codec/src/codec/g723_1.rs`](vendor/dss-codec/src/codec/g723_1.rs), MIT) was written in a clean room. One side read the Recommendation and the reference and wrote [a specification](docs/g723-1/SPEC.md) with no code in it. The other side wrote the decoder from that and the Recommendation alone, and it matches the ITU test vectors sample for sample. The same decoder went to Kieran's crate in [hirparak/dss-codec#26](https://github.com/hirparak/dss-codec/pull/26). It replaces our earlier port of **FFmpeg's G.723.1 decoder**, by Benjamin Larsson (2006) and Mohamed Naufal Basheer (2010), which served LP here from 27 to 30 October 2026 and was the second reference our new decoder was checked against. The story is [chapter 19](docs/19-the-last-format.md).

## The supporting cast

### lamejs ([zhuker](https://github.com/zhuker/lamejs) / [@breezystack/lamejs](https://www.npmjs.com/package/@breezystack/lamejs))

Pure-JavaScript MP3 encoder. LGPL. Used in the optional WASM chain. ~50× slower than native libmp3lame, which is why we ultimately encode with ffmpeg — but for cases where you can't have native binaries (browsers, edge functions), lamejs is excellent.

### FFmpeg ([ffmpeg.org](https://ffmpeg.org/))

The native chain uses ffmpeg as the encoder backbone — `libmp3lame` for MP3 encoding specifically. LGPL/GPL. The encoder hot path is hand-tuned assembly that's been refined for 20+ years.

### jszip ([Stuk/jszip](https://github.com/Stuk/jszip))

Used by the optional browser-side UI to package multiple converted files into a single zip download. MIT / GPL-3.0 dual licensed.

### Ghidra ([ghidra-sre.org](https://ghidra-sre.org/))

The reverse-engineering toolkit released by the NSA in 2019. It's what made Hirpara's work tractable. The whole field of accessible binary reverse-engineering is in a different place because Ghidra exists and is free.

### The FFmpeg ticket #6091 archive

[FFmpeg trac #6091](https://trac.ffmpeg.org/ticket/6091) — opened in 2017, sat unimplemented for 9 years. We mention it because, in a way, that ticket sitting open is what made the whole situation noticeable. Someone reads a ticket like that and decides "this is silly, I'll do it". Hirpara may or may not have read that specific ticket — but the surface of unsolved problems it represents is exactly the kind of thing open-source eventually catches.

---

## Our part

The integration patterns documented in this repo, the validation campaign methodology, the cron / daemon / web UI design, the WASM-to-native migration analysis, and the lessons learned — those are our contribution. They're small relative to the codec work that made them possible, but they're real and they're documented honestly.

This repo is what one production team did with February 2026's gift. Nothing more, but also nothing less.

---

## How to credit this work

If you're using this repo as a starting point for your own integration, the appropriate credits in your project's README:

```markdown
- Codec spec & Rust reference: [hirparak/dss-codec](https://github.com/hirparak/dss-codec) (MIT)
- Rust crate fork (CI, streaming, decryption): [gaspardpetit/dss-codec](https://github.com/gaspardpetit/dss-codec) (MIT)
- WASM build: [gaspardpetit/dss-codec-wasm](https://github.com/gaspardpetit/dss-codec-wasm) (MIT)
- FFmpeg C port (upcoming): [patrickdk77 gist](https://gist.github.com/patrickdk77/330dd3f593696d103e831c4c1d78d1f9) (MIT / public domain)
- Integration patterns: [Guillain-RDCDE/DS2-Anywhere](https://github.com/Guillain-RDCDE/DS2-Anywhere) (MIT)
```

The codec authors are non-negotiable. Mentioning us is optional but appreciated.

---

*Standing on the shoulders of giants — and saying so out loud. 🙏*
