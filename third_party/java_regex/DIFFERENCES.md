# Differences from OpenJDK's java.util.regex

This implementation faithfully reproduces OpenJDK's `java.util.regex.Pattern` behavior — including OpenJDK's well-known quirks (those are documented in [QUIRKS.md](https://github.com/MathieuDutSik/java_regex/blob/main/QUIRKS.md), not here). The only remaining behavioral difference is a structural one rooted in the host language's string representation.

## 1. Position offsets for supplementary code points

`java.util.regex.Matcher.start()` / `Matcher.end()` return UTF-16 *code-unit* offsets, because Java's `String` is internally UTF-16. This implementation returns *char* (Unicode scalar value) offsets, because Rust's `String` is UTF-8 and `char` is a code point.

The two agree exactly on inputs that contain no supplementary characters (every code point ≤ U+FFFF — virtually all text, including all Latin/Greek/Cyrillic/CJK BMP scripts, most emoji-free content, etc.).

For inputs containing supplementary characters (most emoji, several CJK extension blocks, math/historic scripts) a single character is one position in Rust but two positions in Java. **Matched text is always identical; only the integer positions differ.**

```text
Pattern: \Q\E         (zero-width match at every position)
Input:   "😀a"
Rust:    matches at positions 0, 1, 2     (3 chars)
Java:    matches at positions 0, 1, 2, 3  (4 UTF-16 code units)
```

This is a deliberate, fundamental difference rooted in the host language. Code that needs Java-compatible offsets on supplementary input can convert: `s.encode_utf16().take(n_chars).count()` gives the Java offset for the n-th char.

## Summary

| Behavior | OpenJDK 25 | This implementation |
|---|---|---|
| `start()`/`end()` indexing | UTF-16 code units | Unicode code points |

That's it — the only difference. Every other previously-documented divergence has been brought into alignment with OpenJDK:

- **Group capture leaks across `find()` start positions** (was: clean reset, now: leaks like Java does) — see [QUIRKS.md §8](https://github.com/MathieuDutSik/java_regex/blob/main/QUIRKS.md).
- **Lookbehind with unbounded multi-char body** (was: accepted, now: rejected at compile time like Java).
- **Negative lookbehind capture leak** (was: clean reset, now: leaks like Java).

For all tested patterns the matched text, match counts, capture group values, split boundaries, and replacement results are identical to OpenJDK 25 across 200,000+ random differential-fuzzer tests. The only mismatches surface on inputs containing supplementary characters, and only in the position-offset integers — never in matched text.
