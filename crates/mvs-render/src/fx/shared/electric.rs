//! Buff_Damage_Electric and the ally gauntlets: a shared class's effects, made for
//! every match after the characters'. Buff_Damage_Electric is the sim's combat/electric.rs (its ElectricFx events);
//! the ally gauntlets are combat/gauntlet.rs (Buff_SupermanAllyIceGauntlet and its data children
//! Buff_AllyElectricGauntlets, Buff_FireGauntlets and Garnet's Buff_GarnetElecInsideBubble,
//! Buff_GarnetElecOutsideBubble, Buff_GarnetFullPowerGauntlets).
//!
//! Buff_Damage_Electric:
//! - ElectricVFX @1865-2402 (loop on): Particle = SpawnEmitterAttached(PS_Garnet_Debuff_Electricity, the owner's
//!   active mesh, Root, location 0, rotation 0, scale GetActiveMeshVFXScale, KeepRelativeOffset, auto destroy) and its
//!   Team = SelectFloat(0.6, the owner's BPI_GetColorValue, IsLocalOnline) @2402; ReceiveEndPlay @1831 (loop off):
//!   Particle.K2_DestroyComponent.
//! - TriggerVFX, the owner among the chained fighters (burst): PS_Debuff_ElectricityBurst the same way @3047, Team
//!   @3344; another fighter (beam): SpawnEmitterAttached(PS_Garnet_ElecBeam, that fighter's root component, None, 0,
//!   0, scale 1, KeepRelativeOffset, auto destroy) @3504, SetBeamTargetPoint(0, the owner's actor location, 0) @3670
//!   and SetColorParameter(Color, MakeColor(1, 1, SelectFloat(0.6, the owner's colour value, IsLocalOnline), 1))
//!   @3982 (the FColor round trip: color_param). No Team on the beam. The target point is set once: the beam's far
//!   end stays where the owner stood at the Trigger, its near end follows the fighter it hangs on.
//! - Neither emitter is added to the visibility tracking.
//!
//! The ally gauntlets (Buff_SupermanAllyIceGauntlet's graph; the class defaults per class: GauntletVFX, VFXScaling,
//! HitVFX, HitVFXScale, TimedOutVFX):
//! - ApplyGauntlet_FX (status applied @1875; OnMeshChanged @1845 after a RemoveGauntlet_FX(false)): the components of
//!   ParticleSystems destroyed, then SpawnEmitterAttached(GauntletVFX, the owner's active mesh, R_Hand @864 and L_Hand
//!   @1520, 0, 0, VFXScaling x GetActiveMeshVFXScale, KeepRelativeOffset, auto destroy), Team = SelectFloat(0.6,
//!   GetColorIndex, IsLocalOnlinePlayer) of the owner, both kept and tracked for visibility.
//! - RemoveGauntlet_FX(PlayRemove) (status removed @1900 with true; ReceiveEndPlay @935 with false): the kept
//!   components destroyed; with PlayRemove, SFX End (the sim's) and TimedOutVFX on L_Hand @992 and R_Hand @1539 at
//!   (0.2, 0.2, 0.2) x GetActiveMeshVFXScale with the same Team, kept by nothing.
//! - HitVisuals @955-1602 (the do-once hit: the GauntletHit event): SpawnEmitterAttached(HitVFX, the hit fighter's
//!   active mesh, VFX_Root, (0, 0, its CapsuleComponent's CapsuleHalfHeight), 0, HitVFXScale x its
//!   GetActiveMeshVFXScale, KeepRelativeOffset, auto destroy), Team of the gauntlet's owner (GetColorIndex).
//!
//! GetActiveMeshVFXScale is 1 for every fighter of the port (no data sets it; as PS_Heal's and the fire's). The sim
//! has no event for the gauntlet's status: it is seen here as the buff being on the fighter. A buff that ends sends
//! the status update (removed) from the parent's EndPlay, then its own ReceiveEndPlay destroys what is left: one
//! TimedOutVFX pair. Each buff actor keeps its own components: two classes on one fighter (Garnet's OutsideBubble
//! beside her FullPowerGauntlets, which breaks only the electric gauntlet and InsideBubble) draw two pairs.
//!
//! The charged hands of Static Electricity (Buff_ProjectileApplyBuffAfterGrounded, the general perk
//! ElectricProjectile; ported from the decode, offsets of its ubergraph):
//! - CreateBuffVisuals @611 (the tick that charges it @537; not on a dedicated server, the owner a BaseFighter):
//!   SpawnVFX @774: SpawnEmitterAttached(GauntletVFX PS_CreatureCrystal_Buff, the owner's active mesh, L_Hand, 0, 0,
//!   VFXScaling (1.125) x GetActiveMeshVFXScale, KeepRelativeOffset, auto destroy) @180, AddUnique to ParticleSystems,
//!   BPI_AddVFXToVisibilityTracking @340, Team = SelectFloat(0.6, GetColorIndex, IsLocalOnlinePlayer) @598; then
//!   R_Hand the same @836-1254. Then the ActiveMeshChanged bind @797 and the equip sound (the sim's).
//! - RemoveBuffVisuals (HandleProjectileAttack @511, the charge given to a projectile) -> RemoveVFX @1473: every valid
//!   component of ParticleSystems dropped from the visibility tracking and destroyed (no remove effect).
//! - The sim keeps IsCharged on the buff's entry (PerkExt::Grounded): the hands are drawn while it is set.
// Deviation: the buff has no EndPlay of its own and a perk buff is destroyed at its owner's defeat
// (DestroyBuffWhenOwnerDefeated), so in the build a charged buff's hands outlive a ring-out and pile up with the
// next life's charge; here they are killed with the buff's entry (or its charge).
// Deviation: OnMeshChanged (@1548: RemoveVFX and SpawnVFX on the new mesh) is not modelled (one mesh per fighter).

