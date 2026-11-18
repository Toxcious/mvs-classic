//! Batman's effects: the grapple's beam, hook look and
//! impact, Buff_DelayKnockUp's bomb, arrow and explosion, the batarang's trail, bomb and pickup glow, the smoke bomb's
//! aura, impact and fog, and the held bomb's two auras. The meshes (the hook, the batarang, the smoke grenade, the held
//! bomb) and the pose are the view's (mvs_view::chars::batman). Offsets are the class's ubergraph's unless a function
//! is named.

use indexmap::IndexMap;
use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::item::ItemExt;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::batman::grapple::grapple_of;
use mvs_sim::combat::buffs::BuffExt;
use mvs_sim::events::Event;
use mvs_sim::fighter::common::actor_def;
use mvs_sim::fighter::facing;
use mvs_vfx::SystemId;
use mvs_vfx::distribution::color_param;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{att, rot_rows, text, xyz};
use crate::fx::{Follow, Fx, Scale, under};

const DEG: f64 = std::f64::consts::PI / 180.0;

/// Comp_Pawn_SkinManager.GetColorIndex's pair as SelectFloat(0.6, the index, IsLocalOnlinePlayer) (`teamIndex`).
fn team_index(fx: &Fx, id: i32) -> f64 {
    if fx.team(id) == 0.6 { 0.6 } else { f64::from(fx.color_index(id)) }
}

fn has_batman(w: &World) -> bool {
    w.all_ids().into_iter().any(|f| w.names.str(w.fighter(f).ch) == mvs_sim::chars::batman::KEY)
}

fn transpose(r: &Rows) -> Rows {
    [r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]]
}

fn flat(r: &[[f64; 3]; 3]) -> Rows {
    [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2]]
}

/// What Batman's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Batman {
    /// Per grapple (sequence number).
    grapples: IndexMap<i32, GrappleFx>,
    /// Per fighter id: Buff_DelayKnockUp's bomb and arrow, by the buff's KnockUp tick.
    bombs: IndexMap<i32, BombFx>,
    /// Per batarang (sequence number).
    batarangs: IndexMap<i32, BatarangFx>,
    /// Per smoke grenade (sequence number): its aura.
    grenades: IndexMap<i32, Option<SystemId>>,
    /// Per smoke zone (sequence number): its fog emitter.
    fogs: IndexMap<i32, FogFx>,
    /// Per Batman (id): the held bomb's auras.
    held: IndexMap<i32, Vec<Aura>>,
}

impl CharFxHooks for Batman {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        // (his batarangs, his bomb buff and his smoke are only in a match that has him)
        if !has_batman(w) {
            return;
        }
        self.place_batarangs(fx, w);
        self.place_smoke(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        if !has_batman(w) {
            return;
        }
        self.place_bombs(fx, w);
        self.place_held(fx, w);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx, false);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx, true);
    }
}

