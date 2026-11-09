//! ItemParser (item stacks with components, `stone[damage=5]`) and
//! ItemPredicateArgument (ComponentPredicateParser's packrat grammar:
//! `#logs[count~1..|!damage]`), with vanilla's errors and suggestions.
use super::args::{
    ItemInput, Registries, is_id_char, parse_identifier, read_identifier, suggest_resources,
};
use super::codec::{either, is_number, not_map, snbt};
use super::packrat::{Grammar, Reason, State, Sugg, Term, Value, alt, ch, opt, seq};
use super::{CmdResult, CommandError, Reader, SuggestionsBuilder};
use horizon_world::nbt::{Compound, Tag};
use std::rc::Rc;

fn int_of(t: &Tag) -> Option<i64> {
    Some(match t {
        Tag::Byte(v) => i64::from(*v),
        Tag::Short(v) => i64::from(*v),
        Tag::Int(v) => i64::from(*v),
        Tag::Long(v) => *v as i32 as i64,
        Tag::Float(v) => f32::from_bits(*v) as i32 as i64,
        Tag::Double(v) => f64::from_bits(*v) as i32 as i64,
        _ => return None,
    })
}
/// The codecs of the item components commands commonly set.
pub fn decode_component(id: &str, t: &Tag, reg: &Registries) -> Result<(), String> {
    match id {
        "minecraft:damage" | "minecraft:repair_cost" => {
            let v = int_of(t).ok_or("Not a number")?;
            if v < 0 {
                return Err(format!("Value must be non-negative: {v}"));
            }
            Ok(())
        }
        "minecraft:max_damage" => {
            let v = int_of(t).ok_or("Not a number")?;
            if v <= 0 {
                return Err(format!("Value must be positive: {v}"));
            }
            Ok(())
        }
        "minecraft:max_stack_size" => {
            let v = int_of(t).ok_or("Not a number")?;
            if !(1..=99).contains(&v) {
                return Err(format!("Value must be within range [1;99]: {v}"));
            }
            Ok(())
        }
        "minecraft:rarity" => match t {
            Tag::String(s) => {
                let s = s.to_string_lossy();
                if ["common", "uncommon", "rare", "epic"].contains(&s.as_str()) {
                    Ok(())
                } else {
                    Err(format!("Unknown element name:{s}"))
                }
            }
            _ => Err("Not a string".into()),
        },
        "minecraft:enchantments" | "minecraft:stored_enchantments" => enchantment_map(t, reg),
        "minecraft:custom_name" | "minecraft:item_name" => super::component::decode(t),
        // TypedEntityData.codec: the type is the compound's id.
        "minecraft:block_entity_data" => match t {
            Tag::Compound(c) => match c.get(&"id".into()) {
                None => Err(format!("Expected 'id' field in {}", snbt(t))),
                Some(Tag::String(id)) => {
                    let id = id.to_string_lossy();
                    let id = if id.contains(':') {
                        id
                    } else {
                        format!("minecraft:{id}")
                    };
                    let known = horizon_client_core::block_entity_nbt::TYPES
                        .iter()
                        .chain(&horizon_client_core::block_entity_nbt::NEWEST)
                        .any(|k| *k == id);
                    if known {
                        Ok(())
                    } else {
                        Err(format!(
                            "Unknown registry key in ResourceKey[minecraft:root / minecraft:block_entity_type]: {id}"
                        ))
                    }
                }
                Some(_) => Ok(()),
            },
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}
/// `ItemEnchantments.CODEC`: registry keys and boxed-number levels in 1..=255.
/// `UnboundedMapCodec` reports failed entries; commands do not retain its partial map.
fn enchantment_map(t: &Tag, reg: &Registries) -> Result<(), String> {
    let Tag::Compound(map) = t else {
        return Err(not_map(t));
    };
    let mut errors = Vec::new();
    let mut missed = Compound::new();
    for (key, value) in map {
        let mut entry_errors = Vec::new();
        match int_of(value) {
            None => entry_errors.push("Not a number".to_owned()),
            Some(level) if !(1..=255).contains(&level) => {
                entry_errors.push(format!("Value {level} outside of range [1:255]"));
            }
            _ => {}
        }
        let raw = key.to_string_lossy();
        match parse_identifier(&raw) {
            Some(id) if reg.ids("minecraft:enchantment").contains(&id) => {}
            Some(id) => entry_errors.push(format!(
                "Failed to get element {id} from registry minecraft:enchantment"
            )),
            None => entry_errors.push(format!("Not a valid resource location: {raw}")),
        }
        if !entry_errors.is_empty() {
            errors.extend(entry_errors);
            missed.insert(key.clone(), value.clone());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} missed input: {}",
            errors.join("; "),
            snbt(&Tag::Compound(missed))
        ))
    }
}

