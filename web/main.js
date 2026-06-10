import init, { inspect, decode, decodeWithPassword } from "./pkg/dss_codec_wasm.js";

const ready = init();                 // fetch + instantiate the 196 KB wasm

const drop = document.getElementById("drop");
const fileInput = document.getElementById("file");
const out = document.getElementById("out");

const FRIENDLY = {
  grundig_sp: "Grundig DSS-SP",
  ds2_qp: "Olympus DS2 (Quality Play)",
  ds2_sp: "Olympus DS2 (Standard Play)",
  dss_sp: "Olympus DSS-SP",
};

drop.addEventListener("click", () => fileInput.click());
drop.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); fileInput.click(); } });
fileInput.addEventListener("change", e => { if (e.target.files[0]) handleFile(e.target.files[0]); });
["dragenter", "dragover"].forEach(ev => drop.addEventListener(ev, e => { e.preventDefault(); drop.classList.add("hot"); }));
["dragleave", "drop"].forEach(ev => drop.addEventListener(ev, e => { e.preventDefault(); drop.classList.remove("hot"); }));
drop.addEventListener("drop", e => { const f = e.dataTransfer.files && e.dataTransfer.files[0]; if (f) handleFile(f); });

function show(html) { out.style.display = "block"; out.innerHTML = html; }

async function handleFile(file) {
  show(`<div class="card"><span class="spin"></span>Decoding <b>${esc(file.name)}</b>…</div>`);
  try {
    await ready;
    const bytes = new Uint8Array(await file.arrayBuffer());

    let info;
    try { info = inspect(bytes); }
    catch (e) { return show(`<div class="card err">Not a recognized Olympus or Grundig dictation file.</div>`); }
    let format = info.format, rate = info.nativeRate, enc = info.encryption;
    if (info.free) info.free();

    let result;
    if (enc && enc !== "none") {
      const pw = prompt(`This file is encrypted (${prettyEnc(enc)}). Enter the password:`);
      if (pw === null) return show(`<div class="card muted">Cancelled — this file is encrypted and needs a password.</div>`);
      try { result = decodeWithPassword(bytes, new TextEncoder().encode(pw)); }
      catch (e) { return show(`<div class="card err">Wrong password, or the file could not be decrypted.</div>`); }
    } else {
      result = decode(bytes);
    }

    const pcm = result.samples;               // Float32Array (owned copy)
    const sr = result.nativeRate || rate;
    const fmt = result.format || format;
    if (result.free) result.free();

    const blob = new Blob([buildWav(pcm, sr)], { type: "audio/wav" });
    const url = URL.createObjectURL(blob);
    const seconds = pcm.length / sr;
    const dlName = file.name.replace(/\.[^.]+$/, "") + ".wav";
    const encTag = (enc && enc !== "none") ? ` · 🔐 ${prettyEnc(enc)} decrypted` : "";

    show(`
      <div class="card">
        <div class="row"><span class="k">File</span><span class="v">${esc(file.name)}</span></div>
        <div class="row"><span class="k">Format</span><span class="v fmt">${esc(FRIENDLY[fmt] || fmt)}${encTag}</span></div>
        <div class="row"><span class="k">Audio</span><span class="v">${sr.toLocaleString()} Hz · mono · ${fmtTime(seconds)}</span></div>
        <audio controls src="${url}"></audio>
        <div><a class="btn" href="${url}" download="${esc(dlName)}">Download WAV</a></div>
      </div>`);
  } catch (err) {
    show(`<div class="card err">Decode failed: ${esc(String((err && err.message) || err))}</div>`);
  }
}

function buildWav(samples, sampleRate) {
  const n = samples.length;
  const buf = new ArrayBuffer(44 + n * 2);
  const dv = new DataView(buf);
  const wr = (o, s) => { for (let i = 0; i < s.length; i++) dv.setUint8(o + i, s.charCodeAt(i)); };
  wr(0, "RIFF"); dv.setUint32(4, 36 + n * 2, true); wr(8, "WAVE");
  wr(12, "fmt "); dv.setUint32(16, 16, true); dv.setUint16(20, 1, true); dv.setUint16(22, 1, true);
  dv.setUint32(24, sampleRate, true); dv.setUint32(28, sampleRate * 2, true); dv.setUint16(32, 2, true); dv.setUint16(34, 16, true);
  wr(36, "data"); dv.setUint32(40, n * 2, true);
  let o = 44;
  for (let i = 0; i < n; i++) { const s = Math.max(-1, Math.min(1, samples[i])); dv.setInt16(o, s < 0 ? s * 0x8000 : s * 0x7fff, true); o += 2; }
  return buf;
}

function prettyEnc(e) { return String(e).replace("ds2_aes_", "AES-"); }
function fmtTime(s) { const m = Math.floor(s / 60), ss = Math.round(s % 60); return `${m}:${String(ss).padStart(2, "0")}`; }
function esc(s) { return String(s).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])); }
