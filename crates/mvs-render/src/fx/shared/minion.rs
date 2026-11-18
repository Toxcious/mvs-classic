//! The effects of a minion's own events (the slots of
//! Steven's watermelon views and Velma's police car views hand them out): what
//! a fighter's own events draw for a player, on a minion's view and with the minion's colours. Not a module of its own:
//! the modules that draw a minion's view (Steven's, Velma's police) keep one [`Minions`] each.
//!
//! A minion is a pawn of World.minions (ids from MINION_ID_BASE) with no view of the match's; its view is drawn by its
//! creator's module (mvs-view pose.rs place_minion, an ActorPose of the minion) and its body is the minion's own pawn
//! entry of `Fx` (Fx::minion_bodies). handleEvents gives every event that names a minion and is no sound to the
//! modules' minion_event (Fx::names_minion); the slot of the minion's creator takes it:
//!   notifyVfx, particleNotify, hitVfx (the minion the victim), moveFx, projectileDeath (fighter the minion, with an
//!   emitter): the shared path's rules on the minion's entry (Fx::shared_event): its skin's PlayVFX entries, its
//!   colour index, Team = the colour index (MinionFx.team: IsLocalOnlinePlayer false), its mesh's world scale.
//!   smokeTrail: PS_CharacterSmokeTrail at the minion's place (below).
//!   hit (the minion the victim) / pulseHit: the flash is the view's (mvs-view pose.rs pulse_hit).
//! Its components go with the minion (`end`): the attached ones are destroyed with the pawn.
//! Deviation: the smoke trail is BaseFighter's own component, activated and deactivated; here each activation is a
//! new system following the minion, deactivated at the event's off.
//! Deviation: a PlayVFX system's hide while the minion is invisible is taken at its spawn only.

use mvs_sim::World;
use mvs_sim::events::Event;
use mvs_sim::fighter::FighterId;
use mvs_vfx::SystemId;

use crate::fx::attach::att;
use crate::fx::{Fx, event_ids};
use mvs_vfx::math::IDENTITY;

/// One creator's minion view's effects (a minion view's slot and its MinionFx).
#[derive(Debug)]
struct Slot {
    /// The creator's id.
    owner: i32,
    /// The minion of its effects: the last seen, kept past its end for the events of that tick.
    minion: Option<i32>,
    shown: bool,
    /// The smoke trail and the minion's last place (a trail still finishing after the minion went stays there).
    trail: Option<SystemId>,
    at: [f64; 3],
}

/// The minion views of one module: every fighter of the creator's character, each one's minion of `key`.
#[derive(Debug, Default)]
pub(crate) struct Minions {
    slots: Vec<Slot>,
}

impl Minions {
    /// The slots of the match: one per fighter of `creator`, a slot made at the place that first finds its fighter (a
    /// mode's pawns come after the start).
    fn make(&mut self, w: &World, creator: &str) {
        for &f in w.pawns.as_deref().unwrap_or(&w.players) {
            let fi = w.fighter(f);
            if !fi.life.is_minion && w.names.str(fi.ch) == creator && !self.slots.iter().any(|s| s.owner == fi.id) {
                self.slots.push(Slot { owner: fi.id, minion: None, shown: false, trail: None, at: [0.0; 3] });
            }
        }
    }

    /// The live minion of `key` of creator `owner` (minionOf / carOf).
    fn live(w: &World, owner: i32, key: &str) -> Option<FighterId> {
        w.pawns.as_deref().unwrap_or(&[]).iter().copied().find(|&m| {
            let fi = w.fighter(m);
            fi.life.is_minion
                && fi.life.minion_creator.is_some_and(|c| w.fighter(c).id == owner)
                && fi.life.dead == 0.0
                && w.names.str(fi.ch) == key
        })
    }

    /// `seen`: a new minion of the slot's creator; the effects of the one before end with it. The minion's pawn entry
    /// takes the skin its PlayVFX entries are keyed by.
    fn seen(fx: &mut Fx, w: &World, s: &mut Slot, id: i32, skin: &str) {
        if s.minion.is_some_and(|m| m != id) {
            end(fx, s);
        }
        s.minion = Some(id);
        fx.minion_pawn(w, id);
        if let Some(p) = fx.pawns.get_mut(&id) {
            p.skin = skin.to_owned();
        }
    }

    /// Each presentation tick (the module's place): each slot's minion of `key` shown (`seen`, MinionFx.place) or
    /// its effects ended; the smoke trail to the minion's place.
    pub(crate) fn place(&mut self, fx: &mut Fx, w: &World, creator: &str, key: &str, skin: &str) {
        self.make(w, creator);
        for s in &mut self.slots {
            let Some(f) = Self::live(w, s.owner, key) else {
                if s.shown {
                    s.shown = false;
                    end(fx, s);
                }
                continue;
            };
            Self::seen(fx, w, s, w.fighter(f).id, skin);
            s.shown = true;
            let fi = w.fighter(f);
            s.at = [0.0, f64::from(-fi.body.x), f64::from(fi.body.y)];
            if let Some(h) = s.trail.filter(|h| fx.player.alive(*h)) {
                fx.player.attach(h, &att(s.at, &IDENTITY, Some([1.0; 3])));
            }
        }
    }

