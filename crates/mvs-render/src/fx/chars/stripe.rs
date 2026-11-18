//! Stripe's effects (the CharFx hooks and the classes' projectile rules): the saws'
//! ParticleSystem and Sparks, the boombox's explosions (BOOM_RULE), the dropped bomb's
//! TimerEffect (bombMesh's), the ally saw's ParticleSystem and the pawn's bomb fuse; the saw's
//! and the boombox's Decal components (from the decode: see each). Their
//! meshes are mvs-view's (chars/stripe.rs), which the rules here read through `mvs_view::stripe`. The gun reticle
//! is the HUD's.
//! Not drawn: the dash listener (C023B_DashListener_C has no component of its own to draw).

use indexmap::IndexMap;
use mvs_data::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::ProjectileId;
use mvs_sim::actors::item::ItemExt;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::actors::tunnel_carry::carry_hidden;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::stripe::stripe_of;
use mvs_sim::fighter::common::actor_def;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};
use mvs_vfx::player::Attachment;
use mvs_view::actors::{ActorPlace, actor_place, actor_rot};
use mvs_view::stripe::{BoomPlace, SawToggle, ally_saw_place, boom_place, boom_place_of, saw_of};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{att, mul_row, rot_rows, text, transpose, warn_once, xyz};
use crate::fx::{FloorDecal, Fx};

/// What Stripe's effects keep between steps: by the actors' sequence numbers, and the fuses by fighter.
#[derive(Debug, Default)]
pub(crate) struct Stripe {
    saws: IndexMap<i32, SawFx>,
    booms: IndexMap<i32, BoomFx>,
    bombs: IndexMap<i32, Option<SystemId>>,
    allies: IndexMap<i32, Option<SystemId>>,
    /// Per fighter: the fuse and the StripeBombState.fuse_at it was made for.
    fuses: Vec<(Option<SystemId>, i32)>,
}

fn me(c: &mut CharFx) -> &mut Stripe {
    match c {
        CharFx::Stripe(s) => s,
        _ => unreachable!("a Stripe rule entered by another module"),
    }
}

fn me_ref(c: &CharFx) -> &Stripe {
    match c {
        CharFx::Stripe(s) => s,
        _ => unreachable!("a Stripe rule entered by another module"),
    }
}

impl CharFxHooks for Stripe {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_bombs(fx, w);
        self.place_allies(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_fuses(fx, w);
    }

    /// The saws' and the boombox's Decals, placed at the frame as their meshes are (mvs_view::stripe: the actor's
    /// place between the last two ticks, its rotation the latest).
    fn frame(&mut self, fx: &mut Fx, w: &World, alpha: f64) {
        for (&seq, o) in &self.saws {
            fx.decals.extend(saw_decal(w, seq, o, alpha));
        }
        for (&seq, o) in &self.booms {
            fx.decals.extend(boom_decal(w, seq, o, alpha));
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

impl Stripe {
    fn clear(&mut self, fx: &mut Fx) {
        for o in self.saws.values() {
            for h in [o.ps, o.sparks].into_iter().flatten() {
                fx.player.kill(h);
            }
        }
        self.saws.clear();
        for o in self.booms.values() {
            for h in o.systems.iter().flatten() {
                fx.player.kill(*h);
            }
        }
        self.booms.clear();
        for h in self.bombs.values().chain(self.allies.values()).flatten() {
            fx.player.kill(*h);
        }
        self.bombs.clear();
        self.allies.clear();
        for (h, _) in self.fuses.drain(..) {
            if let Some(h) = h {
                fx.player.kill(h);
            }
        }
    }
}

/// The class's components (`actorDefOwner(pr.owner, pr.cls)`'s).
fn comp<'a>(w: &'a World, a: ActorRef, cls: mvs_sim::names::Name, n: &str) -> Option<&'a mvs_data::value::Props> {
    actor_def(w, a, cls)?.components.get(n)?.as_obj()
}

fn template(c: Option<&mvs_data::value::Props>) -> Option<&str> {
    c?.get("Template")?.as_str()
}

fn cap_scale(w: &World, a: ActorRef, cls: mvs_sim::names::Name) -> f64 {
    xyz(comp(w, a, cls, "Capsule").and_then(|c| c.get("RelativeScale3D")), 1.0)[0]
}

/// UDecalComponent's DecalSize where the component sets none (its constructor's (128, 256, 256)).
const DECAL_SIZE: V3 = [128.0, 256.0, 256.0];

/// A class's Decal component (its template): the material, its place under its attach parent and its size.
#[derive(Debug)]
struct DecalComp {
    material: String,
    rel: V3,
    rot: Rows,
    scale: V3,
    size: V3,
}

impl DecalComp {
    fn of(w: &World, a: ActorRef, cls: mvs_sim::names::Name) -> Option<DecalComp> {
        let c = comp(w, a, cls, "Decal")?;
        let s = xyz(c.get("DecalSize"), f64::NAN);
        Some(DecalComp {
            material: text(c, "DecalMaterial")?.to_string(),
            rel: xyz(c.get("RelativeLocation"), 0.0),
            rot: rot_rows(c.get("RelativeRotation")),
            scale: xyz(c.get("RelativeScale3D"), 1.0),
            size: std::array::from_fn(|k| if s[k].is_nan() { DECAL_SIZE[k] } else { s[k] }),
        })
    }

