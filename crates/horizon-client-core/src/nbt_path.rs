//! NBT paths (`NbtPathArgument`): `foo.bar[0]`, `[]`, `list[{id:1}]`,
//! `{root:match}`, with get, set, insert and remove on a tag tree, and the
//! partial NBT comparison (`NbtUtils.compareNbt`) selectors and conditions use.
use horizon_world::nbt::{Compound, JavaString, Tag, parse_snbt};

/// An error carrying the translation key and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct PathError {
    pub key: &'static str,
    pub args: Vec<String>,
    /// Byte offset in the path text (parse errors).
    pub cursor: usize,
}
impl PathError {
    fn new(key: &'static str, args: Vec<String>, cursor: usize) -> Self {
        Self { key, args, cursor }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    /// `name`
    Child(String),
    /// `name{pattern}`
    MatchChild(String, Tag),
    /// `[]`
    AllElements,
    /// `[n]`
    Index(i32),
    /// `[{pattern}]`
    MatchElement(Tag),
    /// `{pattern}` at the start.
    MatchRoot(Tag),
}
#[derive(Clone, Debug, PartialEq)]
pub struct NbtPath {
    original: String,
    nodes: Vec<Node>,
    /// Byte offset just after each node in `original`.
    ends: Vec<usize>,
}

fn key(s: &str) -> JavaString {
    JavaString::from(s)
}
fn get_child<'a>(c: &'a Compound, name: &str) -> Option<&'a Tag> {
    c.get(&key(name))
}

/// `NbtUtils.compareNbt(expected, actual, true)`.
pub fn compare_nbt(expected: &Tag, actual: Option<&Tag>) -> bool {
    let Some(actual) = actual else { return false };
    if std::mem::discriminant(expected) != std::mem::discriminant(actual) {
        return false;
    }
    match (expected, actual) {
        (Tag::Compound(e), Tag::Compound(a)) => {
            a.len() >= e.len() && e.iter().all(|(k, v)| compare_nbt(v, a.get(k)))
        }
        (Tag::List(e), Tag::List(a)) => {
            if e.is_empty() {
                return a.is_empty();
            }
            a.len() >= e.len() && e.iter().all(|x| a.iter().any(|y| compare_nbt(x, Some(y))))
        }
        _ => expected == actual,
    }
}

// ---- list-like tags ----------------------------------------------------------

/// The elements of a list or array tag.
fn elements(tag: &Tag) -> Option<Vec<Tag>> {
    Some(match tag {
        Tag::List(l) => l.clone(),
        Tag::ByteArray(a) => a.iter().map(|b| Tag::Byte(*b as i8)).collect(),
        Tag::IntArray(a) => a.iter().map(|v| Tag::Int(*v)).collect(),
        Tag::LongArray(a) => a.iter().map(|v| Tag::Long(*v)).collect(),
        _ => return None,
    })
}
/// `DoubleTag.intValue` / `FloatTag.intValue` floor before the i32 conversion;
/// integral tags use their intValue narrowing. Byte arrays then keep the low byte.
fn int_value(t: &Tag) -> Option<i32> {
    Some(match t {
        Tag::Byte(v) => i32::from(*v),
        Tag::Short(v) => i32::from(*v),
        Tag::Int(v) => *v,
        Tag::Long(v) => *v as i32,
        Tag::Float(v) => f32::from_bits(*v).floor() as i32,
        Tag::Double(v) => f64::from_bits(*v).floor() as i32,
        _ => return None,
    })
}
/// `FloatTag.longValue` truncates; `DoubleTag.longValue` floors before narrowing.
fn long_value(t: &Tag) -> Option<i64> {
    Some(match t {
        Tag::Byte(v) => i64::from(*v),
        Tag::Short(v) => i64::from(*v),
        Tag::Int(v) => i64::from(*v),
        Tag::Long(v) => *v,
        // FloatTag.longValue truncates; DoubleTag.longValue floors first.
        Tag::Float(v) => f32::from_bits(*v) as i64,
        Tag::Double(v) => f64::from_bits(*v).floor() as i64,
        _ => return None,
    })
}
#[derive(Clone, Copy)]
enum ElementKind {
    List,
    NumericArray,
}
impl ElementKind {
    // Admission only: arrays require NumericTag; lists accept every tag type.
    fn accepts(self, value: &Tag) -> bool {
        matches!(self, Self::List)
            || matches!(
                value,
                Tag::Byte(_)
                    | Tag::Short(_)
                    | Tag::Int(_)
                    | Tag::Long(_)
                    | Tag::Float(_)
                    | Tag::Double(_)
            )
    }
}
/// Rust view adapter: run `f` over the elements, then write arrays using their numeric accessor.
/// None when `tag` is not list-like.
fn with_elements<R>(tag: &mut Tag, f: impl FnOnce(&mut Vec<Tag>, ElementKind) -> R) -> Option<R> {
    match tag {
        Tag::List(l) => Some(f(l, ElementKind::List)),
        Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_) => {
            let mut v = elements(tag)?;
            let r = f(&mut v, ElementKind::NumericArray);
            match tag {
                // Float/double byteValue uses intValue's floor, then narrows.
                Tag::ByteArray(a) => *a = v.iter().filter_map(int_value).map(|n| n as u8).collect(),
                Tag::IntArray(a) => *a = v.iter().filter_map(int_value).collect(),
                Tag::LongArray(a) => *a = v.iter().filter_map(long_value).collect(),
                _ => {}
            }
            Some(r)
        }
        _ => None,
    }
}

