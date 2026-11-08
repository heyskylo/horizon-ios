//! Direct ArmorTrim holders, using the existing stream holder convention.
use crate::{
    Result,
    buf::{Reader, Writer},
};
use horizon_world::nbt::Tag;

#[derive(Clone, Debug, PartialEq)]
pub enum TrimHolder<T> {
    Id(i32),
    Direct(T),
}
#[derive(Clone, Debug, PartialEq)]
pub struct TrimMaterial {
    pub palette_id: String,
    pub description: Tag,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TrimPattern {
    pub asset_id: String,
    pub description: Tag,
    pub decal: bool,
}
impl<T> TrimHolder<T> {
    pub(super) fn read(
        r: &mut Reader,
        direct: impl FnOnce(&mut Reader) -> Result<T>,
    ) -> Result<Self> {
        match r.varint()? {
            0 => Ok(Self::Direct(direct(r)?)),
            ..0 => Err(crate::Error::new("negative holder id")),
            id => Ok(Self::Id(id - 1)),
        }
    }
    pub(super) fn write(&self, w: &mut Writer, direct: impl FnOnce(&T, &mut Writer)) {
        match self {
            Self::Id(id) => w.varint(id + 1),
            Self::Direct(value) => {
                w.varint(0);
                direct(value, w);
            }
        }
    }
}
impl TrimMaterial {
    pub(super) fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            palette_id: r.string(32767)?,
            description: r.nbt()?,
        })
    }
    pub(super) fn write(&self, w: &mut Writer) {
        w.string(&self.palette_id);
        w.nbt(&self.description);
    }
}
impl TrimPattern {
    pub(super) fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            asset_id: r.string(32767)?,
            description: r.nbt()?,
            decal: r.bool()?,
        })
    }
    pub(super) fn write(&self, w: &mut Writer) {
        w.string(&self.asset_id);
        w.nbt(&self.description);
        w.bool(self.decal);
    }
}
