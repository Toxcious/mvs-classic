//! What a buff turns the fighter into or onto: a disguise, a polymorph, a carry, a fire wall
//! (`Disguise`, `Polymorph`, the carry, `FireWallComp`).

use crate::Real;
use crate::State;
use crate::events::NotifyRef;
use crate::fighter::{FighterId, MontageState};
use crate::names::Name;
use indexmap::IndexSet;

/// `ForeignPlay` / `CopyPlay`: a montage instance of a second mesh's anim instance.
#[derive(State, Debug)]
pub struct ForeignPlay {
    pub m: MontageState,
    pub stopped: bool,
    pub blend_out: bool,
    pub prev: Option<(Name, Real)>,
    /// `CopyPlay.neutral`: a Disguise's copy has it; a Polymorph's play (a `ForeignPlay` without it) is none.
    pub neutral: Option<bool>,
}

/// `Disguise`.
#[derive(State, Debug)]
pub struct Disguise {
    /// The victim whose definition, montages and shapes the second mesh carries.
    pub from: FighterId,
    /// DisguiseCharacter (EFighter), by the enum entry's name.
    pub character: Name,
    pub remaining: Real,
    pub ui_max: Real,
    pub activated: bool,
    pub neutral: bool,
    pub taunt: bool,
    pub shown: bool,
    pub scaled: bool,
    pub ground: Name,
    pub air: Name,
    pub taunt_ground: Name,
    pub taunt_air: Name,
    pub ground_cd: bool,
    pub air_cd: bool,
    pub owner_num: Real,
    pub start_num: Real,
    pub seq: Option<Name>,
    pub copy: Option<ForeignPlay>,
    /// `wait`: neutral, ground.
    pub wait: Option<(bool, bool)>,
    pub destroy_in: i32,
    pub sub_hidden: IndexSet<Name>,
    pub hurt_off: IndexSet<i32>,
    pub aim: Real,
}

/// `Polymorph`.
#[derive(State, Debug)]
pub struct Polymorph {
    pub cls: Name,
    /// The data that holds the mesh's montages, by character or items key; `None` when none is in the match.
    pub from: Option<Name>,
    pub mesh: Name,
    pub anim: Name,
    pub scale: Real,
    pub head: Real,
    pub play: Option<ForeignPlay>,
    pub damage: Real,
    pub evade_timer: i32,
    pub show_in: i32,
    pub active: bool,
    pub crouched: bool,
    pub hitstun_bound: bool,
    pub windows: Vec<NotifyRef>,
    /// `orig`: xy, z.
    pub orig: (Real, Real),
}

/// `Carried`: the fighter is carried by an item.
#[derive(State, Debug)]
pub struct Carried {
    /// The attach parent (GetAttachParentActor).
    pub item: crate::actors::ItemId,
}

/// `FireWallComp`: Comp_Pawn_FireWall's state.
#[derive(State, Debug, Default)]
pub struct FireWallComp {
    /// FireWallArray: the walls FireWallOn added, in that order, destroyed ones included until the check takes them
    /// out; by their actor ids (`seq`).
    pub walls: Vec<i32>,
    /// The frames the running Delay(0.2) has left.
    pub wait: i32,
    /// MaxFirewalls as SetMaxFireWalls set it; `None`: the class data's.
    pub max: Option<i32>,
}

/// The buff-driven shapes.
#[derive(State, Debug, Default)]
pub struct BuffShapes {
    pub disguise: Option<Disguise>,
    pub polymorph: Option<Polymorph>,
    pub is_polymorphed: bool,
    pub carried: Option<Carried>,
    pub fire_wall: Option<FireWallComp>,
}
