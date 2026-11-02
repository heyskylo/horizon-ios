//! World-directory symbolic-link policy (`DirectoryValidator` / `PathAllowList`).
//! Validate before acquiring the world lock: even opening session.lock can follow a link.
use java_regex::Regex;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct ForbiddenLink {
    pub link: PathBuf,
    pub target: PathBuf,
}

/// Returns forbidden links without modifying the world. A nonexistent world is
/// valid. I/O failures other than a missing world are reported to the caller.
/// As in validateDirectory(path, true), a top-level world link is followed;
/// descendant links are checked against their raw target, never traversed.
pub fn validate(world: &Path, allowlist: &Path) -> io::Result<Vec<ForbiddenLink>> {
    let policy = fs::read_to_string(allowlist)
        .ok()
        .and_then(|s| Policy::parse(&s).ok())
        .unwrap_or_default();
    let metadata = match fs::symlink_metadata(world) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    if metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "World path is not a directory",
        ));
    }
    // Java uses readSymbolicLink directly here, rather than canonicalizing.
    let root = if metadata.file_type().is_symlink() {
        fs::read_link(world)?
    } else {
        world.to_path_buf()
    };
    let mut forbidden = Vec::new();
    visit(&root, &policy, &mut forbidden)?;
    Ok(forbidden)
}

fn visit(path: &Path, policy: &Policy, forbidden: &mut Vec<ForbiddenLink>) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(path)?;
        if !policy.matches(&target) {
            forbidden.push(ForbiddenLink {
                link: path.to_path_buf(),
                target,
            });
        }
    } else if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            visit(&entry?.path(), policy, forbidden)?;
        }
    }
    Ok(())
}

#[derive(Default)]
struct Policy(Vec<Matcher>);
enum Matcher {
    Prefix(String),
    Pattern(Regex),
}
impl Policy {
    fn parse(text: &str) -> Result<Self, ()> {
        let mut entries = Vec::new();
        // BufferedReader.lines recognizes CR, LF and CRLF; it does not trim.
        for line in text.split(['\r', '\n']) {
            if line.chars().all(java_whitespace) || line.starts_with('#') {
                continue;
            }
            let (kind, pattern) = if let Some(rest) = line.strip_prefix('[') {
                let (kind, pattern) = rest.split_once(']').ok_or(())?;
                (kind, pattern)
            } else {
                ("prefix", line)
            };
            entries.push(match kind {
                "prefix" => Matcher::Prefix(pattern.to_owned()),
                "regex" => Matcher::Pattern(compile(pattern)?),
                "glob" => Matcher::Pattern(compile(&unix_glob(pattern)?)?),
                _ => return Err(()),
            });
        }
        Ok(Self(entries))
    }
    fn matches(&self, path: &Path) -> bool {
        let path = path_text(path);
        self.0.iter().any(|matcher| match matcher {
            Matcher::Prefix(prefix) => path.starts_with(prefix),
            Matcher::Pattern(pattern) => pattern.matches(&path),
        })
    }
}

#[cfg(unix)]
fn path_text(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut bytes = path.as_os_str().as_bytes();
    let mut text = String::new();
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(valid) => {
                text.push_str(valid);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                text.push_str(std::str::from_utf8(&bytes[..valid]).unwrap());
                bytes = &bytes[valid..];
                // Java's UTF-8 decoder consumes a malformed surrogate sequence
                // as one replacement. Rust's scalar decoder replaces each byte.
                let consumed = if bytes[0] == 0xed
                    && bytes.get(1).is_some_and(|b| (0xa0..=0xbf).contains(b))
                {
                    2 + usize::from(bytes.get(2).is_some_and(|b| (0x80..=0xbf).contains(b)))
                } else {
                    error.error_len().unwrap_or(bytes.len())
                };
                text.push('\u{fffd}');
                bytes = &bytes[consumed..];
            }
        }
    }
    text
}