impl Batman {
    /// The clear (a Lab reset): the batarangs', bombs' and smoke's systems ended; the held bomb's auras started
    /// again (hidden) at the next place. `end`: the match's end (the auras killed too).
    fn clear(&mut self, fx: &mut Fx, end: bool) {
        let mut hs: Vec<SystemId> = Vec::new();
        for b in self.bombs.values() {
            hs.extend([b.bomb, b.arrow].into_iter().flatten());
        }
        for b in self.batarangs.values() {
            hs.extend([b.trail, b.bomb, b.pickup].into_iter().flatten());
        }
        hs.extend(self.grenades.values().flatten());
        if end {
            for a in self.held.values().flatten() {
                hs.extend(a.h);
            }
        }
        for h in hs {
            fx.player.kill(h);
        }
        self.bombs.clear();
        self.batarangs.clear();
        self.grenades.clear();
        self.fogs.clear();
        self.held.clear();
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (GRAPPLE, GRAPPLE_RULE),
    // GrappleHookProjectile_Perk_C: a GrappleHookProjectileV2 child with the same components and no graph of its own
    // on them: its parent's rule. Its PS_ShaggyClap is a plain emitter.
    (PERK_GRAPPLE, GRAPPLE_RULE),
    (KNOCK_UP, ProjRule { own_emitter: Some(knock_up_emitter), ..ProjRule::NONE }),
    // SmokeGrenade_C and SmokeGrenadeOcclusionBP_C are World actors: entered for own_emitter only.
    (GRENADE, ProjRule { own_emitter: Some(grenade_emitter), ..ProjRule::NONE }),
    (ZONE, ProjRule { own_emitter: Some(zone_emitter), ..ProjRule::NONE }),
];

fn me(c: &mut CharFx) -> &mut Batman {
    match c {
        CharFx::Batman(s) => s,
        _ => unreachable!("a Batman rule entered by another module"),
    }
}

// ---- GrappleHookProjectileV2 ----
//
// BeamParticleSystem (PS_BatmanGrappleGunProjectile; bAutoManageAttachment, SnapToTarget on the Capsule root, whose
// socket Gem does not exist: the root's place and rotation, RelativeScale3D 1.5 under the root's scale): one beam
// from the UserSet source to BeamTarget, an Actor end whose InstanceParameters entry has no actor, so its
// distribution (0, 0, 0) through the component's transform: the hook.
//   ReceiveTick @1537-1579: SetBeamSourcePoint(0, GetGrappleOrigin(), 0) every tick.
//   GetGrappleOrigin: the owner's active mesh's GetSocketLocation(IsReeling ? Prop1 : the owner's
//     GetCurrentActiveMontage is Batman_Special_F_Montage or Batman_Special_Air_F_Montage ? GrappleOrigin :
//     FacingLeft ? R_Hand : L_Hand).
//   SetTeamColor @4287: ParticleSystem (the hook's glow, PS_BatmanGrappleGunHookGlow under StaticMesh) Team =
//     SelectFloat(0.6, the owner's colour index, local) @4680 (the shared path's owner team); the beam's
//     SetColorParameter(Color, MakeColor(10, 1, that Team, 1)) @5003 (color_param: SetColorParameter's FColor round
//     trip; the beam's Color module reads 'Color' with MaxInput / MaxOutput (100, 100, 1)). Nothing sets the beam's
//     Team.
//   The attach @5062-6020: a fighter (AttachActor valid): K2_AttachToActor @5434; anything else: StaticMesh's
//     relative rotation = MakeRotator(Roll, -Pitch, 0) of FindLookAtRotation(the actor, GetGrappleOrigin()) @5732-
//     5968, once (the hook turns to face back along the rope; the glow under it turns with it; the mesh: the view's
//     rule, which takes the same rotation). Both then SpawnEmitterAttached(PS_BatmanGrappleGunImpact, the root, Root,
//     (10, 0, 0), rotation 0, scale 1, KeepRelativeOffset, auto destroy) @5526: it rides the hook. No Team. The sim
//     raises it as a projectileDeath event (grapple.rs hitServer); the rule takes it (own_emitter) and draws it
//     attached from the tick the sim's Attached is first seen.
//   The component goes with the actor (K2_DestroyActor): the beam, the glow and the impact vanish at once.
// Unverified: the sockets are read from the last drawn pose (as the notify VFX), so the look-at and the beam's source
// lag the build's by the frame between the pose and the tick.

const GRAPPLE: &str = "GrappleHookProjectileV2_C";
const PERK_GRAPPLE: &str = "GrappleHookProjectile_Perk_C";
const BEAM: &str = "PS_BatmanGrappleGunProjectile";
const IMPACT: &str = "PS_BatmanGrappleGunImpact";
const ORIGIN_MONTAGES: [&str; 2] = ["Batman_Special_F_Montage", "Batman_Special_Air_F_Montage"];
/// BeamParticleSystem RelativeScale3D.
const BEAM_SCALE: f64 = 1.5;
const IMPACT_X: f64 = 10.0;

const GRAPPLE_RULE: ProjRule = ProjRule {
    own_emitter: Some(impact_emitter),
    made: Some(grapple_made),
    tick: Some(grapple_tick),
    end: Some(grapple_end),
    ..ProjRule::NONE
};

/// One grapple's own systems.
#[derive(Debug, Default)]
struct GrappleFx {
    beam: Option<SystemId>,
    impact: Option<SystemId>,
    /// The sim's Attached seen.
    stuck: bool,
}

/// GetGrappleOrigin's socket place now.
fn origin(fx: &Fx, w: &World, pr: &mvs_sim::actors::projectile::Projectile, reeling: bool) -> Option<V3> {
    let o = pr.owner?;
    let fi = w.fighter(o);
    let montage = fi.montages.montage.as_ref().map(|m| w.names.str(m.montage.name));
    let socket = if reeling {
        "Prop1"
    } else if montage.is_some_and(|m| ORIGIN_MONTAGES.contains(&m)) {
        "GrappleOrigin"
    } else if facing(w, o) < 0.0 {
        "R_Hand"
    } else {
        "L_Hand"
    };
    let f = fx.socket_follow(w, fi.id, Some(socket), [0.0; 3], &IDENTITY, [1.0; 3]);
    fx.place(w, &f).map(|a| a.loc)
}

/// The impact is the rule's own (drawn at the attach, `grapple_tick`).
fn impact_emitter(_c: &mut CharFx, _fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { vfx, .. } = e else { return false };
    vfx.is_some_and(|v| w.names.str(v) == IMPACT)
}

fn grapple_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let t = color_param([10.0, 1.0, team_index(fx, pr.owner_id)]);
    let f = Follow::Projectile { seq: st.seq, rel: [0.0; 3], rel_rot: IDENTITY, scale: [BEAM_SCALE; 3] };
    let beam = fx.spawn_follow(w, BEAM, f, 1.0, &[("Color.X", t[0]), ("Color.Y", t[1]), ("Color.Z", t[2])]);
    me(c).grapples.insert(st.seq, GrappleFx { beam, ..GrappleFx::default() });
}

