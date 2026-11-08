// Stable ids (docs/perf-platform.md §5.2) as the rows show them: `<kind letter>:<key>`.

/** Whether `text` reads as a stable id: one kind letter, a colon and a key. */
export function isId(text: string | null | undefined): text is string {
  return typeof text === "string" && /^[a-z]:\S+$/.test(text);
}

/** The id of a span, which is its name (a tick's spans list names only). */
export function spanId(name: string): string {
  return `s:${name}`;
}

/** The translation key of an id's explanation. */
export function infoKey(id: string): string {
  return `info.${id}`;
}
