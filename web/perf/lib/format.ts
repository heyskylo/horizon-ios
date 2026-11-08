// Drawing helpers: they format numbers the wasm computed; they never compute a performance number.

const MISSING = "—";

export function grouped(value: number): string {
  const rounded = Math.round(value);
  const sign = rounded < 0 ? "-" : "";
  const digits = Math.abs(rounded).toString();
  return sign + digits.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

/** `num(12.345)` → `12.3`; large values lose their decimals so a card stays narrow. */
export function num(value: number | null | undefined, digits = 1): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return MISSING;
  if (Math.abs(value) >= 1000) return grouped(value);
  return value.toFixed(digits);
}

/** A compact number with its unit, e.g. `1.2 MiB`. */
export function withUnit(value: number | null | undefined, unit: string, digits = 1): string {
  const text = num(value, digits);
  return text === MISSING ? text : `${text} ${unit}`;
}

export function bool(value: boolean | null | undefined, yes = "yes", no = "no"): string {
  if (value === null || value === undefined) return MISSING;
  return value ? yes : no;
}

/** Seconds as `2 m 03 s`, or `1:02:03` past an hour. */
export function duration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return MISSING;
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  if (hours > 0) return `${hours}:${String(minutes).padStart(2, "0")}:${String(rest).padStart(2, "0")}`;
  if (minutes > 0) return `${minutes} m ${String(rest).padStart(2, "0")} s`;
  return `${rest} s`;
}

/** A capture's start, in UTC so two people comparing captures see the same wall clock. */
export function utc(unixMs: number | null | undefined): string {
  if (!unixMs) return MISSING;
  const date = new Date(unixMs);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())} ${pad(
    date.getUTCHours(),
  )}:${pad(date.getUTCMinutes())} UTC`;
}

/** `123.4 KiB/s` from a KiB per second value. */
export function kib(value: number | null | undefined): string {
  return withUnit(value, "KiB/s", 1);
}

export function mib(value: number | null | undefined): string {
  return withUnit(value, "MiB", 1);
}