/// MinMaxBounds.Ints.CODEC.
fn int_bounds(t: &Tag) -> Result<(), String> {
    match t {
        Tag::Compound(_) => Ok(()),
        _ if is_number(t) => Ok(()),
        _ => Err(either(not_map(t), "Not a number".into())),
    }
}
/// DataComponentPredicate type codecs.
pub fn decode_predicate(id: &str, t: &Tag) -> Result<(), String> {
    match id {
        "minecraft:count" => int_bounds(t),
        "minecraft:damage" => match t {
            Tag::Compound(_) => Ok(()),
            _ => Err(not_map(t)),
        },
        _ => Ok(()),
    }
}

// ---- ItemParser -------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum ISugg {
    Nothing,
    Item,
    StartComponents,
    ComponentAssignmentOrRemoval,
    Component,
    Assignment,
    NextOrEndComponents,
}

struct ItemState<'r, 'a> {
    reader: &'r mut Reader<'a>,
    reg: &'r Registries,
    sugg: ISugg,
}
impl ItemState<'_, '_> {
    fn parse(&mut self) -> CmdResult<ItemInput> {
        self.sugg = ISugg::Item;
        let cursor = self.reader.cursor;
        let id = read_identifier(self.reader)?;
        if !self.reg.ids("minecraft:item").contains(&id) {
            self.reader.cursor = cursor;
            return Err(CommandError::tr_at(
                "argument.item.id.invalid",
                &[&id],
                self.reader,
            ));
        }
        self.sugg = ISugg::StartComponents;
        let mut components = None;
        if self.reader.can_read() && self.reader.peek() == '[' {
            self.sugg = ISugg::Nothing;
            let start = self.reader.cursor;
            self.read_components()?;
            components = Some(self.reader.string[start..self.reader.cursor].to_owned());
        }
        Ok(ItemInput {
            id,
            tag: false,
            any: false,
            components,
        })
    }
    fn component_type(&mut self) -> CmdResult<String> {
        if !self.reader.can_read() {
            return Err(CommandError::tr_at(
                "arguments.item.component.expected",
                &[],
                self.reader,
            ));
        }
        let cursor = self.reader.cursor;
        let id = read_identifier(self.reader)?;
        if self.reg.persistent_components.contains(&id) {
            return Ok(id);
        }
        self.reader.cursor = cursor;
        Err(CommandError::tr_at(
            "arguments.item.component.unknown",
            &[&id],
            self.reader,
        ))
    }
    fn read_components(&mut self) -> CmdResult<()> {
        self.reader.expect('[')?;
        self.sugg = ISugg::ComponentAssignmentOrRemoval;
        let mut visited: Vec<String> = Vec::new();
        while self.reader.can_read() && self.reader.peek() != ']' {
            self.reader.skip_whitespace();
            if self.reader.can_read() && self.reader.peek() == '!' {
                self.reader.skip();
                self.sugg = ISugg::Component;
                let ty = self.component_type()?;
                if visited.contains(&ty) {
                    return Err(CommandError::tr(
                        "arguments.item.component.repeated",
                        &[&ty],
                    ));
                }
                visited.push(ty);
                self.sugg = ISugg::Nothing;
                self.reader.skip_whitespace();
            } else {
                let ty = self.component_type()?;
                if visited.contains(&ty) {
                    return Err(CommandError::tr(
                        "arguments.item.component.repeated",
                        &[&ty],
                    ));
                }
                visited.push(ty.clone());
                self.sugg = ISugg::Assignment;
                self.reader.skip_whitespace();
                self.reader.expect('=')?;
                self.sugg = ISugg::Nothing;
                self.reader.skip_whitespace();
                let cursor = self.reader.cursor;
                let tag = super::snbt::parse_tag_argument(self.reader)?;
                if let Err(m) = decode_component(&ty, &tag, self.reg) {
                    self.reader.cursor = cursor;
                    return Err(CommandError::tr_at(
                        "arguments.item.component.malformed",
                        &[&ty, &m],
                        self.reader,
                    ));
                }
                self.reader.skip_whitespace();
            }
            self.sugg = ISugg::NextOrEndComponents;
            if !self.reader.can_read() || self.reader.peek() != ',' {
                break;
            }
            self.reader.skip();
            self.reader.skip_whitespace();
            self.sugg = ISugg::ComponentAssignmentOrRemoval;
            if !self.reader.can_read() {
                return Err(CommandError::tr_at(
                    "arguments.item.component.expected",
                    &[],
                    self.reader,
                ));
            }
        }
        self.reader.expect(']')?;
        self.sugg = ISugg::Nothing;
        Ok(())
    }
    fn suggest_component(&self, b: &mut SuggestionsBuilder, suffix: &str) {
        let contents = b.remaining_lower();
        for id in &self.reg.persistent_components {
            let matched = if contents.contains(':') {
                super::args::matches_sub_str(&contents, id)
            } else {
                let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
                super::args::matches_sub_str(&contents, ns)
                    || super::args::matches_sub_str(&contents, path)
            };
            if matched {
                b.suggest(format!("{id}{suffix}"));
            }
        }
    }
    fn fill(&self, builder: &mut SuggestionsBuilder) {
        let mut b = builder.offset(self.reader.cursor);
        let empty = b.remaining().is_empty();
        match self.sugg {
            ISugg::Nothing => {}
            ISugg::Item => suggest_resources(&mut b, self.reg.ids("minecraft:item"), ""),
            ISugg::StartComponents => {
                if empty {
                    b.suggest("[");
                }
            }
            ISugg::ComponentAssignmentOrRemoval => {
                b.suggest("!");
                self.suggest_component(&mut b, "=");
            }
            ISugg::Component => self.suggest_component(&mut b, ""),
            ISugg::Assignment => {
                if empty {
                    b.suggest("=");
                }
            }
            ISugg::NextOrEndComponents => {
                if empty {
                    b.suggest(",");
                    b.suggest("]");
                }
            }
        }
        builder.add(b);
    }
}