    /// `event` (eventSlot, then MinionFx.event): an event naming a minion of `key` whose creator has a slot here;
    /// true when it is drawn.
    pub(crate) fn event(&mut self, fx: &mut Fx, w: &World, e: &Event, key: &str, skin: &str) -> bool {
        // (a minion of the World, or one gone in this tick that a slot still holds)
        let is_minion = |x: i32| match w.by_id(x) {
            Some(m) => w.fighter(m).life.is_minion,
            None => self.slots.iter().any(|s| s.minion == Some(x)),
        };
        let Some(id) = event_ids(e).into_iter().flatten().find(|&x| is_minion(x)) else {
            return false;
        };
        let k = match w.by_id(id).filter(|&m| w.fighter(m).life.is_minion) {
            Some(m) => {
                let fi = w.fighter(m);
                if w.names.str(fi.ch) != key {
                    return false;
                }
                let owner = fi.life.minion_creator.map(|c| w.fighter(c).id);
                let Some(k) = self.slots.iter().position(|s| Some(s.owner) == owner) else { return false };
                Self::seen(fx, w, &mut self.slots[k], id, skin);
                k
            }
            // (gone in this tick: EarlyDestroy's / DestroyVisuals' burst comes after K2_DestroyActor)
            None => match self.slots.iter().position(|s| s.minion == Some(id)) {
                Some(k) => k,
                None => return false,
            },
        };
        minion_event(fx, w, &mut self.slots[k], e, id)
    }

    /// A Lab reset: every slot's effects end.
    pub(crate) fn reset(&mut self, fx: &mut Fx) {
        for s in &mut self.slots {
            s.shown = false;
            end(fx, s);
            s.minion = None;
        }
    }

    /// The match's end.
    pub(crate) fn end(&mut self, fx: &mut Fx) {
        for s in &mut self.slots {
            end(fx, s);
        }
        self.slots.clear();
    }
}

/// MinionFx.event: true when drawn here.
fn minion_event(fx: &mut Fx, w: &World, s: &mut Slot, e: &Event, id: i32) -> bool {
    match *e {
        Event::NotifyVfx { fighter, on, copy, poly, .. } => {
            if copy || poly {
                return false;
            }
            fx.shared_event(w, e);
            // Comp_FighterVisibilityManager.AddVFX: hidden at once while the minion is invisible
            let invisible = w.by_id(fighter).is_some_and(|f| w.fighter(f).vis.invisible);
            if on && invisible {
                let last = fx.pawns.get(&fighter).and_then(|p| p.notify.last()).and_then(|n| n.1);
                if let Some(h) = last {
                    fx.player.set_visible(h, false);
                }
            }
            true
        }
        Event::ParticleNotify { copy, poly, .. } => {
            if copy || poly {
                return false;
            }
            fx.shared_event(w, e);
            true
        }
        Event::HitVfx { victim, .. } => {
            if victim != id {
                return false;
            }
            fx.shared_event(w, e);
            true
        }
        Event::SmokeTrail { on, .. } => {
            if let Some(h) = s.trail.take() {
                fx.player.deactivate(h);
            }
            if !on {
                return true;
            }
            if let Some(f) = w.by_id(id) {
                let fi = w.fighter(f);
                s.at = [0.0, f64::from(-fi.body.x), f64::from(fi.body.y)];
            }
            let team = fx.team(id);
            let a = att(s.at, &IDENTITY, Some([1.0; 3]));
            s.trail = fx.player.spawn_attached("PS_CharacterSmokeTrail", &a, 1.0, &[("Team", team)]);
            true
        }
        // the flash: the view's
        Event::Hit { victim, .. } => victim == id,
        Event::PulseHit { fighter, .. } => fighter == id,
        Event::MoveFx { fighter, .. } => {
            if fighter != id {
                return false;
            }
            fx.shared_event(w, e);
            true
        }
        Event::ProjectileDeath { fighter, vfx, .. } => {
            if fighter != id || vfx.is_none() {
                return false;
            }
            fx.shared_event(w, e);
            true
        }
        _ => false,
    }
}

/// MinionFx.end: the components on the minion's mesh go with it (its pawn entry dropped), the trail killed.
fn end(fx: &mut Fx, s: &mut Slot) {
    if let Some(h) = s.trail.take() {
        fx.player.kill(h);
    }
    let Some(id) = s.minion else { return };
    if let Some(p) = fx.pawns.get(&id).filter(|p| p.minion.is_some()) {
        let hs: Vec<SystemId> = p.mesh_psc.iter().map(|c| c.2).collect();
        for h in hs {
            fx.player.kill(h);
        }
        fx.pawns.shift_remove(&id);
    }
}
