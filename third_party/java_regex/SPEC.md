# OpenJDK `java.util.regex` Reference Specification

This document specifies what **OpenJDK 25's `java.util.regex.Pattern`** actually does — not what the Javadoc says. The Javadoc is the official spec, but it is underspecified in dozens of places and, in several others, OpenJDK's parser/engine diverges from a literal reading. Because OpenJDK is by far the dominant JVM implementation (Oracle JDK, GraalVM, Amazon Corretto, Azul Zulu, IBM Semeru / OpenJ9, etc.), "what OpenJDK does" is the working definition of Java regex.

This spec describes that working definition. Wherever it differs from a naive reading of the Javadoc, the divergence is called out and cross-referenced to [QUIRKS.md](https://github.com/MathieuDutSik/java_regex/blob/main/QUIRKS.md).

**Scope.** Java 8+, targeting Java 13+ semantics (OpenJDK 25). Excludes the `CANON_EQ` (canonical equivalence) flag, which is rarely used and not implemented by most third-party engines.

**Conventions.** `(pattern)` in headings denotes the regex construct; `→ Some(result)` denotes the matcher's expected output; `[Q-N]` cross-references quirk #N in QUIRKS.md.

---

## Table of contents

- [1. Pattern syntax](#1-pattern-syntax)
  - [1.1 Literals and escape sequences](#11-literals-and-escape-sequences)
  - [1.2 Predefined character classes](#12-predefined-character-classes)
  - [1.3 Character classes (`[...]`)](#13-character-classes-)
  - [1.4 Quantifiers](#14-quantifiers)
  - [1.5 Groups](#15-groups)
  - [1.6 Anchors and boundaries](#16-anchors-and-boundaries)
  - [1.7 Lookarounds](#17-lookarounds)
  - [1.8 Backreferences](#18-backreferences)
  - [1.9 Unicode properties](#19-unicode-properties)
- [2. Flags and inline flag scoping](#2-flags-and-inline-flag-scoping)
- [3. Matching operations](#3-matching-operations)
- [4. Capture-group state model](#4-capture-group-state-model)
- [5. `split()`](#5-split)
- [6. Replacement DSL (`appendReplacement` / `replaceAll`)](#6-replacement-dsl-appendreplacement--replaceall)
- [7. Compile-time errors](#7-compile-time-errors)
- [8. Quirks index](#8-quirks-index)

---

## 1. Pattern syntax

A *pattern* is a string of characters interpreted by the regex compiler. Characters fall into three categories:

- **Literal characters** — match themselves (e.g., `a` matches `a`).
- **Metacharacters** — have special meaning: `\ . * + ? ( ) [ ] { } | ^ $ - = ! < > /` (the last three only in specific contexts).
- **Escape sequences** — backslash + character, with specific meanings (see §1.1).

Whitespace in the pattern is treated literally **unless** the COMMENTS flag (`(?x)` or `Pattern.COMMENTS`) is active, in which case unescaped whitespace and `# … end-of-line` comments are stripped (see §2). COMMENTS-mode whitespace stripping applies both **outside** and **inside** `[...]` character classes.

### 1.1 Literals and escape sequences

A backslash escapes the next character. Every character following `\` has one of the meanings below; an unrecognized character produces an "Illegal/unsupported escape sequence" error.

| Escape | Meaning |
|---|---|
| `\\` | Literal backslash |
| `\.` `\*` `\+` `\?` `\(` `\)` `\[` `\]` `\{` `\}` `\|` `\^` `\$` | The corresponding metacharacter as a literal |
| `\-` `\!` `\=` `\<` `\>` `\/` `\#` `\ ` `\&` `\~` `\@` `\`` `\'` `\"` `\,` `\;` `\:` | These ASCII characters as literals (no-op escapes — Java permits backslash before any of them) |
| `\n` | Newline (U+000A) |
| `\r` | Carriage return (U+000D) |
| `\t` | Horizontal tab (U+0009) |
| `\f` | Form feed (U+000C) |
| `\a` | Bell (U+0007) |
| `\e` | Escape (U+001B) |
| `\0n` | Octal value `n` (1–3 octal digits) |
| `\0nn` | Octal value `nn` |
| `\0mnn` | Octal value `mnn` where `0 ≤ m ≤ 3`, `0 ≤ n ≤ 7` |
| `\xhh` | Hex value `hh` (exactly 2 hex digits) |
| `\uhhhh` | Hex value `hhhh` (exactly 4 hex digits, U+0000..U+FFFF) |
| `\x{hh..h}` | Hex value with any number of digits, U+0000..U+10FFFF (supplementary chars allowed) |
| `\cX` | Control character: the character corresponding to `X` XOR `0x40` (e.g., `\cM` = U+000D, `\c@` = U+0000) |
| `\R` | Any Unicode line break — see §1.6 |
| `\X` | A single grapheme cluster (UAX #29 extended grapheme cluster, including ZWJ sequences) |
| `\Q…\E` | Literal block: every character between `\Q` and `\E` matches literally, with no regex interpretation (see below) |
| `\p{…}`, `\P{…}` | Unicode property / negated Unicode property — see §1.9 |
| `\d \D \w \W \s \S \h \H \v \V` | Predefined character classes — see §1.2 |
| `\b \B \A \z \Z \G` | Anchors — see §1.6 |
| `\1` … `\9` (or longer) | Numbered backreference — see §1.8 |
| `\k<name>` | Named backreference — see §1.8 |
| `\N{…}` | (Reserved syntax — *not* supported by OpenJDK's `Pattern`; it errors.) |

**Octal escape rule.** OpenJDK requires the **leading `0`** for octal: `\0377` is octal 0xFF, but `\377` is the backreference `\3` followed by literal `7`, `7`. (Multiple regex engines accept `\nnn` as octal even without the leading `0`; Java does not.)

**`\Q…\E` semantics.**

- Inside `\Q…\E`, every character — including `\`, `.`, `*`, `(`, etc. — matches literally. No escape sequences are processed.
- An unclosed `\Q` (no `\E` before end-of-pattern) consumes characters to the end of the pattern, equivalent to "literal rest of pattern."
- `\Q\E` (empty literal block) emits *nothing* and is a no-op (it does not match any character). When at the start of an alternation branch followed by `|`, it acts as the empty branch.
- A quantifier following `\Q…\E` quantifies the **last character** of the block — `\Qabc\E+` matches `ab` followed by one or more `c`s, not one or more `abc`s. To quantify the whole block, wrap it in a group: `(?:\Qabc\E)+`.
- `\Q…\E` may appear **inside** a character class: `[\Qabc\E]` is equivalent to `[abc]`. Same end-character rules: unclosed `\Q` consumes to `]`; metacharacters lose their meaning.
- Note: `\E` outside a `\Q…\E` block is an "Illegal/unsupported escape sequence" error.

### 1.2 Predefined character classes

| Class | Default (ASCII) meaning | With `UNICODE_CHARACTER_CLASS` (`U`) flag |
|---|---|---|
| `\d` | `[0-9]` | Any character in Unicode general category `Nd` (Decimal_Number) |
| `\D` | `[^0-9]` | Complement of `\d` |
| `\w` | `[a-zA-Z0-9_]` | Letters, digits, marks, joining `_` (a curated Unicode set per the Javadoc) |
| `\W` | Complement of `\w` | Complement of `\w` |
| `\s` | `[ \t\n\x0B\f\r]` | `[\p{IsWhite_Space}]` |
| `\S` | Complement of `\s` | Complement of `\s` |
| `\h` | `[ \t\xA0 ᠎ -   　]` | Same (horizontal whitespace; Unicode by definition) |
| `\H` | Complement of `\h` | Same |
| `\v` | `[\n\x0B\f\r\x85  ]` | Same (vertical whitespace; Unicode by definition) |
| `\V` | Complement of `\v` | Same |

`\R` is **not** a predefined character class — it is a *line-break matcher* that matches the **two-character** sequence `\r\n` or any **single** character in `[\n\x0B\f\r\x85  ]`. Crucially:

- Inside a quantifier (e.g., `\R{2}`, `\R+`), `\R` is **atomic** — the engine never backtracks into it to try the single-character branch after the two-character branch fits. So `\R{2}` on `"\r\n"` does **not** match: iter 1 consumes `\r\n`, iter 2 has nothing left, and no backtrack is attempted. **[Q-2, Q-3]**
- In a sequence outside a quantifier (e.g., `\R\n`), `\R` does backtrack into its branches.

`\X` matches one **extended grapheme cluster** (per UAX #29), including ZWJ-joined sequences like "👨‍👩‍👦". `\X` is unbounded in length, so it is **rejected** in lookbehind bodies (see §1.7).

### 1.3 Character classes (`[...]`)

A character class matches a single character drawn from a set built by union, range, intersection, and negation.

**Basic forms.**

| Form | Meaning |
|---|---|
| `[abc]` | `a`, `b`, or `c` |
| `[^abc]` | Any character except `a`, `b`, `c` |
| `[a-z]` | Any character in the inclusive range U+0061..U+007A |
| `[a-zA-Z0-9_]` | Union of three ranges and one literal |
| `[[a-z][A-Z]]` | Union of two nested classes (= `[a-zA-Z]`) |
| `[a-z&&[def]]` | Intersection: `{a..z} ∩ {d,e,f} = {d,e,f}` |
| `[a-z&&[^aeiou]]` | Set difference (intersection with complement) |

**Special characters inside `[...]`.** Unlike outside, most metacharacters lose their special meaning inside a character class:

- `\` is still an escape character.
- `]` ends the class. To match a literal `]`, place it **first** (after `[` or `[^`): `[]a]` matches `]` or `a`. Or escape it: `[\]a]`.
- `-` is the range operator between two characters; literal `-` is matched if placed first, last, or escaped.
- `^` immediately after `[` negates the class. Elsewhere it is literal.
- `&&` is the intersection operator (see below). A single `&` is literal.
- `[` opens a nested class.
- `\Q…\E` is supported; see §1.1.

**Predefined classes inside `[...]`.** `\d`, `\w`, `\s`, `\D`, `\W`, `\S`, `\h`, `\H`, `\v`, `\V`, `\p{…}`, and `\P{…}` may all appear inside `[...]` and contribute their set membership.

**Ranges.** The range operator `-` is between two **single characters** (literal or escaped). The start must be ≤ the end (`[z-a]` is rejected). Predefined classes (`\d`, etc.) cannot be range endpoints; using one — e.g., `[\d-x]` — is **not** a range, it is the literal three-element union `\d ∪ {-} ∪ {x}` (the `-` is treated literally when adjacent to a non-single-character item).

**Negation.** `^` immediately inside `[` negates the **entire** class. `[^a-z]` is "anything not in a..z." Negation applies to the union/intersection result, not to individual items.

**Intersection `&&`.** Computes the **set intersection** of the left and right operands:

- `[abc&&[def]]` is empty (no overlap).
- `[a-z&&[^aeiou]]` is "lowercase letters minus vowels."
- `[a-z&&[A-Z]&&xyz]` is *not* a clean three-way intersection. See [Q-4] for the quirk: when the middle operand contains a nested class followed by a literal, the trailing `&&` is silently absorbed into the recursive sub-class scope. Workaround: use explicit nesting `[[a-z&&[A-Z]]&&xyz]`.

**Nested character classes.** `[[a-z][A-Z]]` is the union of two nested classes (= `[a-zA-Z]`). Nesting is supported to arbitrary depth.

**Case-folded ranges.** When `CASE_INSENSITIVE` is active, range membership checks the input character *and* its case variant against the **unmodified** range — not the folded range. So `[1-c]` with `/i` matches `'g'` because `'G'` (U+0047) is inside the range U+0031..U+0063. **[Q-6]**

### 1.4 Quantifiers

A quantifier follows an *atom* and specifies how many times that atom must match.

| Quantifier | Min | Max | Notes |
|---|---|---|---|
| `X?` | 0 | 1 | Optional |
| `X*` | 0 | ∞ | Zero or more |
| `X+` | 1 | ∞ | One or more |
| `X{n}` | n | n | Exactly n |
| `X{n,}` | n | ∞ | At least n |
| `X{n,m}` | n | m | Between n and m (inclusive) |

Each form has three **modes**, distinguished by a suffix:

| Suffix | Mode | Behavior |
|---|---|---|
| (none) | **Greedy** | Take as many as possible, backtrack to free up characters for the rest of the pattern |
| `?` | **Reluctant** (lazy) | Take as few as possible, expand only if rest fails |
| `+` | **Possessive** | Take as many as possible, never backtrack (no give-back) |

Examples: `X*?` (reluctant `*`), `X*+` (possessive `*`), `X{2,5}?` (reluctant range), `X{2,5}+` (possessive range).

**Possessive semantics.** A possessive quantifier matches like a greedy one but commits to its choice. If the rest of the pattern fails after a possessive match, the engine does **not** retry with fewer matches; the whole pattern fails (or backtracks to a point before the possessive).

**Quantified atom compilation.** OpenJDK compiles a quantified atom with one of two strategies, chosen at parse time:

- **`GroupCurly` / `Curly` (atomic body)** — when the body is *deterministic* (single top-level branch, no nested variable-count quantifier). The atom is matched in isolation; the engine does not backtrack into the atom's internal choices.
- **`Loop` / `LazyLoop` (chain body)** — when the body has alternation, a Ques quantifier (`X?` / `X??`), or another non-deterministic shape. The continuation is threaded through the body so the engine *can* backtrack into the atom.

Consequence: `\R{2}` on `"\r\n"` does **not** match. `\R` is a single atom whose two-character branch always wins; with the body deterministic, no backtrack into `\R` is attempted. `(?:a|aa){2}` on `"aaa"` **does** match — alternation forces the chain compilation. **[Q-2, Q-3]**

The atomic split is observable through any wrapper: `\R{2}`, `(?:\R){2}`, `(\R){2}`, `(?>\R){2}`, `(?i:\R){2}` all behave identically (all atomic). Only an internal alternation or a nested variable-count quantifier flips to the chain compilation.

**Zero-width quantified body.** If the body matches zero characters (e.g., `()*` or `(?=a)*`), the engine treats subsequent zero-width iterations as having already reached the maximum count and proceeds to the rest of the pattern. This avoids infinite loops on `(\b)*`-style patterns.

A capturing group with a `*` or `{0,N}` quantifier whose body matched **zero times** (cmin == 0 and the body never executed) leaves the group **null**, not the empty string. Compare: `()`, `()+`, `(\Q\E)`, `(a*)*` all set group 1 to `""` (the body did run, possibly matching empty); but `(\Q\E)*`, `()*` leave group 1 null.

### 1.5 Groups

A group is a sub-expression delimited by `(` and `)`. OpenJDK supports six kinds:

| Syntax | Kind | Captures? | Inherits flags? |
|---|---|---|---|
| `(X)` | Capturing | Yes (numbered, 1-based, left-paren order) | Yes |
| `(?<name>X)` | Named capturing | Yes (numbered AND named) | Yes |
| `(?:X)` | Non-capturing | No | Yes |
| `(?>X)` | Atomic (non-capturing) | No | Yes |
| `(?flags:X)` or `(?flags-flags:X)` | Flag group (non-capturing) | No | Modifies flags locally |
| `(?flags)` or `(?flags-flags)` | Inline flag setter (no body, no parens around content) | No | Modifies flags in enclosing scope |

**Group numbering.** Capturing groups are numbered 1, 2, 3, … in the order their opening `(` appears in the pattern. Group 0 is always the whole match. Non-capturing kinds (`?:`, `?>`, `?=`, `?!`, `?<=`, `?<!`, `?flags:`) do **not** receive a number and do not affect downstream numbering.

**Named groups.** A group name is `[A-Za-z][A-Za-z0-9]*` (must start with a letter; subsequent characters letter or digit; no Unicode names). Named groups are *also* numbered — `(?<a>x)(?<b>y)` gives group 1 = `a`, group 2 = `b`. Duplicate names within one pattern are a compile error.

**Atomic groups `(?>X)`.** Once `X` succeeds, the engine commits and will not backtrack into `X` even if the rest of the pattern fails. Atomic groups are non-capturing and do not propagate inline flags out of the group (see §2).

**Flag groups `(?flags:X)`.** The flags listed in `flags` are added to the active flag set for the duration of `X`. A `-` after the letters subtracts: `(?-i:X)` turns *off* `i` for `X`. Compound: `(?im-s:X)` enables `i` and `m`, disables `s` for `X`. After the closing `)`, the original flags are restored.

**Inline flag setter `(?flags)` / `(?flags-flags)`.** No colon, no body. The setter mutates the *parser's* flag state from that point onward, scoped to the **enclosing group** (or the whole pattern if at the top level). Top-level inline flags propagate across alternation branches; wrapping in any group (including `(?:...)`, `(...)`, `(?>...)`, `(?=...)`) scopes the change to that group. **[Q-5]**

```text
(?s)|.       on "\n"  →  matches: true   (top-level (?s) leaks into alt 2)
(?s)xx|.     on "\n"  →  matches: true   (even from a failing branch)
(?:(?s))|.   on "\n"  →  matches: false  (wrapped → scoped)
((?s))|.     on "\n"  →  matches: false  (wrapped → scoped)
(?>(?s))|.   on "\n"  →  matches: false  (wrapped → scoped)
```

This is a **compile-time** behavior, not runtime: the parser mutates its own flag-state when it encounters `(?…)` and the change persists to subsequent branches parsed afterwards — regardless of whether those branches end up matching at runtime.

### 1.6 Anchors and boundaries

Anchors match a **position**, not a character.

| Anchor | Position |
|---|---|
| `^` | Start of input (or start of line, if MULTILINE) |
| `$` | End of input or before final line terminator (or end of line, if MULTILINE) |
| `\A` | Start of input (regardless of MULTILINE) |
| `\z` | End of input (regardless of MULTILINE) |
| `\Z` | End of input or before final line terminator (regardless of MULTILINE) |
| `\b` | Word boundary (transition between `\w` and `\W`, or input edge) |
| `\B` | Not a word boundary |
| `\G` | End of the previous match (or start of input for the first `find()` call) |

**Line terminator.** By default, "line terminator" means any of `\n`, `\r`, `\r\n`, U+0085 NEL, U+2028 LS, or U+2029 PS. With `UNIX_LINES` (`(?d)` or `Pattern.UNIX_LINES`), only `\n` is a line terminator.

**Multiline `^`.** With MULTILINE, `^` matches at the start of input and immediately after every line terminator — **except at the very end of input**, even if the end of input is also the end of a line. So:

```text
^ on ""       (MULTILINE)  →  no match
^ on "\n"     (MULTILINE)  →  matches at pos 0 only (NOT at pos 1)
^ on "\r\né"  (MULTILINE)  →  matches at positions 0, 1, 2 only
```

This mirrors a Perl quirk. **[Q-1]**

**`$` in non-multiline mode.** `$` matches at end of input, *or* just before a single line terminator at end of input (so the pattern `^abc$` matches `"abc\n"`). With UNIX_LINES, "single line terminator" means `\n` only.

**`\Z`.** Same as `$` but unaffected by MULTILINE.

**`\z`.** Strict end of input; does not match before a trailing newline.

**Word boundary `\b`.** A boundary exists between two adjacent character positions if exactly one of them has `\w` and the other has `\W` (or is past an input edge). With UNICODE_CHARACTER_CLASS active, "word character" expands to the Unicode word set; otherwise it is ASCII `[a-zA-Z0-9_]`.

**`\G`.** For `find()`, anchors at the position where the previous match ended (or start of input for the first call). For `matches()` and `lookingAt()`, anchors at the search start position. Useful for tokenizers: `\G\w+` finds successive word tokens with no gaps.

**Region boundaries.** When `Matcher.region(start, end)` is set:
- By default (anchoring bounds), `^`, `\A`, `$`, `\z`, `\Z` treat the region as if it were the whole input. `\b` similarly treats positions at region edges as input edges.
- With `useAnchoringBounds(false)`, anchors and `\b` see the *real* input boundaries.
- With `useTransparentBounds(true)`, lookarounds can look across region edges into the rest of the actual input. By default (`useTransparentBounds(false)`), lookarounds are clipped at the region.
- `\Z` and `$` (non-multiline) consult the character *before* the current position. They do **not** honor region bounds for this character lookup — they always look at the underlying input character. So `\Z` at position 1 of region `[1, 2)` of input `"\r\n\r"` sees the `\r` at position 0 (outside the region) and correctly identifies that position 1 is inside `\r\n` (so `\Z` does not match here).

### 1.7 Lookarounds

A lookaround matches a **position** by attempting an inner pattern at that position without consuming characters.

| Syntax | Direction | Polarity |
|---|---|---|
| `(?=X)` | Lookahead | Positive (require X) |
| `(?!X)` | Lookahead | Negative (require not X) |
| `(?<=X)` | Lookbehind | Positive (require X immediately before) |
| `(?<!X)` | Lookbehind | Negative (require not X immediately before) |

**Lookahead.** Attempts to match `X` starting at the current position. Position is *not* consumed. Captures inside `X` are written to the matcher's group state — **and they persist** even when the lookahead is negative and the inner match succeeded (which makes the overall lookaround fail). See §4 on capture-state leaks. **[Q-8]**

**Lookbehind.** Attempts to match `X` ending exactly at the current position. The inner pattern is run "backwards" — OpenJDK enumerates all positions `j` in `[i - rmax, i - rmin]` and attempts to match `X` against the slice `input[j..i]`.

**Bounded-length requirement.** Lookbehind requires that the inner pattern have a **statically known maximum length**. OpenJDK's compile-time check uses `TreeInfo.maxLength` over the inner AST. Patterns that fail this check are rejected with `"Look-behind group does not have an obvious maximum length"`. Examples:

```text
(?<=a)            OK  (max len 1)
(?<=ab)           OK  (max len 2)
(?<=a{2,4})       OK  (max len 4)
(?<=a+)           OK  (single-char unbounded is fine — body max len is "really" 1)
(?<=(?:ab)+)      REJECTED  (multi-char unbounded)
(?<=ab*)          REJECTED  (mixed; total is unbounded)
(?<=\R+)          REJECTED  (\R is multi-char)
(?<=\X)           REJECTED  (\X is unbounded)
(?<=\1)           REJECTED  (backref has no compile-time length)
```

The single-char-unbounded exception applies to character-class repeats: `a+`, `\d*`, `[ab]+` all give a body max length the engine clamps to a single-character iteration count. Multi-character repeats (`(?:ab)+`, `\R+`) don't get this exception.

**i32 wrapping in lookbehind body sizing.** OpenJDK's `TreeInfo.maxLength` uses `int` arithmetic with overflow (no saturation). A `*+` possessive contributes `MAX_REPS` (= `0x7FFFFFFF`); enclosing context can push the body's max length past `Integer.MAX_VALUE` and into a *negative* value. The engine then computes the iteration window `[i - rmax, i - rmin]` with that negative `rmax`, which makes `i - rmax` very large — effectively skipping body iteration when the overflow is enough. This is a documented quirk we reproduce; certain `(?<!...)` constructs with large nested counts will accept where a naive non-overflow implementation would reject. (See `pattern_java_max` in the source.)

**Negative lookbehind with `Branch` alternation.** When the body has an alternation `(empty | unbounded)`, Java's `Branch.study` takes `Math.max(0, neg_overflowed)` across atoms. The empty alt contributes 0, which dominates the negative wrap. Net effect: `rmax = 0`, and the engine iterates `j = i` (zero-width), letting the empty alt match. So `(?<!|...)` with an unbounded alt 2 still fails the negative lookbehind because the empty alt matches.

**Lookaround capture state.** Captures set inside a lookaround's inner pattern **persist into the outer matcher state** even if the lookaround is negative and the inversion makes the outer match fail. See §4. **[Q-8]**

### 1.8 Backreferences

A backreference matches the **same characters** as a previously-captured group.

| Syntax | Meaning |
|---|---|
| `\1` `\2` … `\9` | Reference to group N (1-9) |
| `\N` for N ≥ 10 | Reference to group N if N ≤ total group count; otherwise greedy fallback |
| `\k<name>` | Reference to named group |

**Multi-digit number semantics.** OpenJDK consumes digits greedily but caps at the actual group count. With 12 capture groups, `\12` is a reference to group 12. With 5 capture groups, `\12` parses as `\1` followed by literal `2`.

**Case-sensitivity.** When `CASE_INSENSITIVE` is active, the backreference compares case-insensitively character-by-character. With UNICODE_CASE active, the comparison uses Unicode case folding (per-character `toLowerCase`/`toUpperCase`); otherwise it uses ASCII case folding.

**Backreferences with capture groups not yet executed.** If `\N` references a group that has not yet successfully captured (e.g., the group is in an unmatched alternation branch, or the backreference is inside the group itself), the captured value is **null**. A backreference to a null capture **fails** to match — it is not equivalent to an empty match.

**Backreferences to zero-width captures.** A backreference to a group that captured the empty string (e.g., `(?:)`, `(?=.)`) matches zero characters and succeeds.

**Empty-capture backreference in a quantifier.** `Curly` detects `i == matcher.last` (zero-width iteration) and short-circuits to the next iteration's continuation. This is how `(.*)\1+` does not loop forever when the first `.*` captures empty.

**Backreferences inside lookbehind bodies are rejected** (lookbehind requires bounded length; backref is runtime-sized).

### 1.9 Unicode properties

`\p{Name}` matches a character whose Unicode property is `Name`; `\P{Name}` is the complement.

**Property categories.** OpenJDK accepts:

- **General Categories** by full name (`\p{Lu}`, `\p{Letter}`, `\p{Uppercase_Letter}`), short name, or both:
  - `L` / `Letter`, `Lu` / `Uppercase_Letter` / `Upper`, `Ll` / `Lowercase_Letter` / `Lower`, `Lt`, `Lm`, `Lo`, `Lc` (cased letter)
  - `M` / `Mark`, `Mn`, `Mc`, `Me`
  - `N` / `Number`, `Nd` / `Digit`, `Nl`, `No`
  - `P` / `Punctuation` / `Punct`, `Pc`, `Pd`, `Ps`, `Pe`, `Pi`, `Pf`, `Po`
  - `S` / `Symbol`, `Sm`, `Sc`, `Sk`, `So`
  - `Z` / `Separator`, `Zs`, `Zl`, `Zp`
  - `C` / `Control` / `Other`, `Cc`, `Cf` / `Format`, `Co`, `Cn`

- **POSIX classes** (case-insensitive name):
  - `Alpha`, `Lower`, `Upper`, `Digit`, `Alnum`, `Punct`, `Graph`, `Print`, `Blank`, `Cntrl`, `XDigit`, `Space`, `ASCII`
  - These have two modes: ASCII-only (default) and Unicode (when `UNICODE_CHARACTER_CLASS` is active). The Unicode variants of POSIX classes redefine each to its Unicode equivalent (e.g., `\p{Lower}` becomes any character with `Lowercase=True`).

- **Special Java properties** (exact case, `java` prefix):
  - `javaLowerCase`, `javaUpperCase`, `javaTitleCase`, `javaDigit`, `javaLetter`, `javaLetterOrDigit`, `javaAlphabetic`, `javaWhitespace`, `javaSpaceChar`, `javaMirrored`, `javaDefined`, `javaIdentifierIgnorable`, `javaISOControl`, `javaUnicodeIdentifierStart`, `javaUnicodeIdentifierPart`
  - Each corresponds to a `java.lang.Character.isXxx()` predicate.

- **Scripts** with `Is` prefix: `\p{IsLatin}`, `\p{IsGreek}`, `\p{IsCyrillic}`, etc. Both ISO 15924 full names (`IsLatin`) and short codes (`IsLatn`) are accepted.

- **Blocks** with `In` prefix: `\p{InBasicLatin}`, `\p{InGreek}`, `\p{InCJKUnifiedIdeographs}`, etc. The block names follow Unicode's UAX #44 block names with spaces and underscores stripped.

- `\p{L1}` / `\p{Latin1}` — characters U+0000..U+00FF.

**Property syntax variants.**

```text
\pX          → \p{X}   for X a single letter (\pL, \pN, ...)
\p{X}        → property X
\p{IsLatin}  → script Latin
\p{InGreek}  → block Greek
\p{name=value} → property with named value (rarely used; OpenJDK accepts only specific forms)
\p{^X}       → equivalent to \P{X}
```

Property names are matched case-sensitively for the "Is" / "In" / "java" prefixes; categories like `Lu` are case-sensitive (short form is exact case); long names like `Uppercase_Letter` are case-sensitive.

**`\p{Mirrored}` math symbols.** Java's `Character.isMirrored()` is the Bidi_Mirrored Unicode property. For math symbols (`Sm`), OpenJDK uses a curated subset: `∈` (U+2208), `≤` (U+2264), `⊏` (U+228F), etc. are mirrored; `∀` (U+2200) is not. ASCII `<` and `>` are special-cased as mirrored.

---

## 2. Flags and inline flag scoping

Flags can be set at compile time (`Pattern.compile(pat, flags)`), inline (`(?flags)`), or scoped to a group (`(?flags:body)`).

| Flag | Letter | `Pattern.` constant | Effect |
|---|---|---|---|
| CASE_INSENSITIVE | `i` | `CASE_INSENSITIVE` | Match letters case-insensitively (ASCII only unless `u` is also on) |
| MULTILINE | `m` | `MULTILINE` | `^` and `$` match at every line break, not just input edges |
| DOTALL | `s` | `DOTALL` | `.` matches line terminators too |
| COMMENTS | `x` | `COMMENTS` | Strip unescaped whitespace and `# … EOL` comments from the pattern |
| UNICODE_CASE | `u` | `UNICODE_CASE` | Case-insensitivity uses Unicode case folding (not just ASCII) |
| UNICODE_CHARACTER_CLASS | `U` | `UNICODE_CHARACTER_CLASS` | `\d`, `\w`, `\s`, POSIX classes, `\b` use full Unicode sets |
| UNIX_LINES | `d` | `UNIX_LINES` | Only `\n` is a line terminator (`.`, `^`, `$`, `\Z`) |
| LITERAL | — | `LITERAL` | Treat the whole pattern as a literal string (compile-time only) |
| CANON_EQ | — | `CANON_EQ` | (Not implemented; not in scope of this spec) |

**Letter ⇔ constant correspondence.** Inline `(?i)` is `CASE_INSENSITIVE`, `(?m)` is `MULTILINE`, and so on. There is no inline letter for `LITERAL` or `CANON_EQ` (those are constructor-time only).

**Inline flag scoping rules.**

- A bare `(?flags)` at the top level **persists** to the end of the pattern and propagates across `|` alternation branches that come after it. Branches parsed *before* it do not see it.
- A bare `(?flags)` inside any group (`(...)`, `(?:...)`, `(?>...)`, etc.) is **scoped to that group** — the flag change reverts at the closing `)`.
- A scoped flag group `(?flags:X)` applies the flags only to `X`.
- `(?flags-clear)` adds `flags` and removes `clear`. Either side may be empty: `(?-x)` only clears.

**Case-insensitivity (`i`, `u`, `U` interaction).**

- `i` alone: ASCII case folding (`A`-`Z` ↔ `a`-`z`).
- `iu` (or `(?iu)`): Unicode case folding. `Ä` matches `ä`, etc.
- `U` (UNICODE_CHARACTER_CLASS): Expands the *sets* `\d`/`\w`/`\s`/POSIX classes / `\b` to Unicode. Does NOT by itself enable case-folding.
- `iU`: Unicode predefined classes but ASCII case folding (rarely useful — `U` implies wanting Unicode behavior, so `iuU` is the common combination).

---

## 3. Matching operations

`java.util.regex.Matcher` exposes several match operations with subtly different semantics.

### 3.1 `matches()`

Attempts to match the **entire** input (subject to the region, if set). Equivalent to running the pattern with implicit `\A...\z` anchoring (over the region).

- Returns `true` iff the *whole* input from the region start to the region end is consumed by a single match attempt.
- Captures from a successful match are stored.
- A failed `matches()` may leave partial captures from internal sub-paths that succeeded. (See §4.)

### 3.2 `find()`

Scans the input for the next match starting at the current search position (initially region start, or end of previous match for subsequent calls). Returns `true` iff a match is found.

**Position iteration.** OpenJDK's `Start.match` iterates the search position from the current cursor through to (and including) the end of input. At each position, it attempts the pattern; on success, the match ends; on failure, the position advances by one and the attempt repeats. The `groups[]` capture array is **not reset** between attempts. **[Q-8]**

**Empty match advance.** If a `find()` returns a zero-width match at position N, the next `find()` starts at N + 1 to prevent infinite loops.

### 3.3 `lookingAt()`

Attempts to match starting at the **current search position** (initially the region start). The match need not consume the entire input — it just needs to succeed starting at position 0.

- Equivalent to `find()` restricted to starting at the search start position only.
- Useful for "tokenize while pattern matches at the cursor" workflows together with `\G`.

### 3.4 Region

`Matcher.region(start, end)` constrains all subsequent matches to the half-open range `[start, end)`. Position 0-based, UTF-16 code-unit indices.

- `region(start, end)` resets the matcher state (search position, captures, etc.).
- `regionStart()` / `regionEnd()` return the current region.
- `useAnchoringBounds(false)` makes `^`, `\A`, `$`, `\z`, `\Z`, `\b` ignore region boundaries and see the actual input edges.
- `useTransparentBounds(true)` makes lookarounds able to peek outside the region.
- Some character-context anchors (`\Z` looking back at the previous char for `\r\n`) deliberately bypass the region. This is OpenJDK's documented behavior and matters when constructing patterns that consult the boundary character.

### 3.5 `hitEnd()` and `requireEnd()`

After a failed or successful match:

- **`hitEnd()`** returns `true` if the matcher consumed input up to (or past) the region end during the attempt — i.e., the failure or success "touched" the end. Useful for incremental matching: a failed match with `hitEnd()` true means "maybe just need more input."
- **`requireEnd()`** returns `true` if a successful match could have been *longer* given more input — i.e., the match's end position is at the region end and adding characters could extend it. Useful for streaming matchers.

---

## 4. Capture-group state model

This is the part of `Pattern` / `Matcher` semantics least well-described by the Javadoc.

**State storage.** The matcher keeps a single integer array `groups[]` of size `2 * (groupCount + 1)`, holding `(start, end)` pairs for groups 0..N. A capture is considered "set" iff its `start` is non-negative.

**State persistence across constructs.**

- **`Branch.match` (alternation)** — Does *not* save/restore between branches. If branch 1 captures and then later fails (because the rest of the pattern fails after branch 1's commit), captures from branch 1 persist into branch 2's attempt.
- **`Start.match` (find-position loop)** — Does *not* reset `groups[]` between starting-position attempts. Captures from a failed position-N attempt leak into the successful match at a later position. **[Q-8]**
- **`Lookahead` (`(?=X)` and `(?!X)`)** — Does *not* save/restore. Inner captures persist whether the lookahead succeeds or fails (and whether the polarity inverts it).
- **`NotBehind` (`(?<!X)`)** — Does *not* save/restore. If the inner X matches (which then flips the outer to failure), X's captures persist.
- **`Behind` (`(?<=X)`)** — Does *not* save/restore around the inner.
- **`GroupTail` (per-capturing-group end marker)** — *Does* save/restore the group's own slot when the rest of the pattern fails after the group successfully completed. This is the **only** construct that restores capture state.
- **`Curly` / `GroupCurly` (deterministic quantifier)** — Within one iteration, the inner GroupTails do *not* see the outer continuation; they are commits. The outer GroupCurly explicitly re-stamps `groups[idx] = (i - k, i)` for capturing-group atoms after `next.match` succeeds. So `(X){3}` captures the **last** iteration of `X`.
- **`Loop` / `LazyLoop` (non-deterministic quantifier)** — Threads the continuation through the body. Inner GroupTails see the full continuation and restore on downstream failure. So `(a|b){3}` captures the last iteration's value, which is the value at the latest successful chain unwind.

**Consequences (with examples).**

```text
Pattern:    (?=(\w))*\s
Input:      "a "
find()[0]:  start=1, end=2, text=" ", g1="a"     ← captures from failed pos-0 leak
```

```text
Pattern:    (?<!(a|bb))c?
Input:      "ac"
find()[1]:  start=2, end=2, text="", g1="a"     ← inner cap from failed pos-1 negative LB
```

```text
Pattern:    (?:([^\w])+){2}
Input:      "\t\t\r"
find()[0]:  text="\t\t\r", g1="\t"             ← outer GroupCurly's backoff re-stamps g1
```

The third example illustrates the "GroupCurly backoff re-stamp": the inner `([^\w])+` matches once consuming `\t\t\r` (all 3 chars). The outer Loop then needs a second iteration but no chars remain. The inner GroupCurly's backoff loop tries `next.match` at successively smaller match counts; when it finds one that fits (count = 1, `\t` at position 0), it explicitly *re-stamps* `groups[1] = (0, 1) = "\t"` based on its current backoff slice. The recursive 2nd iteration sets `groups[1] = (1, 2) = "\t"`, but the outer GroupCurly's re-stamp at iteration 1 wins on chain unwind. Net effect: `g1 = "\t"` (slice (0,1)).

**Practical rule.** Capture state should be read **only** after a successful match. Reading after a failed match — or between branches — yields partial, attempt-dependent values. The persistence is observable and forms part of the spec; it is not undefined behavior, just often surprising.

---

## 5. `split()`

`Pattern.split(input)` and `Pattern.split(input, limit)` split the input at every non-overlapping occurrence of the pattern.

**Algorithm.** Find matches left-to-right with `find()`. Between matches, emit the text between the previous match's end (or start of input) and the current match's start. After the last match, emit the trailing text.

**Zero-width match suppression at position 0.** If the **very first** match is zero-width *and* at position 0, the would-be leading empty string is **suppressed**. This means:

```text
\Q\E split "abc"    →  ["a", "b", "c"]
\Q\E split "\t"     →  ["\t"]
(?=b) split "abc"   →  ["a", "bc"]
a split "abc"       →  ["", "bc"]            ← non-zero-width → leading empty kept
```

The Javadoc covers this rule; it is easy to miss the first time. **[Q-7]**

**Limit parameter.**

- `limit > 0`: At most `limit - 1` splits performed. The last element contains all remaining input (including any later matches as literal text).
- `limit == 0`: Default. Splits as many times as possible, but **trailing empty strings are removed**.
- `limit < 0`: Splits as many times as possible; trailing empty strings are *retained*.

```text
"a,b,,".split(",")     →  ["a", "b"]          (limit=0, trailing empties removed)
"a,b,,".split(",", -1) →  ["a", "b", "", ""]  (trailing empties kept)
"a,b,,".split(",", 2)  →  ["a", "b,,"]         (limit=2, at most 1 split)
```

---

## 6. Replacement DSL (`appendReplacement` / `replaceAll`)

The replacement string in `Matcher.appendReplacement(sb, replacement)` and `Matcher.replaceAll(replacement)` uses its own mini-language.

**Tokens.**

| Token | Meaning |
|---|---|
| `\$` | Literal `$` |
| `\\` | Literal `\` |
| `\X` (any other X) | Literal X (the `\` is consumed) |
| `$0` | The whole match |
| `$N` (N = 1..9 or more) | Group N's captured text |
| `${name}` | Named group's captured text |

**Multi-digit `$N` rule.** The reader is greedy: it consumes digits *until* the resulting number would exceed the actual group count. So with 5 groups, `$12` is `$1` followed by literal `2`; with 12 groups, `$12` is group 12.

**Null captures.** If group N matched but its value is null (group not executed in the chosen branch), the replacement inserts the empty string.

**Literal `$` and `\` in the replacement.** Use `\$` for `$` and `\\` for `\`. Bare `$` followed by a non-digit / non-`{` is also treated as literal `$` (Java tolerates this — though for safety, escape it).

```text
replace_all("a", r"\$")    →  "$"
replace_all("a", "$$")     →  "$$"  (bare $ before non-digit → literal $)
```

**`Matcher.quoteReplacement(s)`** returns `s` with every `$` and `\` doubled (escaped), useful for inserting arbitrary text without DSL interpretation.

---

## 7. Compile-time errors

OpenJDK's `Pattern.compile` raises `PatternSyntaxException` for the following invalid constructs. The message is `<description> near index <N>\n<pattern>\n<padding>^` where `^` points at the offending position.

| Error | Trigger |
|---|---|
| Unmatched closing brace `)` | `)` without preceding `(` |
| Unmatched closing brace `}` | `}` outside `{n,m}` quantifier or `${name}` |
| Unmatched closing bracket `]` | `]` outside `[…]` |
| Unclosed group | `(` without matching `)` |
| Unclosed character class | `[` without matching `]` |
| Unknown character property `{X}` | `\p{X}` with unrecognized X |
| Look-behind group does not have an obvious maximum length | Lookbehind body has unbounded length |
| Illegal repetition | Quantifier `{n,m}` with m < n, or `{}` with no digits |
| Dangling meta character | `*`, `+`, `?` not preceded by an atom |
| Bad backref | `\N` for N > group count + escape ambiguity |
| Illegal/unsupported escape sequence | `\E` outside `\Q…\E`; `\N{…}` |
| Empty intersection operand | `[…&&]` or `[&&…]` |
| Illegal character range | `[c-c-]` where the second `-` would create an invalid range |
| Invalid hex escape | `\xZZ`, `\u123`, unclosed `\x{…}` |
| Duplicate name | Two named groups with the same name |
| Unknown named group | `\k<name>` or `${name}` referring to a name not defined in the pattern |

The exact message text varies between Java versions; the *condition* is stable.

---

## 8. Quirks index

For each documented divergence from a literal Javadoc reading, see [QUIRKS.md](https://github.com/MathieuDutSik/java_regex/blob/main/QUIRKS.md):

1. **`^` at end of input** — multiline `^` never matches at the very end of input, even after a trailing line terminator.
2. **Deterministic atom atomicity** — `\R{2}`, `(?:\R){2}`, `(?i:\R){2}` are atomic.
3. **`\R` backtracks in sequence but not in a quantifier** — direct consequence of #2.
4. **Chained `[A && B && C]`** — drops trailing clause when the middle operand contains a nested class followed by a literal.
5. **Inline `(?s)` propagates** across alternation, but only at the top level.
6. **`[1-c]/i` matches `g`** — case-insensitive range membership uses input char vs unmodified range.
7. **`split()` suppresses leading empty** for a zero-width match at position 0.
8. **Capture state leaks** across find positions and from failed lookarounds.

For intentional deviations of this Rust port from OpenJDK, see [DIFFERENCES.md](https://github.com/MathieuDutSik/java_regex/blob/main/DIFFERENCES.md) (one item: UTF-16 vs UTF-8 position offsets).

---

## Appendix A: Cross-engine porting tips

- **From PCRE / Perl.** OpenJDK is close but not identical. Notable differences: no `\K` (keep-out), no recursion (`(?R)`), no conditional (`(?(cond)yes|no)`), no possessive at top level (`atomic(?>...)` exists), no `(*VERB)` control verbs. Property syntax `\p{…}` is supported but not all PCRE-specific properties.
- **From `regex` (Rust crate).** Java has lookahead/lookbehind, backreferences, atomic groups, possessive quantifiers — all of which `regex` deliberately excludes. The Rust port (`java_regex`) supports the full surface.
- **From Oniguruma / Onigmo.** Most of OpenJDK's syntax overlaps. `\g<name>` (subroutine call) does *not* exist in Java. Named-group syntax `(?<name>...)` is identical.

## Appendix B: Pattern.compile vs Matcher

`Pattern` is the compiled, immutable, thread-safe representation of a regex. `Matcher` is a per-input scratchpad: it holds the input reference, the current search position, the region bounds, and the `groups[]` capture array. Creating one `Pattern` and reusing it across many `Matcher` instances is the standard idiom and is thread-safe; reusing one `Matcher` across threads is not.
