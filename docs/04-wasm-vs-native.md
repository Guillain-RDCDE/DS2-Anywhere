# 04 — WASM vs native

> Why we shipped with WASM first, why we then switched to native a day later, and what the performance numbers actually look like. A small case study in not over-optimizing on day one.

The upstream codec ships in two forms:

- A **WASM module** (`dss-codec` on npm) — usable from Node.js or a browser.
- A **native Rust crate** that compiles to a standalone CLI binary.

When we shipped this pipeline, we started with the WASM chain. A day later we switched to native. This chapter explains why both, why in that order, and what we measured.

---

## Why WASM first

The WASM module is what we discovered first. The upstream wrapper repo is structured around it — there's a polished demo, a published npm package, well-documented Node.js entry points. It works in five minutes:

```bash
npm install dss-codec @breezystack/lamejs
node script.mjs input.ds2
```

A first-cut conversion pipeline in a single Node script. No build tools, no compiler, no system dependencies beyond `node` itself.

For a project where the goal is *to know if the codec works on our real data*, that's exactly what you want. You don't want to spend a day setting up Rust toolchains before you've even run a single conversion on a real file. So we shipped the WASM chain, validated 35 real files, ran the A/B against Switch, and put it in production.

It worked. End-to-end. Real production traffic. Whisper-equivalent quality.

## Why we then switched to native

The WASM chain has one operational quirk: it's **slow**.

```
Test file: 31.8-minute DS2 QP dictation, 6.4 MB

WASM chain (Node + dss-codec + lamejs):
  decode  + MP3 encode  = 33 seconds total

Native chain (dss-decode-native + ffmpeg/libmp3lame):
  decode (6 s) + MP3 encode (~0.5 s) = ~10 seconds total
```

**~3.3× faster on the same file**, on the same hardware. For a single conversion, the difference is 23 seconds — not earth-shattering. For a backlog (we had 22 stuck files when we first turned the new system on), it would have been the difference between 12 minutes and 4 minutes of catch-up time. Still manageable. But:

- Under load (50 files arriving in a burst), 50 × 33 s = 27 minutes vs 50 × 10 s = 8 minutes. That matters.
- Lower CPU consumption per file frees the server for other things.
- A 5 MB native binary with libc-only dependencies is operationally simpler than a Node runtime + 15 npm packages.
- Switching is reversible in one line: the wrapper bash script changes from `exec node ...` to `exec dss-decode-native ...`.

So we switched, on day two. Took ~3 hours including the validation re-run.

## Where the speed difference comes from

Two main sources, in order of magnitude:

### 1. The MP3 encoder (the big one)

The WASM chain uses **lamejs** — an MP3 encoder rewritten in pure JavaScript by the lame.js authors. It's a faithful port of the C LAME encoder, but it's pure JS. No native code, no SIMD, no asm.

The native chain uses **libmp3lame** via ffmpeg — the original C LAME encoder, with decades of hand-tuned assembly for x86_64 SIMD instructions (SSE, SSE2, AVX).

On a 31-minute speech file:

- lamejs: ~20-25 seconds to encode to 64 kbps mono
- libmp3lame: ~0.3-0.5 seconds to encode the same

That's a **~50× speedup** on the encoder step alone. About 70 % of the WASM chain's total wall-clock comes from this single bottleneck.

### 2. The decoder (smaller but real)

The WASM-compiled Rust decoder runs in V8's WebAssembly VM. WASM's SIMD support is limited to 128-bit operations (roughly equivalent to SSE2 from the early 2000s) and adds bounds-checks on every memory access. The native binary compiles for the host CPU with all available instruction sets (AVX2 on a modern Ryzen) and skips the bounds checks.

For a CELP decoder — which is dominated by short floating-point multiply-accumulate loops over the synthesis filter and the codebook — AVX2 vs SSE2 is a real win.

- WASM decoder: ~10-12 seconds for the 31-min file
- Native decoder: ~6 seconds for the same

