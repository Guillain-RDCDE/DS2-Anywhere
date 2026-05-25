// Local HTTP daemon — bridges the PHP admin UI (inside its Docker container)
// to the native conversion chain on the host. Listens on 127.0.0.1:8765.
//
// Endpoints:
//   GET  /health         — liveness probe
//   POST /convert-upload — body = DS2/DSS bytes, optional ?password=X — returns MP3 binary
//   POST /convert-path   — JSON {input, output_dirs[], output_name[, password]}
//                          — converts then distributes the MP3 to each output_dir
//                          — chmod 666 + chown www-data:www-data on each output file
//                          — returns JSON {ok, format, duration_s, mp3_bytes, distributed[]}

import { createServer } from "node:http";
import { writeFile, readFile, unlink } from "node:fs/promises";
import { mkdirSync, copyFileSync, chmodSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomBytes } from "node:crypto";
import { execSync } from "node:child_process";
import { inspectFile, convertFile } from "../lib/core.mjs";

const PORT = 8765;
const HOST = "127.0.0.1";

function jsonResp(res, code, body) {
  const data = JSON.stringify(body);
  res.writeHead(code, { "Content-Type": "application/json; charset=utf-8", "Content-Length": Buffer.byteLength(data) });
  res.end(data);
}

async function collectBody(req, maxBytes = 200 * 1024 * 1024) {
  const chunks = [];
  let total = 0;
  for await (const c of req) {
    total += c.length;
    if (total > maxBytes) throw new Error("body too large");
    chunks.push(c);
  }
  return Buffer.concat(chunks);
}

function chownWww(path) {
  try { execSync(`chown www-data:www-data "${path}"`); } catch { /* tolerated */ }
}

const srv = createServer(async (req, res) => {
  const t0 = Date.now();
  let logLine = `[${new Date().toISOString()}] ${req.method} ${req.url}`;
  try {
    const url = new URL(req.url, "http://localhost");

    // --- /convert-upload: direct upload from the web UI ---
    if (req.method === "POST" && url.pathname === "/convert-upload") {
      const ext = (url.searchParams.get("ext") || "ds2").toLowerCase();
      if (!["ds2", "dss"].includes(ext)) return jsonResp(res, 400, { ok: false, error: "ext must be ds2 or dss" });
      const password = url.searchParams.get("password") || null;

      const buf = await collectBody(req);
      const tmpIn = join(tmpdir(), `upload_${randomBytes(6).toString("hex")}.${ext}`);
      const tmpOut = tmpIn.replace(/\.[^.]+$/, ".mp3");
      await writeFile(tmpIn, buf);
      try {
        const info = await inspectFile(tmpIn);
        if (info.encryption && info.encryption !== "none" && !password) {
          jsonResp(res, 200, { ok: false, encrypted: true, encryption: info.encryption });
          return;
        }
        const r = await convertFile(tmpIn, tmpOut, { password });
        const mp3 = await readFile(tmpOut);
        res.writeHead(200, {
          "Content-Type": "audio/mpeg",
          "Content-Length": mp3.length,
          "X-Format": r.format,
          "X-Duration-Sec": String(Math.round(r.duration_s)),
          "X-Sample-Rate": String(r.sampleRate),
        });
        res.end(mp3);
        logLine += `  -> 200 ${ext} ${Math.round(r.duration_s)}s ${(mp3.length / 1024).toFixed(0)}KB ${Date.now() - t0}ms`;
      } catch (e) {
        jsonResp(res, 500, { ok: false, error: String(e.message || e) });
        logLine += `  -> 500 ${e.message || e}`;
      } finally {
        await unlink(tmpIn).catch(() => {});
        await unlink(tmpOut).catch(() => {});
      }
      return;
    }

    // --- /convert-path: in-place conversion (unblock-by-cmd-id path) ---
    if (req.method === "POST" && url.pathname === "/convert-path") {
      const raw = await collectBody(req);
      let body;
      try { body = JSON.parse(raw.toString("utf8")); }
      catch { return jsonResp(res, 400, { ok: false, error: "invalid JSON" }); }
      const { input, output_dirs, output_name, password = null } = body;
      if (!input || !Array.isArray(output_dirs) || !output_dirs.length || !output_name)
        return jsonResp(res, 400, { ok: false, error: "missing params" });
      if (!existsSync(input)) return jsonResp(res, 404, { ok: false, error: "input not found: " + input });

      const info = await inspectFile(input);
      if (info.encryption && info.encryption !== "none" && !password)
        return jsonResp(res, 200, { ok: false, encrypted: true, encryption: info.encryption });

      const tmpOut = join(tmpdir(), `enq_${randomBytes(6).toString("hex")}.mp3`);
      try {
        const r = await convertFile(input, tmpOut, { password });
        const distributed = [];
        for (const d of output_dirs) {
          mkdirSync(d, { recursive: true });
          try { execSync(`chmod 777 "${d}"`); } catch {}
          const dest = join(d, output_name);
          copyFileSync(tmpOut, dest);
          chmodSync(dest, 0o666);
          chownWww(dest);
          distributed.push(dest);
        }
        jsonResp(res, 200, {
          ok: true,
          format: r.format,
          duration_s: Math.round(r.duration_s),
          mp3_bytes: r.mp3_bytes,
          distributed,
        });
        logLine += `  -> 200 ${r.format} ${Math.round(r.duration_s)}s to ${distributed.length} dirs ${Date.now() - t0}ms`;
      } catch (e) {
        jsonResp(res, 500, { ok: false, error: String(e.message || e) });
        logLine += `  -> 500 ${e.message || e}`;
      } finally {
        await unlink(tmpOut).catch(() => {});
      }
      return;
    }

    // --- /health: liveness ---
    if (req.method === "GET" && url.pathname === "/health") {
      jsonResp(res, 200, { ok: true, service: "conv-dss-ds2-to-mp3", port: PORT });
      return;
    }

    jsonResp(res, 404, { ok: false, error: "unknown endpoint" });
    logLine += "  -> 404";
  } catch (e) {
    jsonResp(res, 500, { ok: false, error: String(e.message || e) });
    logLine += `  -> 500 ${e.message || e}`;
  } finally {
    console.log(logLine);
  }
});

srv.listen(PORT, HOST, () => {
  console.log(`[http_server] listening on ${HOST}:${PORT}`);
});
