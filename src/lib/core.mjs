// Core conversion module (native chain) — used by the HTTP daemon.
// Spawns dss-decode-native (Rust binary) for the DS2/DSS decode,
// then ffmpeg/libmp3lame for the MP3 encode.
//
// The standalone bash CLI (bin/conv-dss-ds2-to-mp3) does the same thing
// directly without going through Node. This module exists so the HTTP
// daemon can do conversions in-process without spawning the bash wrapper.

import { writeFile, readFile, unlink, stat } from "node:fs/promises";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomBytes } from "node:crypto";

const NATIVE = "/usr/local/bin/dss-decode-native";
const FFMPEG = "/usr/bin/ffmpeg";

// Magic bytes detection (see upstream crypto/ds2_encrypted.rs + demux/ds2.rs)
function detectEncryptionFromHeader(headBuf) {
  if (headBuf.length < 4) return "unknown";
  const a = headBuf[0], b = headBuf[1], c = headBuf[2], d = headBuf[3];
  // \x03ds2 = 03 64 73 32
  if (a === 0x03 && b === 0x64 && c === 0x73 && d === 0x32) return "none";
  // \x03dss = 03 64 73 73
  if (a === 0x03 && b === 0x64 && c === 0x73 && d === 0x73) return "none";
  // \x03enc = 03 65 6e 63
  if (a === 0x03 && b === 0x65 && c === 0x6e && d === 0x63) return "ds2_aes";
  return "unknown";
}

function run(cmd, args, { input } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, { stdio: input ? ["pipe", "pipe", "pipe"] : ["ignore", "pipe", "pipe"] });
    let out = "", err = "";
    child.stdout.on("data", (d) => (out += d.toString()));
    child.stderr.on("data", (d) => (err += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout: out, stderr: err }));
    if (input) {
      child.stdin.end(input);
    }
  });
}

/**
 * Inspect a DS2/DSS file without fully decoding it.
 * @returns {Promise<{format:string, encryption:string, nativeRate:number, bytes:number}>}
 */
export async function inspectFile(path) {
  const st = await stat(path);
  const fd = await readFile(path, { encoding: null });
  const head = fd.subarray(0, Math.min(fd.length, 16));
  const encryption = detectEncryptionFromHeader(head);

  const info = await run(NATIVE, ["--info", path]);
  if (info.code !== 0) {
    // --info often fails on encrypted files — return what we know
    return { format: "", encryption, nativeRate: 0, bytes: st.size };
  }
  // Parse "<path>: Ds2Qp, native rate 16000 Hz"
  const line = (info.stdout || "").split("\n")[0] || "";
  const m = line.match(/:\s*([A-Za-z0-9]+),\s*native rate\s+(\d+)\s*Hz/);
  let format = "", nativeRate = 0;
  if (m) {
    const raw = m[1].toLowerCase();
    format = raw === "ds2qp" ? "ds2_qp"
           : raw === "ds2sp" ? "ds2_sp"
           : raw === "dsssp" ? "dss_sp"
           : raw;
    nativeRate = parseInt(m[2], 10);
  }
  return { format, encryption, nativeRate, bytes: st.size };
}

/**
 * Convert a DS2/DSS file to MP3 (64 kbps mono by default).
 * @param {string} inPath
 * @param {string} outPath
 * @param {{bitrate?:number, password?:string|null}} opts
 * @returns {Promise<{format:string, sampleRate:number, samples:number, duration_s:number, mp3_bytes:number}>}
 */
export async function convertFile(inPath, outPath, { bitrate = 64, password = null } = {}) {
  const tmpWav = join(tmpdir(), `core_dec_${randomBytes(6).toString("hex")}.wav`);
  try {
    // Phase 1: decode -> WAV
    const decArgs = ["-O", tmpWav];
    if (password) decArgs.push("--password", password);
    decArgs.push(inPath);
    const dec = await run(NATIVE, decArgs);
    if (dec.code !== 0) {
      const msg = (dec.stderr || dec.stdout || "").trim();
      throw new Error("decode failed: " + (msg || "code " + dec.code));
    }
    // Re-inspect for output stats
    const wavInfo = await inspectFile(inPath);
    const wavStat = await stat(tmpWav);
    // Sample estimation: (wav_bytes - 44 header) / 2 bytes per sample (16-bit mono)
    const samples = Math.max(0, Math.floor((wavStat.size - 44) / 2));
    const durationS = wavInfo.nativeRate > 0 ? samples / wavInfo.nativeRate : 0;

    // Phase 2: encode MP3 via ffmpeg
    const encArgs = [
      "-y", "-loglevel", "error",
      "-i", tmpWav,
      "-ac", "1",
      "-c:a", "libmp3lame",
      "-b:a", `${bitrate}k`,
      outPath,
    ];
    const enc = await run(FFMPEG, encArgs);
    if (enc.code !== 0) {
      const msg = (enc.stderr || enc.stdout || "").trim();
      throw new Error("mp3 encode failed: " + (msg || "code " + enc.code));
    }
    const mp3Stat = await stat(outPath);
    return {
      format: wavInfo.format,
      sampleRate: wavInfo.nativeRate,
      samples,
      duration_s: durationS,
      mp3_bytes: mp3Stat.size,
    };
  } finally {
    await unlink(tmpWav).catch(() => {});
  }
}