About 2× speedup on decode. Less dramatic than the encoder, but it adds up.

## What stayed the same

After the switch, **the wrapper bash script's interface is identical**. Same arguments, same exit codes, same output format. The cron didn't need to know anything had changed. The web UI didn't need to know. The downstream pipeline didn't need to know.

Internally:

- **Before**: bash wrapper → `node lib/cli.mjs` → imports `dss-codec` WASM → runs in-process → calls `lamejs` (JS) → writes MP3.
- **After**: bash wrapper → `dss-decode-native` (subprocess) → writes WAV → ffmpeg (subprocess) → writes MP3.

Two extra `fork()` calls per conversion (spawning the binaries vs running in-process). On a modern Linux, `fork()` is ~5 ms. Compared to the 10-second conversion, completely negligible.

## The HTTP daemon kept Node

The web UI's PHP backend calls a small Node.js HTTP daemon for synchronous on-demand conversions. After the switch to native, **the daemon kept using Node** — it just spawns the native binary as a subprocess for each request, instead of running the WASM in-process.

Why? Because Node is already a perfectly fine HTTP server, and rewriting the daemon in Go or Rust to "remove" the Node dependency would have been weeks of work for zero observable benefit. The daemon is a 100-line bridge that handles request routing and subprocess management; it's the wrong place to optimize.

## What we kept from the WASM days

The WASM version is still on disk:

```
lib/core-wasm.mjs.bak              # the JS module that called the WASM directly
bin/conv-dss-ds2-to-mp3.wasm.bak   # the bash wrapper for the WASM chain
```

A single `mv` switches back. We've never needed to. But the option is there, for the same reason every safety net in this project is there — because production systems eventually surprise you, and the rollback path should already exist when they do.

## When WASM is the right answer

This project ended up native because we control our deployment environment (one Linux server farm, x86_64). For other use cases, WASM is the obvious choice:

- **Browser-side conversion** — no native binary will ever run client-side; WASM is the only option.
- **Cross-platform tools** — a Node CLI distributed via npm is portable across Linux/macOS/Windows/ARM with zero per-platform builds. The native binary requires one build per target architecture.
- **Embedded in a larger Node application** — sometimes you just want to convert a DS2 in the middle of an existing JS pipeline without spawning external processes.
- **Sandboxed environments** — serverless functions, edge workers, or any place native binaries are difficult to deploy.

The fact that we *can* run native here is a consequence of our setup. If you're shipping a SaaS where users upload DS2 files in their browser, WASM is the correct choice; the speed difference is irrelevant compared to the network round-trip.

## Mini-table: pick your tradeoff

| Scenario | Use |
|---|---|
| Linux server, batch processing, perf matters | Native binary |
| Browser-side conversion | WASM |
| Cross-platform Node CLI | WASM (npm install just works everywhere) |
| Embedded in existing Node app | WASM (no subprocess) |
| Serverless / edge function | WASM (no native binaries allowed) |
| Container-based microservice | Either — native if perf matters, WASM if simpler deploy matters |

## The meta-lesson

There's a principle this story illustrates that's worth naming.

**Ship the simple thing first, even if it's not the fastest. Measure. Only then optimize.**

If we'd insisted on going native on day one, we'd have spent the first day fighting cargo, build-essential, the linker, and the Rust toolchain instead of validating that the codec produces correct audio. The WASM chain let us answer the *risky* question first: does this thing actually work on our data?

Once "yes, it works" was settled, switching to native was a contained optimization — same interface, same outputs, just faster. Three hours of work, including the re-validation. No drama.

Doing it the other way around would have meant fighting the perf-tuning toolchain *while* also being uncertain whether the codec would ultimately work. Two unknowns at the same time is always worse than one then the other.

---

Next: **[05 — Lessons learned](05-lessons-learned.md)** — the bugs we ate, the gotchas we got bitten by, and the operational quirks worth knowing if you do this for your own pipeline.

---

*Five times the speed for the price of one rewrite. ⚡*