/// ItemArgument.parse (ItemParser.parse; the reader is rewound on errors).
pub fn parse_stack(reader: &mut Reader, reg: &Registries) -> CmdResult<ItemInput> {
    let cursor = reader.cursor;
    let r = ItemState {
        reader,
        reg,
        sugg: ISugg::Nothing,
    }
    .parse();
    if r.is_err() {
        reader.cursor = cursor;
    }
    r
}
/// ItemParser.fillSuggestions.
pub fn suggest_stack(builder: &mut SuggestionsBuilder, reg: &Registries) {
    let mut reader = Reader {
        string: builder.input,
        cursor: builder.start,
    };
    let mut state = ItemState {
        reader: &mut reader,
        reg,
        sugg: ISugg::Nothing,
    };
    let _ = state.parse();
    state.fill(builder);
}

// ---- ItemPredicateArgument ------------------------------------------------

#[derive(Clone, Debug)]
pub enum TypeRes {
    Item(String),
    Tag(String),
}
#[derive(Clone, Debug)]
pub enum IV {
    Id(String),
    Type(Option<TypeRes>),
    Elem(TypeRes),
    Unit,
    Test,
    Comp(String),
    Pred(String),
    Tag(Tag),
    List(Vec<IV>),
}
impl Value for IV {
    fn list(items: Vec<Self>) -> Self {
        IV::List(items)
    }
}

fn reg<'a>(state: &State<'a, IV>) -> &'a Registries {
    state
        .ctx
        .and_then(|c| c.downcast_ref::<Registries>())
        .expect("item predicate grammar needs the registries")
}

