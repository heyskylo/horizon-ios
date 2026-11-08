//! ArmorTrim's two registry holders: names or actual inline definitions.
use horizon_world::nbt::Tag;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum Holder<T> {
    Named(Arc<str>),
    Direct(T),
}
impl<T> From<Arc<str>> for Holder<T> {
    fn from(name: Arc<str>) -> Self {
        Self::Named(name)
    }
}
impl<T> From<&str> for Holder<T> {
    fn from(name: &str) -> Self {
        Self::Named(name.into())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub palette_id: Arc<str>,
    pub description: Tag,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub asset_id: Arc<str>,
    pub description: Tag,
    pub decal: bool,
}
pub type Trim = (Holder<Material>, Holder<Pattern>);

/// Existing named-only consumers keep their old behavior without inventing
/// registry names for direct holders.
pub fn named(trim: &Trim) -> Option<(&str, &str)> {
    match trim {
        (Holder::Named(material), Holder::Named(pattern)) => Some((material, pattern)),
        _ => None,
    }
}
fn name(tag: &Tag) -> Option<Arc<str>> {
    match tag {
        Tag::String(s) => Some(s.to_string_lossy().into()),
        _ => None,
    }
}
fn description(tag: &Tag) -> Option<Tag> {
    super::read_text(tag)?;
    Some(super::collapsed_text(tag.clone()))
}
pub fn from_nbt(tag: &Tag) -> Option<Trim> {
    let material = tag.get("material")?;
    let pattern = tag.get("pattern")?;
    Some((
        if let Some(name) = name(material) {
            Holder::Named(name)
        } else {
            Holder::Direct(Material {
                palette_id: name(material.get("palette_id")?)?,
                description: description(material.get("description")?)?,
            })
        },
        if let Some(name) = name(pattern) {
            Holder::Named(name)
        } else {
            Holder::Direct(Pattern {
                asset_id: name(pattern.get("asset_id")?)?,
                description: description(pattern.get("description")?)?,
                decal: matches!(pattern.get("decal"), Some(Tag::Byte(v)) if *v != 0),
            })
        },
    ))
}
pub fn to_nbt(trim: &Trim) -> Tag {
    let material = match &trim.0 {
        Holder::Named(name) => Tag::String((&**name).into()),
        Holder::Direct(value) => Tag::compound([
            ("palette_id", Tag::String((&*value.palette_id).into())),
            ("description", value.description.clone()),
        ]),
    };
    let pattern = match &trim.1 {
        Holder::Named(name) => Tag::String((&**name).into()),
        Holder::Direct(value) => Tag::compound([
            ("asset_id", Tag::String((&*value.asset_id).into())),
            ("description", value.description.clone()),
            ("decal", Tag::Byte(i8::from(value.decal))),
        ]),
    };
    Tag::compound([("material", material), ("pattern", pattern)])
}
