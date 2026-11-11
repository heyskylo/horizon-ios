//! The packets the HUD draws from: the boss bar (`ClientboundBossEventPacket`), the
//! scoreboard's objectives and scores, and the teams.
use crate::{
    Error, Result,
    buf::{Codec, List, Nbt, Opt, Reader, Str, VarInt, Writer},
    packet,
};
use horizon_world::nbt::Tag;

/// `ClientboundBossEventPacket` (`boss_event`).
#[derive(Clone, Debug, PartialEq)]
pub enum BossAction {
    Add {
        name: Tag,
        progress: f32,
        /// `BossBarColor` ordinal.
        color: i32,
        /// `BossBarOverlay` ordinal.
        overlay: i32,
        darken: bool,
        music: bool,
        fog: bool,
    },
    Remove,
    UpdateProgress {
        progress: f32,
    },
    UpdateName {
        name: Tag,
    },
    UpdateStyle {
        color: i32,
        overlay: i32,
    },
    UpdateProperties {
        darken: bool,
        music: bool,
        fog: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct BossEvent {
    pub id: u128,
    pub action: BossAction,
}
fn boss_flags(darken: bool, music: bool, fog: bool) -> u8 {
    u8::from(darken) | (u8::from(music) << 1) | (u8::from(fog) << 2)
}
fn split_flags(flags: u8) -> (bool, bool, bool) {
    (flags & 1 != 0, flags & 2 != 0, flags & 4 != 0)
}
impl Codec<BossEvent> for BossEvent {
    fn read(r: &mut Reader) -> Result<BossEvent> {
        let id = r.uuid()?;
        let action = match r.varint()? {
            0 => {
                let name = r.nbt()?;
                let progress = r.f32()?;
                let color = r.varint()?;
                let overlay = r.varint()?;
                let (darken, music, fog) = split_flags(r.u8()?);
                BossAction::Add {
                    name,
                    progress,
                    color,
                    overlay,
                    darken,
                    music,
                    fog,
                }
            }
            1 => BossAction::Remove,
            2 => BossAction::UpdateProgress { progress: r.f32()? },
            3 => BossAction::UpdateName { name: r.nbt()? },
            4 => BossAction::UpdateStyle {
                color: r.varint()?,
                overlay: r.varint()?,
            },
            5 => {
                let (darken, music, fog) = split_flags(r.u8()?);
                BossAction::UpdateProperties { darken, music, fog }
            }
            op => return Err(Error::new(format!("unknown boss event operation {op}"))),
        };
        Ok(BossEvent { id, action })
    }
    fn write(v: &BossEvent, w: &mut Writer) {
        w.uuid(v.id);
        match &v.action {
            BossAction::Add {
                name,
                progress,
                color,
                overlay,
                darken,
                music,
                fog,
            } => {
                w.varint(0);
                w.nbt(name);
                w.f32(*progress);
                w.varint(*color);
                w.varint(*overlay);
                w.u8(boss_flags(*darken, *music, *fog));
            }
            BossAction::Remove => w.varint(1),
            BossAction::UpdateProgress { progress } => {
                w.varint(2);
                w.f32(*progress);
            }
            BossAction::UpdateName { name } => {
                w.varint(3);
                w.nbt(name);
            }
            BossAction::UpdateStyle { color, overlay } => {
                w.varint(4);
                w.varint(*color);
                w.varint(*overlay);
            }
            BossAction::UpdateProperties { darken, music, fog } => {
                w.varint(5);
                w.u8(boss_flags(*darken, *music, *fog));
            }
        }
    }
}

packet!(
    /// ClientboundSetDisplayObjectivePacket: `DisplaySlot` id (0 list,
    /// 1 sidebar, 2 below name, 3.. team sidebars) and the objective ("" clears).
    SetDisplayObjective { slot: i32 => VarInt, objective: String => Str }
);
packet!(
    /// ClientboundResetScorePacket: one objective, or all of them when absent.
    ResetScore { owner: String => Str, objective: Option<String> => Opt<Str> }
);
/// `NumberFormatTypes.STREAM_CODEC`, by `number_format_type` registry id.
#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormat {
    /// 0: `minecraft:blank`.
    Blank,
    /// 1: `minecraft:styled`, a style compound.
    Styled(Tag),
    /// 2: `minecraft:fixed`, a component.
    Fixed(Tag),
}
impl Codec<NumberFormat> for NumberFormat {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(match r.varint()? {
            0 => NumberFormat::Blank,
            1 => NumberFormat::Styled(r.nbt()?),
            2 => NumberFormat::Fixed(r.nbt()?),
            other => return Err(crate::Error::new(format!("unknown number format {other}"))),
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        match v {
            NumberFormat::Blank => w.varint(0),
            NumberFormat::Styled(t) => {
                w.varint(1);
                w.nbt(t);
            }
            NumberFormat::Fixed(t) => {
                w.varint(2);
                w.nbt(t);
            }
        }
    }
}
packet!(
    /// ClientboundSetScorePacket.
    SetScore {
        owner: String => Str,
        objective: String => Str,
        score: i32 => VarInt,
        display: Option<Tag> => Opt<Nbt>,
        number_format: Option<NumberFormat> => Opt<NumberFormat>,
    }
);
/// ClientboundSetObjectivePacket: method 0 adds, 1 removes, 2 changes;
/// adds and changes carry the display name, render type (0 integer,
/// 1 hearts) and number format.
#[derive(Clone, Debug, PartialEq)]
pub struct SetObjective {
    pub name: String,
    pub method: i8,
    pub display: Option<(Tag, i32, Option<NumberFormat>)>,
}
impl Codec<SetObjective> for SetObjective {
    fn read(r: &mut Reader) -> Result<Self> {
        let name = r.string(32767)?;
        let method = r.i8()?;
        let display = if method == 0 || method == 2 {
            Some((r.nbt()?, r.varint()?, <Opt<NumberFormat>>::read(r)?))
        } else {
            None
        };
        Ok(Self {
            name,
            method,
            display,
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        w.string(&v.name);
        w.i8(v.method);
        if v.method == 0 || v.method == 2 {
            let (name, render, format) = v.display.clone().unwrap_or((Tag::End, 0, None));
            w.nbt(&name);
            w.varint(render);
            <Opt<NumberFormat>>::write(&format, w);
        }
    }
}

/// `ClientboundSetPlayerTeamPacket.Parameters`.
#[derive(Clone, Debug, PartialEq)]
pub struct TeamParameters {
    pub display_name: Tag,
    pub prefix: Tag,
    pub suffix: Tag,
    /// `Team.Visibility` id.
    pub name_tag_visibility: i32,
    /// `Team.CollisionRule` id.
    pub collision_rule: i32,
    /// `TeamColor` id (0 black .. 15 white), absent for no colour.
    pub color: Option<i32>,
    /// 1: friendly fire, 2: see friendly invisibles.
    pub options: i8,
}
/// ClientboundSetPlayerTeamPacket: method 0 adds (parameters and players),
/// 1 removes, 2 changes (parameters), 3 joins and 4 leaves (players).
#[derive(Clone, Debug, PartialEq)]
pub struct SetPlayerTeam {
    pub name: String,
    pub method: i8,
    pub parameters: Option<TeamParameters>,
    pub players: Vec<String>,
}
impl Codec<SetPlayerTeam> for SetPlayerTeam {
    fn read(r: &mut Reader) -> Result<Self> {
        let name = r.string(32767)?;
        let method = r.i8()?;
        let parameters = if method == 0 || method == 2 {
            Some(TeamParameters {
                display_name: r.nbt()?,
                prefix: r.nbt()?,
                suffix: r.nbt()?,
                name_tag_visibility: r.varint()?,
                collision_rule: r.varint()?,
                color: <Opt<VarInt>>::read(r)?,
                options: r.i8()?,
            })
        } else {
            None
        };
        let players = if matches!(method, 0 | 3 | 4) {
            <List<Str>>::read(r)?
        } else {
            Vec::new()
        };
        Ok(Self {
            name,
            method,
            parameters,
            players,
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        w.string(&v.name);
        w.i8(v.method);
        if v.method == 0 || v.method == 2 {
            let p = v.parameters.clone().unwrap_or(TeamParameters {
                display_name: Tag::End,
                prefix: Tag::End,
                suffix: Tag::End,
                name_tag_visibility: 0,
                collision_rule: 0,
                color: None,
                options: 0,
            });
            w.nbt(&p.display_name);
            w.nbt(&p.prefix);
            w.nbt(&p.suffix);
            w.varint(p.name_tag_visibility);
            w.varint(p.collision_rule);
            <Opt<VarInt>>::write(&p.color, w);
            w.i8(p.options);
        }
        if matches!(v.method, 0 | 3 | 4) {
            <List<Str>>::write(&v.players, w);
        }
    }
}
