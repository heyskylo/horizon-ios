import { NextRequest, NextResponse } from "next/server";
import { getCaptureMeta, deleteCapture } from "@/lib/server/storage";
import { ensureSweep } from "@/lib/server/sweep";

export const runtime = "nodejs";

function json(body: unknown, status = 200) {
  return NextResponse.json(body, { status, headers: { "cache-control": "no-store" } });
}

function err(code: string, message: string, status: number) {
  return json({ error: { code, message } }, status);
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
  return json({
    id: meta.id,
    bytes: meta.bytes,
    created_utc: meta.created_utc,
    expires_utc: meta.expires_utc,
    summary: meta.summary,
  });
}
