//! Arya's effects (the CharFx hooks and the knife's projectile rule): the
//! aryaKnife event's dash trail, the stuck knife's emitters, AryaDisguiseShock's and AryaKillPie's components and the
//! buff effects of Buff_Sharpened, Buff_TurnEnemyAwayOnHit and Buff_PieHeal. The knife's SetVFXRotation is mvs-view's rule
//! (chars/arya.rs knife_rot: actor_rot reads it for both crates); the pie's mesh is mvs-view's.
//! The pie's PopText ("Arya's Pie", UI_Functions.PopText on the pie actor @2156-2659) is the HUD's (mvs-ui hud.rs
//! pie_pops), from the same actor.

use std::f64::consts::{FRAC_PI_2, PI};

use indexmap::IndexMap;
use mvs_data::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt};
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::arya::knife::Knife;
use mvs_sim::events::{Event, KnifeKind};
use mvs_sim::fighter::common::actor_def;
use mvs_sim::fighter::facing;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, V3, compose_rows, rotation_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{D2R, att, warn_once, xyz};
use crate::fx::{Follow, Fx, snap_scale};

/// What Arya's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Arya {
    /// The stuck knives' emitters by the projectiles' sequence numbers.
    stuck: IndexMap<i32, Stuck>,
    /// The shocks and the pies by the actors' sequence numbers.
    shocks: IndexMap<i32, ShockFx>,
    pies: IndexMap<i32, Option<SystemId>>,
    /// Per BUFF_FX entry, per fighter: the two hands' components while the buff is on.
    hands: [Vec<Option<Vec<SystemId>>>; 2],
    /// The Buff_PieHeal entries already seen, by fighter id and serial.
    healed: Vec<(i32, u32)>,
}

const DASH_TRAIL: &str = "PS_AryaDashAllyTrail02";

impl CharFxHooks for Arya {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_actors(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_buffs(fx, w);
        self.pie_heal(fx, w);
    }

