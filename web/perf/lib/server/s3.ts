import crypto from "node:crypto";

function env(name: string, fallback = ""): string {
  return process.env[name] ?? fallback;
}

function requireEnv(name: string): string {
  const v = env(name);
  if (!v) throw new Error(`missing env ${name}`);
  return v;
}

export interface S3Object {
  key: string;
  size: number;
  lastModified: string;
}

interface Cfg {
  endpoint: string;
  bucket: string;
  region: string;
  accessKeyId: string;
  secretAccessKey: string;
  pathStyle: boolean;
}

function cfg(): Cfg {
  return {
    endpoint: env("PERF_S3_ENDPOINT").replace(/\/$/, ""),
    bucket: requireEnv("PERF_S3_BUCKET"),
    region: env("PERF_S3_REGION", "us-east-1"),
    accessKeyId: requireEnv("PERF_S3_ACCESS_KEY_ID"),
    secretAccessKey: requireEnv("PERF_S3_SECRET_ACCESS_KEY"),
    pathStyle: env("PERF_S3_PATH_STYLE", "true") !== "false",
  };
}

function sha256Hex(data: Uint8Array | string): string {
  const buf = typeof data === "string" ? Buffer.from(data, "utf8") : Buffer.from(data);
  return crypto.createHash("sha256").update(buf).digest("hex");
}

function hmac(key: Buffer | string, data: string): Buffer {
  return crypto.createHmac("sha256", key).update(data, "utf8").digest();
}

function amzNow(): { amzDate: string; dateStamp: string } {
  const iso = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
  const amzDate = iso.replace(/[:-]/g, "");
  return { amzDate, dateStamp: amzDate.slice(0, 8) };
}

function buildUrl(c: Cfg, key: string): string {
  const encKey = key
    .split("/")
    .map((p) => encodeURIComponent(p))
    .join("/");
  if (c.pathStyle) return `${c.endpoint}/${c.bucket}/${encKey}`;
  const host = new URL(c.endpoint).host;
  const protocol = new URL(c.endpoint).protocol;
  return `${protocol}//${c.bucket}.${host}/${encKey}`;
}

function canonicalUri(key: string): string {
  return "/" + key.split("/").map((p) => encodeURIComponent(p)).join("/");
}

async function signedFetch(method: string, key: string, body?: Uint8Array, headers: Record<string, string> = {}, query?: string): Promise<Response> {
  const c = cfg();
  const url = buildUrl(c, key) + (query ? `?${query}` : "");
  const u = new URL(url);
  const { amzDate, dateStamp } = amzNow();
  const payloadHash = body && body.length > 0 ? sha256Hex(body) : sha256Hex("");
  const host = u.host;
  const allHeaders: Record<string, string> = { host, ...headers };
  allHeaders["x-amz-date"] = amzDate;
  allHeaders["x-amz-content-sha256"] = payloadHash;

  const signedHeaderKeys = Object.keys(allHeaders).map((k) => k.toLowerCase()).sort();
  const canonicalHeaders = signedHeaderKeys.map((k) => `${k}:${allHeaders[k] ?? allHeaders[k.toUpperCase()] ?? ""}\n`).join("");
  const signedHeaders = signedHeaderKeys.join(";");

  let path = u.pathname || "/";
  if (c.pathStyle) {
    // pathname already includes bucket
  } else {
    // virtual host: pathname includes key only, already
  }

  const canonicalRequest = [
    method,
    canonicalUri(key).replace(/^\//, path === "/" ? "/" : ""), // keep simple
    query ? canonicalQuery(query) : "",
    canonicalHeaders,
    signedHeaders,
    payloadHash,
  ].join("\n");

  // Simpler canonical URI from key for path style is /bucket/key; recompute properly:
  const canonicalUriFinal = c.pathStyle ? `/${c.bucket}${canonicalUri(key === "" ? "" : (key.startsWith("/") ? key : "/" + key))}` : canonicalUri(key);

  const canonicalRequestFixed = [
    method,
    canonicalUriFinal,
    query ? canonicalQuery(query) : "",
    canonicalHeaders,
    signedHeaders,
    payloadHash,
  ].join("\n");

  const credentialScope = `${dateStamp}/${c.region}/s3/aws4_request`;
  const stringToSign = `AWS4-HMAC-SHA256\n${amzDate}\n${credentialScope}\n${sha256Hex(canonicalRequestFixed)}`;
  const kDate = hmac("AWS4" + c.secretAccessKey, dateStamp);
  const kRegion = hmac(kDate, c.region);
  const kService = hmac(kRegion, "s3");
  const kSigning = hmac(kService, "aws4_request");
  const signature = crypto.createHmac("sha256", kSigning).update(stringToSign, "utf8").digest("hex");
  const authorization = `AWS4-HMAC-SHA256 Credential=${c.accessKeyId}/${credentialScope}, SignedHeaders=${signedHeaders}, Signature=${signature}`;

  const fetchHeaders = { ...headers, "x-amz-date": amzDate, "x-amz-content-sha256": payloadHash, authorization, host };
  // host header usually not needed; remove host to let fetch set? keep is fine
  delete (fetchHeaders as Record<string, string>).host;

  return fetch(url, {
    method,
    headers: fetchHeaders,
    body: body ? Buffer.from(body) : undefined,
  });
}

function canonicalQuery(q: string): string {
  const params = new URLSearchParams(q);
  const pairs: [string, string][] = [];
  params.forEach((v, k) => pairs.push([k, v]));
  pairs.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : a[1] < b[1] ? -1 : 1));
  return pairs.map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join("&");
}

