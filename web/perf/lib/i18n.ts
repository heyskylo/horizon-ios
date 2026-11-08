// Translation-ready strings (docs/perf-platform.md §7.1): the UI shows these keys, never text from
// the wasm except the summary. Add a language by adding a file beside en_us.json and calling
// setMessages with it.
import en from "@/lang/en_us.json";

export type Messages = Record<string, string>;

const fallback = en as Messages;
let messages: Messages = fallback;

export function setMessages(next: Messages): void {
  messages = { ...fallback, ...next };
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

/** `t` for a sentence with values, so call sites read as the sentence does. */
export function tf(key: string, vars: Record<string, string | number>): string {
  return t(key, vars);
}
