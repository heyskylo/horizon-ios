import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import path from "node:path";
import os from "node:os";

export interface ValidatedCapture {
  summary: { kind: string; seconds: number; version: string };
}

let wasmExports: any = null;
let wasmLoadAttempted = false;

async function loadWasm(): Promise<any | null> {
  if (wasmLoadAttempted) return wasmExports;
  wasmLoadAttempted = true;
  const wasmPath =
    process.env.PERF_WASM_PATH ?? path.join(process.cwd(), "public", "perf", "horizon_perf.wasm");
  try {
    const buf = readFileSync(wasmPath);
    const mod = await WebAssembly.compile(buf);
    const instance = await WebAssembly.instantiate(mod, {});
    wasmExports = instance.exports;
  } catch {
    wasmExports = null;
  }
  return wasmExports;
}

function callOpen(exports: any, bytes: Uint8Array): number {
  const ptr = exports.hp_alloc(bytes.length);
  const mem = new Uint8Array(exports.memory.buffer);
  mem.set(bytes, ptr);
  const replyPtr = exports.hp_open(ptr, bytes.length);
  exports.hp_free(ptr, bytes.length);

  const dv = new DataView(exports.memory.buffer);
  const kind = new Uint8Array(exports.memory.buffer)[replyPtr];
  const len = dv.getUint32(replyPtr + 1, true);
  const payload = new Uint8Array(exports.memory.buffer, replyPtr + 5, len);
  const text = new TextDecoder().decode(payload);
  exports.hp_free(replyPtr, 5 + len);

  const parsed = JSON.parse(text);
  if (kind !== 0 || parsed?.ok === false) {
    const code = parsed?.error?.code ?? "invalid_capture";
    throw new Error(code);
  }
  return parsed.handle as number;
}

function callQuery(exports: any, handle: number, request: object): any {
  const enc = new TextEncoder();
  const body = enc.encode(JSON.stringify(request));
  const ptr = exports.hp_alloc(body.length);
  const mem = new Uint8Array(exports.memory.buffer);
  mem.set(body, ptr);
  const replyPtr = exports.hp_query(handle, ptr, body.length);
  exports.hp_free(ptr, body.length);

  const dv = new DataView(exports.memory.buffer);
  const kind = new Uint8Array(exports.memory.buffer)[replyPtr];
  const len = dv.getUint32(replyPtr + 1, true);
  const payload = new Uint8Array(exports.memory.buffer, replyPtr + 5, len);
  const text = new TextDecoder().decode(payload);
  exports.hp_free(replyPtr, 5 + len);

  const parsed = JSON.parse(text);
  if (kind !== 0) throw new Error("invalid_capture");
  return parsed;
}

export async function validateCapture(bytes: Uint8Array): Promise<ValidatedCapture> {
  if (bytes.length === 0) throw new Error("invalid_capture");

  const exports = await loadWasm();
  if (exports) {
    let handle: number;
    try {
      handle = callOpen(exports, bytes);
    } catch (e) {
      const code = e instanceof Error ? e.message : "invalid_capture";
      throw new Error(code);
    }
    try {
      const res = callQuery(exports, handle, { op: "meta" });
      const row = Array.isArray(res.rows) ? res.rows[0] : null;
      const cap = Array.isArray(res.captures) ? res.captures[0] : null;
      const kind = (row?.kind ?? cap?.kind ?? "unknown").toString();
      const seconds = Number(row?.seconds ?? cap?.seconds ?? 0);
      const version = (row?.build?.version ?? "").toString();
      return { summary: { kind, seconds, version } };
    } catch (e) {
      const code = e instanceof Error ? e.message : "invalid_capture";
      if (code === "unsupported_format") throw new Error("unsupported_format");
      throw new Error("invalid_capture");
    } finally {
      try {
        exports.hp_close(handle);
      } catch {
        // ignore
      }
    }
  }

  // Native CLI fallback
  const dir = mkdtempSync(path.join(os.tmpdir(), "perf-"));
  const fpath = path.join(dir, "capture.hzperf");
  try {
    writeFileSync(fpath, bytes);
    const bin = process.env.PERF_NATIVE_CLI ?? "horizon-perf";
    const out = execFileSync(bin, ["query", "meta", fpath], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
    const parsed = JSON.parse(out.trim());
    const row = Array.isArray(parsed.rows) ? parsed.rows[0] : null;
    const cap = Array.isArray(parsed.captures) ? parsed.captures[0] : null;
    return {
      summary: {
        kind: (row?.kind ?? cap?.kind ?? "unknown").toString(),
        seconds: Number(row?.seconds ?? cap?.seconds ?? 0),
        version: (row?.build?.version ?? "").toString(),
      },
    };
  } catch (e: any) {
    const stderr = e?.stderr?.toString?.() ?? "";
    const msg = (stderr + " " + (e?.message ?? "")).toLowerCase();
    if (msg.includes("format")) throw new Error("unsupported_format");
    throw new Error("invalid_capture");
  } finally {
    try {
      rmSync(dir, { recursive: true, force: true });
    } catch {
      // ignore
    }
  }
}