    /// The decal at relative location `rel` under a parent at `loc` with rows `rot` and world scale `s`: its world
    /// place, the box's half extents DecalSize x its world scale (GetTransformIncludingDecalSize). (The material's name
    /// is FloorDecal's own String: one copy per decal per frame.)
    fn under(&self, seq: i32, rel: V3, loc: V3, rot: &Rows, s: V3, team: Option<f64>) -> FloorDecal {
        let r = mul_row(std::array::from_fn(|k| rel[k] * s[k]), rot);
        FloorDecal {
            actor: seq,
            material: self.material.clone(),
            loc: std::array::from_fn(|k| loc[k] + r[k]),
            rows: compose_rows(&self.rot, rot),
            extent: std::array::from_fn(|k| self.size[k] * self.scale[k] * s[k]),
            team,
            alpha: None,
            dissolve: None,
        }
    }
}

// ---- the saw ------------------------------------------------------------------------------------------------------

/// C023B_SawBlade_EdgeTrace_C and its data children _StartOnGround_C / _FromAlly_EdgeTrace_C (mvs-sim
/// chars/stripe/saw.rs): ToggleVFX(On, Clockwise, OnWall) @8419: ParticleSystem.SetVisibility(On), Rotate VFX(Clockwise)
/// @10288 (ParticleSystem's world rotation yaw 90 clockwise, else -90), Sparks.Activate(true) with On AND OnWall, else
/// Sparks.Deactivate(). SetSparksRotation(Direction) @10911: Sparks' world rotation the look-at along the travel
/// direction. SetTeamColor @10598 (the class's own, no parent call): Team on Sparks alone (SelectFloat(0.6, colour
/// value, IsLocalOnline)).
/// The ToggleVFX calls the sim does not record are read off the saw's state each tick (mvs_view::stripe::SawToggle,
/// whose notes say how). Before the first call the ParticleSystem is shown (its template's), not turned; Sparks
/// inactive.
/// The Decal (C023B_SawBlade_Ground_RDBA_MAT, DBM_Translucent, DecalSize (50, 256, 256)) under StaticMesh
/// (ParentProjectilePhysics', identity under the Capsule) at (-70, 0, 0), rotation (P 90, Y 14.04, R 104.04): BeginPlay
/// @1788-2037 gives it a dynamic instance with Team = the owner's colour index as a float (no 0.6 select). Rotate VFX
/// @10403-10540 (from every ToggleVFX @8480) sets its world location to the actor's + (0, 70, 0) clockwise, else
/// (0, -70, 0): from then on that offset rides the actor (its relative location in the parent's space). Its rotation
/// stays the template's under the actor's; nothing hides it. Ported from the decode.
// Deviation: the Reflected path's Rotate VFX(!Clockwise) @10210, which runs only while IsMovingOnGround (@9756), before
// the edge is attached again 0.05 s later, is not followed: neither the Decal's move nor the ParticleSystem's matching
// turn @10288-10351.
// DeathVFX (PS_CreatureCrystal_Destroy): the shared death path's.
const SAWS: [&str; 3] =
    ["C023B_SawBlade_EdgeTrace_C", "C023B_SawBlade_EdgeTrace_StartOnGround_C", "C023B_SawBlade_FromAlly_EdgeTrace_C"];
const SPARKS: &str = "Sparks";

/// One saw's effect state: the ToggleVFX values, its components and their placing.
#[derive(Debug)]
struct SawFx {
    toggle: SawToggle,
    ps: Option<SystemId>,
    sparks: Option<SystemId>,
    spark_rot: Rows,
    /// ParticleSystem's relative rotation (its template's turn under the actor's until Rotate VFX), the Capsule's
    /// scale.
    own_rot: Rows,
    cap: f64,
    /// The saw, its Decal component, the Decal's relative location (Rotate VFX's) and its owner's colour index.
    id: ProjectileId,
    decal_comp: Option<DecalComp>,
    decal_rel: V3,
    team_index: f64,
}

/// Rotate VFX's SelectVector((0, 70, 0), (0, -70, 0), Clockwise) @10403.
const SAW_DECAL_SHIFT: f64 = 70.0;

/// The actor's place, rows and the Capsule's world scale (StaticMesh's, the Decal's parent).
fn saw_root(w: &World, st: &ProjFxState, o: &SawFx) -> Option<(V3, Rows, f64)> {
    let pr = w.actors.projectiles.get(st.id)?;
    Some(([0.0, -st.x, st.y], actor_rot(w, pr), o.cap * f64::from(pr.scale)))
}

/// Rotate VFX @10403-10540: the Decal's world location the actor's + (0, +-70, 0), kept as its relative location.
fn saw_decal_turn(w: &World, st: &ProjFxState, o: &mut SawFx, cw: bool) {
    let Some((_, rot, s)) = saw_root(w, st, o) else { return };
    let off = [0.0, if cw { SAW_DECAL_SHIFT } else { -SAW_DECAL_SHIFT }, 0.0];
    // (the rows are a rotation: their inverse is the transpose)
    o.decal_rel = mul_row(off, &transpose(&rot)).map(|v| v / s);
}

/// The actor's place and whether a tunnel carry hides its children (Buff_ProjectileThroughTunnel's, read off the
/// world: Fx's own flag is set after the rules' hooks).
fn shown_place(w: &World, id: ProjectileId) -> Option<ActorPlace> {
    let a = ActorRef::Projectile(id);
    if carry_hidden(w, a) {
        return None;
    }
    actor_place(w, a)
}

/// The saw's Decal at the frame `alpha` between the last two ticks; None while hidden.
fn saw_decal(w: &World, seq: i32, o: &SawFx, alpha: f64) -> Option<FloorDecal> {
    let d = o.decal_comp.as_ref()?;
    let p = shown_place(w, o.id)?;
    let (x, y) = p.at(alpha);
    Some(d.under(seq, o.decal_rel, [0.0, -x, y], &p.rot, [o.cap * p.scale; 3], Some(o.team_index)))
}

fn saw_ps_at(w: &World, st: &ProjFxState, o: &SawFx) -> Option<Attachment> {
    let pr = w.actors.projectiles.get(st.id)?;
    let rot = match o.toggle.on {
        None => compose_rows(&o.own_rot, &actor_rot(w, pr)),
        Some(_) => rotation_rows(0.0, if o.toggle.cw { 90.0_f64 } else { -90.0 }.to_radians(), 0.0),
    };
    Some(att([0.0, -st.x, st.y], &rot, Some([o.cap * f64::from(pr.scale); 3])))
}

fn saw_sparks_at(w: &World, st: &ProjFxState, o: &SawFx) -> Option<Attachment> {
    let pr = w.actors.projectiles.get(st.id)?;
    Some(att([0.0, -st.x, st.y], &o.spark_rot, Some([o.cap * f64::from(pr.scale); 3])))
}

fn saw_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let Some((s, _, age)) = saw_of(w, st.id) else { return };
    // the default ParticleSystem draw (with a Team the class's SetTeamColor does not give it) is this rule's own
    if let Some(h) = st.h.take() {
        fx.player.kill(h);
    }
    let a = ActorRef::Projectile(st.id);
    let pc = comp(w, a, pr.cls, "ParticleSystem");
    let decal_comp = DecalComp::of(w, a, pr.cls);
    let team_index = pr.owner.map_or(0.0, |f| f64::from(fx.color_index(w.fighter(f).id)));
    let mut o = SawFx {
        toggle: SawToggle::new(s, age),
        ps: None,
        sparks: None,
        spark_rot: IDENTITY,
        own_rot: rot_rows(pc.and_then(|c| c.get("RelativeRotation"))),
        cap: cap_scale(w, a, pr.cls),
        id: st.id,
        decal_rel: decal_comp.as_ref().map_or([0.0; 3], |d| d.rel),
        decal_comp,
        team_index,
    };
    if let Some(t) = template(pc) {
        let at = saw_ps_at(w, st, &o);
        o.ps = at.and_then(|at| fx.player.spawn_attached(t, &at, 1.0, &[]));
        if o.ps.is_none() {
            warn_once(format!("Stripe: {t} is not loaded"));
        }
    }
    st.h = o.ps;
    me(c).saws.insert(st.seq, o);
}