/// IdentifierParseRule: skip whitespace, a non-empty identifier (no error).
fn id_rule(state: &mut State<IV>) -> Option<IV> {
    let mut r = state.reader();
    r.skip_whitespace();
    let start = r.cursor;
    while r.can_read() && is_id_char(r.peek()) {
        r.skip();
    }
    let raw = &r.string[start..r.cursor];
    let id = if raw.is_empty() {
        None
    } else {
        parse_identifier(raw)
    };
    match id {
        Some(id) => {
            state.set_reader(&r);
            Some(IV::Id(id))
        }
        None => {
            r.cursor = start;
            state.set_reader(&r);
            None
        }
    }
}

/// ResourceLookupRule: an identifier validated by `validate`.
fn lookup(
    state: &mut State<IV>,
    id_rule: usize,
    possible: Rc<Vec<String>>,
    validate: &dyn Fn(&str, &Reader) -> Result<IV, CommandError>,
) -> Option<IV> {
    let mut r = state.reader();
    r.skip_whitespace();
    state.set_reader(&r);
    let mark = state.cursor;
    match state.parse(id_rule) {
        Some(IV::Id(id)) => match validate(&id, &state.reader()) {
            Ok(v) => Some(v),
            Err(e) => {
                state.store(mark, Sugg::Resources(possible), Reason::Error(e));
                None
            }
        },
        _ => {
            state.store(
                mark,
                Sugg::Resources(possible),
                Reason::Delayed("argument.id.invalid", Vec::new()),
            );
            None
        }
    }
}