impl NbtPath {
    /// `NbtPathArgument.parse` from the start of `text`; returns the path and
    /// the bytes consumed (it stops at a space).
    pub fn parse_prefix(text: &str) -> Result<(NbtPath, usize), PathError> {
        let b = text.as_bytes();
        let mut i = 0;
        let mut nodes = Vec::new();
        let mut ends = Vec::new();
        let mut first = true;
        while i < b.len() && b[i] != b' ' {
            let (node, next) = parse_node(text, i, first)?;
            nodes.push(node);
            i = next;
            ends.push(i);
            first = false;
            if i < b.len() && !matches!(b[i], b' ' | b'[' | b'{') {
                if b[i] != b'.' {
                    return Err(PathError::new("parsing.expected", vec![".".into()], i));
                }
                i += 1;
            }
        }
        Ok((
            NbtPath {
                original: text[..i].to_owned(),
                nodes,
                ends,
            },
            i,
        ))
    }
    pub fn parse(text: &str) -> Result<NbtPath, PathError> {
        let (p, used) = Self::parse_prefix(text)?;
        if used != text.len() {
            return Err(PathError::new(
                "arguments.nbtpath.node.invalid",
                vec![],
                used,
            ));
        }
        Ok(p)
    }
    pub fn as_str(&self) -> &str {
        &self.original
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    fn not_found(&self, node: usize) -> PathError {
        let end = self.ends[node];
        PathError::new(
            "arguments.nbtpath.nothing_found",
            vec![self.original[..end.min(self.original.len())].to_owned()],
            0,
        )
    }

    // ---- reading --------------------------------------------------------

    fn step<'a>(node: &Node, parent: &'a Tag, out: &mut Vec<&'a Tag>) {
        match node {
            Node::Child(name) => {
                if let Tag::Compound(c) = parent
                    && let Some(t) = get_child(c, name)
                {
                    out.push(t);
                }
            }
            Node::MatchChild(name, pattern) => {
                if let Tag::Compound(c) = parent
                    && let Some(t) = get_child(c, name)
                    && compare_nbt(pattern, Some(t))
                {
                    out.push(t);
                }
            }
            Node::AllElements => {
                if let Tag::List(l) = parent {
                    out.extend(l.iter());
                }
            }
            Node::Index(i) => {
                if let Tag::List(l) = parent {
                    let n = l.len() as i64;
                    let idx = if *i < 0 {
                        n + i64::from(*i)
                    } else {
                        i64::from(*i)
                    };
                    if (0..n).contains(&idx) {
                        out.push(&l[idx as usize]);
                    }
                }
            }
            Node::MatchElement(pattern) => {
                if let Tag::List(l) = parent {
                    out.extend(l.iter().filter(|t| compare_nbt(pattern, Some(t))));
                }
            }
            Node::MatchRoot(pattern) => {
                if matches!(parent, Tag::Compound(_)) && compare_nbt(pattern, Some(parent)) {
                    out.push(parent);
                }
            }
        }
    }
    /// Array tags read as lists of their elements.
    fn read_steps(&self, root: &Tag) -> Result<Vec<Tag>, PathError> {
        let mut current: Vec<Tag> = vec![root.clone()];
        for (i, node) in self.nodes.iter().enumerate() {
            let mut next = Vec::new();
            for parent in &current {
                // Arrays expose their elements to element nodes.
                let list;
                let p = if matches!(node, Node::AllElements | Node::Index(_)) {
                    match elements(parent) {
                        Some(l) if !matches!(parent, Tag::List(_)) => {
                            list = Tag::List(l);
                            &list
                        }
                        _ => parent,
                    }
                } else {
                    parent
                };
                let mut out = Vec::new();
                Self::step(node, p, &mut out);
                next.extend(out.into_iter().cloned());
            }
            if next.is_empty() {
                return Err(self.not_found(i));
            }
            current = next;
        }
        Ok(current)
    }
    /// `get`: every matching tag (an error when nothing matches).
    pub fn get(&self, root: &Tag) -> Result<Vec<Tag>, PathError> {
        self.read_steps(root)
    }
    /// `countMatching`.
    pub fn count(&self, root: &Tag) -> usize {
        self.read_steps(root).map_or(0, |v| v.len())
    }