    /// MarkTeleportProjectile's TeleportVisuals @10024 (ActivateObject @5480): SpawnEmitterAtLocation(
    /// PS_AryaDashAllyTrail02, the instigator's CapsuleComponent.GetSocketLocation(VFX_Root), rotation 0, scale 1)
    /// @10208 with the instigator's Team @10441. A capsule has no socket: the component's location (the event's). The
    /// stick and the ground kinds are drawn from the knife's state (its rule).
    fn event(&mut self, fx: &mut Fx, _w: &World, e: &Event) -> bool {
        match e {
            Event::AryaKnife { fighter, kind, x, y, .. } => {
                if matches!(kind, KnifeKind::Dash) {
                    let team = fx.team(*fighter);
                    fx.player.spawn(
                        DASH_TRAIL,
                        [0.0, f64::from(-x), f64::from(*y)],
                        [0.0; 3],
                        [1.0; 3],
                        &[("Team", team)],
                    );
                }
                true
            }
            _ => false,
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

impl Arya {
    fn clear(&mut self, fx: &mut Fx) {
        for s in self.stuck.values() {
            s.kill(fx);
        }
        self.stuck.clear();
        for v in self.shocks.values() {
            v.kill(fx);
        }
        self.shocks.clear();
        for h in self.pies.values().flatten() {
            fx.player.kill(*h);
        }
        self.pies.clear();
        for per in &mut self.hands {
            for hs in per.drain(..).flatten() {
                for h in hs {
                    fx.player.kill(h);
                }
            }
        }
    }
}

fn me(c: &mut CharFx) -> &mut Arya {
    match c {
        CharFx::Arya(s) => s,
        _ => unreachable!("an Arya rule entered by another module"),
    }
}

// ---- the dagger ---------------------------------------------------------------------------------------------------

/// MarkTeleportProjectile (a projectile of World.projectiles):
///   StaticMesh has no mesh asset. The knife in flight is the mesh emitter of the inherited ParticleSystem component
///   (PS_AryaSPF_Knife_Projectile, under StaticMesh under the Capsule, no relative transform), with Team by
///   InitialVisuals @12104: the shared projectile effect's, under the actor's rotation (bRotationFollowsVelocity).
///   "Spawn Attach Point Visuals" @8532 (on a fighter and in the stage alike): ParticleSystem.K2_DestroyComponent;
///   SpawnEmitterAttached(PS_AryaSPF_Knife_Debuff, the root, None, (100, 'Facing Left At Throw' ? 15 : -15, -20),
///   rotation 0, scale 0.9, SnapToTarget, auto destroy) @8882 (the location is an offset in the root's space, the
///   world scale is 0.9); Team (the instigator's) @9233; Delay(0) @9316, then the emitter's K2_SetWorldRotation(
///   MakeRotator(0, 70, left at throw ? 90 : -90)) @2531-2578: Roll 0, Pitch 70, Yaw 90 / -90. The emitter is the
///   knife seen for the rest of the actor's life and goes with it.
///   The function runs again for a knife that lay in the stage and is taken by a fighter (HitAlly @11176 -> @11749,
///   HitEnemyPlayer @10594: Knife.visuals): a second emitter at the same offset and rotation, the first lives on.
///   On a fighter the actor is then attached to it (K2_AttachToActor, SnapToTarget location, KeepWorld rotation
///   @10609 / @11190): its place is the fighter's actor location and it turns with the fighter's actor.
///   The end: the parent's DeathVFX (PS_BatmanPickup at scale 2) and 'Ak Death Location' are the sim's
///   projectileDeath event and sound. TetherBreakWarning @937 has no reader in her classes: nothing is drawn.
// Deviation: the destroyed component is drawn as a hidden one (ProjRule.shown: its particles are gone at once either
//   way, and a rollback that takes the stick back shows it again).
// Deviation: the Delay(0) before the emitter's world rotation is not drawn (one tick at rotation 0 under the root).
// Unverified: the root's rotation at the stick is the actor's of that sim tick (Knife.root_pitch / root_yaw: pitch of
//   its last velocity, yaw by its side); a fighter's actor is taken to turn by 180 degrees of yaw with its facing,
//   which carries the knife's offset and rotation round with it.
const KNIFE: &str = "MarkTeleportProjectile_C";
const DEBUFF: &str = "PS_AryaSPF_Knife_Debuff";
const DEBUFF_SCALE: f64 = 0.9;
const REL: V3 = [100.0, 0.0, -20.0];

/// The stuck knife's record: drawn from the knife's sim state alone (root_pitch / root_yaw / face_at, written at the
/// sim tick of the stick), so a rollback over the stick or a fighter that turned before the first draw changes
/// nothing.
#[derive(Debug)]
struct Stuck {
    h: Option<SystemId>,
    /// The second run's emitter (Knife.visuals 2: a knife taken off the stage by a fighter), on the same root with
    /// the same offset and world rotation as the first, which lives on.
    h2: Option<SystemId>,
    /// The emitter's offset from the root, in world axes at the stick.
    off: V3,
    /// What `off` was made from (a rollback can stick the same knife another way).
    pitch: f64,
    yaw: i32,
    /// 'Facing Left At Throw'.
    left: bool,
}

impl Stuck {
    fn kill(&self, fx: &mut Fx) {
        for h in [self.h, self.h2].into_iter().flatten() {
            fx.player.kill(h);
        }
    }
}

fn knife(pr: &Projectile) -> Option<&Knife> {
    match &pr.ext {
        ProjectileExt::Knife(k) => Some(k),
        _ => None,
    }
}

fn on_something(k: &Knife) -> bool {
    k.stuck || k.ground
}

fn knife_shown(_w: &World, pr: &Projectile) -> bool {
    knife(pr).is_none_or(|k| !on_something(k))
}

/// The emitters' place: at the root plus the offset, (the fighter it is on turned round since the attach) the
/// offset and the rotation gone round with it.
fn stuck_at(w: &World, k: &Knife, s: &Stuck, x: f64, y: f64) -> mvs_vfx::player::Attachment {
    let turned = k.target.is_some_and(|t| k.face_at != 0.0 && facing(w, t) != k.face_at);
    let d = if turned { -1.0 } else { 1.0 };
    // the emitter's world rotation @2531: thrown facing left, right; each with its parent turned round
    let mut rot = rotation_rows(70.0 * D2R, if s.left { FRAC_PI_2 } else { -FRAC_PI_2 }, 0.0);
    if turned {
        rot = compose_rows(&rot, &rotation_rows(0.0, PI, 0.0));
    }
    att([d * s.off[0], -x + d * s.off[1], y + s.off[2]], &rot, Some([DEBUFF_SCALE; 3]))
}

fn knife_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let me = me(c);
    let Some(k) = w.actors.projectiles.get(st.id).and_then(knife) else { return };
    if !on_something(k) {
        if let Some(s) = me.stuck.shift_remove(&st.seq) {
            s.kill(fx);
        }
        return;
    }
    let s = me.stuck.entry(st.seq).or_insert(Stuck {
        h: None,
        h2: None,
        off: [0.0; 3],
        pitch: f64::NAN,
        yaw: 0,
        left: false,
    });
    let left = k.facing_left_at_throw;
    if s.pitch != f64::from(k.root_pitch).to_radians() || s.yaw != k.root_yaw || s.left != left {
        s.pitch = f64::from(k.root_pitch).to_radians();
        s.yaw = k.root_yaw;
        s.left = left;
        let a = rotation_rows(f64::from(k.root_pitch).to_radians(), f64::from(k.root_yaw) * FRAC_PI_2, 0.0);
        let rel = [REL[0], if left { 15.0 } else { -15.0 }, REL[2]];
        s.off = std::array::from_fn(|c| rel[0] * a[c] + rel[1] * a[3 + c] + rel[2] * a[6 + c]);
    }
    let at = stuck_at(w, k, s, st.x, st.y);
    for h in [s.h, s.h2].into_iter().flatten() {
        fx.player.attach(h, &at);
    }
    let two = k.visuals > 1;
    if s.h.is_some() && two == s.h2.is_some() {
        return;
    }
    let Some(owner) = w.actors.projectiles.get(st.id).and_then(|p| p.owner) else { return };
    let team = fx.team(w.fighter(owner).id);
    if s.h.is_none() {
        s.h = fx.player.spawn_attached(DEBUFF, &at, 1.0, &[("Team", team)]);
        if s.h.is_none() {
            return warn_once(format!("Arya: {DEBUFF} is not loaded"));
        }
    }
    if two {
        if s.h2.is_none() {
            s.h2 = fx.player.spawn_attached(DEBUFF, &at, 1.0, &[("Team", team)]);
        }
    } else if let Some(h) = s.h2.take() {
        // (a rollback behind the second run)
        fx.player.kill(h);
    }
}

fn knife_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(s) = me(c).stuck.shift_remove(&st.seq) {
        s.kill(fx);
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] =
    &[(KNIFE, ProjRule { shown: Some(knife_shown), tick: Some(knife_tick), end: Some(knife_end), ..ProjRule::NONE })];

// ---- the shock and the pie ----------------------------------------------------------------------------------------

/// AryaDisguiseShock (an actor of World.actors, attached to its owner): PS_Arya_FearWarning (its own component, auto
/// activated) from the spawn, world scale (1, 1, 1) @939; ParticleSystem (PS_Arya_Fear, bAutoActivate false) from
/// MulticastActivate @1049 (DisguiseShock.activated). Team (the instigator's) on both @556 / @614. Both end with the
/// actor. 'Windup Visual' is hidden in game by its template and again @30: never drawn.
// Deviation: both components are drawn without a rotation (the actor's is its owner's by the attachment; the systems
//   are sprites and domes).
const SHOCK: &str = "AryaDisguiseShock_C";

#[derive(Debug)]
struct ShockFx {
    /// The actor's place and the two components' world scales: ParticleSystem under the Capsule, PS_Arya_FearWarning
    /// at world scale 1 of BeginPlay.
    loc: V3,
    scale: V3,
    warn_scale: V3,
    warning: Option<SystemId>,
    fear: Option<SystemId>,
    fear_name: Option<String>,
}

impl ShockFx {
    fn kill(&self, fx: &mut Fx) {
        for h in [self.warning, self.fear].into_iter().flatten() {
            fx.player.kill(h);
        }
    }
}

/// AryaKillPie (an actor of World.actors): VisualIndicator @1679 (BeginPlay @1031): SpawnEmitterAttached(PS_Arya_Pie,
/// Capsule, None, 0, 0, scale 1, KeepRelativeOffset, auto destroy); with the owner's SkinManager valid Team (the
/// owner's) @2098. No effect at its end (the eater's Buff_PieHeal has its own).
// Unverified: the actor's rotation is taken as zero (Comp_SpawnArea's spawn transform is not read for it).
const PIE: &str = "AryaKillPie_C";
const PIE_FX: &str = "PS_Arya_Pie";

impl Arya {
    fn place_actors(&mut self, fx: &mut Fx, w: &World) {
        if self.shocks.is_empty() && self.pies.is_empty() && w.actors.actors.iter().next().is_none() {
            return;
        }
        let mut seen = Vec::new();
        for (_, a) in w.actors.actors.iter() {
            if a.dead {
                continue;
            }
            let comps = || actor_def(w, ActorRef::Fighter(a.owner), a.cls).map(|d| &d.components);
            let comp = |n: &str| comps().and_then(|c| c.get(n)).and_then(Value::as_obj);
            match &a.state {
                WorldActorState::DisguiseShock(d) if w.names.str(a.cls) == SHOCK => {
                    seen.push(a.seq);
                    let team = fx.team(w.fighter(a.owner).id);
                    let loc = [0.0, -d.x, d.y];
                    let v = self.shocks.entry(a.seq).or_insert_with(|| {
                        let scale = xyz(comp("Capsule").and_then(|c| c.get("RelativeScale3D")), 1.0);
                        let t = comp("PS_Arya_FearWarning").and_then(|c| c.get("Template")).and_then(Value::as_str);
                        let warning = t.and_then(|t| {
                            fx.player.spawn_attached(
                                t,
                                &att(loc.map(f64::from), &IDENTITY, Some([1.0; 3])),
                                1.0,
                                &[("Team", team)],
                            )
                        });
                        if warning.is_none() {
                            warn_once(
                                "Arya: the shock's PS_Arya_FearWarning is not drawn (its system is not loaded)".into(),
                            );
                        }
                        let fear_name = comp("ParticleSystem")
                            .and_then(|c| c.get("Template"))
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        ShockFx { loc: loc.map(f64::from), scale, warn_scale: [1.0; 3], warning, fear: None, fear_name }
                    });
                    v.loc = loc.map(f64::from);
                    // SetActorScale3D (the sim's size scale) sets the root's world scale whole; PS_Arya_FearWarning's
                    // world scale 1 of BeginPlay was a relative scale of 1 / the root's then, which now rides the new
                    // one.
                    if let Some(k) = a.size_scale
                        && v.scale[0] != f64::from(k)
                    {
                        for c in 0..3 {
                            v.warn_scale[c] = f64::from(k) / (v.scale[c] / v.warn_scale[c]);
                            v.scale[c] = f64::from(k);
                        }
                    }
                    if let Some(h) = v.warning {
                        fx.player.attach(h, &att(v.loc, &IDENTITY, Some(v.warn_scale)));
                    }
                    if let Some(h) = v.fear {
                        fx.player.attach(h, &att(v.loc, &IDENTITY, Some(v.scale)));
                    }
                    if d.activated == v.fear.is_some() {
                        continue;
                    }
                    if !d.activated {
                        // (a rollback behind ActivateObject)
                        if let Some(h) = v.fear.take() {
                            fx.player.kill(h);
                        }
                    } else {
                        let at = att(v.loc, &IDENTITY, Some(v.scale));
                        v.fear = v
                            .fear_name
                            .as_deref()
                            .and_then(|t| fx.player.spawn_attached(t, &at, 1.0, &[("Team", team)]));
                        if v.fear.is_none() {
                            warn_once(
                                "Arya: the shock's ParticleSystem is not drawn (its system is not loaded)".into(),
                            );
                        }
                    }
                }
                WorldActorState::KillPie(d) if w.names.str(a.cls) == PIE => {
                    seen.push(a.seq);
                    let root = xyz(comp("Capsule").and_then(|c| c.get("RelativeScale3D")), 1.0);
                    let at = att([0.0, f64::from(-d.x), f64::from(d.y)], &IDENTITY, Some(root));
                    match self.pies.get(&a.seq) {
                        Some(Some(h)) => fx.player.attach(*h, &at),
                        Some(None) => {}
                        None => {
                            let team = fx.team(w.fighter(a.owner).id);
                            let h = fx.player.spawn_attached(PIE_FX, &at, 1.0, &[("Team", team)]);
                            if h.is_none() {
                                warn_once(format!("Arya: {PIE_FX} is not loaded"));
                            }
                            self.pies.insert(a.seq, h);
                        }
                    }
                }
                _ => {}
            }
        }
        // K2_DestroyActor takes the actor's components with it.
        self.shocks.retain(|seq, v| {
            let keep = seen.contains(seq);
            if !keep {
                v.kill(fx);
            }
            keep
        });
        self.pies.retain(|seq, h| {
            let keep = seen.contains(seq);
            if !keep && let Some(h) = h {
                fx.player.kill(*h);
            }
            keep
        });
    }
}

// ---- the two buff effects -----------------------------------------------------------------------------------------

/// Buff_Sharpened @844 / @1126: with a Comp_FighterVisibilityManager on the owner, SpawnEmitterAttached(
/// PS_SharpenBuff, the owner's Mesh, L_Hand and R_Hand, 0, 0, scale 0.75, KeepRelativeOffset, auto destroy), each
/// handed to the manager's AddVFX; no Team. Buff_TurnEnemyAwayOnHit @1786 / @2068: the same with PS_BuffTurnAround at
/// 1.25 and the owner's Team @2599 / @2657. Both destroy the components at ReceiveDestroyed (@1757, @525).
/// The sim has no event for a buff's BeginPlay: seen here as the buff being on a fighter (as LeBron's ball buff).
// Deviation: a spawn that replaces a live buff (CanOverride) makes new components in the build and is not seen here.
struct BuffFx {
    cls: &'static str,
    ps: &'static str,
    scale: f64,
    team: bool,
}

const BUFF_FX: [BuffFx; 2] = [
    BuffFx { cls: "Buff_Sharpened_C", ps: "PS_SharpenBuff", scale: 0.75, team: false },
    BuffFx { cls: "Buff_TurnEnemyAwayOnHit_C", ps: "PS_BuffTurnAround", scale: 1.25, team: true },
];
const HANDS: [&str; 2] = ["L_Hand", "R_Hand"];

/// Buff_PieHeal (the pie's eater; ported from the decode) ReceiveBeginPlay @619 -> @30: on
/// authority HealVFXMulticast @60 -> @634: not on a dedicated server, SpawnEmitterAttached(PS_Heal, the owner's root
/// component, None, (0, 0, -150), 0, (1.3, 1.3, 1.3), SnapToTarget, auto destroy) @756: the capsule's place and yaw,
/// the offset under the actor scale, the relative scale 1.3 over the capsule's world scale at the spawn
/// (SnapToTarget). No Team, no visibility tracking; the component is the owner's, so it plays out after the buff's
/// Delay(2.0) ends the actor.
// Unverified: the sim has no event for the buff's BeginPlay: a Buff_PieHeal entry is seen here at its first sight
// after the step; a buff replaced in one tick by another of the class (CanOverride) is a new serial and draws again.
const PIE_HEAL: &str = "Buff_PieHeal_C";
const PIE_HEAL_FX: &str = "PS_Heal";
const PIE_HEAL_AT: V3 = [0.0, 0.0, -150.0];
const PIE_HEAL_SCALE: f64 = 1.3;

impl Arya {
    fn pie_heal(&mut self, fx: &mut Fx, w: &World) {
        let Some(cls) = w.names.get(PIE_HEAL) else { return };
        let ids = w.pawns.as_ref().unwrap_or(&w.players);
        // forget the entries gone (no allocation: the list is kept and only shrinks here)
        self.healed.retain(|&(id, serial)| {
            ids.iter().any(|&f| {
                let fi = w.fighter(f);
                fi.id == id && fi.buffs.buffs.iter().any(|b| b.cls == cls && b.serial == serial)
            })
        });
        for &f in ids {
            let fi = w.fighter(f);
            for b in fi.buffs.buffs.iter().filter(|b| b.cls == cls) {
                let key = (fi.id, b.serial);
                if self.healed.contains(&key) {
                    continue;
                }
                self.healed.push(key);
                let at = Follow::CapsuleRel { id: fi.id, rel: PIE_HEAL_AT, scale: snap_scale(w, f, PIE_HEAL_SCALE) };
                if fx.spawn_follow(w, PIE_HEAL_FX, at, 1.0, &[]).is_none() {
                    warn_once(format!("Arya: {PIE_HEAL_FX} is not loaded"));
                }
            }
        }
    }

    fn place_buffs(&mut self, fx: &mut Fx, w: &World) {
        let ids = w.all_ids();
        for (b, d) in BUFF_FX.iter().enumerate() {
            let per = &mut self.hands[b];
            if per.len() < ids.len() {
                per.resize_with(ids.len(), || None);
            }
            for (i, &f) in ids.iter().enumerate() {
                let fi = w.fighter(f);
                let dead = fi.life.dead != 0.0;
                let has = !dead && fi.buffs.buffs.iter().any(|x| x.frames > 0.0 && w.names.str(x.cls) == d.cls);
                if !has {
                    for h in per[i].take().into_iter().flatten() {
                        fx.player.kill(h);
                    }
                    continue;
                }
                let id = fi.id;
                let hs = per[i].get_or_insert_with(|| {
                    let team = fx.team(id);
                    let params: &[(&str, f64)] = if d.team { &[("Team", team)] } else { &[] };
                    let mut hs = Vec::new();
                    for socket in HANDS {
                        let fo = fx.socket_follow(w, id, Some(socket), [0.0; 3], &IDENTITY, [d.scale; 3]);
                        hs.extend(fx.attach_to_mesh(w, id, d.ps, socket, fo, params));
                    }
                    if hs.is_empty() {
                        warn_once(format!("Arya: {} is not loaded", d.ps));
                    }
                    hs
                });
                // Comp_FighterVisibilityManager.AddVFX: hidden with the pawn (meshHidden: dead or invisible; the
                // smoke's occlusion is not ported, as the view's)
                let hidden = dead || fi.vis.invisible;
                for h in hs.iter() {
                    fx.player.set_visible(*h, !hidden);
                }
            }
        }
    }
}