fn grapple_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let Some(g) = grapple_of(pr) else { return };
    let o = origin(fx, w, pr, g.reeling);
    let Some(r) = me(c).grapples.get_mut(&st.seq) else { return };
    // Deviation: a socket with no drawn place (origin None) sets no source: the beam keeps the last one.
    if let (Some(b), Some(o)) = (r.beam, o) {
        fx.player.set_beam_source_point(b, 0, o, 0);
    }
    if !g.stuck || r.stuck {
        return;
    }
    r.stuck = true;
    if g.target.is_none()
        && let Some(o) = o
    {
        // FindLookAtRotation(the actor, the origin): the pitch of (origin - actor); MakeRotator(Roll 0, -Pitch, 0)
        let (dx, dy, dz) = (o[0], o[1] + st.x, o[2] - st.y);
        let pitch = dz.atan2(dx.hypot(dy));
        look_glow(fx, w, st, &rotation_rows(-pitch, 0.0, 0.0));
    }
    let f = Follow::Projectile { seq: st.seq, rel: [IMPACT_X, 0.0, 0.0], rel_rot: IDENTITY, scale: [1.0; 3] };
    r.impact = fx.spawn_follow(w, IMPACT, f, 1.0, &[]);
}

/// The glow (ParticleSystem under StaticMesh) turned with the hook's look-at rotation: its follower placed under the
/// StaticMesh's new relative rotation.
fn look_glow(fx: &mut Fx, w: &World, st: &ProjFxState, look: &Rows) {
    let Some(h) = st.h else { return };
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let Some(cs) = pr.owner.and_then(|o| actor_def(w, ActorRef::Fighter(o), pr.cls)).map(|d| &d.components) else {
        return;
    };
    let Some(c) = cs.get(st.cn).and_then(Value::as_obj) else { return };
    let base = mvs_sim::actors::projectile::attach_parent(cs, st.cn);
    let mut par = base.clone();
    let br: Rows = std::array::from_fn(|k| f64::from(base.rot[k]));
    par.rot = compose_rows(look, &br).map(mvs_sim::math::narrow).to_vec();
    let (rel, rs, _, rel_rot) = under(
        &par,
        xyz(c.get("RelativeLocation"), 0.0),
        &rot_rows(c.get("RelativeRotation")),
        xyz(c.get("RelativeScale3D"), 1.0),
        None,
        base.scale.map(f64::from),
    );
    if let Some(f) = fx.followers.iter_mut().find(|(s, _)| *s == h) {
        f.1 = Follow::Projectile { seq: st.seq, rel, rel_rot, scale: rs };
    }
}

fn grapple_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(r) = me(c).grapples.shift_remove(&st.seq) {
        for h in [r.beam, r.impact].into_iter().flatten() {
            fx.player.kill(h);
        }
    }
}