    // ---- writing --------------------------------------------------------

    /// The parents of the last node, creating the missing ones (`getOrCreateParents`).
    fn visit_parents(
        &self,
        root: &mut Tag,
        create: bool,
        f: &mut dyn FnMut(&mut Tag),
    ) -> Result<(), PathError> {
        self.descend(0, self.nodes.len().saturating_sub(1), root, create, None, f)
    }
    /// Walk `nodes[from..to]` from `tag`, calling `f` on every tag reached.
    fn descend(
        &self,
        from: usize,
        to: usize,
        tag: &mut Tag,
        create: bool,
        last_value: Option<&dyn Fn() -> Tag>,
        f: &mut dyn FnMut(&mut Tag),
    ) -> Result<(), PathError> {
        if from >= to {
            f(tag);
            return Ok(());
        }
        let node = &self.nodes[from];
        // What a created child should be: the next node's preferred parent,
        // or the caller's value for the last node.
        let default_child = || -> Tag {
            match (self.nodes.get(from + 1), last_value) {
                (Some(next), _) if from + 1 < to => preferred_parent(next),
                (_, Some(v)) => v(),
                (Some(next), None) => preferred_parent(next),
                _ => Tag::Compound(Compound::new()),
            }
        };
        let mut found = false;
        let mut error = None;
        let mut visit = |child: &mut Tag| {
            found = true;
            if error.is_none()
                && let Err(e) = self.descend(from + 1, to, child, create, last_value, f)
            {
                error = Some(e);
            }
        };
        step_mut(
            node,
            tag,
            create.then_some(&default_child as &dyn Fn() -> Tag),
            &mut visit,
        );
        if let Some(e) = error {
            return Err(e);
        }
        if !found {
            return Err(self.not_found(from));
        }
        Ok(())
    }
    /// `getOrCreate`: the tags at the path, creating missing ones as
    /// `new_value` (used for the last node).
    pub fn get_or_create(
        &self,
        root: &mut Tag,
        new_value: &dyn Fn() -> Tag,
        f: &mut dyn FnMut(&mut Tag),
    ) -> Result<(), PathError> {
        self.descend(0, self.nodes.len(), root, true, Some(new_value), f)
    }
    /// `set`: returns how many tags changed.
    pub fn set(&self, root: &mut Tag, value: &Tag) -> Result<usize, PathError> {
        if too_deep(value, self.nodes.len()) {
            return Err(PathError::new("arguments.nbtpath.too_deep", vec![], 0));
        }
        let Some(last) = self.nodes.last() else {
            return Ok(0);
        };
        let mut changed = 0;
        self.visit_parents(root, true, &mut |parent| {
            changed += set_tag(last, parent, value);
        })?;
        Ok(changed)
    }
    /// `remove`: returns how many tags were removed.
    pub fn remove(&self, root: &mut Tag) -> usize {
        let Some(last) = self.nodes.last() else {
            return 0;
        };
        let mut removed = 0;
        let _ = self.visit_parents(root, false, &mut |parent| {
            removed += remove_tag(last, parent);
        });
        removed
    }
    /// `NbtPathArgument.NbtPath.insert`: insert accepted tags into every reached collection.
    pub fn insert(
        &self,
        index: i32,
        root: &mut Tag,
        values: &[Tag],
    ) -> Result<usize, CollectionError> {
        for v in values {
            if too_deep(v, self.nodes.len()) {
                return Err(CollectionError::Path(PathError::new(
                    "arguments.nbtpath.too_deep",
                    vec![],
                    0,
                )));
            }
        }
        let mut modified = 0;
        let mut error: Option<CollectionError> = None;
        self.get_or_create(root, &|| Tag::List(Vec::new()), &mut |target| {
            if error.is_some() {
                return;
            }
            let shown = crate::snbt::to_snbt(target);
            let result = with_elements(target, |list, kind| {
                let start = if index < 0 {
                    list.len() as i64 + i64::from(index) + 1
                } else {
                    i64::from(index)
                };
                let mut any = false;
                let mut at = start;
                for v in values {
                    if !kind.accepts(v) {
                        continue;
                    }
                    if at < 0 || at as usize > list.len() {
                        return Err(CollectionError::InvalidIndex(at as i32));
                    }
                    list.insert(at as usize, v.clone());
                    at += 1;
                    any = true;
                }
                Ok(any)
            });
            match result {
                None => error = Some(CollectionError::ExpectedList(shown)),
                Some(Err(e)) => error = Some(e),
                Some(Ok(any)) => modified += usize::from(any),
            }
        })
        .map_err(CollectionError::Path)?;
        match error {
            Some(e) => Err(e),
            None => Ok(modified),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum CollectionError {
    Path(PathError),
    /// `commands.data.modify.expected_list` with the offending tag.
    ExpectedList(String),
    /// `commands.data.modify.invalid_index`.
    InvalidIndex(i32),
}

/// `isTooDeep`.
pub fn too_deep(tag: &Tag, depth: usize) -> bool {
    if depth >= 512 {
        return true;
    }
    match tag {
        Tag::Compound(c) => c.values().any(|t| too_deep(t, depth + 1)),
        Tag::List(l) => l.iter().any(|t| too_deep(t, depth + 1)),
        _ => false,
    }
}
fn preferred_parent(node: &Node) -> Tag {
    match node {
        Node::AllElements | Node::Index(_) | Node::MatchElement(_) => Tag::List(Vec::new()),
        _ => Tag::Compound(Compound::new()),
    }
}
/// Children of `parent`; the array creation arm follows
/// `NbtPathArgument.AllElementsNode.getOrCreateTag` admission.
fn step_mut(
    node: &Node,
    parent: &mut Tag,
    create: Option<&dyn Fn() -> Tag>,
    out: &mut dyn FnMut(&mut Tag),
) {
    match node {
        Node::Child(name) => {
            if let Tag::Compound(c) = parent {
                let k = key(name);
                if !c.contains_key(&k) {
                    let Some(make) = create else { return };
                    c.insert(k.clone(), make());
                }
                if let Some(t) = c.get_mut(&k) {
                    out(t);
                }
            }
        }
        Node::MatchChild(name, pattern) => {
            if let Tag::Compound(c) = parent {
                let k = key(name);
                if !c.contains_key(&k) {
                    if create.is_none() {
                        return;
                    }
                    c.insert(k.clone(), pattern.clone());
                    if let Some(t) = c.get_mut(&k) {
                        out(t);
                    }
                } else if let Some(t) = c.get_mut(&k)
                    && compare_nbt(pattern, Some(t))
                {
                    out(t);
                }
            }
        }
        Node::AllElements => {
            let make = create;
            let _ = with_elements(parent, |list, kind| {
                if list.is_empty() {
                    if let Some(make) = make {
                        let value = make();
                        if kind.accepts(&value) {
                            list.push(value);
                            out(&mut list[0]);
                        }
                    }
                } else {
                    for t in list.iter_mut() {
                        out(t);
                    }
                }
            });
        }
        Node::Index(i) => {
            let _ = with_elements(parent, |list, _kind| {
                let n = list.len() as i64;
                let idx = if *i < 0 {
                    n + i64::from(*i)
                } else {
                    i64::from(*i)
                };
                if (0..n).contains(&idx) {
                    out(&mut list[idx as usize]);
                }
            });
        }
        Node::MatchElement(pattern) => {
            if let Tag::List(list) = parent {
                let mut any = false;
                for t in list.iter_mut() {
                    if compare_nbt(pattern, Some(t)) {
                        any = true;
                        out(t);
                    }
                }
                if !any && create.is_some() {
                    list.push(pattern.clone());
                    if let Some(t) = list.last_mut() {
                        out(t);
                    }
                }
            }
        }
        Node::MatchRoot(pattern) => {
            if matches!(parent, Tag::Compound(_)) && compare_nbt(pattern, Some(parent)) {
                out(parent);
            }
        }
    }
}
/// Element setters follow `NbtPathArgument.AllElementsNode.setTag` and
/// `NbtPathArgument.IndexedElementNode.setTag`; object nodes replace their children.
fn set_tag(node: &Node, parent: &mut Tag, value: &Tag) -> usize {
    match node {
        Node::Child(name) => {
            if let Tag::Compound(c) = parent {
                let prev = c.insert(key(name), value.clone());
                return usize::from(prev.as_ref() != Some(value));
            }
            0
        }
        Node::MatchChild(name, pattern) => {
            if let Tag::Compound(c) = parent {
                let k = key(name);
                if compare_nbt(pattern, c.get(&k)) && c.get(&k) != Some(value) {
                    c.insert(k, value.clone());
                    return 1;
                }
            }
            0
        }
        Node::AllElements => with_elements(parent, |list, kind| {
            let len = list.len();
            if len == 0 {
                if kind.accepts(value) {
                    list.push(value.clone());
                }
                // AllElementsNode returns 1 here even when addTag rejects.
                return 1;
            }
            // Compare the original tags, before numeric conversion.
            let changed = list.iter().filter(|t| *t != value).count();
            if changed == 0 {
                return 0;
            }
            list.clear();
            if !kind.accepts(value) {
                return 0;
            }
            list.resize(len, value.clone());
            changed
        })
        .unwrap_or(0),
        Node::Index(i) => with_elements(parent, |list, kind| {
            let n = list.len() as i64;
            let idx = if *i < 0 {
                n + i64::from(*i)
            } else {
                i64::from(*i)
            };
            if (0..n).contains(&idx) && list[idx as usize] != *value && kind.accepts(value) {
                list[idx as usize] = value.clone();
                1
            } else {
                0
            }
        })
        .unwrap_or(0),
        Node::MatchElement(pattern) => {
            let Tag::List(list) = parent else { return 0 };
            if list.is_empty() {
                list.push(value.clone());
                return 1;
            }
            let mut changed = 0;
            for t in list.iter_mut() {
                if compare_nbt(pattern, Some(t)) && t != value {
                    *t = value.clone();
                    changed += 1;
                }
            }
            changed
        }
        Node::MatchRoot(_) => 0,
    }
}
/// Removal arms, including `NbtPathArgument.AllElementsNode.removeTag`.
fn remove_tag(node: &Node, parent: &mut Tag) -> usize {
    match node {
        Node::Child(name) => match parent {
            Tag::Compound(c) => usize::from(c.remove(&key(name)).is_some()),
            _ => 0,
        },
        Node::MatchChild(name, pattern) => match parent {
            Tag::Compound(c) => {
                let k = key(name);
                if compare_nbt(pattern, c.get(&k)) {
                    usize::from(c.remove(&k).is_some())
                } else {
                    0
                }
            }
            _ => 0,
        },
        Node::AllElements => with_elements(parent, |l, _kind| {
            let n = l.len();
            l.clear();
            n
        })
        .unwrap_or(0),
        Node::Index(i) => with_elements(parent, |l, _kind| {
            let n = l.len() as i64;
            let idx = if *i < 0 {
                n + i64::from(*i)
            } else {
                i64::from(*i)
            };
            if (0..n).contains(&idx) {
                l.remove(idx as usize);
                1
            } else {
                0
            }
        })
        .unwrap_or(0),
        Node::MatchElement(pattern) => match parent {
            Tag::List(l) => {
                let before = l.len();
                l.retain(|t| !compare_nbt(pattern, Some(t)));
                before - l.len()
            }
            _ => 0,
        },
        Node::MatchRoot(_) => 0,
    }
}

// ---- parsing -------------------------------------------------------------------

fn parse_node(text: &str, i: usize, first: bool) -> Result<(Node, usize), PathError> {
    let b = text.as_bytes();
    let invalid = |at: usize| PathError::new("arguments.nbtpath.node.invalid", vec![], at);
    match b[i] {
        b'"' | b'\'' => {
            let (name, next) = read_quoted(text, i)?;
            read_object_node(text, name, next)
        }
        b'[' => {
            let mut j = i + 1;
            match b.get(j) {
                Some(b'{') => {
                    let (pattern, next) = parse_compound(text, j)?;
                    j = next;
                    expect(text, j, b']')?;
                    Ok((Node::MatchElement(pattern), j + 1))
                }
                Some(b']') => Ok((Node::AllElements, j + 1)),
                _ => {
                    let start = j;
                    if b.get(j) == Some(&b'-') {
                        j += 1;
                    }
                    while b.get(j).is_some_and(u8::is_ascii_digit) {
                        j += 1;
                    }
                    let n: i32 = text[start..j].parse().map_err(|_| {
                        PathError::new(
                            "parsing.int.invalid",
                            vec![text[start..j].to_owned()],
                            start,
                        )
                    })?;
                    expect(text, j, b']')?;
                    Ok((Node::Index(n), j + 1))
                }
            }
        }
        b'{' => {
            if !first {
                return Err(invalid(i));
            }
            let (pattern, next) = parse_compound(text, i)?;
            Ok((Node::MatchRoot(pattern), next))
        }
        _ => {
            let mut j = i;
            while j < b.len()
                && !matches!(b[j], b' ' | b'"' | b'\'' | b'[' | b']' | b'.' | b'{' | b'}')
            {
                j += 1;
            }
            if j == i {
                return Err(invalid(i));
            }
            read_object_node(text, text[i..j].to_owned(), j)
        }
    }
}
fn read_object_node(text: &str, name: String, i: usize) -> Result<(Node, usize), PathError> {
    if name.is_empty() {
        return Err(PathError::new("arguments.nbtpath.node.invalid", vec![], i));
    }
    if text.as_bytes().get(i) == Some(&b'{') {
        let (pattern, next) = parse_compound(text, i)?;
        return Ok((Node::MatchChild(name, pattern), next));
    }
    Ok((Node::Child(name), i))
}
fn expect(text: &str, i: usize, c: u8) -> Result<(), PathError> {
    if text.as_bytes().get(i) == Some(&c) {
        Ok(())
    } else {
        Err(PathError::new(
            "parsing.expected",
            vec![(c as char).to_string()],
            i,
        ))
    }
}
fn read_quoted(text: &str, i: usize) -> Result<(String, usize), PathError> {
    let quote = text.as_bytes()[i] as char;
    let mut out = String::new();
    let mut escaped = false;
    for (o, c) in text[i + 1..].char_indices() {
        if escaped {
            out.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Ok((out, i + 1 + o + c.len_utf8()));
        } else {
            out.push(c);
        }
    }
    Err(PathError::new(
        "parsing.quote.expected.end",
        vec![],
        text.len(),
    ))
}
fn parse_compound(text: &str, i: usize) -> Result<(Tag, usize), PathError> {
    // Find the balanced end of the braces, then parse the SNBT inside.
    let b = text.as_bytes();
    let mut depth = 0;
    let mut quote: Option<u8> = None;
    let mut j = i;
    while j < b.len() {
        let c = b[j];
        match quote {
            Some(_) if c == b'\\' => j += 1,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'{' || c == b'[' => depth += 1,
            None if c == b'}' || c == b']' => {
                depth -= 1;
                if depth == 0 {
                    let (tag, _) = parse_snbt(&text[i..=j])
                        .map_err(|e| PathError::new("argument.nbt.invalid", vec![e], i))?;
                    return Ok((tag, j + 1));
                }
            }
            None => {}
        }
        j += 1;
    }
    Err(PathError::new("argument.nbt.expected.compound", vec![], i))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &str) -> Tag {
        parse_snbt(s).unwrap().0
    }
    fn path(s: &str) -> NbtPath {
        NbtPath::parse(s).unwrap_or_else(|e| panic!("{s}: {e:?}"))
    }
    fn show(v: &[Tag]) -> Vec<String> {
        v.iter().map(crate::snbt::to_snbt).collect()
    }

    #[test]
    fn get_follows_the_path() {
        let root = tag("{a:{b:[{id:1,n:\"x\"},{id:2,n:\"y\"},{id:3}]},c:5,\"d e\":7}");
        assert_eq!(show(&path("c").get(&root).unwrap()), ["5"]);
        assert_eq!(show(&path("\"d e\"").get(&root).unwrap()), ["7"]);
        assert_eq!(show(&path("a.b[0].n").get(&root).unwrap()), ["\"x\""]);
        assert_eq!(show(&path("a.b[-1]").get(&root).unwrap()), ["{id:3}"]);
        assert_eq!(show(&path("a.b[].id").get(&root).unwrap()), ["1", "2", "3"]);
        assert_eq!(show(&path("a.b[{id:2}].n").get(&root).unwrap()), ["\"y\""]);
        assert_eq!(
            show(&path("a{b:[{id:1}]}.b[0].id").get(&root).unwrap()),
            ["1"]
        );
        assert_eq!(show(&path("{c:5}.c").get(&root).unwrap()), ["5"]);
        assert!(path("{c:6}.c").get(&root).is_err());
        let e = path("a.zzz.q").get(&root).unwrap_err();
        assert_eq!(e.key, "arguments.nbtpath.nothing_found");
        assert_eq!(e.args, ["a.zzz"]);
        assert_eq!(path("a.b[].id").count(&root), 3);
        assert_eq!(path("a.nope").count(&root), 0);
        // Arrays act like lists.
        let arr = tag("{v:[I;4,5,6]}");
        assert_eq!(show(&path("v[1]").get(&arr).unwrap()), ["5"]);
        assert_eq!(show(&path("v[]").get(&arr).unwrap()), ["4", "5", "6"]);
    }

    #[test]
    fn parse_errors() {
        assert_eq!(
            NbtPath::parse("a..b").unwrap_err().key,
            "arguments.nbtpath.node.invalid"
        );
        assert_eq!(
            NbtPath::parse("a.{b:1}").unwrap_err().key,
            "arguments.nbtpath.node.invalid"
        );
        assert_eq!(NbtPath::parse("a[1").unwrap_err().key, "parsing.expected");
        assert_eq!(NbtPath::parse("a]").unwrap_err().key, "parsing.expected");
        let (p, used) = NbtPath::parse_prefix("a.b[0] rest").unwrap();
        assert_eq!((p.as_str(), used), ("a.b[0]", 6));
    }

    #[test]
    fn set_creates_and_reports_changes() {
        let mut root = tag("{a:{x:1}}");
        assert_eq!(path("a.x").set(&mut root, &tag("2")).unwrap(), 1);
        assert_eq!(path("a.x").set(&mut root, &tag("2")).unwrap(), 0);
        assert_eq!(path("b.c.d").set(&mut root, &tag("\"deep\"")).unwrap(), 1);
        assert_eq!(crate::snbt::to_snbt(&root), "{a:{x:2},b:{c:{d:\"deep\"}}}");
        let mut root = tag("{l:[1,2,3]}");
        assert_eq!(path("l[1]").set(&mut root, &tag("9")).unwrap(), 1);
        assert_eq!(path("l[7]").set(&mut root, &tag("9")).unwrap(), 0);
        assert_eq!(path("l[]").set(&mut root, &tag("0")).unwrap(), 3);
        assert_eq!(crate::snbt::to_snbt(&root), "{l:[0,0,0]}");
        let mut root = tag("{l:[{id:1,v:0},{id:2,v:0}]}");
        assert_eq!(path("l[{id:2}].v").set(&mut root, &tag("5")).unwrap(), 1);
        assert_eq!(crate::snbt::to_snbt(&root), "{l:[{id:1,v:0},{id:2,v:5}]}");
        // A missing match is created from the pattern.
        assert_eq!(path("l[{id:3}].v").set(&mut root, &tag("1")).unwrap(), 1);
        assert_eq!(path("l[{id:3}].v").count(&root), 1);
        // New lists come from index nodes.
        let mut root = tag("{}");
        assert_eq!(path("x[].y").set(&mut root, &tag("1")).unwrap(), 1);
        assert_eq!(crate::snbt::to_snbt(&root), "{x:[{y:1}]}");
    }

    #[test]
    fn remove_counts() {
        let mut root = tag("{a:1,l:[1,2,3,2],c:{d:1}}");
        assert_eq!(path("a").remove(&mut root), 1);
        assert_eq!(path("a").remove(&mut root), 0);
        assert_eq!(path("l[1]").remove(&mut root), 1);
        assert_eq!(path("c.d").remove(&mut root), 1);
        assert_eq!(crate::snbt::to_snbt(&root), "{c:{},l:[1,3,2]}");
        let mut root = tag("{l:[{k:1},{k:2},{k:1}]}");
        assert_eq!(path("l[{k:1}]").remove(&mut root), 2);
        assert_eq!(crate::snbt::to_snbt(&root), "{l:[{k:2}]}");
        assert_eq!(path("l[]").remove(&mut root), 1);
    }

    #[test]
    fn insert_positions() {
        let mut root = tag("{l:[1,2]}");
        assert_eq!(path("l").insert(0, &mut root, &[tag("0")]).unwrap(), 1);
        assert_eq!(path("l").insert(-1, &mut root, &[tag("9")]).unwrap(), 1);
        assert_eq!(
            path("l")
                .insert(1, &mut root, &[tag("5"), tag("6")])
                .unwrap(),
            1
        );
        assert_eq!(crate::snbt::to_snbt(&root), "{l:[0,5,6,1,2,9]}");
        assert_eq!(
            path("l").insert(99, &mut root, &[tag("1")]).unwrap_err(),
            CollectionError::InvalidIndex(99)
        );
        // A missing list is created.
        assert_eq!(path("n").insert(-1, &mut root, &[tag("1")]).unwrap(), 1);
        assert!(matches!(
            path("l[0]").insert(0, &mut root, &[tag("1")]).unwrap_err(),
            CollectionError::ExpectedList(_)
        ));
    }

    #[test]
    fn numeric_array_edits_use_the_destination_numeric_accessor() {
        for (initial, value, inserted, indexed, all) in [
            (
                "{v:[B;1b,2b]}",
                "-3.5f",
                "{v:[B;1b,-4b,2b]}",
                "{v:[B;-4b,2b]}",
                "{v:[B;-4b,-4b]}",
            ),
            (
                "{v:[B;1b,2b]}",
                "-3.5d",
                "{v:[B;1b,-4b,2b]}",
                "{v:[B;-4b,2b]}",
                "{v:[B;-4b,-4b]}",
            ),
            (
                "{v:[I;1,2]}",
                "-3.5f",
                "{v:[I;1,-4,2]}",
                "{v:[I;-4,2]}",
                "{v:[I;-4,-4]}",
            ),
            (
                "{v:[I;1,2]}",
                "-3.5d",
                "{v:[I;1,-4,2]}",
                "{v:[I;-4,2]}",
                "{v:[I;-4,-4]}",
            ),
            (
                "{v:[L;1L,2L]}",
                "-3.5f",
                "{v:[L;1L,-3L,2L]}",
                "{v:[L;-3L,2L]}",
                "{v:[L;-3L,-3L]}",
            ),
            (
                "{v:[L;1L,2L]}",
                "-3.5d",
                "{v:[L;1L,-4L,2L]}",
                "{v:[L;-4L,2L]}",
                "{v:[L;-4L,-4L]}",
            ),
        ] {
            let value = tag(value);
            let mut root = tag(initial);
            assert_eq!(
                path("v")
                    .insert(1, &mut root, std::slice::from_ref(&value))
                    .unwrap(),
                1
            );
            assert_eq!(root, tag(inserted));
            let mut root = tag(initial);
            assert_eq!(path("v[0]").set(&mut root, &value).unwrap(), 1);
            assert_eq!(root, tag(indexed));
            let mut root = tag(initial);
            assert_eq!(path("v[]").set(&mut root, &value).unwrap(), 2);
            assert_eq!(root, tag(all));
        }
    }

    #[test]
    fn numeric_arrays_reject_nonnumeric_insert_and_indexed_set() {
        for initial in ["{v:[B;1b,2b]}", "{v:[I;1,2]}", "{v:[L;1L,2L]}"] {
            let original = tag(initial);
            let value = tag("\"bad\"");
            for index in [0, 99, -99] {
                let mut root = original.clone();
                assert_eq!(
                    path("v")
                        .insert(index, &mut root, std::slice::from_ref(&value))
                        .unwrap(),
                    0
                );
                assert_eq!(root, original);
            }
            let mut root = original.clone();
            assert_eq!(path("v[0]").set(&mut root, &value).unwrap(), 0);
            assert_eq!(root, original);
        }
    }

    #[test]
    fn empty_array_wildcard_set_preserves_vanillas_count_on_rejection() {
        for initial in ["{v:[B;]}", "{v:[I;]}", "{v:[L;]}"] {
            let mut root = tag(initial);
            assert_eq!(path("v[]").set(&mut root, &tag("\"bad\"")).unwrap(), 1);
            assert_eq!(root, tag(initial));
        }
    }

    #[test]
    fn partial_comparison() {
        let actual = tag("{a:1,b:{c:[1,2,3],d:\"x\"},e:2}");
        assert!(compare_nbt(&tag("{b:{c:[2]}}"), Some(&actual)));
        assert!(compare_nbt(&tag("{a:1}"), Some(&actual)));
        assert!(!compare_nbt(&tag("{a:2}"), Some(&actual)));
        assert!(!compare_nbt(&tag("{a:1L}"), Some(&actual)));
        assert!(!compare_nbt(&tag("{z:1}"), Some(&actual)));
        assert!(compare_nbt(&tag("{b:{c:[]}}"), Some(&tag("{b:{c:[]}}"))));
        assert!(!compare_nbt(&tag("{b:{c:[]}}"), Some(&actual)));
    }
}
