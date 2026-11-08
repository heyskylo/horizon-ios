import { NextRequest, NextResponse } from "next/server";
import { validateCapture } from "@/lib/server/validate";
import { putCapture, generateId, generateToken, hashToken, storageUsageBytes } from "@/lib/server/storage";
import { rateLimitOk } from "@/lib/server/ratelimit";
import { ensureSweep } from "@/lib/server/sweep";

export const runtime = "nodejs";

const MAX_BYTES = Number(process.env.PERF_MAX_BYTES ?? 64 * 1024 * 1024);
const CAPACITY = Number(process.env.PERF_S3_CAPACITY_BYTES ?? 1024 * 1024 * 1024);

function json(body: unknown, status = 200) {
  return NextResponse.json(body, {
    status,
    headers: { "cache-control": "no-store" },
  });
}

function err(code: string, message: string, status: number) {
  return json({ error: { code, message } }, status);
}

function clientIp(req: NextRequest): string {
  const xf = req.headers.get("x-forwarded-for");
  if (xf) return xf.split(",")[0].trim();
  return req.headers.get("x-real-ip") ?? "unknown";
}

export async function POST(req: NextRequest) {
  ensureSweep();
  if (!rateLimitOk(clientIp(req))) {
    return err("rate_limited", "too many uploads", 429);
  }
  const buf = await req.arrayBuffer();
  const bytes = new Uint8Array(buf);
  if (bytes.length === 0) return err("invalid_capture", "empty body", 415);
  if (bytes.length > MAX_BYTES) return err("too_large", "payload too large", 413);

  let validated;
  try {
    validated = await validateCapture(bytes);
  } catch (e) {
    const code = e instanceof Error ? e.message : "invalid_capture";
    if (code === "unsupported_format") return err("unsupported_format", "unsupported format", 422);
    return err("invalid_capture", "invalid capture", 415);
  }

  // storage capacity check
  try {
    const usage = await storageUsageBytes();
    if (usage + bytes.length > CAPACITY * 0.9) {
      return err("storage_full", "storage full", 507);
    }
  } catch {
    // if storage check fails, proceed
  }

  // unique id
  let id = generateId();
  for (let i = 0; i < 5; i++) {
    const existing = await (await import("@/lib/server/storage")).getCaptureMeta(id);
    if (!existing) break;
    id = generateId();
  }

  const deleteToken = generateToken();
  const deleteTokenHash = hashToken(deleteToken);
  const now = new Date().toISOString();
  const expires = new Date(Date.now() + 60 * 60 * 1000).toISOString();

  const meta = {
    id,
    bytes: bytes.length,
    created_utc: now,
    expires_utc: expires,
    shared: false,
    delete_token_hash: deleteTokenHash,
    summary: validated.summary,
  };

  await putCapture(id, bytes, meta);

  const origin = new URL(req.url).origin;
  return json(
    {
      id,
      url: `${origin}/perf/c/${id}`,
      delete_token: deleteToken,
      shared: false,
      expires_utc: expires,
      bytes: bytes.length,
      summary: validated.summary,
    },
    201,
  );
}