// ---- Buff_DelayKnockUp ----
//
// TriggerVisuals @1302: SpawnEmitterAtLocation(PS_BatmanBombAttach, the owner's location, rotation 0, scale 1) with
//   Team = SelectFloat(0.6, the owner's colour index, local) @1803-2121.
// TriggerArrowVisuals @2949 (not on a dedicated server):
//   SpawnEmitterAttached(PS_BatmanBomb, the owner's Mesh, VB Spine, location 0, rotation 0, scale 1.1, SnapToTarget,
//     auto destroy) @3137, then K2_SetWorldRotation((0, 90, 0)) @3251 once: its relative rotation is that under the
//     bone's of the moment, and it turns with the spine from then on. SnapToTarget: world scale 1.1. Team =
//     |the owner's colour index - 1| @3584.
//   SpawnEmitterAttached(PS_BatBomb_WarningUp, the owner's 'Attached UI v3 Top' (a WidgetComponent: no socket
//     VFX_Root, the component itself), (0, 0, 150), rotation 0, scale 1, KeepRelativeOffset) @3881 = KnockUpArrow,
//     Team = |the owner's colour index - 1| @4268. The widget component hangs on CharacterMesh0 at the pawn's
//     relative Z with RelativeScale3D 0.8333333 (BaseFighter's and Pawn_BatmanV2's templates).
//   Both go in Particles; DestroyBuff @2429 destroys each (K2_DestroyComponent: at once), at the KnockUp, at
//   OnExitedArena, and with the buff.
// UpdateBuff (ReceiveTick): KnockUpArrow.SetColorParameter(Color, ...): no module of PS_BatBomb_WarningUp reads a
//   parameter 'Color' (its one parameter is Team): nothing drawn changes, and it is not written here.
// KnockUpVisuals @4414: SpawnEmitterAtLocation(PS_BatmanBombExplosion, the owner's location, rotation 0, scale 1),
//   Team = the owner's colour index != 0 ? 0 : 1 @4944-5118.
// The sim raises the two located spawns as projectileDeath events of the class (mvs-sim chars/batman/bomb.rs); the
// entry takes them for their Team.
// Unverified: the bomb's relative rotation is fixed against the last drawn pose of the spine.
// Deviation: the arrow follows the widget component's template place: a graph's BPI_SetUITransformTop while the bomb
// is on is not followed.

const KNOCK_UP: &str = "Buff_DelayKnockUp_C";
const BOMB: &str = "PS_BatmanBomb";
const ARROW: &str = "PS_BatBomb_WarningUp";
const BOMB_SCALE: f64 = 1.1;
const WIDGET_SCALE: f64 = 0.8333333;
const ARROW_Z: f64 = 150.0;
const SPINE: &str = "VB Spine";

fn knock_up_emitter(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx, x, y, ox, scale, .. } = *e else { return false };
    let Some(v) = vfx.map(|v| w.names.str(v)) else { return false };
    let team = match v {
        "PS_BatmanBombAttach" => team_index(fx, fighter),
        "PS_BatmanBombExplosion" => {
            if fx.color_index(fighter) != 0 {
                0.0
            } else {
                1.0
            }
        }
        _ => return false,
    };
    fx.player.spawn(
        v,
        [f64::from(ox), f64::from(-x), f64::from(y)],
        [0.0; 3],
        [f64::from(scale); 3],
        &[("Team", team)],
    );
    true
}

/// One fighter's bomb buff systems, by the buff's KnockUp tick.
#[derive(Debug)]
struct BombFx {
    at: f64,
    bomb: Option<SystemId>,
    arrow: Option<SystemId>,
}

/// "Attached UI v3 Top"_GEN_VARIABLE's RelativeLocation.Z: the pawn's own component, else BaseFighter's.
fn ui_top(w: &World, f: mvs_sim::fighter::FighterId) -> f64 {
    let comps = &w.char_data(f).fighter.components;
    let name = "Attached UI v3 Top_GEN_VARIABLE";
    let z = |m: &mvs_data::fighter::Component| m.p.as_obj()?.get("RelativeLocation")?.as_obj()?.get("Z")?.as_f64();
    comps.pawn.get(name).and_then(z).or_else(|| comps.base.get(name).and_then(z)).unwrap_or(0.0)
}

impl Batman {
    /// Per presentation tick (CharFx.place): a Buff_DelayKnockUp on a fighter that had none of it (or another one) the
    /// tick before is the buff's BeginPlay; one gone is DestroyBuff.
    fn place_bombs(&mut self, fx: &mut Fx, w: &World) {
        for f in w.all_ids() {
            let fi = w.fighter(f);
            let id = fi.id;
            let mut at = -1.0;
            for b in &fi.buffs.buffs {
                if w.names.str(b.cls) == KNOCK_UP
                    && let BuffExt::DelayKnockUp(s) = &b.ext
                {
                    at = s.at;
                }
            }
            if self.bombs.get(&id).is_some_and(|r| r.at == f64::from(at)) {
                continue;
            }
            if let Some(r) = self.bombs.shift_remove(&id) {
                for h in [r.bomb, r.arrow].into_iter().flatten() {
                    fx.player.kill(h);
                }
            }
            if at < 0.0 {
                continue;
            }
            let team = (f64::from(fx.color_index(id)) - 1.0).abs();
            let spine = fx.socket_follow(w, id, Some(SPINE), [0.0; 3], &IDENTITY, [1.0; 3]);
            let bomb = fx.place(w, &spine).and_then(|a| {
                let rel = compose_rows(&rotation_rows(0.0, 90.0 * DEG, 0.0), &transpose(&flat(&a.rot)));
                let mut f = fx.socket_follow(w, id, Some(SPINE), [0.0; 3], &rel, [1.0; 3]);
                if let Follow::Socket { scale, .. } = &mut f {
                    *scale = Scale::Fixed([BOMB_SCALE; 3]);
                }
                fx.attach_to_mesh(w, id, BOMB, SPINE, f, &[("Team", team)])
            });
            let z = ui_top(w, f) + ARROW_Z * WIDGET_SCALE;
            let fa = fx.socket_follow(w, id, None, [0.0, 0.0, z], &IDENTITY, [WIDGET_SCALE; 3]);
            let arrow = fx.spawn_follow(w, ARROW, fa, 1.0, &[("Team", team)]);
            self.bombs.insert(id, BombFx { at: f64::from(at), bomb, arrow });
        }
    }
}

