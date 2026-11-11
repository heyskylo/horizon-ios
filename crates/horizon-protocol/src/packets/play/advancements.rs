//! The advancement packets: `ClientboundUpdateAdvancementsPacket` and
//! `ServerboundSeenAdvancementsPacket`.
use crate::{
    Result,
    buf::{Codec, Reader, Writer},
};
use horizon_world::nbt::Tag;

/// `DisplayInfo` of an advancement as sent.
#[derive(Clone, Debug, PartialEq)]
pub struct AdvancementDisplay {
    pub title: Tag,
    pub description: Tag,
    pub icon: crate::item::Stack,
    /// `AdvancementType` ordinal: task, challenge, goal.
    pub frame: i32,
    pub show_toast: bool,
    pub hidden: bool,
    pub background: Option<String>,
}
/// `ClientboundUpdateAdvancementsPacket.PositionedAdvancement`.
#[derive(Clone, Debug, PartialEq)]
pub struct Advancement {
    pub id: String,
    pub parent: Option<String>,
    pub display: Option<AdvancementDisplay>,
    pub requirements: Vec<Vec<String>>,
    pub telemetry: bool,
    pub x: f32,
    pub y: f32,
}
/// `ClientboundUpdateAdvancementsPacket`: whether it replaces what the
/// client has, the advancements added and removed, the progress of each
/// changed one (criterion, when obtained in epoch milliseconds), and whether
/// to show toasts.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateAdvancements {
    pub reset: bool,
    pub added: Vec<Advancement>,
    pub removed: Vec<String>,
    pub progress: Vec<(String, Vec<(String, Option<i64>)>)>,
    pub show: bool,
}
impl Codec<UpdateAdvancements> for UpdateAdvancements {
    fn read(r: &mut Reader) -> Result<Self> {
        let reset = r.bool()?;
        let n = r.count(r.remaining())?;
        let mut added = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            let id = r.string(32767)?;
            let parent = if r.bool()? {
                Some(r.string(32767)?)
            } else {
                None
            };
            let display = if r.bool()? {
                let title = r.nbt()?;
                let description = r.nbt()?;
                let icon = crate::item::Template::read(r)?;
                let frame = r.varint()?;
                let flags = r.i32()?;
                let background = if flags & 1 != 0 {
                    Some(r.string(32767)?)
                } else {
                    None
                };
                Some(AdvancementDisplay {
                    title,
                    description,
                    icon,
                    frame,
                    show_toast: flags & 2 != 0,
                    hidden: flags & 4 != 0,
                    background,
                })
            } else {
                None
            };
            let groups = r.count(r.remaining())?;
            let mut requirements = Vec::with_capacity(groups.min(256));
            for _ in 0..groups {
                let k = r.count(r.remaining())?;
                requirements.push((0..k).map(|_| r.string(32767)).collect::<Result<_>>()?);
            }
            let telemetry = r.bool()?;
            let (x, y) = (r.f32()?, r.f32()?);
            added.push(Advancement {
                id,
                parent,
                display,
                requirements,
                telemetry,
                x,
                y,
            });
        }
        let n = r.count(r.remaining())?;
        let removed = (0..n).map(|_| r.string(32767)).collect::<Result<_>>()?;
        let n = r.count(r.remaining())?;
        let mut progress = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            let id = r.string(32767)?;
            let k = r.count(r.remaining())?;
            let mut criteria = Vec::with_capacity(k.min(256));
            for _ in 0..k {
                let name = r.string(32767)?;
                let at = if r.bool()? { Some(r.i64()?) } else { None };
                criteria.push((name, at));
            }
            progress.push((id, criteria));
        }
        let show = r.bool()?;
        Ok(Self {
            reset,
            added,
            removed,
            progress,
            show,
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        w.bool(v.reset);
        w.varint(v.added.len() as i32);
        for a in &v.added {
            w.string(&a.id);
            w.bool(a.parent.is_some());
            if let Some(p) = &a.parent {
                w.string(p);
            }
            w.bool(a.display.is_some());
            if let Some(d) = &a.display {
                w.nbt(&d.title);
                w.nbt(&d.description);
                crate::item::Template::write(&d.icon, w);
                w.varint(d.frame);
                w.i32(
                    i32::from(d.background.is_some())
                        | i32::from(d.show_toast) << 1
                        | i32::from(d.hidden) << 2,
                );
                if let Some(b) = &d.background {
                    w.string(b);
                }
            }
            w.varint(a.requirements.len() as i32);
            for g in &a.requirements {
                w.varint(g.len() as i32);
                for c in g {
                    w.string(c);
                }
            }
            w.bool(a.telemetry);
            w.f32(a.x);
            w.f32(a.y);
        }
        w.varint(v.removed.len() as i32);
        for id in &v.removed {
            w.string(id);
        }
        w.varint(v.progress.len() as i32);
        for (id, criteria) in &v.progress {
            w.string(id);
            w.varint(criteria.len() as i32);
            for (name, at) in criteria {
                w.string(name);
                w.bool(at.is_some());
                if let Some(at) = at {
                    w.i64(*at);
                }
            }
        }
        w.bool(v.show);
    }
}

/// ServerboundSeenAdvancementsPacket: the tab the player opened (action 0, `OPENED_TAB`), or
/// that its screen closed (1, `CLOSED_SCREEN`, which carries no tab).
#[derive(Clone, Debug, PartialEq)]
pub struct SeenAdvancements {
    pub action: i32,
    pub tab: Option<String>,
}
impl Codec<SeenAdvancements> for SeenAdvancements {
    fn read(r: &mut Reader) -> Result<SeenAdvancements> {
        let action = r.varint()?;
        // The identifier is there for OPENED_TAB only (`readIdentifier` in the other branch).
        let tab = match action {
            0 => Some(r.string(32767)?),
            _ => None,
        };
        Ok(SeenAdvancements { action, tab })
    }
    fn write(v: &SeenAdvancements, w: &mut Writer) {
        w.varint(v.action);
        if v.action == 0 {
            w.string(v.tab.as_deref().unwrap_or_default());
        }
    }
}