use std::collections::{BTreeSet, HashMap};

use indexmap::IndexMap;
use mvs_data::value::Props;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::combat::buffs::BuffExt;
use mvs_sim::combat::perks::PerkExt;
use mvs_sim::events::{ElectricKind, Event};
use mvs_sim::names::Name;
use mvs_vfx::SystemId;
use mvs_vfx::distribution::color_param;
use mvs_vfx::math::{IDENTITY, V3};

use crate::fx::attach::{text, xyz};
use crate::fx::chars::{CharFxHooks, ProjRule};
use crate::fx::{Follow, Fx};

// Deviation: the beam's ends: SetBeamTargetPoint and the Target UserSet are taken as the source's rule;
// PS_Garnet_ElecBeam's InterpolationPoints 5 and its UserSet tangents are mvs-vfx's beam port's. The root
// component's place is the sim's of this tick, its scale 1.
// Deviation: OnMeshChanged (Remove(false) then Apply) is not modelled (one mesh per fighter); a gauntlet given again
// while one is on (CanRefresh) keeps its components here.
// Deviation: a fighter is hidden here when it is dead or invisible; the smoke's AddInvisibility for the viewer
// is not ported, as mvs-view's place_view.

const LOOP: &str = "PS_Garnet_Debuff_Electricity";
const BURST: &str = "PS_Debuff_ElectricityBurst";
const BEAM: &str = "PS_Garnet_ElecBeam";
const GAUNTLETS: [&str; 6] = [
    "Buff_SupermanAllyIceGauntlet_C",
    "Buff_AllyElectricGauntlets_C",
    "Buff_FireGauntlets_C",
    "Buff_GarnetElecInsideBubble_C",
    "Buff_GarnetElecOutsideBubble_C",
    "Buff_GarnetFullPowerGauntlets_C",
];
const REMOVE_SCALE: V3 = [0.2; 3];
const HANDS: [&str; 2] = ["R_Hand", "L_Hand"];

/// What its effects keep between steps, by fighter id: the electric loop, and the gauntlet classes drawn now with
/// their two hand components (in the order they were put on); the systems warned of.
#[derive(Debug, Default)]
pub(crate) struct Electric {
    loops: HashMap<i32, SystemId>,
    worn: HashMap<i32, IndexMap<Name, Vec<SystemId>>>,
    /// Static Electricity's charged hands: the buff entry's serial and its two components.
    charged: HashMap<i32, (u32, Vec<SystemId>)>,
    warned: BTreeSet<String>,
}

/// Whether fighter `f` has a live buff of class `cls`.
fn has_on(w: &World, f: mvs_sim::fighter::FighterId, cls: Name) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|b| b.cls == cls && b.frames > 0.0)
}

