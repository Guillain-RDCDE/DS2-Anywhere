// Grundig / Philips ("GR/PH") DSS-Pro container normalizer.
//
// The Digital Speech Standard was defined by the IVA consortium — Olympus,
// Grundig and Philips. Olympus `.dss`/`.ds2` files start with a version byte of
// 2 or 3; the upstream codec (and our pipeline) assume a fixed 0x600 header with
// the first audio block right after it.
//
// Grundig/Philips recorders (header tag `GR/PH9607`, e.g. the Grundig Digta
// series — see hirparak/dss-codec#11) write the SAME CELP audio but frame it
// differently: the first byte is the header size in 512-byte blocks (6 for
// `.dss`, 7 for `.ds2`), and the extra blocks hold `0xFF`-padded `GR___`-tagged
// device-id records sitting exactly where Olympus stores the first audio block.
// A decoder expecting `\x03ds2` + audio-at-0x600 rejects the file with
// "unsupported DS2 format type: 6/7" (the value is the unexpected first byte).
//
// This rewrites such a file to a plain Olympus container — first byte 3, audio
// at 0x600 — so the existing decoder/demuxer handle it byte-for-byte unchanged.
// Verified against the licensed Olympus reference (NCH Switch): corr 1.0000,
// 68.8 dB on a real DS2-QP dictation.
//
// First byte = header size in 512-byte blocks is the actual format law (Olympus
// `.dss` already uses version*512 for its header); this only extends it to the
// `.ds2` side and the larger GR/PH header sizes.

import { readFile, writeFile, unlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomBytes } from "node:crypto";

const STD_HEADER = 0x600;
const BLOCK = 512;

/**
 * Normalize a GR/PH DSS/DS2 container to the standard Olympus layout.
 * Accepts a Uint8Array/Buffer of the whole file.
 * @returns {Uint8Array|null} normalized bytes, or null when the input is already
 *   standard (Olympus version 2/3) or not a DSS/DS2 file — decode the original.
 */
export function normalizeGrph(data) {
  if (data.length < 4) return null;
  // bytes 1..3 spell "ds2" or "dss"; byte 0 is the header size in 512-blocks.
  const isDs2 = data[1] === 0x64 && data[2] === 0x73 && data[3] === 0x32;
  const isDss = data[1] === 0x64 && data[2] === 0x73 && data[3] === 0x73;
  if (!isDs2 && !isDss) return null;
  const headerBlocks = data[0];
  if (headerBlocks <= 3 || headerBlocks > 16) return null; // Olympus 2/3 untouched
  const headerSize = headerBlocks * BLOCK;
  if (data.length < headerSize + BLOCK) return null;

  const out = new Uint8Array(STD_HEADER + (data.length - headerSize));
  out.set(data.subarray(0, STD_HEADER), 0); // keep the real metadata header...
  out[0] = 0x03;                            // ...but standardize the version byte
  out.set(data.subarray(headerSize), STD_HEADER); // audio after the GR___ records
  return out;
}

/**
 * Path-based helper for the native chain (which decodes from a file path):
 * if `inPath` is a GR/PH file, write its normalized form to a temp file and
 * return that path plus a cleanup function; otherwise return the path as-is.
 * @returns {Promise<{path:string, cleanup:()=>Promise<void>}>}
 */
export async function withNormalizedFile(inPath) {
  const data = new Uint8Array(await readFile(inPath));
  const norm = normalizeGrph(data);
  if (!norm) return { path: inPath, cleanup: async () => {} };
  const tmp = join(tmpdir(), `grph_${randomBytes(6).toString("hex")}.ds2`);
  await writeFile(tmp, norm);
  return { path: tmp, cleanup: async () => { await unlink(tmp).catch(() => {}); } };
}