// ---- Item_BatarangV2 ----
//
// The actor's rotation is (0, 0, 0) (K2_SetActorRotation at BeginPlay @1775 and in StopBatarang @385; its
// ProjectileMovement does not turn it with the velocity); Geo = SM_BatarangV2 at (0, 0, -7), yaw -90, scale 3 under
// the Capsule root (bAbsoluteScale: the actor's scale).
// MulticastCreateVisuals @9257 (not on a dedicated server): a live SpawnedEffect is shown again (and Geo hidden);
//   otherwise SpawnEmitterAttached(PS_Batarang, Geo, None, location 0, the owner FacingLeft ? (0, 180, 0) : 0, scale
//   1.4, SnapToTarget, auto destroy) @9641 = SpawnedEffect, shown when no bomb rides @9781, Team =
//   SelectFloat(0.6, the Instigator's colour value, local) @10028; Geo hidden @9394. StopBatarang @66-228: Geo shown,
//   SpawnedEffect destroyed. The sim keeps them as trail / trail_shown.
// AttachBatarangBomb @731: SpawnEmitterAttached(PS_Bombarang, Geo, None, location 0, rotation 0, the owner
//   FacingLeft ? (-3, 3, 3) : (3, 3, 3), SnapToTarget, auto destroy) = BombVFX, Team = SelectFloat(0.6, the
//   Instigator's colour index, local) @1150; DetachBatarangBomb destroys it. The sim's bomb.
// HitSpawnPickupVisuals @4510 (once: IsValid(TeamColorPickupVFX)): SpawnEmitterAttached(PS_ShaggyScoobySnack, the
//   Capsule, None, location 0, rotation 0, scale 5, SnapToTargetIncludingScale, auto destroy) @4614, Team =
//   SelectFloat(0.6, the owner's colour index, local) @4959; DestroyVFX @5684 (pickup, EndPlay) destroys it. The
//   sim's pickup_vfx.
// SnapToTarget's relative scale is Scale over the parent's world scale then (SpawnEmitterAttached): the world scale is
// Scale while the actor's scale stays; it follows a later change of it in proportion.
// The actor's SetActorHiddenInGame (the death blink, Item.hidden) hides its components with it.
// Unverified: the Instigator is taken as the item's owner; the FacingLeft is the owner's at the spawn.
// Deviation: a held batarang draws none of the three (PickUp stops the trail, detaches the bomb and destroys the
// glow); their place follows the actor at the sim's tick position with the actor's rotation (0, 0, 0).

const BATARANG: &str = "Item_BatarangV2_C";
const TRAIL: &str = "PS_Batarang";
const BOMBARANG: &str = "PS_Bombarang";
const PICKUP: &str = "PS_ShaggyScoobySnack";
const GEO_Z: f64 = -7.0;

/// One batarang's systems and the sim's flags as last seen.
#[derive(Debug, Default)]
struct BatarangFx {
    trail: Option<SystemId>,
    bomb: Option<SystemId>,
    pickup: Option<SystemId>,
    trail_on: bool,
    bomb_on: bool,
    pickup_on: bool,
}