/// `meshHidden` (less the smoke's occlusion).
fn hidden(w: &World, f: mvs_sim::fighter::FighterId) -> bool {
    let fi = w.fighter(f);
    fi.life.dead != 0.0 || fi.vis.invisible
}

/// A gauntlet class's defaults: the match's fighters' data, else the perk tables (common.rs buff_def's order,
/// from the first fighter).
fn def_of(w: &World, cls: Name) -> Option<&Props> {
    let f = *w.all_ids().first()?;
    mvs_sim::fighter::common::buff_def(w, ActorRef::Fighter(f), cls)
}

impl Electric {
    /// The system when it is loaded, else a warning once.
    fn has(&mut self, fx: &mut Fx, ps: Option<&str>) -> bool {
        let Some(ps) = ps.filter(|s| !s.is_empty() && *s != "None") else { return false };
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: electric: {ps} is not loaded");
        }
        false
    }

    fn clear(&mut self, fx: &mut Fx) {
        for (_, h) in self.loops.drain() {
            fx.player.kill(h);
        }
        for (_, on) in self.worn.drain() {
            for h in on.into_values().flatten() {
                fx.player.kill(h);
            }
        }
        for (_, (_, hs)) in self.charged.drain() {
            for h in hs {
                fx.player.kill(h);
            }
        }
    }

    /// Static Electricity's hands on fighter `f`: made when its buff is charged, destroyed when the charge goes.
    fn charged_hands(&mut self, fx: &mut Fx, w: &World, f: mvs_sim::fighter::FighterId, hide: bool) {
        let fi = w.fighter(f);
        let id = fi.id;
        let on = fi.buffs.buffs.iter().find(|b| {
            b.class == BuffClass::BuffProjectileApplyBuffAfterGrounded
                && matches!(b.ext, BuffExt::Perk(PerkExt::Grounded { charged: true, .. }))
        });
        let serial = on.map(|b| b.serial);
        if self.charged.get(&id).map(|r| r.0) != serial
            && let Some((_, hs)) = self.charged.remove(&id)
        {
            for h in hs {
                fx.player.kill(h);
            }
        }
        let Some(b) = on else { return };
        if !self.charged.contains_key(&id) {
            let d = def_of(w, b.cls);
            let ps = d.and_then(|d| text(d, "GauntletVFX")).map(str::to_owned);
            let mut hs = Vec::new();
            if self.has(fx, ps.as_deref()) {
                let ps = ps.expect("a GauntletVFX");
                let scale = xyz(d.and_then(|d| d.get("VFXScaling")), 1.0);
                let team = team_index(fx, id);
                for hand in ["L_Hand", "R_Hand"] {
                    hs.extend(on_mesh(fx, w, id, &ps, hand, [0.0; 3], scale, team));
                }
            }
            self.charged.insert(id, (b.serial, hs));
        }
        for &h in &self.charged[&id].1 {
            fx.player.set_visible(h, !hide);
        }
    }
}

/// GetColorIndex's Team (0.6 for the local player online, as Fx::team).
fn team_index(fx: &Fx, id: i32) -> f64 {
    let t = fx.team(id);
    if t == 0.6 { 0.6 } else { f64::from(fx.color_index(id)) }
}

/// A component on fighter `id`'s mesh at `socket`.
#[allow(clippy::too_many_arguments)]
fn on_mesh(fx: &mut Fx, w: &World, id: i32, ps: &str, socket: &str, loc: V3, scale: V3, team: f64) -> Option<SystemId> {
    let f = fx.socket_follow(w, id, Some(socket), loc, &IDENTITY, scale);
    fx.attach_to_mesh(w, id, ps, socket, f, &[("Team", team)])
}

