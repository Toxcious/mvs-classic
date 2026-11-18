//! The haste visuals of Comp_State_Manager (ported from the decode): PS_Buff_Haste_Speed on
//! a fighter whose speed is over its default speed. A shared class's effects, made for every match after the
//! characters'.
//!
//! UpdatePlayerStatus @5: CalculateBuffValues gives CharacterSpeedModifier, DefaultSpeed, NeedsHastsVisuals (any
//! buff of the list without NoHasteVisuals, @10949-11014, whatever its SpeedModType) and the override; a new
//! DefaultSpeed is written by SetDefaultSpeed @270 (DefaultMovementSpeed, the sim's default speed fold), then
//! ApplyMovementSpeedBuffs @294.
//! ApplyMovementSpeedBuffs: only when the modifier or UsingSpeedOverride changed @0-114 (CharacterSpeedModifier =
//! SelectFloat(1, the modifier, UsingSpeedOverride) @297), on authority @896: with NeedsHaste
//! SpeedBuffVisuals(CharacterSpeedModifier) @924, else when CharacterSpeedModifier == DefaultMovementSpeed
//! SpeedBuffVisuals(DefaultMovementSpeed) @1004.
//! SpeedBuffVisuals -> ubergraph @2193: LastVisualSpeed = HandleSpeedBuffVisuals(the speed, DefaultMovementSpeed,
//! LastVisualSpeed). HandleSpeedBuffVisuals (not on a dedicated server), new N, default D, last L:
//! - N == D: CurrentSpeedVFX destroyed (and dropped from the visibility tracking) @116-304;
//! - N > D: L == D spawns @473; L < D destroys and spawns @1495-1716; L > D keeps it;
//! - N < D: L > D destroys @1103-1291; else nothing;
//! - the answer is N in each of these (@337).
//!
//! The spawn @681: SpawnEmitterAttached(PS_Buff_Haste_Speed, the owner's root component, Root, 0, 0, (1.4, 1.4, 1.4)
//! x GetActiveMeshVFXScale, KeepRelativeOffset, auto destroy), no Team, BPI_AddVFXToVisibilityTracking @891.
//! GetActiveMeshVFXScale is 1 for every fighter of the port. The root component is the capsule, which has no socket
//! Root: the component's own place, under the actor's scale.
//! CDO: CharacterSpeedModifier, LastVisualSpeed and DefaultMovementSpeed 1, UsingSpeedOverride false.
// Unverified: the sim has no event for UpdatePlayerStatus: the values are read once per tick after it, so two updates
// in one tick are seen as their last; the gate compares the stored CharacterSpeedModifier with the new one where the
// build compares it with the new MovementSpeedModifier (they differ only under a speed override, where the sim keeps
// no MovementSpeedModifier).
// Deviation: a fighter is hidden here when it is dead or invisible (electric.rs's rule); the smoke's occlusion of
// the viewer is not ported.

use std::collections::HashMap;

use mvs_sim::Real;
use mvs_sim::World;
use mvs_sim::combat::buffs::{BuffRef, inst_view};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::moves::default_movement_speed;
use mvs_vfx::SystemId;

use crate::fx::attach::warn_once;
use crate::fx::chars::{CharFxHooks, ProjRule};
use crate::fx::{Follow, Fx};

const HASTE: &str = "PS_Buff_Haste_Speed";
const HASTE_SCALE: f64 = 1.4;

/// One fighter's Comp_State_Manager values the visuals read.
#[derive(Debug)]
struct Haste {
    /// CharacterSpeedModifier and UsingSpeedOverride, as the last update left them.
    speed: Real,
    over: bool,
    /// LastVisualSpeed.
    last: Real,
    /// CurrentSpeedVFX.
    h: Option<SystemId>,
    /// Components CurrentSpeedVFX no longer names: the spawn at L == D (@473) does not destroy a live one, which a
    /// default speed raised under a running haste leaves (it plays on, still tracked for visibility).
    stray: Vec<SystemId>,
}

impl Default for Haste {
    fn default() -> Haste {
        Haste { speed: 1.0, over: false, last: 1.0, h: None, stray: Vec::new() }
    }
}

/// What its effects keep between steps, by fighter id.
#[derive(Debug, Default)]
pub(crate) struct Speed {
    per: HashMap<i32, Haste>,
}

impl Speed {
    fn clear(&mut self, fx: &mut Fx) {
        for (_, r) in self.per.drain() {
            for h in r.h.into_iter().chain(r.stray) {
                fx.player.kill(h);
            }
        }
    }
}

/// CalculateBuffValues' NeedsHastsVisuals: a live buff without NoHasteVisuals.
fn needs_haste(w: &World, f: FighterId) -> bool {
    w.fighter(f)
        .buffs
        .buffs
        .iter()
        .filter(|b| b.frames > 0.0)
        .any(|b| !inst_view(w, BuffRef { fighter: f, serial: b.serial }).flag("NoHasteVisuals"))
}

/// HandleSpeedBuffVisuals: the component made or destroyed; the new LastVisualSpeed.
fn handle(fx: &mut Fx, w: &World, id: i32, r: &mut Haste, n: Real, d: Real) {
    let spawn = |fx: &mut Fx, r: &mut Haste| {
        let f = Follow::CapsuleRel { id, rel: [0.0; 3], scale: [HASTE_SCALE; 3] };
        let h = fx.spawn_follow(w, HASTE, f, 1.0, &[]);
        if h.is_none() {
            warn_once(format!("haste: {HASTE} is not loaded"));
        }
        r.stray.extend(std::mem::replace(&mut r.h, h));
    };
    let destroy = |fx: &mut Fx, r: &mut Haste| {
        if let Some(h) = r.h.take() {
            fx.player.kill(h);
        }
    };
    // (a NaN on any path returns with the output at its default, 0)
    let l = r.last;
    if n == d {
        destroy(fx, r);
    } else if n > d && l == d {
        spawn(fx, r);
    } else if n > d && l < d {
        destroy(fx, r);
        spawn(fx, r);
    } else if n < d && l > d {
        destroy(fx, r);
    } else if n.is_nan() || d.is_nan() || l.is_nan() {
        r.last = 0.0;
        return;
    }
    r.last = n;
}

impl CharFxHooks for Speed {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        for &f in w.pawns.as_ref().unwrap_or(&w.players) {
            let fi = w.fighter(f);
            let id = fi.id;
            let r = self.per.entry(id).or_default();
            let speed = fi.buffs.speed_mod;
            let over = fi.moves.speed_override.is_some();
            if speed != r.speed || over != r.over {
                r.speed = speed;
                r.over = over;
                let d = default_movement_speed(w, f);
                if needs_haste(w, f) {
                    handle(fx, w, id, r, speed, d);
                } else if speed == d {
                    handle(fx, w, id, r, d, d);
                }
            }
            let hidden = fi.life.dead != 0.0 || fi.vis.invisible;
            r.stray.retain(|&h| fx.player.alive(h));
            for &h in r.h.iter().chain(&r.stray) {
                fx.player.set_visible(h, !hidden);
            }
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
