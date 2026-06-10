# web/ — in-browser decoder (GitHub Pages)

A static drag-and-drop page that decodes Olympus & Grundig dictation files
**entirely in the browser** — no upload, no install. One ~196 KB WebAssembly
module decodes all the formats:

| Drop this | Decoded as |
|---|---|
| `.dss` (Olympus) | DSS-SP @ 11025 Hz |
| `.ds2` (Olympus) | DS2 SP @ 12000 / QP @ 16000 Hz |
| `.ds2` 🔐 encrypted | AES-128/256 — prompts for the password |
| `.dss` (Grundig) | Grundig DSS-SP @ 16000 Hz (bit-exact) |

Output is a WAV you can play and download. Nothing leaves your machine.

## Files
- `index.html` / `main.js` — the page (vanilla, no framework).
- `pkg/` — the wasm-bindgen output (`dss_codec_wasm.js` + `_bg.wasm`). Committed
  for instant use; **rebuilt on every deploy** by `.github/workflows/pages.yml`
  from [`vendor/`](../vendor/).
- `.nojekyll` — so GitHub Pages serves `pkg/` (underscore-prefixed assets) as-is.

## Build the wasm locally
```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.118     # must match the crate
cargo build --manifest-path ../vendor/dss-codec-wasm/Cargo.toml --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir pkg --out-name dss_codec_wasm \
  ../vendor/dss-codec-wasm/target/wasm32-unknown-unknown/release/dss_codec_wasm.wasm
python3 -m http.server   # then open http://localhost:8000/web/
```

MP3 export is intentionally omitted here to keep the page dependency-free and
MIT-clean; use the CLI / pipeline for MP3.