impl CharFxHooks for Electric {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        for f in w.all_ids() {
            let id = w.fighter(f).id;
            let hide = hidden(w, f);
            self.charged_hands(fx, w, f, hide);
            let mut on = self.worn.remove(&id).unwrap_or_default();
            on.retain(|&cls, hs| {
                if has_on(w, f, cls) {
                    for &h in hs.iter() {
                        fx.player.set_visible(h, !hide);
                    }
                    return true;
                }
                for &h in hs.iter() {
                    fx.player.kill(h);
                }
                // RemoveGauntlet_FX(true): TimedOutVFX on both hands
                let ps = def_of(w, cls).and_then(|d| text(d, "TimedOutVFX")).map(str::to_owned);
                if self.has(fx, ps.as_deref()) {
                    let ps = ps.expect("a TimedOutVFX");
                    let team = team_index(fx, id);
                    for hand in ["L_Hand", "R_Hand"] {
                        on_mesh(fx, w, id, &ps, hand, [0.0; 3], REMOVE_SCALE, team);
                    }
                }
                false
            });
            for cls in GAUNTLETS {
                let Some(cls) = w.names.get(cls) else { continue };
                if on.contains_key(&cls) || !has_on(w, f, cls) {
                    continue;
                }
                let d = def_of(w, cls);
                let ps = d.and_then(|d| text(d, "GauntletVFX")).map(str::to_owned);
                let mut hs = Vec::new();
                if self.has(fx, ps.as_deref()) {
                    let ps = ps.expect("a GauntletVFX");
                    let scale = xyz(d.and_then(|d| d.get("VFXScaling")), 1.0);
                    let team = team_index(fx, id);
                    hs.extend(HANDS.iter().filter_map(|hand| on_mesh(fx, w, id, &ps, hand, [0.0; 3], scale, team)));
                }
                // (BPI_AddVFXToVisibilityTracking: a component added while the fighter is hidden is hidden at once)
                if hide {
                    for &h in &hs {
                        fx.player.set_visible(h, false);
                    }
                }
                on.insert(cls, hs);
            }
            if !on.is_empty() {
                self.worn.insert(id, on);
            }
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        match e {
            Event::ElectricFx { fighter, kind, on, target, x, y } => {
                let i = *fighter;
                if w.by_id(i).is_none() {
                    return true;
                }
                match kind {
                    ElectricKind::Loop => {
                        if let Some(h) = self.loops.remove(&i) {
                            fx.player.kill(h);
                        }
                        if on.unwrap_or(false) && self.has(fx, Some(LOOP)) {
                            let team = fx.team(i);
                            if let Some(h) = on_mesh(fx, w, i, LOOP, "Root", [0.0; 3], [1.0; 3], team) {
                                self.loops.insert(i, h);
                            }
                        }
                    }
                    ElectricKind::Burst => {
                        if self.has(fx, Some(BURST)) {
                            let team = fx.team(i);
                            on_mesh(fx, w, i, BURST, "Root", [0.0; 3], [1.0; 3], team);
                        }
                    }
                    ElectricKind::Beam => {
                        let Some(t) = target.filter(|&t| w.by_id(t).is_some()) else { return true };
                        if !self.has(fx, Some(BEAM)) {
                            return true;
                        }
                        let c = color_param([1.0, 1.0, fx.team(i)]);
                        let params = [("Color.X", c[0]), ("Color.Y", c[1]), ("Color.Z", c[2])];
                        // (the target fighter's root component: CapsuleComponent, yaw -90 facing right, +90 left)
                        let follow = Follow::Root { id: t, rel: IDENTITY, up: 0.0, scale: Some([1.0; 3]) };
                        if let Some(h) = fx.spawn_follow(w, BEAM, follow, 1.0, &params) {
                            fx.player.set_beam_target_point(
                                h,
                                0,
                                [0.0, f64::from(-x.unwrap_or(0.0)), f64::from(y.unwrap_or(0.0))],
                                0,
                            );
                        }
                    }
                }
                true
            }
            Event::GauntletHit { fighter, cls, victim } => {
                let Some(v) = w.by_id(*victim) else { return true };
                let d = def_of(w, *cls);
                let ps = d.and_then(|d| text(d, "HitVFX")).map(str::to_owned);
                if !self.has(fx, ps.as_deref()) {
                    return true;
                }
                // (CapsuleComponent: the pawn's root capsule, its own CapsuleHalfHeight: unscaled, as a relative offset)
                let up = w.fighter(v).capsule.cap_half_u;
                let scale = xyz(d.and_then(|d| d.get("HitVFXScale")), 1.0);
                let team = team_index(fx, *fighter);
                on_mesh(fx, w, *victim, &ps.expect("a HitVFX"), "VFX_Root", [0.0, 0.0, f64::from(up)], scale, team);
                true
            }
            _ => false,
        }
    }
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