impl Batman {
    fn place_batarangs(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (id, it) in w.actors.items.iter() {
            if w.names.str(it.cls) != BATARANG {
                continue;
            }
            let ItemExt::Batarang(b) = &it.ext else { continue };
            let Some(owner) = it.owner else { continue };
            seen.push(it.seq);
            let oid = w.fighter(owner).id;
            let left = facing(w, owner) < 0.0;
            let r = self.batarangs.entry(it.seq).or_default();
            let a = ActorRef::Item(id);
            // The Geo component's place: (0, 0, -7) under the root, its rotation, the actor's scale.
            let geo =
                |rot: Rows, s: V3| Follow::Actor { turned: false, a, rel: [0.0, 0.0, GEO_Z], rel_rot: rot, scale: s };
            let geo_rot = rotation_rows(0.0, -90.0 * DEG, 0.0);
            if b.trail != r.trail_on {
                r.trail_on = b.trail;
                if let Some(h) = r.trail.take() {
                    fx.player.kill(h);
                }
                if b.trail {
                    let rot =
                        if left { compose_rows(&rotation_rows(0.0, 180.0 * DEG, 0.0), &geo_rot) } else { geo_rot };
                    let team = fx.team(oid);
                    r.trail = fx.spawn_follow(w, TRAIL, geo(rot, [1.4; 3]), 1.0, &[("Team", team)]);
                }
            }
            if b.bomb != r.bomb_on {
                r.bomb_on = b.bomb;
                if let Some(h) = r.bomb.take() {
                    fx.player.kill(h);
                }
                if b.bomb {
                    let team = team_index(fx, oid);
                    let s = [if left { -3.0 } else { 3.0 }, 3.0, 3.0];
                    r.bomb = fx.spawn_follow(w, BOMBARANG, geo(geo_rot, s), 1.0, &[("Team", team)]);
                }
            }
            if b.pickup_vfx != r.pickup_on {
                r.pickup_on = b.pickup_vfx;
                if let Some(h) = r.pickup.take() {
                    fx.player.kill(h);
                }
                if b.pickup_vfx {
                    let team = team_index(fx, oid);
                    let f = Follow::Actor { turned: false, a, rel: [0.0; 3], rel_rot: IDENTITY, scale: [5.0; 3] };
                    r.pickup = fx.spawn_follow(w, PICKUP, f, 1.0, &[("Team", team)]);
                }
            }
            let shown = !it.hidden();
            if let Some(h) = r.trail {
                fx.player.set_visible(h, shown && b.trail_shown);
            }
            for h in [r.bomb, r.pickup].into_iter().flatten() {
                fx.player.set_visible(h, shown);
            }
        }
        let gone: Vec<i32> = self.batarangs.keys().copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some(r) = self.batarangs.shift_remove(&k) {
                for h in [r.trail, r.bomb, r.pickup].into_iter().flatten() {
                    fx.player.kill(h);
                }
            }
        }
    }
}

// ---- the smoke bomb ----
//
// SmokeGrenade_C (a World actor):
//   The actor: Comp_Spawner CompSpawnActor's transform, yaw FacingLeft ? 90 : -90 (Comp_Spawner @1385-1479), the
//     Capsule root (bAbsoluteScale: world scale 1) at the sim's place. MineMesh: the view's rule.
//   ParticleSystem: PS_C020PolymorphSatchelAura under the Capsule, RelativeScale3D 2 (no relative location or
//     rotation); Team = the Instigator's PlayerData team index @1463. Drawn while the actor lives.
//   K2_DestroyActor @289 takes the components with it: the aura vanishes at once.
// DetonateClient @2530: SpawnEmitterAttached(PS_BatmanSmokeBombImpact, the OWNER's Mesh, Root, location 0, rotation 0,
//   scale 1, KeepRelativeOffset, auto destroy) @2615, Team = Conv_IntToFloat(GetColorIndex) @2931 (no local 0.6
//   select): the Root bone's world transform, following him from then on. The sim raises it as a projectileDeath
//   event of SmokeGrenade_C; the rule takes it (own_emitter). A child of the main mesh: always drawn.
// SmokeGrenadeOcclusionBP FogVisuals @2488-3342: SpawnEmitterAtLocation(PS_BatmanSmoke, the OWNER's Mesh location +
//   (0, 0, 100), rotation 0, scale 0.6) @2809, Team = SelectFloat(0.5, the colour index, IsLocalOnline) @3093,
//   FogParticle = it @3281. The sim raises the spawn as a projectileDeath event of SmokeGrenadeOcclusionBP_C; the rule
//   spawns it and files it under the zone the same tick added (the newest zone of that owner not yet filed).
//   Delay(FogDuration + 0.25) @3342 -> @494: FogParticle.SetFloatParameter(Dissolve, 1.0), Deactivate() @548: the
//     sim's SmokeZone.visuals reaching 0. Dissolve reaches nothing drawn (a baked constant); the Deactivate shows.
//   ReceiveEndPlay @5111 with FogActivated -> @460: a zone that leaves the world while its fog is on takes the emitter
//     with it at once; one that ends with FogActivated false leaves the emitter to play out.
// Unverified: FogActivated at the zone's end is read as the effects saw it at the tick before.