/// Sparks.Activate(true): the system made anew; Deactivate.
fn sparks_on(fx: &mut Fx, w: &World, st: &ProjFxState, o: &mut SawFx, on: bool) {
    if !on {
        if let Some(h) = o.sparks {
            fx.player.deactivate(h);
        }
        return;
    }
    if let Some(h) = o.sparks.take() {
        fx.player.kill(h);
    }
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let Some(t) = template(comp(w, ActorRef::Projectile(st.id), pr.cls, SPARKS)) else { return };
    let Some(owner) = pr.owner else { return };
    let team = fx.team(w.fighter(owner).id);
    let Some(at) = saw_sparks_at(w, st, o) else { return };
    o.sparks = fx.player.spawn_attached(t, &at, 1.0, &[("Team", team)]);
    if o.sparks.is_none() {
        warn_once(format!("Stripe: {t} is not loaded"));
    }
}

fn saw_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(o) = me(c).saws.get_mut(&st.seq) else { return };
    let Some((s, vx, age)) = saw_of(w, st.id) else { return };
    if s.rider.active {
        // SetSparksRotation: FindLookAtRotation along (0, -dirX, dirY)
        let (dx, dy) = (-s.rider.ctd[1], s.rider.ctd[2]);
        let yaw = if dx == 0.0 && dy == 0.0 { 0.0 } else { (-dx).atan2(0.0) };
        o.spark_rot = rotation_rows(dy.atan2(dx.abs()), yaw, 0.0);
    }
    if let Some(call) = o.toggle.tick(s, vx, age) {
        if let Some(h) = o.ps {
            fx.player.set_visible(h, call.on);
        }
        sparks_on(fx, w, st, o, call.on && call.wall);
        saw_decal_turn(w, st, o, call.cw);
    }
    if let (Some(h), Some(at)) = (o.ps, saw_ps_at(w, st, o)) {
        fx.player.attach(h, &at);
    }
    if let (Some(h), Some(at)) = (o.sparks, saw_sparks_at(w, st, o)) {
        fx.player.attach(h, &at);
    }
}

