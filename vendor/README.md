# vendor/ — the Rust decoder crates (bundled for reproducible builds)

These are **vendored copies** of the Rust codec, bundled in-tree so that both
decoders this repo ships are built from the same source, with no external
submodule or fork dependency (a force-push or rename upstream can't break our CI):

- the native `dss-decode` binary in the Docker image ([`Dockerfile`](../Dockerfile));
- the WebAssembly module behind the [web/](../web/) page ([`pages.yml`](../.github/workflows/pages.yml)).

CI decodes the sample files with both and requires identical samples
([`scripts/codec-parity.mjs`](../scripts/codec-parity.mjs)).

| Dir | Origin | License |
|---|---|---|
| `dss-codec/` | [`Guillain-RDCDE/dss-codec`](https://github.com/Guillain-RDCDE/dss-codec) `main` (sources as of commit `5532715`), our fork of [`hirparak/dss-codec`](https://github.com/hirparak/dss-codec) — Olympus DSS/DS2 SP+QP, **encrypted DS2** (AES-128/256, contributed upstream by Gaspard Petit), **our Grundig DSS-SP decoder**, and our DSS fixes (block-header framing, 11000 Hz, header recovery) | MIT, © Kieran Hirpara ([LICENSE](dss-codec/LICENSE)) |
| `dss-codec-wasm/` | [`gaspardpetit/dss-codec-wasm`](https://github.com/gaspardpetit/dss-codec-wasm) — the wasm-bindgen wrapper | MIT, © Gaspard Petit ([LICENSE](dss-codec-wasm/LICENSE)) |

Differences from those origins:

- `dss-codec/`: the `src/` tree is the fork's, run through `cargo fmt` (whitespace
  only: CI checks formatting, and the tests and the native/web parity check show the
  output unchanged); `Cargo.toml` adds a `[lints.clippy]` table saying which lints are
  left to upstream and why. This copy's `README.md` and `CODEC_SPECIFICATION.md` carry
  the 11000 Hz correction, and its DSS SP integration test expects 11000 Hz. The
  integration tests decode the small public samples in [`test_files/`](test_files/)
  and [`dss-codec/tests/fixtures/`](dss-codec/tests/fixtures/).
- `dss-codec-wasm/`: `Cargo.toml` points its `dss-codec` dependency at `../dss-codec`
  (the vendored copy) instead of a git submodule, and `format_name()` also covers
  `DssLp`, a format only the vendored crate knows; the sources are `cargo fmt`-formatted
  and `Cargo.toml` allows one clippy lint, with its reason.

The Grundig codec (`dss-codec/src/{codec,demux,tables}/grundig*.rs`) is our own
clean-room reimplementation, verified bit-exact against the vendor decoder — see
[`docs/SPEC-grundig-dss-sp.md`](../docs/SPEC-grundig-dss-sp.md).

Upstream attribution and the full credit chain: [`CREDITS.md`](../CREDITS.md) and [`NOTICE`](../NOTICE).

To move the codec: re-copy `dss-codec/src` from the fork at the new commit, run
`cargo fmt`, update the commit above, and let CI re-check the native/web parity.
