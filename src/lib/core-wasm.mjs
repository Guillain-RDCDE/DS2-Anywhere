// Core conversion module (WASM fallback chain).
// Uses the dss-codec npm package (Hirpara's codec, WASM build by Petit) for the
// decode, and lamejs (pure-JS encoder) for the MP3 encode. Entirely in-process,
// no subprocess, no native binary. Slower than the native chain (~3-5x) but
// portable across any Node 18+ environment.
//
// Kept here as a documented fallback. To activate, replace lib/core.mjs with
// this file (or change the import in bin/http_server.mjs).
//
// Install dependencies:
//   npm install dss-codec @breezystack/lamejs

import { readFile, writeFile } from "node:fs/promises";
import { decode, decodeWithPassword, inspect } from "dss-codec";
import lamejs from "@breezystack/lamejs";
import { normalizeGrph } from "./grph.mjs";

/**
 * Inspect a DS2/DSS file without fully decoding it.
 * @returns {Promise<{format:string, encryption:string, nativeRate:number, bytes:number}>}
 */
export async function inspectFile(path) {
  let bytes = new Uint8Array(await readFile(path));
  bytes = normalizeGrph(bytes) || bytes; // GR/PH -> Olympus layout
  const head = bytes.subarray(0, Math.min(bytes.length, 4096));
  const ins = inspect(head);
  const info = {
    format: ins.format,
    encryption: ins.encryption,
    nativeRate: ins.nativeRate,
    bytes: bytes.length,
  };
  ins.free();
  return info;
}

/**
 * Convert a DS2/DSS file to MP3 (64 kbps mono by default).
 * @param {string} inPath
 * @param {string} outPath
 * @param {{bitrate?:number, password?:string|null}} opts
 * @returns {Promise<{format:string, sampleRate:number, samples:number, duration_s:number, mp3_bytes:number}>}
 */
export async function convertFile(inPath, outPath, { bitrate = 64, password = null } = {}) {
  let bytes = new Uint8Array(await readFile(inPath));
  bytes = normalizeGrph(bytes) || bytes; // GR/PH -> Olympus layout

  let result;
  if (password) {
    const pwd = new TextEncoder().encode(password);
    result = decodeWithPassword(bytes, pwd);
  } else {
    result = decode(bytes);
  }
  const pcm = result.samples.slice();
  const sampleRate = result.nativeRate;
  const format = result.format;
  result.free();

  // PCM Float32 [-1, 1] -> Int16
  const i16 = new Int16Array(pcm.length);
  for (let i = 0; i < pcm.length; i++) {
    const x = Math.max(-1, Math.min(1, pcm[i]));
    i16[i] = (x * 32767) | 0;
  }

  // Block-by-block MP3 encode (lamejs)
  const enc = new lamejs.Mp3Encoder(1, sampleRate, bitrate);
  const blockSize = 1152;
  const chunks = [];
  for (let i = 0; i < i16.length; i += blockSize) {
    const c = i16.subarray(i, Math.min(i + blockSize, i16.length));
    const out = enc.encodeBuffer(c);
    if (out.length > 0) chunks.push(out);
  }
  const tail = enc.flush();
  if (tail.length > 0) chunks.push(tail);

  let total = 0;
  for (const c of chunks) total += c.length;
  const merged = new Uint8Array(total);
  let off = 0;
  for (const c of chunks) {
    merged.set(c, off);
    off += c.length;
  }
  await writeFile(outPath, merged);

  return {
    format,
    sampleRate,
    samples: pcm.length,
    duration_s: pcm.length / sampleRate,
    mp3_bytes: merged.length,
  };
}
