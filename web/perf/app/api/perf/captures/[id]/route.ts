import { NextRequest, NextResponse } from "next/server";
import { getCaptureBytes, getCaptureMeta, deleteCapture, hashToken } from "@/lib/server/storage";
import { ensureSweep } from "@/lib/server/sweep";

export const runtime = "nodejs";

function json(body: unknown, status = 200) {
  return NextResponse.json(body, { status, headers: { "cache-control": "no-store" } });
}

function err(code: string, message: string, status: number) {
  return json({ error: { code, message } }, status);
}

function authorized(meta: { delete_token_hash: string }, token: string | null): boolean {
  if (!token) return false;
  return hashToken(token) === meta.delete_token_hash;
}

export async function GET(req: NextRequest, ctx: { params: Promise<{ id: string }> }) {
  ensureSweep();
  const { id } = await ctx.params;
  const meta = await getCaptureMeta(id);
  if (!meta) return err("not_found", "capture not found", 404);
  if (new Date(meta.expires_utc).getTime() <= Date.now()) {
    await deleteCapture(id);
    return err("not_found", "capture expired", 404);
  }
  const bytes = await getCaptureBytes(id);
  if (!bytes) return err("not_found", "capture not found", 404);
  return new NextResponse(Buffer.from(bytes), {
    status: 200,
    headers: {
      "content-type": "application/vnd.horizon.perf",
      "cache-control": "public, max-age=31536000, immutable",
      "content-length": String(bytes.length),
    },
  });
}

export async function DELETE(req: NextRequest, ctx: { params: Promise<{ id: string }> }) {
  ensureSweep();
  const { id } = await ctx.params;
  const meta = await getCaptureMeta(id);
  if (!meta) return err("not_found", "capture not found", 404);
  const auth = req.headers.get("authorization");
  const token = auth?.startsWith("Bearer ") ? auth.slice(7) : null;
  if (!authorized(meta, token)) return err("unauthorized", "invalid token", 401);
  await deleteCapture(id);
  return new NextResponse(null, { status: 204 });
}
