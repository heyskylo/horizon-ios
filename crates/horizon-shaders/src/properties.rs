//! `.properties` files: `shaders.properties` (spec 3.1) and the id files `block.properties`,
//! `item.properties`, `entity.properties` (4.1). `key=value` lines, `#` comments, a trailing `\`
//! continuing a value on the next line, and conditional directives evaluated with the
//! standard and option macros (1.5 item 2). `#define` and `#undef` work in every one of them:
//! a public pack defines a macro in its `shaders.properties` and tests it further down.
//! No other escape is read: values are names, numbers and expressions. A later duplicate key
//! replaces the earlier one, with a note.

use crate::Macros;
use crate::condition::{Conditionals, split_directive};

/// One `key=value`, with the line it starts on (from 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: String,
    pub value: String,
    pub line: u32,
}

/// Something wrong with a line, for the pack's log.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub line: u32,
    pub message: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Properties {
    /// In file order; a key appears once (a later duplicate replaces the earlier one).
    pub entries: Vec<Entry>,
    pub notes: Vec<Note>,
    /// Shared live shader option environment, when read through `from_pack`.
    pub option_macros: Option<Macros>,
    /// Active dimension when its programs fall back to the base folder.
    pub active_dimension: Option<i32>,
}

impl Properties {
    pub fn in_dimension(mut self, dimension: i32) -> Self {
        self.active_dimension = Some(dimension);
        self
    }
    /// Parses shaders.properties with the shared effective option macros of the selected
    /// dimension. Includes and platform conditionals are evaluated by the same preprocessor
    /// as the shaders. When sources declare options, stages sharing the most widely included
    /// option source determine the settings; standalone stages cannot erase those settings.
    /// A stage-local define is excluded unless those stages agree on it.
    pub fn from_pack(pack: &crate::Pack, folder: &str, macros: &Macros) -> Self {
        let mut stages = Vec::new();
        let mut option_sources = std::collections::BTreeMap::<String, usize>::new();
        for file in pack.list(folder) {
            // Optional integrations such as Distant Horizons have their own feature
            // environment and are not Horizon programs in this frame sequence.
            if ![
                "gbuffers_",
                "shadow",
                "setup",
                "begin",
                "prepare",
                "deferred",
                "composite",
                "final",
            ]
            .iter()
            .any(|prefix| file.starts_with(prefix))
            {
                continue;
            }
            if ![".vsh", ".fsh", ".csh"]
                .iter()
                .any(|ext| file.ends_with(ext))
            {
                continue;
            }
            let path = if folder.is_empty() {
                file
            } else {
                format!("{folder}/{file}")
            };
            let pp = crate::preprocess(pack, &path, macros);
            if !pp.diagnostics.is_empty() {
                continue;
            }
            for source in pp.files.iter().skip(1) {
                // Public option syntax: annotated numeric choices or commented switch defaults.
                // Simple stages which never include options do not erase the pack's settings.
                if pack.read_text(source).is_some_and(|text| {
                    text.lines().any(|line| {
                        let line = line.trim();
                        line.starts_with("//#define")
                            || (line.starts_with("#define")
                                && line.contains("//")
                                && line.contains('['))
                    })
                }) {
                    *option_sources.entry(source.clone()).or_default() += 1;
                }
            }
            stages.push((pp.files, pp.macros));
        }
        let common = option_sources
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(name, _)| name);
        let mut shared: Option<Macros> = None;
        for (files, stage_macros) in stages {
            if common
                .as_ref()
                .is_some_and(|source| !files.contains(source))
            {
                continue;
            }
            match &mut shared {
                Some(shared) => shared.retain_shared(&stage_macros),
                None => shared = Some(stage_macros),
            }
        }
        let effective = shared.unwrap_or_else(|| macros.clone());
        let mut out = Self::parse(
            &pack.read_text("shaders.properties").unwrap_or_default(),
            &effective,
        );
        out.option_macros = Some(effective);
        out
    }

    /// Reads `text`; `#define`/`#undef` change the macros for the lines after them only.
    pub fn parse(text: &str, macros: &Macros) -> Self {
        let mut out = Self::default();
        let mut macros = macros.clone();
        let mut conditionals = Conditionals::default();
        let mut lines = text.lines().zip(1u32..);
        while let Some((raw, number)) = lines.next() {
            let line = raw.trim();
            let on = conditionals.active();
            if let Some(directive) = line.strip_prefix('#') {
                if let Some((word, rest)) = split_directive(directive) {
                    out.directive(word, rest, number, &mut conditionals, &mut macros);
                }
                continue;
            }
            if !on {
                continue;
            }
            // A directive line ends a continued entry, as the empty line the preprocessor leaves
            // for it would.
            let mut logical = line.to_owned();
            while ends_with_continuation(&logical) {
                logical.pop();
                let Some((next, n)) = lines.next() else {
                    break;
                };
                let next = next.trim();
                if let Some(directive) = next.strip_prefix('#') {
                    if let Some((word, rest)) = split_directive(directive) {
                        out.directive(word, rest, n, &mut conditionals, &mut macros);
                    }
                    break;
                }
                logical.push_str(next);
            }
            if logical.is_empty() {
                continue;
            }
            match logical.split_once('=') {
                Some((key, value)) if !key.trim().is_empty() => out.insert(Entry {
                    key: key.trim().to_owned(),
                    value: value.trim().to_owned(),
                    line: number,
                }),
                _ => out.note(number, "not a key=value line"),
            }
        }
        if conditionals.open() {
            out.note(text.lines().count() as u32, "`#if` without `#endif`");
        }
        out
    }

    /// Program toggles use the same live options which selected the property entries.
    pub(crate) fn program_enabled(
        &self,
        folder: &str,
        name: &str,
        macros: &Macros,
        switches: &std::collections::BTreeSet<String>,
    ) -> bool {
        let dimension = self.active_dimension.unwrap_or_else(|| {
            folder
                .strip_prefix("world")
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        });
        let keys = [
            format!("program.{folder}/{name}.enabled"),
            format!("program.world{dimension}/{name}.enabled"),
            format!("program.{name}.enabled"),
        ];
        let Some(expr) = keys.iter().find_map(|key| self.get(key)) else {
            return true;
        };
        let environment = self.option_macros.as_ref().unwrap_or(macros);
        let condition = crate::macros::lex(expr)
            .into_iter()
            .map(|token| -> String {
                match token {
                    "true" => "1".into(),
                    "false" => "0".into(),
                    t if t.starts_with(|c: char| c.is_alphabetic() || c == '_') => {
                        if self.option_macros.is_none() && switches.contains(t) {
                            return "1".into();
                        }
                        match environment.get(t) {
                            Some(value) if value.trim().is_empty() => "1".into(),
                            Some(_) => t.into(),
                            None => "0".into(),
                        }
                    }
                    t => t.into(),
                }
            })
            .collect::<String>();
        crate::eval_condition(&condition, environment).ok() != Some(0)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entry(key).map(|e| e.value.as_str())
    }

    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key == key)
    }

    fn insert(&mut self, entry: Entry) {
        if let Some(at) = self.entries.iter().position(|e| e.key == entry.key) {
            let first = self.entries.remove(at).line;
            self.note(
                entry.line,
                &format!(
                    "`{}` is set again (first on line {first}); this one counts",
                    entry.key
                ),
            );
        }
        self.entries.push(entry);
    }

    fn note(&mut self, line: u32, message: &str) {
        self.notes.push(Note {
            line,
            message: message.to_owned(),
        });
    }

    /// A line starting with `#` whose first word may be a directive; any other word makes it a
    /// comment.
    fn directive(
        &mut self,
        word: &str,
        rest: &str,
        number: u32,
        conditionals: &mut Conditionals,
        macros: &mut Macros,
    ) {
        match conditionals.apply(word, rest, macros) {
            Err(e) => self.note(number, &e),
            Ok(true) => {}
            Ok(false) if conditionals.active() => match word {
                "define" => {
                    let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                    macros.define(name, value.trim());
                }
                "undef" => macros.undef(rest),
                _ => {}
            },
            Ok(false) => {}
        }
    }
}

