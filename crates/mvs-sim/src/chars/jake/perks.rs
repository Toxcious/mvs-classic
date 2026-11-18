//! Jake's perks the house and the stretch reach: Buff_Jake_SafetyNet
//! ([`SafetyNetExt`], AddAirSuperiorityServer), Buff_AirSuperiority ([`AirExt`], the looping RefreshAirMoves),
//! Buff_Jake_BouncyHouse (a data marker JakeHasBouncyHousePerk asks for). Buff_Jake_QuickStretch is a data marker the
//! stretch reads by `buff_of` (not entered); WeaponMaster is weapons.rs's.

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::buffs::{buff, buff_mut, spawn_buff};
use crate::combat::hit::timer_ticks;
use crate::combat::{BuffClass, BuffExt, BuffRef};
use crate::events::Event;
use crate::fighter::common::{buff_def, warn_text};
use crate::fighter::moves::reset_jump_counts;
use crate::fighter::{FighterId, actor_y};
use crate::name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::num_or;
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// Buff_Jake_SafetyNet's class.
pub const SAFETY_NET: &str = "Buff_Jake_SafetyNet_C";

/// Buff_AirSuperiority's class.
pub const AIR_SUPERIORITY: &str = "Buff_AirSuperiority_C";

/// Buff_Jake_QuickStretch's class (a data marker: no entry).
pub const QUICK_STRETCH: &str = "Buff_Jake_QuickStretch_C";

/// Buff_Jake_BouncyHouse's class.
pub const BOUNCY_HOUSE: &str = "Buff_Jake_BouncyHouse_C";

/// An entry of SafetyNet's Allies map: the ally's fighter and the game time it was last given the buff.
#[derive(State, Copy, PartialEq, Debug)]
pub struct AllyStamp {
    pub id: FighterId,
    pub t: Real,
}

/// Buff_Jake_SafetyNet_C's extension (`SafetyNetState`: its Allies map).
#[derive(State, Debug, Default)]
pub struct SafetyNetExt {
    pub allies: Vec<AllyStamp>,
}

/// Buff_AirSuperiority_C's extension (`AirState`: RefreshTImer's next fire, the fighter tick).
#[derive(State, Debug, Default)]
pub struct AirExt {
    pub at: Real,
}

/// `addAirSuperiorityServer` (AddAirSuperiorityServer(Ally) @10) on `owner`'s SafetyNet buff `b`. Caller: JakeHouse's
/// ApplyBounceHouseEffects (house.rs).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated test, kept for its NaN result")]
pub fn add_air_superiority_server(w: &mut World, owner: FighterId, b: BuffRef, ally: FighterId) {
    let now = w.fighter(owner).clock.time;
    let def = buff_def(w, ActorRef::Fighter(owner), name!("Buff_Jake_SafetyNet_C")).cloned().unwrap_or_default();
    let cd = num_or(&def, "AirSuperiorityCooldown", 14.0);
    let dur = num_or(&def, "AirSuperiorityDuration", 8.0);
    let Some(x) = buff_mut(w, b) else { return };
    if !matches!(x.ext, BuffExt::JakeSafetyNet(_)) {
        x.ext = BuffExt::JakeSafetyNet(SafetyNetExt::default());
    }
    let BuffExt::JakeSafetyNet(s) = &mut x.ext else { return };
    match s.allies.iter_mut().find(|e| e.id == ally) {
        Some(e) => {
            if !((now - e.t) >= cd) {
                return;
            }
            e.t = now;
        }
        None => s.allies.push(AllyStamp { id: ally, t: now }),
    }
    spawn_buff(w, ally, name!("Buff_AirSuperiority_C"), dur, Some(0), Some(owner), true, Some(ally));
}

/// Buff_AirSuperiority BeginPlay @96: RefreshTImer set (AirMovesRefreshTimer, looping); set inside the spawn (the tick
/// groups): the timer counts from the next timer pass (timer_ticks).
fn air_begin(w: &mut World, f: FighterId, b: BuffRef, _by: Option<FighterId>, d: Option<Real>) {
    let _ = d;
    let def = buff_def(w, ActorRef::Fighter(f), name!("Buff_AirSuperiority_C")).cloned().unwrap_or_default();
    let at = crate::real(w.fighter(f).clock.tick) + timer_ticks(Some(num_or(&def, "AirMovesRefreshTimer", 2.0)));
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::AirSuperiority(AirExt { at });
    }
}

/// Buff_AirSuperiority's RefreshTImer -> RefreshAirMoves @493: BPI_ResetJump -> ResetJump(0) @664, PS_BigJump on the root
/// at (0, 0, -100) @827.
fn air_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let at = match buff(w, b).map(|x| &x.ext) {
        Some(BuffExt::AirSuperiority(e)) => e.at,
        _ => {
            warn_text(w, "Buff_AirSuperiority_C: its timer without its BeginPlay state");
            return false;
        }
    };
    if crate::real(w.fighter(f).clock.tick) < at {
        return false;
    }
    let def = buff_def(w, ActorRef::Fighter(f), name!("Buff_AirSuperiority_C")).cloned().unwrap_or_default();
    let step = timer_ticks(Some(num_or(&def, "AirMovesRefreshTimer", 2.0)));
    if let Some(BuffExt::AirSuperiority(e)) = buff_mut(w, b).map(|x| &mut x.ext) {
        e.at += step; // looping
    }
    reset_jump_counts(w, f);
    let (fighter, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f) - 100.0);
    w.fighter_mut(f).events.push(Event::ProjectileDeath {
        cls: name!("Buff_AirSuperiority_C"),
        fighter,
        vfx: Some(name!("PS_BigJump")),
        x,
        y,
        ox: 0.0,
        rot: [0.0; 3],
        scale: 1.0,
        tint: None,
        to: None,
    });
    false
}

/// This file's registry entries (Buff_Jake_SafetyNet_C and Buff_Jake_BouncyHouse_C with no hooks,
/// Buff_AirSuperiority_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(SAFETY_NET, BuffEntry::new(BuffClass::BuffJakeSafetyNet, BuffHooks::default()))?;
    r.buffs.enter(BOUNCY_HOUSE, BuffEntry::new(BuffClass::BuffJakeBouncyHouse, BuffHooks::default()))?;
    r.buffs.enter(
        AIR_SUPERIORITY,
        BuffEntry::new(
            BuffClass::BuffAirSuperiority,
            BuffHooks { begin: Some(air_begin), timer: Some(air_timer), ..BuffHooks::default() },
        ),
    )
}
