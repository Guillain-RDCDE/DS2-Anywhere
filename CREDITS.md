# Credits

> Where the work in this project actually came from. The honest version.

This repo is an integration recipe. It would not exist — *could not* exist — without the work of others. Crediting them clearly, and in the right order, is part of the deal.

---

## The codec — the genius part

### Kieran Hirpara — [hirparak/dss-codec](https://github.com/hirparak/dss-codec)

The reverse-engineering of the Olympus DSS / DS2 codec, published **February 2026**, MIT licensed. ~4 400 lines of Rust + a Python reference decoder + a complete written specification + byte-for-byte verification against the official Olympus DirectShow filter.

Before this work, DS2 was undecodable on any open-source stack. After this work, anyone can integrate DS2 audio into a Linux pipeline. The entire premise of this repo rests on it.

The full story of how this work was done is in [docs/01-reverse-engineering.md](docs/01-reverse-engineering.md).

### Gaspard Petit — [gaspardpetit/dss-codec-wasm](https://github.com/gaspardpetit/dss-codec-wasm)

The WASM build of `dss-codec`, the npm packaging, the JavaScript bindings, the streaming decoder API, the wasm-bindgen wrapper. MIT licensed. This is what made the codec usable from any JS/TS project (Node, browser, edge worker) without needing a Rust toolchain.

The WASM build was our first integration target. We later switched to the native binary for performance reasons (see [docs/04-wasm-vs-native.md](docs/04-wasm-vs-native.md)), but the WASM is what got us off the ground in a few hours, and it's the chain we kept as a documented fallback.

---

## The supporting cast

### lamejs ([zhuker](https://github.com/zhuker/lamejs) / [breezystack fork](https://github.com/breezystack/lamejs))

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
- Codec: [hirparak/dss-codec](https://github.com/hirparak/dss-codec) (MIT)
- WASM build: [gaspardpetit/dss-codec-wasm](https://github.com/gaspardpetit/dss-codec-wasm) (MIT)
- Integration patterns: [Guillain-RDCDE/DS2-Anywhere](https://github.com/Guillain-RDCDE/DS2-Anywhere) (MIT)
```

The codec authors are non-negotiable. Mentioning us is optional but appreciated.

---

*Standing on the shoulders of giants — and saying so out loud. 🙏*