fn saw_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(o) = me(c).saws.shift_remove(&st.seq)
        && let Some(h) = o.sparks
    {
        fx.player.kill(h);
    }
}

fn saw_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c).saws.get(&st.seq).map_or_else(Vec::new, |o| [o.ps, o.sparks].into_iter().flatten().collect())
}

const SAW_FX: ProjRule = ProjRule {
    made: Some(saw_made),
    tick: Some(saw_tick),
    end: Some(saw_end),
    systems: Some(saw_systems),
    ..ProjRule::NONE
};

// ---- the boombox --------------------------------------------------------------------------------------------------

/// BoomBox_SpawnedActor_C (mvs-sim chars/stripe/dash.rs BoomBox: a Projectile kind). Each Detonate (BoomBox.detonations):
/// EffectMulticast @3204: SpawnEmitterAttached(PS_C023B_BoomBox_Explosion, Mesh, (0, 0, 50), KeepRelativeOffset) with
/// Team (SelectFloat(0.6, the owner's colour index, local) @3498); a component of the box, gone with it. The Mesh turns
/// with the shake (mvs_view::stripe::boom_place).
/// The blob shadow: the Decal (Character_BlobShadow_MAT, DBM_AlphaComposite, DecalSize the component default) under the
/// Mesh at (0, 0, -0.88), yaw -90, scale (0.909, 0.409, 0.02): a box turned with the Mesh and its shake, not projected
/// down the world Z; no graph sets its material. Ported from the component data.
const BOOMBOX: &str = "BoomBox_SpawnedActor_C";
const BOOM_FX: &str = "PS_C023B_BoomBox_Explosion";
const BOOM_FX_Z: f64 = 50.0;

