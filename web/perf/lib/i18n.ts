// Translation-ready strings (docs/perf-platform.md §7.1): the UI shows these keys, never text from the wasm except the summary.
import en from "@/lang/en_us.json";

export type Messages = Record<string, string>;

const fallback = en as Messages;
let messages: Messages = fallback;

export function setMessages(next: Messages): void {
  messages = { ...fallback, ...next };
}

/** Whether the language has text for `key`. */
export function has(key: string): boolean {
  return key in messages;
}

/** `t("health.tps")`, with `{name}` placeholders filled from `vars`. */
export function t(key: string, vars?: Record<string, string | number>): string {
  let text = messages[key] ?? key;
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      text = text.replaceAll(`{${name}}`, String(value));
    }
  }
  return text;
}
