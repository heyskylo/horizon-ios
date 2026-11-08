import { NextRequest, NextResponse } from "next/server";
import { getCaptureMeta, updateCaptureMeta, hashToken } from "@/lib/server/storage";
import { ensureSweep } from "@/lib/server/sweep";

export const runtime = "nodejs";

function json(body: unknown, status = 200) {
  return NextResponse.json(body, { status, headers: { "cache-control": "no-store" } });
}

function err(code: string, message: string, status: number) {
  return json({ error: { code, message } }, status);
}

const SHARED_DAYS = Number(process.env.PERF_SHARED_DAYS ?? 30);

export async function POST(req: NextRequest, ctx: { params: Promise<{ id: string }> }) {
  ensureSweep();
  const { id } = await ctx.params;
  const meta = await getCaptureMeta(id);
  if (!meta) return err("not_found", "capture not found", 404);

  const auth = req.headers.get("authorization");
  const token = auth?.startsWith("Bearer ") ? auth.slice(7) : null;
  if (!token || hashToken(token) !== meta.delete_token_hash) {
    return err("unauthorized", "invalid token", 401);
  }

  meta.shared = true;
  meta.expires_utc = new Date(Date.now() + SHARED_DAYS * 24 * 60 * 60 * 1000).toISOString();
  await updateCaptureMeta(id, meta);

  const origin = new URL(req.url).origin;
  return json({
    id,
    url: `${origin}/perf/c/${id}`,
    shared: true,
    expires_utc: meta.expires_utc,
  });
}