const GRENADE: &str = "SmokeGrenade_C";
const ZONE: &str = "SmokeGrenadeOcclusionBP_C";
const AURA: &str = "PS_C020PolymorphSatchelAura";
const SMOKE_IMPACT: &str = "PS_BatmanSmokeBombImpact";
const FOG: &str = "PS_BatmanSmoke";
/// The aura's RelativeScale3D.
const AURA_SCALE: f64 = 2.0;

/// A zone's fog emitter and what the effects last saw of it.
#[derive(Debug)]
struct FogFx {
    h: Option<SystemId>,
    fog: bool,
    dissolved: bool,
    seen: bool,
}

fn grenade_emitter(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx, .. } = *e else { return false };
    if !vfx.is_some_and(|v| w.names.str(v) == SMOKE_IMPACT) {
        return false;
    }
    let team = f64::from(fx.color_index(fighter));
    let f = fx.socket_follow(w, fighter, Some("Root"), [0.0; 3], &IDENTITY, [1.0; 3]);
    fx.attach_to_mesh(w, fighter, SMOKE_IMPACT, "Root", f, &[("Team", team)]);
    true
}

fn zone_emitter(c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx, x, y, ox, scale, .. } = *e else { return false };
    if !vfx.is_some_and(|v| w.names.str(v) == FOG) {
        return false;
    }
    let team = if fx.team(fighter) == 0.6 { 0.5 } else { f64::from(fx.color_index(fighter)) };
    let h = fx.player.spawn(
        FOG,
        [f64::from(ox), f64::from(-x), f64::from(y)],
        [0.0; 3],
        [f64::from(scale); 3],
        &[("Team", team)],
    );
    let s = me(c);
    let z = w
        .actors
        .actors
        .iter()
        .filter(|(_, a)| w.names.str(a.cls) == ZONE && a.owner_id == fighter && !s.fogs.contains_key(&a.seq))
        .max_by_key(|(_, a)| a.seq);
    if let Some((_, a)) = z {
        let fog = matches!(&a.state, WorldActorState::SmokeZone(z) if z.fog_activated);
        s.fogs.insert(a.seq, FogFx { h, fog, dissolved: false, seen: true });
    }
    true
}

impl Batman {
    fn place_smoke(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for f in self.fogs.values_mut() {
            f.seen = false;
        }
        for (_, a) in w.actors.actors.iter() {
            match &a.state {
                WorldActorState::SmokeGrenade(g) if w.names.str(a.cls) == GRENADE => {
                    seen.push(a.seq);
                    let k = a.size_scale.unwrap_or(1.0);
                    let h = std::f64::consts::FRAC_PI_2;
                    let yaw = if facing(w, a.owner) < 0.0 { h } else { -h };
                    let at = att(
                        [0.0, f64::from(-g.x), f64::from(g.y)],
                        &rotation_rows(0.0, yaw, 0.0),
                        Some([AURA_SCALE * f64::from(k); 3]),
                    );
                    match self.grenades.get(&a.seq) {
                        Some(Some(s)) => fx.player.attach(*s, &at),
                        Some(None) => {}
                        None => {
                            let team = f64::from(w.fighter(a.owner).team.team);
                            let s = fx.player.spawn_attached(AURA, &at, 1.0, &[("Team", team)]);
                            self.grenades.insert(a.seq, s);
                        }
                    }
                }
                WorldActorState::SmokeZone(z) if w.names.str(a.cls) == ZONE => {
                    let Some(fr) = self.fogs.get_mut(&a.seq) else { continue };
                    fr.seen = true;
                    fr.fog = z.fog_activated;
                    // OCC @494: the particle's Dissolve 1 and Deactivate
                    if z.visuals == 0.0 && !fr.dissolved {
                        fr.dissolved = true;
                        if let Some(h) = fr.h {
                            fx.player.set_param(h, "Dissolve", 1.0);
                            fx.player.deactivate(h);
                        }
                    }
                }
                _ => {}
            }
        }
        let gone: Vec<i32> = self.grenades.keys().copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some(Some(s)) = self.grenades.shift_remove(&k) {
                fx.player.kill(s);
            }
        }
        // OCC ReceiveEndPlay @5111 -> @460: a zone gone with its fog on takes the particle with it
        let gone: Vec<i32> = self.fogs.iter().filter(|(_, f)| !f.seen).map(|(k, _)| *k).collect();
        for k in gone {
            if let Some(f) = self.fogs.shift_remove(&k)
                && f.fog
                && let Some(h) = f.h
            {
                fx.player.kill(h);
            }
        }
    }
}