#[derive(Debug)]
struct BoomFx {
    seen: i32,
    systems: Vec<Option<SystemId>>,
    place: BoomPlace,
    /// The box and its Decal component.
    id: ProjectileId,
    decal_comp: Option<DecalComp>,
}

fn detonations(w: &World, st: &ProjFxState) -> Option<i32> {
    match &w.actors.projectiles.get(st.id)?.ext {
        ProjectileExt::BoomBox(b) => Some(b.detonations),
        _ => None,
    }
}

fn boom_at(w: &World, st: &ProjFxState, q: &BoomPlace) -> Option<Attachment> {
    let pr = w.actors.projectiles.get(st.id)?;
    let x = boom_place(w, st.id, q, st.x, st.y, &actor_rot(w, pr), f64::from(pr.scale));
    let z = BOOM_FX_Z * x.scale[2];
    let loc = std::array::from_fn(|j| x.loc[j] + z * x.rot[6 + j]);
    Some(att(loc, &x.rot, Some(x.scale)))
}

fn boom_made(c: &mut CharFx, _fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let seen = detonations(w, st).unwrap_or(0);
    let a = ActorRef::Projectile(st.id);
    let place = boom_place_of(w, a);
    let decal_comp = w.actors.projectiles.get(st.id).and_then(|pr| DecalComp::of(w, a, pr.cls));
    me(c).booms.insert(st.seq, BoomFx { seen, systems: Vec::new(), place, id: st.id, decal_comp });
}

/// The Decal under the Mesh's place at the frame `alpha` between the last two ticks; None while hidden.
fn boom_decal(w: &World, seq: i32, o: &BoomFx, alpha: f64) -> Option<FloorDecal> {
    let d = o.decal_comp.as_ref()?;
    let p = shown_place(w, o.id)?;
    let (x, y) = p.at(alpha);
    let m = boom_place(w, o.id, &o.place, x, y, &p.rot, p.scale);
    Some(d.under(seq, d.rel, m.loc, &m.rot, m.scale, None))
}