fn predicate_grammar() -> Grammar<IV> {
    let mut g: Grammar<IV> = Grammar::new();
    let id = g.put_custom("id", id_rule);
    let ty = g.named("type");
    let conditions = g.named("conditions");
    let ty2 = g.named("type");
    g.put_term(
        "top",
        alt(vec![
            seq(vec![ty, ch('['), Term::Cut, opt(conditions), ch(']')]),
            ty2,
        ]),
        |s| {
            let mut out = Vec::new();
            if let Some(t) = s.scope.get("type") {
                out.push(t);
            }
            if let Some(IV::List(c)) = s.scope.get("conditions") {
                out.extend(c);
            }
            Some(IV::List(out))
        },
    );
    let element = g.named("element_type");
    let tag = g.named("tag_type");
    let any = g.named("any_type");
    g.put_term(
        "type",
        alt(vec![element, seq(vec![ch('#'), Term::Cut, tag]), any]),
        |s| {
            Some(IV::Type(
                match s.scope.get_any(&["element_type", "tag_type"]) {
                    Some(IV::Elem(t)) => Some(t),
                    _ => None,
                },
            ))
        },
    );
    g.put_term("any_type", ch('*'), |_| Some(IV::Unit));
    g.put_custom("element_type", move |s| {
        let r = reg(s);
        let items = Rc::new(r.ids("minecraft:item").to_vec());
        lookup(s, id, items, &|id, reader| {
            if r.ids("minecraft:item").iter().any(|i| i == id) {
                Ok(IV::Elem(TypeRes::Item(id.to_owned())))
            } else {
                Err(CommandError::tr_at(
                    "argument.item.id.invalid",
                    &[&id],
                    reader,
                ))
            }
        })
    });
    g.put_custom("tag_type", move |s| {
        let r = reg(s);
        let tags = Rc::new(r.tags("minecraft:item").to_vec());
        lookup(s, id, tags, &|id, reader| {
            if r.tags("minecraft:item").iter().any(|i| i == id) {
                Ok(IV::Elem(TypeRes::Tag(id.to_owned())))
            } else {
                Err(CommandError::tr_at(
                    "arguments.item.tag.unknown",
                    &[&id],
                    reader,
                ))
            }
        })
    });
    let alternatives = g.named("alternatives");
    let rest = g.named("conditions");
    g.put_term(
        "conditions",
        seq(vec![alternatives, opt(seq(vec![ch(','), rest]))]),
        |s| {
            let mut out = vec![IV::Test];
            if let Some(IV::List(c)) = s.scope.get("conditions") {
                out.extend(c);
            }
            Some(IV::List(out))
        },
    );
    let term = g.named("term");
    let rest = g.named("alternatives");
    g.put_term(
        "alternatives",
        seq(vec![term, opt(seq(vec![ch('|'), rest]))]),
        |s| {
            let mut out = vec![s.scope.get("term").unwrap_or(IV::Test)];
            if let Some(IV::List(c)) = s.scope.get("alternatives") {
                out.extend(c);
            }
            Some(IV::List(out))
        },
    );
    let test = g.named("test");
    let negation = g.named("negation");
    g.put_term("term", alt(vec![test, seq(vec![ch('!'), negation])]), |s| {
        s.scope.get_any(&["test", "negation"])
    });
    let test = g.named("test");
    g.put_term("negation", test, |_| Some(IV::Test));
    let comp = g.named("component_type");
    let tag_a = g.named("tag");
    let pred = g.named("predicate_type");
    let tag_b = g.named("tag");
    let comp2 = g.named("component_type");
    g.put_term(
        "test",
        alt(vec![
            seq(vec![comp, ch('='), Term::Cut, tag_a]),
            seq(vec![pred, ch('~'), Term::Cut, tag_b]),
            comp2,
        ]),
        |s| {
            let value = match s.scope.get("tag") {
                Some(IV::Tag(t)) => Some(t),
                _ => None,
            };
            let reader = s.reader();
            let result = if let Some(IV::Pred(p)) = s.scope.get("predicate_type") {
                decode_predicate(&p, value.as_ref().unwrap_or(&Tag::End)).map_err(|m| {
                    CommandError::tr_at("arguments.item.predicate.malformed", &[&p, &m], &reader)
                })
            } else if let (Some(IV::Comp(c)), Some(v)) = (s.scope.get("component_type"), value) {
                decode_component(&c, &v, reg(s)).map_err(|m| {
                    CommandError::tr_at("arguments.item.component.malformed", &[&c, &m], &reader)
                })
            } else {
                Ok(())
            };
            match result {
                Ok(()) => Some(IV::Test),
                Err(e) => {
                    s.store(s.cursor, Sugg::Empty, Reason::Error(e));
                    None
                }
            }
        },
    );
    g.put_custom("component_type", move |s| {
        let r = reg(s);
        let mut list = vec!["minecraft:count".to_owned()];
        list.extend(r.persistent_components.iter().cloned());
        lookup(s, id, Rc::new(list), &|id, reader| {
            if id == "minecraft:count" {
                return Ok(IV::Comp(id.to_owned()));
            }
            let known = r
                .ids("minecraft:data_component_type")
                .iter()
                .any(|c| c == id);
            if known && r.persistent_components.iter().any(|c| c == id) {
                Ok(IV::Comp(id.to_owned()))
            } else {
                Err(CommandError::tr_at(
                    "arguments.item.component.unknown",
                    &[&id],
                    reader,
                ))
            }
        })
    });
    g.put_custom("predicate_type", move |s| {
        let r = reg(s);
        let mut list = vec!["minecraft:count".to_owned()];
        list.extend(
            r.ids("minecraft:data_component_predicate_type")
                .iter()
                .cloned(),
        );
        lookup(s, id, Rc::new(list), &|id, reader| {
            if id == "minecraft:count"
                || r.ids("minecraft:data_component_predicate_type")
                    .iter()
                    .any(|c| c == id)
            {
                return Ok(IV::Pred(id.to_owned()));
            }
            if r.ids("minecraft:data_component_type")
                .iter()
                .any(|c| c == id)
            {
                // A component existence predicate (decodes Unit).
                return Ok(IV::Pred(format!("exists:{id}")));
            }
            Err(CommandError::tr_at(
                "arguments.item.predicate.unknown",
                &[&id],
                reader,
            ))
        })
    });
    g.put_custom("tag", |s| super::snbt::rule_in(s, IV::Tag));
    g.top = g.id("top");
    g
}