// ---- the held bomb's auras ----
//
// SM_DebrisBallProjectile's children (Pawn_BatmanV2's SCS): SCS_Node_18 BombAuraVFX (PS_BatmanBombAura,
// RelativeScale3D 1.5) and SCS_Node_20 BombAuraVFX1 (PS_Explosion_SatchelAura, RelativeScale3D 0.75, bVisible false),
// no relative location or rotation, on the bomb's socket Prop2; Team = SelectFloat(0.6, GetColorIndex, local) @5799 /
// @5857. The bomb's mesh is the view's.
// Their visibility (Comp_FighterVisibilityManager): not in the mesh manager's Meshes nor in the VFX tracking array,
//   so only the propagation of the bomb's own SetVisibility reaches them:
//   - SetupInitialVisibility @962 SetVisibility(false, true): both hidden at the start;
//   - SetSubMeshVisibility(tag, true, true) with InvisibilityCount <= 0 @346: both shown;
//   - SetSubMeshVisibility(tag, false, true) @1432: both hidden;
//   - AddInvisibility / RemoveInvisibility: the smoke's invisibility for a viewer, not ported (the view's Deviation).
// Activation: both auras start once with the pawn (bAutoActivate), hidden, and an equip only shows them: the one-shot
//   DarkBG burst plays unseen at the spawn.
// Unverified: the hidden systems go on ticking here (set_visible); whether the build's hidden component simulates is
//   native (not read). The edges are read once a presentation tick; the socket is the last drawn pose's.

const BOMB_COMP: &str = "SM_DebrisBallProjectile_GEN_VARIABLE";
const AURAS: [&str; 2] = ["BombAuraVFX_GEN_VARIABLE", "BombAuraVFX1_GEN_VARIABLE"];
const BOMB_TAG: &str = "satchelcharge";

/// One aura of a held bomb.
#[derive(Debug)]
struct Aura {
    ps: String,
    scale: f64,
    h: Option<SystemId>,
    /// Activated once with the pawn.
    started: bool,
    socket: String,
    /// The sim's visibility as last seen; the last propagating SetVisibility was a show.
    on: bool,
    aura_on: bool,
}

fn auras_of(w: &World, f: mvs_sim::fighter::FighterId) -> Vec<Aura> {
    let comps = &w.char_data(f).fighter.components.pawn;
    let Some(socket) = comps.get(BOMB_COMP).and_then(|c| c.socket.clone()) else { return Vec::new() };
    let mut out = Vec::new();
    for name in AURAS {
        let Some(p) = comps.get(name).and_then(|c| c.p.as_obj()) else { continue };
        let Some(ps) = text(p, "Template") else { continue };
        let scale = xyz(p.get("RelativeScale3D"), 1.0)[0];
        out.push(Aura {
            ps: ps.to_owned(),
            scale,
            h: None,
            started: false,
            socket: socket.clone(),
            on: false,
            aura_on: false,
        });
    }
    out
}

impl Batman {
    fn place_held(&mut self, fx: &mut Fx, w: &World) {
        for f in w.all_ids() {
            if w.names.str(w.fighter(f).ch) != mvs_sim::chars::batman::KEY {
                continue;
            }
            let fi = w.fighter(f);
            let id = fi.id;
            let auras = self.held.entry(id).or_insert_with(|| auras_of(w, f));
            let on = fi.life.dead == 0.0
                && w.char_data(f).def.sub_meshes.iter().any(|s| s == BOMB_TAG)
                && !w.names.get(BOMB_TAG).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            for a in auras.iter_mut() {
                if on && !a.on {
                    a.aura_on = true; // SetSubMeshVisibility(true, propagate) @346
                }
                if !on && a.on {
                    a.aura_on = false; // SetSubMeshVisibility(false, propagate) @1432
                }
                a.on = on;
                if !a.started {
                    a.started = true;
                    let team = fx.team(id);
                    let s = a.scale;
                    let fl = fx.socket_follow(w, id, Some(&a.socket), [0.0; 3], &IDENTITY, [s; 3]);
                    a.h = fx.attach_to_mesh(w, id, &a.ps, &a.socket, fl, &[("Team", team)]);
                }
                if let Some(h) = a.h {
                    fx.player.set_visible(h, on && a.aura_on);
                }
            }
        }
    }
}