fn boom_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(o) = me(c).booms.get_mut(&st.seq) else { return };
    let at = boom_at(w, st, &o.place);
    if let Some(at) = &at {
        for h in o.systems.iter().flatten() {
            fx.player.attach(*h, at);
        }
    }
    let Some(n) = detonations(w, st) else { return };
    if n == o.seen {
        return;
    }
    o.seen = n;
    let (Some(at), Some(owner)) = (at, w.actors.projectiles.get(st.id).and_then(|p| p.owner)) else { return };
    let team = fx.team(w.fighter(owner).id);
    let h = fx.player.spawn_attached(BOOM_FX, &at, 1.0, &[("Team", team)]);
    if h.is_none() {
        warn_once(format!("Stripe: {BOOM_FX} is not loaded"));
    }
    o.systems.push(h);
}

fn boom_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(o) = me(c).booms.shift_remove(&st.seq) {
        for h in o.systems.into_iter().flatten() {
            fx.player.kill(h);
        }
    }
}

fn boom_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    me_ref(c).booms.get(&st.seq).map_or_else(Vec::new, |o| o.systems.iter().flatten().copied().collect())
}

const BOOM_RULE: ProjRule = ProjRule {
    made: Some(boom_made),
    tick: Some(boom_tick),
    end: Some(boom_end),
    systems: Some(boom_systems),
    ..ProjRule::NONE
};

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] =
    &[(SAWS[0], SAW_FX), (SAWS[1], SAW_FX), (SAWS[2], SAW_FX), (BOOMBOX, BOOM_RULE)];

// ---- the dropped bomb's timer -------------------------------------------------------------------------------------

/// C023B_DroppedBomb_C (an item, mvs-sim chars/stripe/bomb.rs): TimerEffect (PS_Timer, scale 2 on the Capsule) with
/// Team @3896 (SelectFloat(0.6, the colour index, IsLocalOnline)), TimerOffset 1 - DetonateTime / 5 @4118 and Duration
/// BombDuration @4176 (MulticastTimerValues @891 at BeginPlay: the item's first sight). Shown with the item.
// Unverified: placed at the item's place of the latest tick, not at the frame's.
const TIMER: &str = "TimerEffect";
const FUSE_SPAN: f64 = 5.0;

impl Stripe {
    fn place_bombs(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (id, it) in w.actors.items.iter() {
            let ItemExt::DroppedBomb(b) = &it.ext else { continue };
            if it.dead {
                continue;
            }
            let a = ActorRef::Item(id);
            let Some(p) = actor_place(w, a) else { continue };
            let Some(owner) = it.owner.or(it.spawner) else { continue };
            seen.push(it.seq);
            let tc = comp(w, a, it.cls, TIMER);
            let ts = xyz(tc.and_then(|c| c.get("RelativeScale3D")), 1.0)[0];
            let at = att([0.0, -p.x, p.y], &p.rot, Some([ts * p.scale; 3]));
            let h = *self.bombs.entry(it.seq).or_insert_with(|| {
                // (made with the bomb's mesh: no mesh, no TimerEffect)
                if !mvs_view::stripe::bomb_drawn(w, it) || comp(w, a, it.cls, "SkeletalMesh").is_none() {
                    return None;
                }
                let t = template(tc)?;
                let team = fx.team(w.fighter(owner).id);
                let params = [
                    ("Team", team),
                    ("TimerOffset", 1.0 - f64::from(b.detonate_time) / FUSE_SPAN),
                    ("Duration", f64::from(b.bomb_duration)),
                ];
                let h = fx.player.spawn_attached(t, &at, 1.0, &params);
                if h.is_none() {
                    warn_once(format!("Stripe: {t} is not loaded"));
                }
                h
            });
            if let Some(h) = h {
                fx.player.attach(h, &at);
                fx.player.set_visible(h, !p.hidden);
            }
        }
        self.bombs.retain(|seq, h| {
            let keep = seen.contains(seq);
            if !keep && let Some(h) = h {
                fx.player.kill(*h);
            }
            keep
        });
    }
}

