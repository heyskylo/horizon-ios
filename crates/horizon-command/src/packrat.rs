//! A port of vanilla's packrat parsing library (`net.minecraft.util.parsing.packrat`):
//! terms, named rules with per-position memoisation, scopes with frames,
//! cuts, and the "longest only" error collector whose entries become both
//! the parse error and the completion suggestions.
use super::{CmdResult, CommandError, Reader, SuggestionsBuilder, args};
use std::collections::HashMap;
use std::rc::Rc;

pub type Atom = &'static str;
pub type RuleId = usize;

/// A value a rule produces (an element of `Scope`).
pub trait Value: Clone {
    fn list(items: Vec<Self>) -> Self;
}

/// Why a term failed (DelayedException or a ready exception).
#[derive(Clone, Debug)]
pub enum Reason {
    /// A translatable exception created at the error cursor.
    Delayed(&'static str, Vec<String>),
    /// An exception with its own cursor.
    Error(CommandError),
}

/// SuggestionSupplier.
#[derive(Clone, Debug)]
pub enum Sugg {
    Empty,
    Values(Rc<Vec<String>>),
    /// ResourceSuggestion: ids completed with suggestResource.
    Resources(Rc<Vec<String>>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Key {
    Frame(usize),
    Atom(Atom),
}

/// Scope: a stack of frames of atom values.
pub struct Scope<V> {
    stack: Vec<(Key, Option<V>)>,
    top_entry: usize,
    top_marker: usize,
}
impl<V: Clone> Scope<V> {
    fn new() -> Self {
        Self {
            stack: vec![(Key::Frame(0), None)],
            top_entry: 0,
            top_marker: 0,
        }
    }
    fn set(&mut self, i: usize, key: Key, value: Option<V>) {
        while self.stack.len() <= i {
            self.stack.push((Key::Frame(0), None));
        }
        self.stack[i] = (key, value);
    }
    fn value_index(&self, atoms: &[Atom]) -> Option<usize> {
        let mut i = self.top_entry;
        while i > self.top_marker {
            if let Key::Atom(a) = self.stack[i].0
                && atoms.contains(&a)
            {
                return Some(i);
            }
            i -= 1;
        }
        None
    }
    fn setup_new_frame(&mut self) {
        self.top_entry += 1;
        let prev = self.top_marker;
        self.set(self.top_entry, Key::Frame(prev), None);
        self.top_marker = self.top_entry;
    }
    fn previous_marker(&self, marker: usize) -> usize {
        match self.stack[marker].0 {
            Key::Frame(p) => p,
            Key::Atom(_) => 0,
        }
    }
    pub fn push_frame(&mut self) {
        self.setup_new_frame();
    }
    pub fn pop_frame(&mut self) {
        let marker = self.top_marker;
        self.top_entry = marker - 1;
        self.top_marker = self.previous_marker(marker);
    }
    fn split_frame(&mut self) {
        let current = self.top_marker;
        let n = self.top_entry - self.top_marker;
        self.setup_new_frame();
        let mut target = self.top_entry;
        for source in current + 1..current + 1 + n {
            target += 1;
            let key = self.stack[source].0;
            self.set(target, key, None);
        }
        self.top_entry = target;
    }
    fn clear_frame_values(&mut self) {
        let mut i = self.top_entry;
        while i > self.top_marker {
            self.stack[i].1 = None;
            i -= 1;
        }
    }
    fn merge_frame(&mut self) {
        let previous_marker = self.previous_marker(self.top_marker);
        let mut previous = previous_marker;
        let mut current = self.top_marker;
        while current < self.top_entry {
            previous += 1;
            current += 1;
            let (new_key, new_value) = self.stack[current].clone();
            let old_key = self.stack.get(previous).map(|e| e.0);
            if old_key != Some(new_key) {
                self.set(previous, new_key, new_value);
            } else if new_value.is_some() {
                self.stack[previous].1 = new_value;
            }
        }
        self.top_entry = previous;
        self.top_marker = previous_marker;
    }
    pub fn put(&mut self, atom: Atom, value: V) {
        match self.value_index(&[atom]) {
            Some(i) => self.stack[i].1 = Some(value),
            None => {
                self.top_entry += 1;
                self.set(self.top_entry, Key::Atom(atom), Some(value));
            }
        }
    }
    pub fn get(&self, atom: Atom) -> Option<V> {
        self.value_index(&[atom])
            .and_then(|i| self.stack[i].1.clone())
    }
    /// Scope.getAny: the newest of several atoms.
    pub fn get_any(&self, atoms: &[Atom]) -> Option<V> {
        self.value_index(atoms)
            .and_then(|i| self.stack[i].1.clone())
    }
    /// Whether the atom has an entry (even a null one).
    pub fn has(&self, atom: Atom) -> bool {
        self.value_index(&[atom]).is_some()
    }
}

/// A rule: a term with an action, or hand-written parsing.
pub enum Rule<V> {
    Term(Term<V>, Rc<dyn Fn(&mut State<V>) -> Option<V>>),
    Custom(Rc<dyn Fn(&mut State<V>) -> Option<V>>),
}

pub enum Term<V> {
    Seq(Vec<Term<V>>),
    Alt(Vec<Term<V>>),
    Opt(Box<Term<V>>),
    Repeated {
        rule: RuleId,
        list: Atom,
        min: usize,
    },
    RepeatedSep {
        rule: RuleId,
        list: Atom,
        sep: Box<Term<V>>,
        min: usize,
        trailing: bool,
    },
    Look(Box<Term<V>>, bool),
    Marker(Atom, V),
    Cut,
    Empty,
    Fail(Reason),
    Named {
        rule: RuleId,
        store: Atom,
    },
    /// StringReaderTerms.TerminalCharacters: one of `values` (or `accept`).
    Chars {
        values: Vec<char>,
        accept: Option<fn(char) -> bool>,
    },
    /// StringReaderTerms.TerminalWord.
    Word(String),
}

pub fn seq<V>(t: Vec<Term<V>>) -> Term<V> {
    Term::Seq(t)
}
pub fn alt<V>(t: Vec<Term<V>>) -> Term<V> {
    Term::Alt(t)
}
pub fn opt<V>(t: Term<V>) -> Term<V> {
    Term::Opt(Box::new(t))
}
pub fn ch<V>(c: char) -> Term<V> {
    Term::Chars {
        values: vec![c],
        accept: None,
    }
}
pub fn chs<V>(a: char, b: char) -> Term<V> {
    Term::Chars {
        values: vec![a, b],
        accept: None,
    }
}
pub fn look<V>(t: Term<V>) -> Term<V> {
    Term::Look(Box::new(t), true)
}

/// Dictionary: named rules, with forward references.
pub struct Grammar<V> {
    names: Vec<Atom>,
    rules: Vec<Option<Rule<V>>>,
    pub top: RuleId,
}
impl<V: Value> Default for Grammar<V> {
    fn default() -> Self {
        Self::new()
    }
}
impl<V: Value> Grammar<V> {
    pub fn new() -> Self {
        Self {
            names: Vec::new(),
            rules: Vec::new(),
            top: 0,
        }
    }
    /// Dictionary.forward / getOrCreateEntry.
    pub fn id(&mut self, name: Atom) -> RuleId {
        if let Some(i) = self.names.iter().position(|n| *n == name) {
            return i;
        }
        self.names.push(name);
        self.rules.push(None);
        self.names.len() - 1
    }
    pub fn put(&mut self, name: Atom, rule: Rule<V>) -> RuleId {
        let id = self.id(name);
        assert!(self.rules[id].is_none(), "Trying to override rule: {name}");
        self.rules[id] = Some(rule);
        id
    }
    pub fn put_term(
        &mut self,
        name: Atom,
        term: Term<V>,
        action: impl Fn(&mut State<V>) -> Option<V> + 'static,
    ) -> RuleId {
        self.put(name, Rule::Term(term, Rc::new(action)))
    }
    pub fn put_custom(
        &mut self,
        name: Atom,
        f: impl Fn(&mut State<V>) -> Option<V> + 'static,
    ) -> RuleId {
        self.put(name, Rule::Custom(Rc::new(f)))
    }
    /// Dictionary.named: parse the rule and store it under its own name.
    pub fn named(&mut self, name: Atom) -> Term<V> {
        let rule = self.id(name);
        Term::Named { rule, store: name }
    }
    /// Dictionary.namedWithAlias.
    pub fn named_as(&mut self, name: Atom, store: Atom) -> Term<V> {
        let rule = self.id(name);
        Term::Named { rule, store }
    }

    /// Grammar.parseForCommands: parse at the reader's cursor; on failure
    /// the first error stored at the furthest position.
    pub fn parse_for_commands(&self, reader: &mut Reader) -> CmdResult<V> {
        self.parse_for_commands_with(reader, None)
    }
    /// parse_for_commands with a context value the rules can read (`State::ctx`).
    pub fn parse_for_commands_with(
        &self,
        reader: &mut Reader,
        ctx: Option<&dyn std::any::Any>,
    ) -> CmdResult<V> {
        let mut state = State::new(self, reader.string, reader.cursor);
        state.ctx = ctx;
        let result = state.parse_top();
        reader.cursor = state.cursor;
        match result {
            Some(v) => Ok(v),
            None => Err(state.first_error()),
        }
    }
    /// Grammar.parseForSuggestions.
    pub fn parse_for_suggestions(&self, builder: &mut SuggestionsBuilder) {
        self.parse_for_suggestions_with(builder, None)
    }
    pub fn parse_for_suggestions_with(
        &self,
        builder: &mut SuggestionsBuilder,
        ctx: Option<&dyn std::any::Any>,
    ) {
        let mut state = State::new(self, builder.input, builder.start);
        state.ctx = ctx;
        state.parse_top();
        if state.errors.is_empty() {
            return;
        }
        let mut offset = builder.offset(state.last_cursor.max(0) as usize);
        for (sugg, _) in &state.errors {
            match sugg {
                Sugg::Empty => {}
                Sugg::Values(v) => offset.suggest_matching(v.iter().map(String::as_str)),
                Sugg::Resources(ids) => args::suggest_resources(&mut offset, ids.iter(), ""),
            }
        }
        builder.add(offset);
    }
}

/// CachedParseState over a string (StringReaderParserState) with a
/// LongestOnly error collector.
pub struct State<'g, V> {
    grammar: &'g Grammar<V>,
    pub input: &'g str,
    pub cursor: usize,
    cache: HashMap<(usize, RuleId), Option<(V, usize)>>,
    pub scope: Scope<V>,
    controls: Vec<bool>,
    errors: Vec<(Sugg, Reason)>,
    last_cursor: isize,
    /// Current rule-recursion depth, bounded by `MAX_DEPTH` so a crafted
    /// deeply nested value (SNBT compound/list or a JSON text component's
    /// `extra` chain) fails to parse instead of overflowing the stack.
    depth: usize,
    /// Context for the rules (e.g. the registries a lookup checks).
    pub ctx: Option<&'g dyn std::any::Any>,
}
/// The deepest rule nesting the parser accepts. Well below the point where a
/// recursive parse overflows the parsing thread's stack (measured at ~300 for
/// the server thread), and far above any real command or data value.
const MAX_DEPTH: usize = 128;
const UNBOUND: usize = usize::MAX;
impl<'g, V: Value> State<'g, V> {
    pub fn new(grammar: &'g Grammar<V>, input: &'g str, cursor: usize) -> Self {
        Self {
            grammar,
            input,
            cursor,
            cache: HashMap::new(),
            scope: Scope::new(),
            controls: Vec::new(),
            errors: Vec::new(),
            last_cursor: -1,
            depth: 0,
            ctx: None,
        }
    }
    pub fn reader(&self) -> Reader<'g> {
        Reader {
            string: self.input,
            cursor: self.cursor,
        }
    }
    pub fn set_reader(&mut self, r: &Reader) {
        self.cursor = r.cursor;
    }
    fn discard_shorter(&mut self, cursor: usize) {
        if cursor as isize > self.last_cursor {
            self.last_cursor = cursor as isize;
            self.errors.clear();
        }
    }
    /// ErrorCollector.store.
    pub fn store(&mut self, cursor: usize, sugg: Sugg, reason: Reason) {
        self.discard_shorter(cursor);
        if cursor as isize == self.last_cursor {
            self.errors.push((sugg, reason));
        }
    }
    pub fn store_at_mark(&mut self, reason: Reason) {
        self.store(self.cursor, Sugg::Empty, reason);
    }
    fn parse_top(&mut self) -> Option<V> {
        let r = self.parse(self.grammar.top);
        if r.is_some() {
            self.discard_shorter(self.cursor);
        }
        r
    }
    fn first_error(&self) -> CommandError {
        if let Some((_, reason)) = self.errors.first() {
            return match reason {
                Reason::Delayed(key, args) => {
                    let args: Vec<&dyn std::fmt::Display> =
                        args.iter().map(|a| a as &dyn std::fmt::Display).collect();
                    CommandError::tr_at(
                        key,
                        &args,
                        &Reader {
                            string: self.input,
                            cursor: self.last_cursor.max(0) as usize,
                        },
                    )
                }
                Reason::Error(e) => e.clone(),
            };
        }
        CommandError::new(horizon_client_core::text::Text::literal("Failed to parse"))
    }
    /// CachedParseState.parse: a named rule, memoised per position.
    pub fn parse(&mut self, rule: RuleId) -> Option<V> {
        let mark = self.cursor;
        if let Some(entry) = self.cache.get(&(mark, rule)) {
            return match entry {
                None => None,
                Some((v, after)) => {
                    let v = v.clone();
                    self.cursor = *after;
                    Some(v)
                }
            };
        }
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let result = match self.grammar.rules[rule].as_ref() {
            Some(Rule::Term(term, action)) => {
                let action = action.clone();
                self.scope.push_frame();
                let ok = self.term(term, UNBOUND, false);
                let r = if ok { action(self) } else { None };
                self.scope.pop_frame();
                r
            }
            Some(Rule::Custom(f)) => {
                let f = f.clone();
                f(self)
            }
            None => panic!("Unbound rule {}", self.grammar.names[rule]),
        };
        self.depth -= 1;
        let entry = result.clone().map(|v| (v, self.cursor));
        self.cache.insert((mark, rule), entry);
        result
    }
    fn skip_whitespace(&mut self) {
        let mut r = self.reader();
        r.skip_whitespace();
        self.cursor = r.cursor;
    }
    fn term(&mut self, term: &'g Term<V>, control: usize, silent: bool) -> bool {
        match term {
            Term::Seq(items) => {
                let mark = self.cursor;
                for t in items {
                    if !self.term(t, control, silent) {
                        self.cursor = mark;
                        return false;
                    }
                }
                true
            }
            Term::Alt(items) => {
                self.controls.push(false);
                let ctl = self.controls.len() - 1;
                let mark = self.cursor;
                self.scope.split_frame();
                let mut ok = false;
                for t in items {
                    if self.term(t, ctl, silent) {
                        self.scope.merge_frame();
                        ok = true;
                        break;
                    }
                    self.scope.clear_frame_values();
                    self.cursor = mark;
                    if self.controls[ctl] {
                        break;
                    }
                }
                if !ok {
                    self.scope.pop_frame();
                }
                self.controls.pop();
                ok
            }
            Term::Opt(t) => {
                let mark = self.cursor;
                if !self.term(t, control, silent) {
                    self.cursor = mark;
                }
                true
            }
            Term::Repeated { rule, list, min } => {
                let mark = self.cursor;
                let mut items = Vec::new();
                loop {
                    let entry = self.cursor;
                    match self.parse(*rule) {
                        Some(v) => items.push(v),
                        None => {
                            self.cursor = entry;
                            if items.len() < *min {
                                self.cursor = mark;
                                return false;
                            }
                            self.scope.put(list, V::list(items));
                            return true;
                        }
                    }
                }
            }
            Term::RepeatedSep {
                rule,
                list,
                sep,
                min,
                trailing,
            } => {
                let list_mark = self.cursor;
                let mut items = Vec::new();
                let mut first = true;
                loop {
                    let before = self.cursor;
                    if !first && !self.term(sep, control, silent) {
                        self.cursor = before;
                        break;
                    }
                    let after = self.cursor;
                    match self.parse(*rule) {
                        Some(v) => items.push(v),
                        None => {
                            if first {
                                self.cursor = after;
                            } else {
                                if !*trailing {
                                    self.cursor = list_mark;
                                    return false;
                                }
                                self.cursor = after;
                            }
                            break;
                        }
                    }
                    first = false;
                }
                if items.len() < *min {
                    self.cursor = list_mark;
                    return false;
                }
                self.scope.put(list, V::list(items));
                true
            }
            Term::Look(t, positive) => {
                let mark = self.cursor;
                let r = self.term(t, control, true);
                self.cursor = mark;
                *positive == r
            }
            Term::Marker(atom, v) => {
                self.scope.put(atom, v.clone());
                true
            }
            Term::Cut => {
                if control != UNBOUND {
                    self.controls[control] = true;
                }
                true
            }
            Term::Empty => true,
            Term::Fail(reason) => {
                if !silent {
                    self.store(self.cursor, Sugg::Empty, reason.clone());
                }
                false
            }
            Term::Named { rule, store } => match self.parse(*rule) {
                Some(v) => {
                    self.scope.put(store, v);
                    true
                }
                None => false,
            },
            Term::Chars { values, accept } => {
                self.skip_whitespace();
                let cursor = self.cursor;
                let mut r = self.reader();
                if r.can_read() {
                    let c = r.read();
                    self.cursor = r.cursor;
                    let ok = match accept {
                        Some(f) => f(c),
                        None => values.contains(&c),
                    };
                    if ok {
                        return true;
                    }
                }
                if !silent {
                    let joined = values
                        .iter()
                        .map(char::to_string)
                        .collect::<Vec<_>>()
                        .join("|");
                    let sugg = Sugg::Values(Rc::new(values.iter().map(char::to_string).collect()));
                    self.store(
                        cursor,
                        sugg,
                        Reason::Delayed("argument.literal.incorrect", vec![joined]),
                    );
                }
                false
            }
            Term::Word(word) => {
                self.skip_whitespace();
                let cursor = self.cursor;
                let mut r = self.reader();
                let value = r.read_unquoted_string();
                self.cursor = r.cursor;
                if value != word {
                    if !silent {
                        self.store(
                            cursor,
                            Sugg::Values(Rc::new(vec![word.clone()])),
                            Reason::Delayed("argument.literal.incorrect", vec![word.clone()]),
                        );
                    }
                    return false;
                }
                true
            }
        }
    }
}
