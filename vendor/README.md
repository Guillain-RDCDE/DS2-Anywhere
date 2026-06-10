# vendor/ — the Rust decoder crates (bundled for reproducible builds)

These are **vendored copies** of the Rust codec, bundled in-tree so the
[web/](../web/) WebAssembly module rebuilds in GitHub Actions with no external
submodule or fork dependency (a force-push or rename upstream can't break our CI).

| Dir | Origin | License |
|---|---|---|
| `dss-codec/` | [`gaspardpetit/dss-codec`](https://github.com/gaspardpetit/dss-codec) (a fork of [`hirparak/dss-codec`](https://github.com/hirparak/dss-codec)) — Olympus DSS/DS2 SP+QP and **encrypted DS2** (AES-128/256), **plus our Grundig DSS-SP decoder** ported from [hirparak/dss-codec PR #12](https://github.com/hirparak/dss-codec/pull/12) | MIT |
| `dss-codec-wasm/` | [`gaspardpetit/dss-codec-wasm`](https://github.com/gaspardpetit/dss-codec-wasm) — the wasm-bindgen wrapper | MIT |

The only change from upstream wiring: `dss-codec-wasm/Cargo.toml` points its
`dss-codec` dependency at `../dss-codec` (the vendored copy) instead of a git
submodule, and `format_name()` gains a `GrundigSp` arm. The Grundig codec
(`dss-codec/src/{codec,demux,tables}/grundig*.rs`) is our own clean-room
reimplementation, verified bit-exact against the vendor decoder — see
[`docs/SPEC-grundig-dss-sp.md`](../docs/SPEC-grundig-dss-sp.md).

Upstream attribution and the full credit chain: [`CREDITS.md`](../CREDITS.md).

To track upstream, re-pull the relevant crate and re-apply the two changes above.