// Public API
export async function putObject(key: string, body: Uint8Array, contentType?: string): Promise<void> {
  const headers: Record<string, string> = {};
  if (contentType) headers["content-type"] = contentType;
  const res = await signedFetch("PUT", key, body, headers);
  if (!res.ok) throw new Error(`s3 put ${key}: ${res.status} ${await res.text()}`);
}

export async function getObject(key: string): Promise<Uint8Array | null> {
  const res = await signedFetch("GET", key);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`s3 get ${key}: ${res.status} ${await res.text()}`);
  const ab = await res.arrayBuffer();
  return new Uint8Array(ab);
}

export async function headObject(key: string): Promise<{ size: number } | null> {
  const res = await signedFetch("HEAD", key);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`s3 head ${key}: ${res.status}`);
  const size = Number(res.headers.get("content-length") ?? "0");
  return { size };
}

export async function deleteObject(key: string): Promise<void> {
  const res = await signedFetch("DELETE", key);
  if (!res.ok && res.status !== 404) throw new Error(`s3 delete ${key}: ${res.status} ${await res.text()}`);
}

export async function listObjects(prefix: string): Promise<S3Object[]> {
  const out: S3Object[] = [];
  let continuationToken: string | null = null;
  do {
    const q = new URLSearchParams();
    q.set("list-type", "2");
    q.set("prefix", prefix);
    q.set("max-keys", "1000");
    if (continuationToken) q.set("continuation-token", continuationToken);
    const res = await signedFetch("GET", "", undefined, {}, q.toString());
    // GET on bucket root with query; key passed empty
    // Actually signedFetch builds url with key "", query appended. buildUrl("") -> endpoint/bucket/ -> good.
    if (!res.ok) throw new Error(`s3 list: ${res.status} ${await res.text()}`);
    const xml = await res.text();
    const contents = [...xml.matchAll(/<Contents>([\s\S]*?)<\/Contents>/g)];
    for (const m of contents) {
      const block = m[1];
      const key = block.match(/<Key>([\s\S]*?)<\/Key>/)?.[1] ?? "";
      const size = Number(block.match(/<Size>([\s\S]*?)<\/Size>/)?.[1] ?? "0");
      const last = block.match(/<LastModified>([\s\S]*?)<\/LastModified>/)?.[1] ?? "";
      out.push({ key: decodeURIComponent(key), size, lastModified: last });
    }
    const next = xml.match(/<NextContinuationToken>([\s\S]*?)<\/NextContinuationToken>/)?.[1] ?? null;
    continuationToken = next ? decodeURIComponent(next) : null;
    const truncated = xml.match(/<IsTruncated>([\s\S]*?)<\/IsTruncated>/)?.[1] === "true";
    if (!truncated) continuationToken = null;
  } while (continuationToken);
  return out;
}