#[cfg(not(unix))]
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn java_whitespace(c: char) -> bool {
    matches!(c, '\u{9}'..='\u{d}' | '\u{1c}'..='\u{20}' | '\u{1680}' | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{205f}' | '\u{3000}')
}

fn compile(pattern: &str) -> Result<Regex, ()> {
    Regex::new_with_name_resolver(pattern, java_named_character).map_err(|_| ())
}

const CONTROL_NAMES: &[(u32, &str)] = &[
    (0x0, "NULL"),
    (0x1, "START OF HEADING"),
    (0x2, "START OF TEXT"),
    (0x3, "END OF TEXT"),
    (0x4, "END OF TRANSMISSION"),
    (0x5, "ENQUIRY"),
    (0x6, "ACKNOWLEDGE"),
    (0x7, "BEL"),
    (0x8, "BACKSPACE"),
    (0x9, "CHARACTER TABULATION"),
    (0xA, "LINE FEED (LF)"),
    (0xB, "LINE TABULATION"),
    (0xC, "FORM FEED (FF)"),
    (0xD, "CARRIAGE RETURN (CR)"),
    (0xE, "SHIFT OUT"),
    (0xF, "SHIFT IN"),
    (0x10, "DATA LINK ESCAPE"),
    (0x11, "DEVICE CONTROL ONE"),
    (0x12, "DEVICE CONTROL TWO"),
    (0x13, "DEVICE CONTROL THREE"),
    (0x14, "DEVICE CONTROL FOUR"),
    (0x15, "NEGATIVE ACKNOWLEDGE"),
    (0x16, "SYNCHRONOUS IDLE"),
    (0x17, "END OF TRANSMISSION BLOCK"),
    (0x18, "CANCEL"),
    (0x19, "END OF MEDIUM"),
    (0x1A, "SUBSTITUTE"),
    (0x1B, "ESCAPE"),
    (0x1C, "INFORMATION SEPARATOR FOUR"),
    (0x1D, "INFORMATION SEPARATOR THREE"),
    (0x1E, "INFORMATION SEPARATOR TWO"),
    (0x1F, "INFORMATION SEPARATOR ONE"),
    (0x7F, "DELETE"),
    (0x80, "PADDING CHARACTER"),
    (0x81, "HIGH OCTET PRESET"),
    (0x82, "BREAK PERMITTED HERE"),
    (0x83, "NO BREAK HERE"),
    (0x84, "LATIN 1 SUPPLEMENT 84"),
    (0x85, "NEXT LINE (NEL)"),
    (0x86, "START OF SELECTED AREA"),
    (0x87, "END OF SELECTED AREA"),
    (0x88, "CHARACTER TABULATION SET"),
    (0x89, "CHARACTER TABULATION WITH JUSTIFICATION"),
    (0x8A, "LINE TABULATION SET"),
    (0x8B, "PARTIAL LINE FORWARD"),
    (0x8C, "PARTIAL LINE BACKWARD"),
    (0x8D, "REVERSE LINE FEED"),
    (0x8E, "SINGLE SHIFT TWO"),
    (0x8F, "SINGLE SHIFT THREE"),
    (0x90, "DEVICE CONTROL STRING"),
    (0x91, "PRIVATE USE ONE"),
    (0x92, "PRIVATE USE TWO"),
    (0x93, "SET TRANSMIT STATE"),
    (0x94, "CANCEL CHARACTER"),
    (0x95, "MESSAGE WAITING"),
    (0x96, "START OF GUARDED AREA"),
    (0x97, "END OF GUARDED AREA"),
    (0x98, "START OF STRING"),
    (0x99, "SINGLE GRAPHIC CHARACTER INTRODUCER"),
    (0x9A, "SINGLE CHARACTER INTRODUCER"),
    (0x9B, "CONTROL SEQUENCE INTRODUCER"),
    (0x9C, "STRING TERMINATOR"),
    (0x9D, "OPERATING SYSTEM COMMAND"),
    (0x9E, "PRIVACY MESSAGE"),
    (0x9F, "APPLICATION PROGRAM COMMAND"),
];

// Java uses block-name + uppercase hex for defined characters whose names are
// algorithmic (and for private-use/surrogate code points), rather than UAX44's
// Hangul syllable / singular CJK IDEOGRAPH spelling.
const JAVA_NAME_BLOCKS: &[(u32, u32, &str, &str)] = &[
    (
        0x3400,
        0x4DBF,
        "CJK UNIFIED IDEOGRAPHS EXTENSION A",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (
        0x4E00,
        0x9FFF,
        "CJK UNIFIED IDEOGRAPHS",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (
        0x20000,
        0x2A6DF,
        "CJK UNIFIED IDEOGRAPHS EXTENSION B",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (
        0x2A700,
        0x2B739,
        "CJK UNIFIED IDEOGRAPHS EXTENSION C",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (
        0x2B740,
        0x2B81D,
        "CJK UNIFIED IDEOGRAPHS EXTENSION D",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (
        0x2B820,
        0x2CEA1,
        "CJK UNIFIED IDEOGRAPHS EXTENSION E",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (
        0x2CEB0,
        0x2EBE0,
        "CJK UNIFIED IDEOGRAPHS EXTENSION F",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (
        0x2EBF0,
        0x2EE5D,
        "CJK UNIFIED IDEOGRAPHS EXTENSION I",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (
        0x30000,
        0x3134A,
        "CJK UNIFIED IDEOGRAPHS EXTENSION G",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (
        0x31350,
        0x323AF,
        "CJK UNIFIED IDEOGRAPHS EXTENSION H",
        "CJK UNIFIED IDEOGRAPH",
    ),
    (0xAC00, 0xD7A3, "HANGUL SYLLABLES", ""),
    (0x17000, 0x187F7, "TANGUT", "TANGUT IDEOGRAPH"),
    (0x18D00, 0x18D08, "TANGUT SUPPLEMENT", "TANGUT IDEOGRAPH"),
    (0xE000, 0xF8FF, "PRIVATE USE AREA", ""),
    (0xF0000, 0xFFFFD, "SUPPLEMENTARY PRIVATE USE AREA A", ""),
    (0x100000, 0x10FFFD, "SUPPLEMENTARY PRIVATE USE AREA B", ""),
    (0xD800, 0xDB7F, "HIGH SURROGATES", ""),
    (0xDB80, 0xDBFF, "HIGH PRIVATE USE SURROGATES", ""),
    (0xDC00, 0xDFFF, "LOW SURROGATES", ""),
];

fn java_named_character(name: &str) -> Option<u32> {
    // Character.codePointOf uses String.trim (ASCII <= U+0020), then ROOT uppercase.
    let name = name.trim_matches(|c: char| c <= ' ').to_uppercase();
    if let Some((point, _)) = CONTROL_NAMES.iter().find(|(_, n)| *n == name) {
        return Some(*point);
    }
    if let Some((block, hex)) = name.rsplit_once(' ')
        && let Ok(point) = u32::from_str_radix(hex, 16)
        && format!("{point:X}") == hex
        && JAVA_NAME_BLOCKS
            .iter()
            .any(|(lo, hi, n, _)| point >= *lo && point <= *hi && *n == block)
    {
        return Some(point);
    }
    let scalar = unicode_names2::character(&name)?;
    let point = u32::from(scalar);
    // These algorithmic Unicode names are not accepted by Java's name lookup.
    if JAVA_NAME_BLOCKS
        .iter()
        .any(|(lo, hi, _, _)| point >= *lo && point <= *hi)
    {
        return None;
    }
    let canonical = unicode_names2::name(scalar)?.to_string();
    (canonical == name).then_some(point)
}

fn literal(out: &mut String, c: char) {
    if ".^$+{}[]|()\\*?".contains(c) {
        out.push('\\');
    }
    out.push(c);
}

/// Unix FileSystem glob grammar, following JDK Globs.toUnixRegexPattern:
/// stars cannot cross '/', '**' can, classes exclude '/', and groups cannot nest.
fn unix_glob(pattern: &str) -> Result<String, ()> {
    let chars: Vec<_> = pattern.chars().collect();
    let mut i = 0;
    let mut group = false;
    let mut out = String::new();
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '\\' => {
                let c = *chars.get(i).ok_or(())?;
                i += 1;
                literal(&mut out, c);
            }
            '*' if chars.get(i) == Some(&'*') => {
                i += 1;
                // Java '.' excludes all five line terminators by default.
                out.push_str("[^\\n\\r\u{85}\u{2028}\u{2029}]*");
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '{' => {
                if group {
                    return Err(());
                }
                group = true;
                out.push_str("(?:(?:");
            }
            '}' if group => {
                group = false;
                out.push_str("))");
            }
            ',' if group => out.push_str(")|(?:"),
            '[' => {
                out.push_str("[[^/]&&[");
                if chars.get(i) == Some(&'^') {
                    out.push_str("\\^");
                    i += 1;
                } else {
                    if chars.get(i) == Some(&'!') {
                        out.push('^');
                        i += 1;
                    }
                    if chars.get(i) == Some(&'-') {
                        out.push('-');
                        i += 1;
                    }
                }
                let mut previous = None;
                loop {
                    let c = *chars.get(i).ok_or(())?;
                    i += 1;
                    if c == ']' {
                        break;
                    }
                    if c == '/' {
                        return Err(());
                    }
                    if c == '\\' || c == '[' || (c == '&' && chars.get(i) == Some(&'&')) {
                        out.push('\\');
                    }
                    out.push(c);
                    if c == '-' {
                        let from = previous.take().ok_or(())?;
                        let to = *chars.get(i).ok_or(())?;
                        i += 1;
                        if to == ']' {
                            break;
                        }
                        if to < from {
                            return Err(());
                        }
                        out.push(to);
                    } else {
                        previous = Some(c);
                    }
                }
                out.push_str("]]");
            }
            c => literal(&mut out, c),
        }
    }
    if group {
        return Err(());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the private Java PathAllowList oracle JSONL"]
    fn compare_java_reference_matrix() {
        let path = std::env::var_os("HORIZON_WORLD_LINK_ORACLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/world-link-policy-audit/pattern-vanilla.jsonl")
            });
        let mut mismatches = Vec::new();
        for line in fs::read_to_string(path).unwrap().lines() {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let pattern = row["pattern"].as_str().unwrap();
            let target = row["target"].as_str().unwrap();
            let expected = row["matches"].as_bool().unwrap();
            let got = Policy::parse(pattern)
                .unwrap_or_default()
                .matches(Path::new(target));
            if got != expected {
                mismatches.push(format!(
                    "{pattern:?}, {target:?}: Java={expected}, Rust={got:?}"
                ));
            }
        }
        for mismatch in &mismatches {
            eprintln!("{mismatch}");
        }
        assert!(
            mismatches.is_empty(),
            "{} Java policy mismatches",
            mismatches.len()
        );
    }

    #[cfg(unix)]
    #[test]
    fn raw_path_decoding_follows_java_replacement_boundaries() {
        use std::os::unix::ffi::OsStrExt;
        let cases: &[(&[u8], &str)] = &[
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xff,
                ],
                "../trusted/\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xc0, 0xaf,
                ],
                "../trusted/\u{fffd}\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xe0, 0x80,
                    0x80,
                ],
                "../trusted/\u{fffd}\u{fffd}\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xed, 0xa0,
                    0x80,
                ],
                "../trusted/\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xed, 0xbf,
                    0xbf,
                ],
                "../trusted/\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xed, 0xa0,
                ],
                "../trusted/\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xed, 0xbf,
                    0x41,
                ],
                "../trusted/\u{fffd}A",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xed, 0xa0,
                    0x41,
                ],
                "../trusted/\u{fffd}A",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xed, 0xa0,
                    0xc0,
                ],
                "../trusted/\u{fffd}\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xed, 0xa0,
                    0x80, 0x41,
                ],
                "../trusted/\u{fffd}A",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xf4, 0x90,
                    0x80, 0x80,
                ],
                "../trusted/\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xe1, 0x80,
                ],
                "../trusted/\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xf1, 0x80,
                    0x80,
                ],
                "../trusted/\u{fffd}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xed, 0x9f,
                    0xbf,
                ],
                "../trusted/\u{d7ff}",
            ),
            (
                &[
                    0x2e, 0x2e, 0x2f, 0x74, 0x72, 0x75, 0x73, 0x74, 0x65, 0x64, 0x2f, 0xf0, 0x90,
                    0x80, 0x80,
                ],
                "../trusted/\u{10000}",
            ),
        ];
        for (bytes, expected) in cases {
            let path = Path::new(std::ffi::OsStr::from_bytes(bytes));
            assert_eq!(path_text(path), *expected, "{bytes:x?}");
        }
    }

    /// Golden results from the pinned client's PathAllowList on Java 25.
    #[test]
    fn java_pattern_reference_cases() {
        let cases = [
            ("[glob]/a/*", "/a/one", true),
            ("[glob]/a/*", "/a/one/two", false),
            ("[glob]/a/**", "/a/one/two", true),
            ("[glob]/a/**", "/a/a\nb", false),
            ("[glob]a/{b,c?}/[!0-9]", "a/cx/q", true),
            ("[glob]a/{b,c?}/[!0-9]", "a/b/5", false),
            ("[glob]a/\\*", "a/*", true),
            ("[regex]a|abc", "abc", true),
            ("[regex]a", "aa", false),
            ("[regex]\\d+", "١", false),
            ("[regex]\\w+", "é", false),
            ("[regex](a)\\1", "aa", true),
            ("[regex][a-z&&[^aeiou]]+", "bcdf", true),
            ("[regex](?U)\\w+", "é", true),
            ("[regex](?iu)é", "É", true),
            ("[regex](?<name>x)\\k<name>", "xx", true),
            ("[regex]\\p{javaLowerCase}+", "é", true),
            ("[regex]\\N{LATIN CAPITAL LETTER A}", "A", true),
            ("[regex]a(?<=a)b", "ab", true),
            ("[regex]\\h+", "\t", true),
            ("[regex]\\R", "\r\n", true),
            ("[regex]\\Q/a.*\\E", "/a.*", true),
            ("[regex](?s).+", "a\nb", true),
            ("[regex].+", "a\rb", false),
            ("[regex](?d).+", "a\rb", true),
            ("[regex](?iu)ß", "ss", false),
            ("[regex](?iu)ﬀ", "ff", false),
            ("[regex](?iu)k", "K", true),
            ("[regex](?i)k", "K", false),
            ("[regex](?i)k(?u)x", "KX", false),
            ("[regex](?i:a)(?iu:b)", "AB", true),
            ("[regex](?U:\\w+)", "é", true),
            ("[regex](?u:i)", "İ", false),
            ("[regex](?iu:i)", "İ", true),
            ("[regex](?i)é", "É", false),
            ("[regex]\\h+", " ", true),
            ("[regex]\\v", "\u{85}", true),
            ("[regex]\\R", "\u{85}", true),
            ("[regex].", "\u{85}", false),
            ("[regex]\\x{85}", "\u{85}", true),
            ("[glob]**", "a\u{85}b", false),
            ("[regex]\\uD83D\\uDE00", "😀", true),
            ("[regex][\\uD83D\\uDE00]", "😀", true),
            ("[regex][\\uD800-b]", "a", false),
            ("[regex][a-\\uDFFF]", "é", true),
            ("[regex][\\uD800-\\uDFFF]", "a", false),
            (
                "[regex]\\N{HIGH SURROGATES D800}\\N{LOW SURROGATES DC00}",
                "𐀀",
                false,
            ),
            ("[regex]\\N{HIGH SURROGATES D800}", "a", false),
            (
                "[prefix]/allowed\n[regex]\\N{HIGH SURROGATES D800}",
                "/allowed/x",
                true,
            ),
            ("[regex][^\\N{HIGH SURROGATES D800}]", "a", true),
            ("[regex][a-\\N{HIGH SURROGATES D800}]", "é", true),
            (
                "[regex][\\N{HIGH SURROGATES D800}-\\N{PRIVATE USE AREA E000}]",
                "",
                true,
            ),
            (
                "[regex][\\N{HIGH SURROGATES D800}-\\N{LOW SURROGATES DFFF}]",
                "a",
                false,
            ),
            ("[regex]\\Q\\N{SNOWMAN}\\E", "\\N{SNOWMAN}", true),
            ("[regex]\\\\N\\{SNOWMAN\\}", "\\N{SNOWMAN}", true),
            ("[regex](?x)#\\N{BOGUS}", "", true),
            ("[regex](?x)#\\N{SNOWMAN}", "", true),
            ("[regex](?iu)(ẞ)\\1", "ẞß", true),
            ("[regex](?i)(k)\\1", "kK", false),
            ("[regex](?iu)(k)\\1", "kK", true),
            ("[regex](?U)\\p{Lower}+", "é", true),
            ("[regex](?iu)[a&&A]", "a", true),
            ("[regex](?iu)ß", "ẞ", false),
            ("[regex](?iu)ẞ", "ß", true),
            ("[regex]a*+a", "aaa", false),
            ("[regex](?<x>a)\\k<x>", "aa", true),
            ("[regex](?iu)[ß]", "ẞ", false),
            ("[regex](?iu)[ẞ]", "ß", true),
            ("[regex](?iu)[a&&A]", "A", true),
            ("[regex](?iu)ı", "İ", true),
        ];
        for (pattern, target, expected) in cases {
            let actual = Policy::parse(pattern)
                .unwrap_or_default()
                .matches(Path::new(target));
            assert_eq!(actual, expected, "{pattern:?}: {target:?}");
        }
    }

    #[test]
    fn named_characters_use_strict_java_names_not_loose_uax44_matching() {
        for (name, point) in [
            (" LATIN CAPITAL LETTER A ", 0x41),
            ("latin capital letter a", 0x41),
            ("NULL", 0),
            ("LINE FEED (LF)", 10),
            ("SNOWMAN", 0x2603),
            ("CJK UNIFIED IDEOGRAPHS 4E00", 0x4E00),
            ("CJK UNIFIED IDEOGRAPHS EXTENSION A 3400", 0x3400),
            ("PRIVATE USE AREA E000", 0xE000),
            ("HIGH SURROGATES D800", 0xD800),
            ("CJK COMPATIBILITY IDEOGRAPH-F900", 0xF900),
            ("TANGUT 17000", 0x17000),
            ("TANGUT SUPPLEMENT 18D08", 0x18D08),
        ] {
            assert_eq!(java_named_character(name), Some(point), "{name}");
        }
        for name in [
            "LATIN  CAPITAL LETTER A",
            "LATINCAPITALLETTERA",
            "LATIN-CAPITAL-LETTER-A",
            "NUL",
            "LINE FEED",
            "LF",
            "BOM",
            "BYTE ORDER MARK",
            "LATIN CAPITAL LETTER GHA",
            "HANGUL SYLLABLE GA",
            "CJK UNIFIED IDEOGRAPH-4E00",
            "CJK UNIFIED IDEOGRAPHS 04E00",
            "TANGUT IDEOGRAPH-17000",
            "TANGUT 187F8",
            "TANGUT SUPPLEMENT 18D09",
            "CJK UNIFIED IDEOGRAPHS EXTENSION C 2B73A",
            "UNASSIGNED 0378",
            "BASIC LATIN 0041",
        ] {
            assert_eq!(java_named_character(name), None, "{name}");
        }
    }

    #[test]
    fn named_escapes_are_resolved_by_the_lexer_not_text_replacement() {
        assert!(Policy::parse("/allowed\n[regex]\\N{3}").is_err());
        assert!(Policy::parse("[regex][\\N{3}]").is_err());
        assert!(Policy::parse("/allowed\n[regex]\\q").is_err());
        assert!(compile(r"\Q\N{3}\E").unwrap().matches(r"\N{3}"));
        assert!(compile(r"[\Q\N{3}\E]+").unwrap().matches(r"\N{3}"));
        assert!(compile(r"(?x)#\N{3}").unwrap().matches(""));
        assert!(compile(r"\N{SNOWMAN}").unwrap().matches("☃"));
    }

    #[test]
    fn prefix_and_invalid_configuration_follow_java() {
        assert!(
            Policy::parse("# comment\n\u{2003}\n/prefix")
                .unwrap()
                .matches(Path::new("/prefix-suffix"))
        );
        assert!(
            !Policy::parse(" \n#nothing")
                .unwrap()
                .matches(Path::new("/x"))
        );
        assert!(Policy::parse("/valid\n[unknown]x").is_err());
        assert!(Policy::parse("/valid\n[regex](").is_err());
        assert!(Policy::parse("/valid\n[regex]x{0,2147483648}").is_err());
        assert!(compile("x{0,2147483647}").unwrap().matches(""));
        assert!(Policy::parse("[glob]{nested,{group}}").is_err());
    }
    #[test]
    fn unix_globs_and_regex_match_whole_raw_targets() {
        for (pattern, target, expected) in [
            ("[glob]/a/*", "/a/one", true),
            ("[glob]/a/*", "/a/one/two", false),
            ("[glob]/a/**", "/a/one/two", true),
            ("[glob]/a/**", "/a/a\nb", false),
            ("[glob]a/{b,c?}/[!0-9]", "a/cx/q", true),
            ("[glob]a/{b,c?}/[!0-9]", "a/b/5", false),
            ("[glob]a/\\*", "a/*", true),
            ("[regex]a|abc", "abc", true),
            ("[regex]a", "aa", false),
            (r"[regex]\d+", "١", false),
            (r"[regex]\w+", "é", false),
            (r"[regex](a)\1", "aa", true),
            (r"[regex][a-z&&[^aeiou]]+", "bcdf", true),
        ] {
            assert_eq!(
                Policy::parse(pattern).unwrap().matches(Path::new(target)),
                expected,
                "{pattern}: {target}"
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn validates_nested_raw_links_and_allows_root_link() {
        use std::os::unix::fs::symlink;
        let scratch = std::env::temp_dir().join(format!(
            "horizon-links-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(scratch.join("world")).unwrap();
        let policy = scratch.join("allowed_symlinks.txt");
        symlink("../target", scratch.join("world/nested")).unwrap();
        assert_eq!(validate(&scratch.join("world"), &policy).unwrap().len(), 1);
        fs::write(&policy, "[prefix]../target").unwrap();
        assert!(
            validate(&scratch.join("world"), &policy)
                .unwrap()
                .is_empty()
        );
        fs::remove_file(scratch.join("world/nested")).unwrap();
        use std::os::unix::ffi::OsStrExt;
        symlink(
            std::ffi::OsStr::from_bytes(b"../target/\xff"),
            scratch.join("world/nested"),
        )
        .unwrap();
        assert!(
            validate(&scratch.join("world"), &policy)
                .unwrap()
                .is_empty()
        );
        fs::remove_file(scratch.join("world/nested")).unwrap();
        symlink(scratch.join("world"), scratch.join("root-link")).unwrap();
        fs::remove_file(policy).unwrap();
        assert!(
            validate(&scratch.join("root-link"), &scratch.join("absent"))
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(scratch).unwrap();
    }
}
