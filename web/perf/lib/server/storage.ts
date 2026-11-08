import { createHash, randomBytes } from "node:crypto";
import { putObject, getObject, deleteObject, listObjects } from "./s3";

export interface CaptureMeta {
  id: string;
  bytes: number;
  created_utc: string;
  expires_utc: string;
  shared: boolean;
  delete_token_hash: string;
  summary: { kind: string; seconds: number; version: string };
}

const PREFIX = "captures/";

export function binKey(id: string) {
  return `${PREFIX}${id}.bin`;
}
export function metaKey(id: string) {
  return `${PREFIX}${id}.meta.json`;
}

function sha256Hex(s: string) {
  return createHash("sha256").update(s).digest("hex");
}

export function generateId(): string {
  // 10 base32 chars (RFC4648 lowercased)
  const alphabet = "abcdefghijklmnopqrstuvwxyz234567";
  const bytes = randomBytes(10);
  let out = "";
  for (let i = 0; i < 10; i++) out += alphabet[bytes[i] % alphabet.length];
  return out;
}

export function generateToken(): string {
  return randomBytes(24).toString("base64url");
}

export function hashToken(token: string): string {
  return sha256Hex(token);
}

export async function putCapture(id: string, bytes: Uint8Array, meta: CaptureMeta): Promise<void> {
  await putObject(binKey(id), bytes, "application/vnd.horizon.perf");
  await putObject(metaKey(id), new TextEncoder().encode(JSON.stringify(meta)), "application/json");
}

export async function getCaptureBytes(id: string): Promise<Uint8Array | null> {
  return getObject(binKey(id));
}

export async function getCaptureMeta(id: string): Promise<CaptureMeta | null> {
  const buf = await getObject(metaKey(id));
  if (!buf) return null;
  try {
    return JSON.parse(new TextDecoder().decode(buf)) as CaptureMeta;
  } catch {
    return null;
  }
}

export async function updateCaptureMeta(id: string, meta: CaptureMeta): Promise<void> {
  await putObject(metaKey(id), new TextEncoder().encode(JSON.stringify(meta)), "application/json");
}

export async function deleteCapture(id: string): Promise<void> {
  await deleteObject(binKey(id));
  await deleteObject(metaKey(id));
}

export async function listCaptureMetas(): Promise<CaptureMeta[]> {
  const objs = await listObjects(PREFIX);
  const metas: CaptureMeta[] = [];
  for (const o of objs) {
    if (!o.key.endsWith(".meta.json")) continue;
    const id = o.key.slice(PREFIX.length, -".meta.json".length);
    const m = await getCaptureMeta(id);
    if (m) metas.push(m);
  }
  return metas;
}

export async function storageUsageBytes(): Promise<number> {
  const objs = await listObjects(PREFIX);
  return objs.reduce((a, o) => a + o.size, 0);
}
