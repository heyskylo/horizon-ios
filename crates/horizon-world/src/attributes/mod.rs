//! Ordered attribute modifiers, native cache invalidation and permanent state.
use crate::compression::invalid;
use std::{fmt, io, sync::Arc};
pub mod entity;
pub mod live;
mod operation_map;
use operation_map::OperationMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Operation {
    AddValue,
    AddMultipliedBase,
    AddMultipliedTotal,
}
impl Operation {
    pub const ALL: [Self; 3] = [
        Self::AddValue,
        Self::AddMultipliedBase,
        Self::AddMultipliedTotal,
    ];
}
#[derive(Debug)]
struct ModifierData {
    id: String,
    hash: u32,
    amount: f64,
    operation: Operation,
}
/// Clones retain native modifier reference identity. Reconstructing an equal
/// modifier creates a distinct reference for add-or-update dirty notifications.
#[derive(Clone, Debug)]
pub struct Modifier(Arc<ModifierData>);
impl Modifier {
    pub fn new(id: &str, amount: f64, operation: Operation) -> io::Result<Self> {
        let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
        let namespace = if namespace.is_empty() {
            "minecraft"
        } else {
            namespace
        };
        let valid = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-".contains(&c);
        if !namespace.bytes().all(valid) || !path.bytes().all(|c| valid(c) || c == b'/') {
            return Err(invalid("invalid attribute modifier identifier"));
        }
        let hash = |s: &str| {
            s.bytes()
                .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
        };
        Ok(Self(Arc::new(ModifierData {
            id: format!("{namespace}:{path}"),
            hash: hash(namespace).wrapping_mul(31).wrapping_add(hash(path)),
            amount,
            operation,
        })))
    }
    pub fn id(&self) -> &str {
        &self.0.id
    }
    pub fn amount(&self) -> f64 {
        self.0.amount
    }
    pub fn operation(&self) -> Operation {
        self.0.operation
    }
    pub fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Definition {
    default: f64,
    range: Option<(f64, f64)>,
}
impl Definition {
    pub fn basic(default: f64) -> Self {
        Self {
            default,
            range: None,
        }
    }
    pub fn ranged(default: f64, min: f64, max: f64) -> io::Result<Self> {
        if min > max || default < min || default > max {
            return Err(invalid("invalid ranged attribute bounds/default"));
        }
        Ok(Self {
            default,
            range: Some((min, max)),
        })
    }
    pub fn default_value(self) -> f64 {
        self.default
    }
    pub fn sanitize(self, value: f64) -> f64 {
        let Some((min, max)) = self.range else {
            return value;
        };
        if value.is_nan() || value < min {
            min
        } else if max.is_nan() {
            max
        } else if value == 0.0 && max == 0.0 {
            f64::from_bits(value.to_bits() | max.to_bits())
        } else {
            value.min(max)
        }
    }
}
pub trait Observer {
    type Error;
    /// Called after dirty=true and preceding writes. Reading the value here can
    /// clear the dirty flag, and an error preserves all writes already made.
    fn dirty(&mut self, attribute: &mut Instance) -> Result<(), Self::Error>;
}
#[derive(Debug)]
pub enum AttributeError<E> {
    DuplicateModifier(String),
    Observer(E),
}
impl<E: fmt::Display> fmt::Display for AttributeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateModifier(id) => write!(f, "attribute modifier already applied: {id}"),
            Self::Observer(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for AttributeError<E> {}
#[derive(Clone, Debug)]
pub struct Packed {
    pub base: f64,
    pub modifiers: Vec<Modifier>,
}
#[derive(Clone, Debug)]
pub struct Instance {
    definition: Definition,
    base: f64,
    dirty: bool,
    cached: f64,
    by_id: Vec<Modifier>,
    permanent: Vec<Modifier>,
    operations: [Option<OperationMap>; 3],
}
impl Instance {
    pub fn new(definition: Definition) -> Self {
        Self {
            definition,
            base: definition.default,
            dirty: true,
            cached: 0.0,
            by_id: Vec::new(),
            permanent: Vec::new(),
            operations: [None, None, None],
        }
    }
    pub fn base_value(&self) -> f64 {
        self.base
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn cached_value(&self) -> f64 {
        self.cached
    }
    pub fn modifiers(&self) -> &[Modifier] {
        &self.by_id
    }
    pub fn permanent_modifiers(&self) -> &[Modifier] {
        &self.permanent
    }
    pub fn modifier(&self, id: &str) -> Option<&Modifier> {
        self.by_id.iter().find(|m| m.id() == id)
    }
    pub fn operation_modifiers(&self, op: Operation) -> impl Iterator<Item = &Modifier> {
        self.operations[op as usize].iter().flat_map(|m| m.values())
    }
    fn mark_dirty<O: Observer>(
        &mut self,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        self.dirty = true;
        observer.dirty(self).map_err(AttributeError::Observer)
    }
    pub fn set_base_value<O: Observer>(
        &mut self,
        value: f64,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        if value == self.base {
            return Ok(());
        }
        self.base = value;
        self.mark_dirty(observer)
    }
    fn insert_operation(&mut self, m: Modifier) {
        self.operations[m.operation() as usize]
            .get_or_insert_with(OperationMap::default)
            .put(m);
    }
    pub fn add_transient<O: Observer>(
        &mut self,
        m: Modifier,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        if self.modifier(m.id()).is_some() {
            return Err(AttributeError::DuplicateModifier(m.id().into()));
        }
        self.by_id.push(m.clone());
        self.insert_operation(m);
        self.mark_dirty(observer)
    }
    pub fn add_or_update_transient<O: Observer>(
        &mut self,
        m: Modifier,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        let old = array_put(&mut self.by_id, m.clone());
        if old.is_some_and(|old| m.same_identity(&old)) {
            return Ok(());
        }
        // Native replacement retains an older operation bucket for the same ID.
        self.insert_operation(m);
        self.mark_dirty(observer)
    }
    pub fn add_permanent<O: Observer>(
        &mut self,
        m: Modifier,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        self.add_transient(m.clone(), observer)?;
        array_put(&mut self.permanent, m);
        Ok(())
    }
    pub fn add_or_replace_permanent<O: Observer>(
        &mut self,
        m: Modifier,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        self.remove(m.id(), observer)?;
        self.add_permanent(m, observer)
    }
    pub fn add_permanent_many<O: Observer>(
        &mut self,
        mods: impl IntoIterator<Item = Modifier>,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        for m in mods {
            self.add_permanent(m, observer)?;
        }
        Ok(())
    }
    pub fn remove<O: Observer>(
        &mut self,
        id: &str,
        observer: &mut O,
    ) -> Result<bool, AttributeError<O::Error>> {
        let Some(index) = self.by_id.iter().position(|m| m.id() == id) else {
            return Ok(false);
        };
        let old = self.by_id.remove(index);
        self.operations[old.operation() as usize]
            .get_or_insert_with(OperationMap::default)
            .remove(&old);
        if let Some(i) = self.permanent.iter().position(|m| m.id() == id) {
            self.permanent.remove(i);
        }
        self.mark_dirty(observer)?;
        Ok(true)
    }
    pub fn remove_all<O: Observer>(
        &mut self,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        let snapshot = self.by_id.clone();
        for m in snapshot {
            self.remove(m.id(), observer)?;
        }
        Ok(())
    }
    pub fn compute_value(&self) -> f64 {
        if !self.dirty {
            return self.cached;
        }
        let mut base = self.base;
        for m in self.operation_modifiers(Operation::AddValue) {
            base += m.amount();
        }
        let mut value = base;
        for m in self.operation_modifiers(Operation::AddMultipliedBase) {
            value += base * m.amount();
        }
        for m in self.operation_modifiers(Operation::AddMultipliedTotal) {
            value *= 1.0 + m.amount();
        }
        self.definition.sanitize(value)
    }
    pub fn value(&mut self) -> f64 {
        if self.dirty {
            self.cached = self.compute_value();
            self.dirty = false;
        }
        self.cached
    }
    pub fn replace_from<O: Observer>(
        &mut self,
        source: &Self,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        self.base = source.base;
        self.by_id = source.by_id.clone();
        self.permanent = source.permanent.clone();
        self.operations = std::array::from_fn(|i| {
            source.operations[i]
                .as_ref()
                .map(OperationMap::copy_for_replace)
        });
        self.mark_dirty(observer)
    }
    /// Java's replaceFrom(this) clears all three maps before copying them.
    pub fn replace_from_self<O: Observer>(
        &mut self,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        self.by_id.clear();
        self.permanent.clear();
        self.operations = [None, None, None];
        self.mark_dirty(observer)
    }
    pub fn pack(&self) -> Packed {
        Packed {
            base: self.base,
            modifiers: self.permanent.clone(),
        }
    }
    /// Packed data merges into existing maps; it does not clear transient state.
    pub fn apply<O: Observer>(
        &mut self,
        packed: &Packed,
        observer: &mut O,
    ) -> Result<(), AttributeError<O::Error>> {
        self.base = packed.base;
        for m in &packed.modifiers {
            array_put(&mut self.by_id, m.clone());
            self.insert_operation(m.clone());
            array_put(&mut self.permanent, m.clone());
        }
        self.mark_dirty(observer)
    }
}
fn array_put(map: &mut Vec<Modifier>, m: Modifier) -> Option<Modifier> {
    if let Some(old) = map.iter_mut().find(|v| v.id() == m.id()) {
        Some(std::mem::replace(old, m))
    } else {
        map.push(m);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Silent;
    impl Observer for Silent {
        type Error = std::convert::Infallible;
        fn dirty(&mut self, _: &mut Instance) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn compute_value_evaluates_dirty_modifiers_without_clearing_state() {
        let def = Definition::ranged(4.0, 0.0, 1024.0).unwrap();
        let mut inst = Instance::new(def);
        assert_eq!(inst.base_value(), 4.0);
        assert_eq!(inst.compute_value(), 4.0);

        let m1 = Modifier::new("test:add", 16.0, Operation::AddValue).unwrap();
        inst.add_transient(m1, &mut Silent).unwrap();
        assert!(inst.is_dirty());
        assert_eq!(inst.compute_value(), 20.0);
        // compute_value did not mutate or reset dirty flag
        assert!(inst.is_dirty());

        let m2 = Modifier::new("test:mult_base", 0.5, Operation::AddMultipliedBase).unwrap();
        inst.add_transient(m2, &mut Silent).unwrap();
        // (4.0 + 16.0) + (20.0 * 0.5) = 30.0
        assert_eq!(inst.compute_value(), 30.0);

        let m3 = Modifier::new("test:mult_total", 0.1, Operation::AddMultipliedTotal).unwrap();
        inst.add_transient(m3, &mut Silent).unwrap();
        // 30.0 * 1.1 = 33.0
        assert!((inst.compute_value() - 33.0).abs() < 1e-6);

        // Calling value() updates cache and clears dirty
        assert!((inst.value() - 33.0).abs() < 1e-6);
        assert!(!inst.is_dirty());
        assert!((inst.compute_value() - 33.0).abs() < 1e-6);
    }
}