thread_local! {
    static PREDICATE: Grammar<IV> = predicate_grammar();
}
/// ItemPredicateArgument.parse.
pub fn parse_predicate(reader: &mut Reader, reg: &Registries) -> CmdResult<ItemInput> {
    let start = reader.cursor;
    let result = PREDICATE.with(|g| g.parse_for_commands_with(reader, Some(reg)))?;
    let IV::List(items) = result else {
        return Err(CommandError::tr_at("argument.id.invalid", &[], reader));
    };
    let text = &reader.string[start..reader.cursor];
    let (id, tag, any) = match items.first() {
        Some(IV::Type(Some(TypeRes::Item(i)))) => (i.clone(), false, false),
        Some(IV::Type(Some(TypeRes::Tag(t)))) => (t.clone(), true, false),
        _ => ("*".to_owned(), false, true),
    };
    let components = text.find('[').map(|i| text[i..].to_owned());
    Ok(ItemInput {
        id,
        tag,
        any,
        components,
    })
}
/// ItemPredicateArgument.listSuggestions.
pub fn suggest_predicate(builder: &mut SuggestionsBuilder, reg: &Registries) {
    PREDICATE.with(|g| g.parse_for_suggestions_with(builder, Some(reg)));
}

#[cfg(test)]
mod tests {
    use super::decode_component;

    #[test]
    fn enchantment_maps_validate_entries_against_the_supplied_registry() {
        use super::{Reader, Registries, parse_predicate, parse_stack};
        use crate::args::RegistryIds;
        let mut reg = Registries {
            items: vec!["minecraft:diamond_sword".into()],
            persistent_components: vec![
                "minecraft:enchantments".into(),
                "minecraft:stored_enchantments".into(),
            ],
            ..Default::default()
        };
        reg.by_registry.insert(
            "minecraft:data_component_type".into(),
            RegistryIds {
                ids: reg.persistent_components.clone(),
                tags: vec![],
            },
        );
        reg.by_registry.insert(
            "minecraft:enchantment".into(),
            RegistryIds {
                ids: vec!["minecraft:sharpness".into()],
                tags: vec![],
            },
        );
        for component in ["enchantments", "stored_enchantments"] {
            for value in [
                "{}",
                "{sharpness:1}",
                "{sharpness:255}",
                "{sharpness:1.5f}",
                "{sharpness:1b}",
                "{sharpness:4294967297L}",
                "{sharpness:9007199254740993L}",
                "{\"minecraft:sharpness\":1}",
            ] {
                let input = format!("diamond_sword[{component}={value}]");
                for parser in [parse_stack, parse_predicate] {
                    let mut reader = Reader::new(&input);
                    assert!(parser(&mut reader, &reg).is_ok(), "{input}");
                    assert_eq!(reader.cursor, input.len());
                }
            }
            for value in [
                "{sharpness:-1}",
                "{sharpness:0}",
                "{sharpness:256}",
                "{sharpness:100000}",
                "{sharpness:-0.5d}",
                "{sharpness:\"1\"}",
                "{levels:{sharpness:1}}",
                "{\"audit:missing\":1}",
            ] {
                let input = format!("diamond_sword[{component}={value}]");
                for parser in [parse_stack, parse_predicate] {
                    assert!(
                        parser(&mut Reader::new(&input), &reg).is_err(),
                        "{input} crossed command parsing"
                    );
                }
            }
        }
    }

    fn check(snbt: &str) -> Result<(), String> {
        let tag = horizon_world::nbt::parse_snbt(snbt).expect("snbt").0;
        decode_component(
            "minecraft:block_entity_data",
            &tag,
            &super::Registries::default(),
        )
    }

    #[test]
    fn block_entity_data_needs_a_known_type() {
        assert_eq!(check("{id:\"chest\",Lock:1}"), Ok(()));
        assert_eq!(
            check("{Command:\"say\"}"),
            Err("Expected 'id' field in {Command:\"say\"}".into())
        );
        assert_eq!(
            check("{id:\"minecraft:stone\"}"),
            Err("Unknown registry key in ResourceKey[minecraft:root / minecraft:block_entity_type]: minecraft:stone".into())
        );
    }
}
