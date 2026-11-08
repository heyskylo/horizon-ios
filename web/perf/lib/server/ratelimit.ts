const buckets = new Map<string, number[]>();

export function rateLimitOk(ip: string, max = 20, windowMs = 3_600_000): boolean {
  const now = Date.now();
  const arr = (buckets.get(ip) ?? []).filter((t) => now - t < windowMs);
  if (arr.length >= max) {
    buckets.set(ip, arr);
    return false;
  }
  arr.push(now);
  buckets.set(ip, arr);
  return true;
}