/// An odd number of trailing backslashes: the last one joins the next line (`\\` is a literal).
fn ends_with_continuation(line: &str) -> bool {
    line.bytes().rev().take_while(|&b| b == b'\\').count() % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Properties {
        let mut m = Macros::standard();
        m.define("SHADOWS", "");
        m.define("QUALITY", "2");
        Properties::parse(text, &m)
    }

    fn pairs(p: &Properties) -> Vec<(&str, &str)> {
        p.entries
            .iter()
            .map(|e| (e.key.as_str(), e.value.as_str()))
            .collect()
    }

    #[test]
    fn dimension_program_rules_apply_to_base_folder_fallback() {
        let macros = Macros::standard();
        let switches = Default::default();
        let make = || {
            Properties::parse(
                "program.world0/shadowcomp.enabled=false\nprogram.world-1/shadowcomp.enabled=true\nprogram.shadowcomp.enabled=false",
                &macros,
            )
        };
        assert!(!make().program_enabled("", "shadowcomp", &macros, &switches));
        assert!(
            !make()
                .in_dimension(0)
                .program_enabled("", "shadowcomp", &macros, &switches)
        );
        assert!(
            make()
                .in_dimension(-1)
                .program_enabled("", "shadowcomp", &macros, &switches)
        );
        assert!(make().program_enabled("world-1", "shadowcomp", &macros, &switches));
        assert!(
            !make()
                .in_dimension(1)
                .program_enabled("", "shadowcomp", &macros, &switches)
        );
    }
    #[test]
    fn pack_properties_use_shared_live_include_options() {
        let pack = crate::Pack::from_zip_bytes("options", crate::pack::tests::zip(&[
            ("shaders/settings.glsl", "#define SIZE 192 //[128 192]\n#if !defined MC_OS_MAC\n#define VOXELS\n#endif"),
            ("shaders/world0/composite.vsh", "#define VERTEX_ONLY\n#include \"/settings.glsl\""),
            ("shaders/world0/composite.fsh", "#include \"/settings.glsl\""),
            ("shaders/world0/final.vsh", "void main(){}"),
            ("shaders/world0/dh_terrain.fsh", "#include \"/settings.glsl\"\n#undef VOXELS"),
            ("shaders/world-1/composite.fsh", "#define SIZE 128"),
            ("shaders/shaders.properties", "#if SIZE == 192 && defined VOXELS\nimage.volume=vox rgba rgba16f half_float true false 192 96 192\n#endif\nprogram.shadowcomp.enabled=VOXELS && SIZE == 192\n#ifdef VERTEX_ONLY\nbad=true\n#endif"),
        ])).unwrap();
        let mut macros = Macros::standard();
        macros.undef("MC_OS_MAC");
        let properties = Properties::from_pack(&pack, "world0", &macros);
        assert!(properties.get("image.volume").is_some());
        assert!(properties.get("bad").is_none());
        let raw_switches = std::collections::BTreeSet::from(["VOXELS".into()]);
        assert!(properties.program_enabled("world0", "shadowcomp", &macros, &raw_switches));
        macros.define("MC_OS_MAC", "");
        let disabled = Properties::from_pack(&pack, "world0", &macros);
        assert!(disabled.get("image.volume").is_none());
        assert!(!disabled.program_enabled("world0", "shadowcomp", &macros, &raw_switches));
        assert!(
            Properties::from_pack(&pack, "world-1", &macros)
                .get("image.volume")
                .is_none()
        );
    }

    #[test]
    fn keys_values_and_comments() {
        let p =
            read("# a comment\n\n  sun = false\n#commented.key=1\nscreen=A B [C]\r\nbad line\n");
        assert_eq!(pairs(&p), [("sun", "false"), ("screen", "A B [C]")]);
        assert_eq!(p.entry("screen").unwrap().line, 5);
        assert_eq!(
            p.notes,
            [Note {
                line: 6,
                message: "not a key=value line".into()
            }]
        );
    }

    #[test]
    fn continuation_lines_join() {
        let p = read("screen=A B \\\n    C D\\\n  E\nsliders=X\nliteral=a\\\\\nnext=1");
        assert_eq!(
            pairs(&p),
            [
                ("screen", "A B C DE"),
                ("sliders", "X"),
                ("literal", "a\\\\"),
                ("next", "1")
            ]
        );
        assert_eq!(p.entry("sliders").unwrap().line, 4);
    }

    /// A continued value ending before `#else` stops there, and the line after `#endif` is an
    /// entry of its own.
    #[test]
    fn continuations_stop_at_directives() {
        let text = "#define ON\n#ifdef ON\nu = if( \\\n  a, 1.0, \\\n  0.5) \\\n#else\nu = 1.0\n#endif\n\n\
                    #ifdef OFF\nv = 1 \\\n  2\n#endif\nw = 3";
        assert_eq!(pairs(&read(text)), [("u", "if( a, 1.0, 0.5)"), ("w", "3")]);
        let text = "#define ON\n#ifdef ON\nu = 1 \\\n#else\nu = 2\n#endif\nw = 3";
        assert_eq!(pairs(&read(text)), [("u", "1"), ("w", "3")]);
    }

    #[test]
    fn conditionals_pick_one_branch() {
        let p = read(
            "#if MC_VERSION >= 11300\nnew=1\n#else\nold=1\n#endif\n\
             #ifdef SHADOWS\n#if QUALITY == 1\nq=1\n#elif QUALITY == 2\nq=2\n#elif QUALITY >= 2\nq=3\n#endif\n#endif\n\
             #ifndef SHADOWS\nnone=1\n#elif 0\nnever=1\n#else\nfallback=1\n#endif\n#   ifdef SHADOWS // trailing comment\nspaced=1\n#endif",
        );
        assert_eq!(
            pairs(&p),
            [("new", "1"), ("q", "2"), ("fallback", "1"), ("spaced", "1")]
        );
        assert!(p.notes.is_empty(), "{:?}", p.notes);
    }

    #[test]
    fn comment_lines_are_not_directives() {
        let p = read("#ifdefault=1\n#iffy\n#endiff\n#else-where\nkey=1");
        assert_eq!(pairs(&p), [("key", "1")]);
        assert!(p.notes.is_empty(), "{:?}", p.notes);
    }

    #[test]
    fn defines_apply_to_the_lines_after_them() {
        let text = "#ifdef MC_OS_NONE\n#define WATER 0\n#endif\n#define WATER 8\n\
                    #if WATER == 8\nblock.8=minecraft:water\n#endif\n#undef WATER\n#ifdef WATER\nno=1\n#endif";
        let p = Properties::parse(text, &Macros::default());
        assert_eq!(pairs(&p), [("block.8", "minecraft:water")]);
        assert!(p.notes.is_empty(), "{:?}", p.notes);
    }

    #[test]
    fn duplicate_keys_keep_the_last() {
        let p = read("a=1\nb=2\na=3");
        assert_eq!(pairs(&p), [("b", "2"), ("a", "3")]);
        assert_eq!(p.notes[0].line, 3);
    }

    #[test]
    fn broken_conditionals_are_noted() {
        let p = read("#endif\n#else\n#if 1 +\nx=1\n#endif\n#if 1\n#else\n#else\n");
        let lines: Vec<u32> = p.notes.iter().map(|n| n.line).collect();
        assert_eq!(lines, [1, 2, 3, 8, 8]);
        assert!(p.entries.is_empty());
    }
}
