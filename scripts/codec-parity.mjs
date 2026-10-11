#!/usr/bin/env node
// Decode the same files with the native CLI (dss-decode, as built by the
// Dockerfile) and with the WebAssembly module (as built for the web page), and
// require identical samples.
//
//   node scripts/codec-parity.mjs <native dss-decode> <wasm-bindgen --target web out dir> <file>...
//
// Both decoders run the same crate (vendor/dss-codec). The native CLI is asked
// for a 32-bit float WAV, whose samples are `(s / 32768) as f32`; the wasm
// module returns `(s / 32768).clamp(-1, 1) as f32`. After applying the same
// clamp to the native side, the two Float32 streams must match byte for byte.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [native, pkgDir, ...files] = process.argv.slice(2);
if (!native || !pkgDir || files.length === 0) {
  console.error("usage: codec-parity.mjs <dss-decode> <wasm pkg dir> <file>...");
  process.exit(2);
}

// The wasm-bindgen glue is an ES module: give it a package.json saying so.
const work = mkdtempSync(join(tmpdir(), "codec-parity-"));
for (const f of readdirSync(pkgDir)) copyFileSync(join(pkgDir, f), join(work, f));
writeFileSync(join(work, "package.json"), '{"type":"module"}\n');
const wasm = await import(pathToFileURL(join(work, "dss_codec_wasm.js")).href);
wasm.initSync({ module: readFileSync(join(work, "dss_codec_wasm_bg.wasm")) });

// Minimal RIFF reader: returns { rate, format tag, bits, data bytes }.
function readWav(buf) {
  if (buf.toString("ascii", 0, 4) !== "RIFF" || buf.toString("ascii", 8, 12) !== "WAVE") {
    throw new Error("not a WAV file");
  }
  let pos = 12;
  let fmt = null;
  while (pos + 8 <= buf.length) {
    const id = buf.toString("ascii", pos, pos + 4);
    const size = buf.readUInt32LE(pos + 4);
    const body = pos + 8;
    if (id === "fmt ") {
      fmt = { tag: buf.readUInt16LE(body), channels: buf.readUInt16LE(body + 2),
              rate: buf.readUInt32LE(body + 4), bits: buf.readUInt16LE(body + 14) };
    } else if (id === "data") {
      return { ...fmt, data: buf.subarray(body, body + size) };
    }
    pos = body + size + (size & 1);
  }
  throw new Error("no data chunk");
}

const sha = (b) => createHash("sha256").update(b).digest("hex").slice(0, 16);
let failed = 0;

for (const file of files) {
  const name = basename(file);
  const input = readFileSync(file);

  const out = join(work, "native.wav");
  execFileSync(resolve(native), ["-q", "-b", "32", "-O", out, file], { stdio: "inherit" });
  const wav = readWav(readFileSync(out));
  const nat = new Float32Array(wav.data.buffer.slice(wav.data.byteOffset, wav.data.byteOffset + wav.data.length));
  let clamped = 0;
  for (let i = 0; i < nat.length; i++) {
    if (nat[i] > 1) { nat[i] = 1; clamped++; } else if (nat[i] < -1) { nat[i] = -1; clamped++; }
  }

  const res = wasm.decode(new Uint8Array(input));
  const web = res.samples;
  const webRate = res.nativeRate;
  const webFormat = res.format;
  res.free();

  const a = Buffer.from(nat.buffer);
  const b = Buffer.from(web.buffer, web.byteOffset, web.byteLength);
  // 3 = IEEE float; 0xFFFE = WAVE_FORMAT_EXTENSIBLE, which hound uses for 32-bit float.
  const isFloat = (wav.tag === 3 || wav.tag === 0xfffe) && wav.bits === 32;
  if (!isFloat) throw new Error(`${name}: native WAV is not 32-bit float (tag ${wav.tag}, ${wav.bits} bits)`);
  const same = wav.channels === 1 && wav.rate === webRate && a.equals(b);
  let firstDiff = -1;
  if (!a.equals(b)) {
    const n = Math.min(nat.length, web.length);
    for (let i = 0; i < n; i++) if (Object.is(nat[i], web[i]) === false) { firstDiff = i; break; }
  }
  console.log(
    `${same ? "same" : "DIFF"}  ${name}  ${webFormat} @ ${webRate} Hz  ` +
    `native ${nat.length} samples @ ${wav.rate} Hz sha256:${sha(a)}  ` +
    `wasm ${web.length} samples sha256:${sha(b)}` +
    (clamped ? `  (${clamped} native samples clamped to [-1, 1])` : "") +
    (firstDiff >= 0 ? `  first difference at sample ${firstDiff}` : ""),
  );
  if (!same) failed++;
}

rmSync(work, { recursive: true, force: true });
if (failed) {
  console.error(`${failed} of ${files.length} file(s) decode differently`);
  process.exit(1);
}
console.log(`native and wasm decoders agree on all ${files.length} file(s)`);