// ---- the ally saw -------------------------------------------------------------------------------------------------

/// C023B_AllySaw_C (mvs-sim chars/stripe/saw.rs AllySaw): ParticleSystem (PS_C023B_NormalNeutral under SawBlade, scale
/// 1.1667) with Team (SetTeamColor @220: SelectFloat(0.6, the colour value, IsLocalOnline)), at the blade's place
/// (mvs_view::stripe::ally_saw_place).
impl Stripe {
    fn place_allies(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (id, x) in w.actors.actors.iter() {
            if x.dead || !matches!(x.state, WorldActorState::AllySaw(_)) {
                continue;
            }
            let a = ActorRef::Actor(id);
            let Some((loc, rot, g)) = ally_saw_place(w, a) else { continue };
            seen.push(x.seq);
            let bs = xyz(comp(w, a, x.cls, "SawBlade").and_then(|c| c.get("RelativeScale3D")), 1.0);
            let pc = comp(w, a, x.cls, "ParticleSystem");
            let ps = xyz(pc.and_then(|c| c.get("RelativeScale3D")), 1.0)[0];
            let s: V3 = std::array::from_fn(|k| bs[k] * ps * g);
            let at = att(loc, &rot, Some(s));
            let h = *self.allies.entry(x.seq).or_insert_with(|| {
                let t = template(pc)?;
                let team = fx.team(w.fighter(x.owner).id);
                let h = fx.player.spawn_attached(t, &at, 1.0, &[("Team", team)]);
                if h.is_none() {
                    warn_once(format!("Stripe: {t} is not loaded"));
                }
                h
            });
            if let Some(h) = h {
                fx.player.attach(h, &at);
            }
        }
        self.allies.retain(|seq, h| {
            let keep = seen.contains(seq);
            if !keep && let Some(h) = h {
                fx.player.kill(*h);
            }
            keep
        });
    }
}

// ---- the pawn's bomb fuse -----------------------------------------------------------------------------------------

/// The bomb fuse: Comp_C023B_Bomb SetBomb @1024-1544 SpawnEmitterAttached(PS_Timer, the pawn's mesh, fml_un_R_wrist, no
/// offset, scale 2) with Duration BombTime, TimerOffset and Team; K2_DestroyComponent @1874. Read off
/// StripeBombState.fuse_at / fuse_offset.
const FUSE_PS: &str = "PS_Timer";
const FUSE_SOCKET: &str = "fml_un_R_wrist";
const FUSE_SCALE: f64 = 2.0;

impl Stripe {
    fn place_fuses(&mut self, fx: &mut Fx, w: &World) {
        let ids = w.all_ids();
        if self.fuses.len() < ids.len() {
            self.fuses.resize(ids.len(), (None, -1));
        }
        for (k, &f) in ids.iter().enumerate() {
            let Some(s) = stripe_of(w, f) else { continue };
            let b = &s.bomb;
            let at = b.fuse_at;
            if at == self.fuses[k].1 {
                continue;
            }
            if let Some(h) = self.fuses[k].0.take() {
                fx.player.kill(h);
            }
            self.fuses[k].1 = at;
            if at < 0 {
                continue;
            }
            let id = w.fighter(f).id;
            let time = mvs_sim::fighter::control::pawn_comp_in(&w.data, w, f, "Comp_C023B_Bomb_GEN_VARIABLE")
                .and_then(|p| p.get("BombTime"))
                .and_then(Value::as_f64)
                .unwrap_or(5.0);
            let fo = fx.socket_follow(w, id, Some(FUSE_SOCKET), [0.0; 3], &IDENTITY, [FUSE_SCALE; 3]);
            let params = [("Team", fx.team(id)), ("TimerOffset", f64::from(b.fuse_offset)), ("Duration", time)];
            self.fuses[k].0 = fx.attach_to_mesh(w, id, FUSE_PS, FUSE_SOCKET, fo, &params);
        }
    }
}
