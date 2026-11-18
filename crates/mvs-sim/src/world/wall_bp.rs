//! The shared stage wall classes: DestructibleObjectBP_C and its child WallBP_C,
//! as far as a placed child reaches them (Cromulons' speakers, Batcave's cars), the two multicast delegates a
//! listener binds on them and the casts it binds through. The Comp_Actor_Destructible / Comp_Pawn_DamageManager
//! values the hit path reads are the child's `StageActorEntry` (`world::stage_actors`).
//! Offsets are WallBP's ubergraph's unless named.
//! - BeginPlay @1856: InitStage(0) (CurrentStage 0, HP = Stages[0].HP), StaticMeshVisuals.
//! - ReceiveAnyDamage @1908 -> @870 (authority; no Destroyed_0 gate): HP - Damage, BPI_DamageVisuals(null) @982, then
//!   HP <= 0: InitStage(CurrentStage + 1) @1083 (no carry of the remainder: the next stage's HP replaces it).
//! - InitStage: CurrentStage = Stage; Stage >= Stages.length -> BPI_Destroy @177 (HP left as it is), else
//!   HP = Stages[Stage].HP @360. So every hit with HP <= 0 past the last stage calls BPI_Destroy again.
//! - BPI_Destroy (DestructibleObjectBP @383): broadcast ObjectDestroyed, then DestroyDestructible, the class's override
//!   (a child's own runs the parent's first: M009_DestructiblePlatformBP @3253).
//! - DestroyDestructible @2772: Destroyed_0 already true: nothing; else @2386 Destroyed_0 = true, broadcast
//!   StructureDestroyed @15, Delay(0.1) @34 -> @89: CollisionBox, SkeletalMesh, DamageBox, CollisionBoxBottom
//!   SetCollisionEnabled(NoCollision) @89-203, StaticMesh1 / 2 (and StaticMesh with UseStaticMesh) hidden,
//!   DestroyMontage played @357 (its OnCompleted @1391 -> @791 hides SkeletalMesh and StaticMesh while Destroyed_0:
//!   the drawing's, from `off_at` and the montage's length).
//! - BPI_DamageVisuals (DestructibleObjectBP @426) -> DamageVisuals, WallBP's override @2792: the parent's (@53: the
//!   DamageAudio Env_Audio_Emit's AkEvent = Ak_Damage_Environment, ManualStart(the causer cast to Pawn: null here)),
//!   then DamageMontage played @1913 (`hit_at` / `hit_montage`).
//!
//! The two broadcasts and their listener lists:
//! - DestructibleObjectBP ObjectDestroyed (@383, at every BPI_Destroy, so again at each hit past the last stage), on
//!   every DestructibleObjectBP child (WallBP's, Batcave's cars);
//! - WallBP StructureDestroyed (@15, in WallBP's DestroyDestructible, once: behind Destroyed_0), on WallBP and its
//!   children (the speakers) alone: a listener casts the actor to WallBP before its bind ([`is_wall_bp`]).
//!
//! The listeners bind from their own modules, each on the actor its ground trace meets ([`ground_destructible`]):
//! C018Flag (OnStructureDestroyed on a WallBP @4135, on a DestructibleObjectBP @4736), SlowtoStunGrenade
//! (DestroyIfPlatformDestroyed), FartBomb_SpawnedActor (GroundDestroyed @2746), BugsBunnyTunnelParent (Destroy
//! @2249), BoomBox_SpawnedActor (GroundDestroyed @2604), MouseTrap_SpawnedActor (GroundDestroyed @2645), AryaKillPie
//! (Destroy @1536) and HealOverTimeZoneBP (DestroyIfPlatformDestroyed @6873): all but the flag bind ObjectDestroyed
//! alone.
//!
//! Not ported:
//! - ResetDestructible / BPI_Reset (@2787: the four components QueryAndPhysics, Destroyed_0 false, visibility,
//!   InitStage 0): nothing calls it in a match (ResetLevelObjects has no caller).
//! - The CollisionBox's overlap with a PortalParent (@2816: the portal pushed to the Box's side): Rick's portals are
//!   not pushed off a wall.
//! - StaticMeshVisuals (UseStaticMesh meshes, the Tint loop): the drawing's.

use std::sync::Arc;

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::world_actor::actor_dead;
use crate::combat::boxes::WorldSphere;
use crate::events::Event;
use crate::fighter::common::{warn_parts, warn_text};
use crate::fighter::moves::V2;
use crate::fighter::traces::TraceHit;
use crate::names::Name;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::stage::{stage_delay, stage_solid_enable};
use crate::world::stage_actors::{StageActorOwn, StageHurt};
use mvs_data::value::Value;

/// `WallDelegate`: the delegate a listener binds.
#[derive(crate::State, Copy, PartialEq, Eq, Debug)]
pub enum WallDelegate {
    /// DestructibleObjectBP ObjectDestroyed__DelegateSignature.
    ObjectDestroyed,
    /// WallBP StructureDestroyed__DelegateSignature.
    StructureDestroyed,
}

/// `WallActor`: a placed stage actor the wall classes run on (a WallBP or DestructibleObjectBP child whose own state
/// holds a [`WallPart`]), by its place in `StageState::actors` (its `ActorRef::Stage` slot, the `u32` the
/// `StageActorEntry` hooks take).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WallActor(pub u32);

/// `WallActor`'s own fields (WallActor beyond StageActor): the part of a WallBP or
/// DestructibleObjectBP child's state the shared wall code keeps, held by the child's own state (Cromulons'
/// `Speaker::wall`, Batcave's `BatcaveCar::wall`); [`wall_part`] reaches it from either. Made by [`wall_create`].
#[derive(crate::State, Debug, Default)]
pub struct WallPart {
    /// `hp`: HP (float32).
    pub hp: Real,
    /// `stage`: CurrentStage: 0 .. Stages.length - 1, then one more per hit that finds HP <= 0 (BPI_Destroy each time).
    pub stage: i32,
    /// `destroyed`: Destroyed_0.
    pub destroyed: bool,
    /// `damageMontage`: DamageMontage now (a child may change it), its object name.
    pub damage_montage: String,
    /// `destroyMontage`: DestroyMontage.
    pub destroy_montage: String,
    /// `hitAt`: the world tick of the last DamageVisuals montage start, -1 none.
    pub hit_at: i32,
    /// `hitMontage`: that montage ("" none).
    pub hit_montage: String,
    /// `brokenAt`: the world tick DestroyDestructible first ran (Destroyed_0 set), -1.
    pub broken_at: i32,
    /// `offAt`: the world tick the 0.1 s Delay turned the components off and DestroyMontage started, -1.
    pub off_at: i32,
    /// `listeners`: the bound functions of ObjectDestroyed and StructureDestroyed, in bind order; `None` until the
    /// first bind (the record writes the key only then).
    pub listeners: Option<Vec<WallListener>>,
    /// The actor's static data (`GEO`), set by [`wall_create`].
    #[state(skip)]
    pub geo: Arc<WallGeo>,
}

/// `WallListener`: a dynamic delegate bound on the actor: the listener and its function, by `"<Class>.<Function>"`
/// (an entry of [`wall_delegate_fns`]).
#[derive(crate::State, Copy, PartialEq, Eq, Debug)]
pub struct WallListener {
    pub d: WallDelegate,
    /// `self`.
    pub listener: ActorRef,
    /// `fn`.
    pub f: &'static str,
}

/// `WallGeo`: the actor's static data, made once at its create.
#[derive(Debug, Default)]
pub struct WallGeo {
    /// Stages[i].HP (float32).
    pub stages: Vec<Real>,
    /// Ak_Damage_Environment.
    pub damage_audio: Option<Name>,
    /// The DamageAudio child actor's place, sim space.
    pub audio_at: V2,
    /// The components' `StageData.solids` ids (CollisionBox, DamageBox; a NoCollision one has none).
    pub solids: Vec<i32>,
    /// DamageBox (DamageDestructible).
    pub hurt: Vec<StageHurt>,
}

/// The [`WallPart`] of a stage actor's own state: a Cromulons speaker's or a Batcave car's; `None` for another class.
pub fn wall_part(own: &StageActorOwn) -> Option<&WallPart> {
    match own {
        StageActorOwn::Speaker(s) => Some(&s.wall),
        StageActorOwn::BatcaveCar(c) => Some(&c.wall),
        _ => None,
    }
}

/// [`wall_part`], mutable.
pub fn wall_part_mut(own: &mut StageActorOwn) -> Option<&mut WallPart> {
    match own {
        StageActorOwn::Speaker(s) => Some(&mut s.wall),
        StageActorOwn::BatcaveCar(c) => Some(&mut c.wall),
        _ => None,
    }
}

/// The [`WallPart`] of the wall actor `a` (a panic for an actor that holds none: a bug of the caller).
pub fn wall(w: &World, a: WallActor) -> &WallPart {
    let r = w.stage_run.as_ref().expect("a stage state");
    wall_part(&r.actors[a.0 as usize].own).unwrap_or_else(|| panic!("stage actor {} holds no WallPart", a.0))
}

/// [`wall`], mutable.
pub fn wall_mut(w: &mut World, a: WallActor) -> &mut WallPart {
    let r = w.stage_run.as_mut().expect("a stage state");
    wall_part_mut(&mut r.actors[a.0 as usize].own).unwrap_or_else(|| panic!("stage actor {} holds no WallPart", a.0))
}

/// The wall actor whose owner number (`StageTimer::owner`: its `StageData.placed` index) is `owner`, for a stage's
/// `StageEntry::timer` (the [`WALL_DELAY`] link); `None` when that actor holds no [`WallPart`].
pub fn wall_of_owner(w: &World, owner: i32) -> Option<WallActor> {
    let r = w.stage_run.as_ref()?;
    let i = r.actors.iter().position(|a| i64::from(a.index) == i64::from(owner))?;
    wall_part(&r.actors[i].own).map(|_| WallActor(u32::try_from(i).expect("a list index")))
}

/// `refName`: the object name a reference value (its path) ends in
/// ("/Game/.../Speaker_Hit_Anim_Montage.Speaker_Hit_Anim_Montage" -> "Speaker_Hit_Anim_Montage"); "" for a value
/// that is no string.
pub fn ref_name(v: Option<&Value>) -> &str {
    match v {
        Some(Value::Str(s)) => s.rsplit(['.', ':']).next().unwrap_or(""),
        _ => "",
    }
}

/// `WALL_DELAY`: the Delay node @34's key and the event its link runs (@89): a stage's
/// `StageEntry::timer` calls [`wall_delayed`] on it.
pub const WALL_DELAY: i32 = 1;

/// The actor's `StageData.placed` index (`StageActor.index`).
fn placed_index(w: &World, a: WallActor) -> u32 {
    w.stage_run.as_ref().expect("a stage state").actors[a.0 as usize].index
}

/// `wallCreate`: the WallBP part of a placed actor's create (BeginPlay's InitStage(0), run
/// at the load). `a`: the actor, already in `StageState::actors` (its `index` names its `StageData.placed` entry);
/// `damage_audio_rel`: the DamageAudio child actor's RelativeLocation (UE, actor space). The class's create puts the
/// result in its own state; the actor's root (`x`, `y`) is set here from the placed transform.
pub fn wall_create(w: &mut World, a: WallActor, damage_audio_rel: [Real; 3]) -> WallPart {
    let data = Arc::clone(&w.data);
    let index = placed_index(w, a) as usize;
    let d = &data.stage.placed.as_ref().expect("a placed stage actor's data")[index];
    let stages: Vec<Real> =
        d.p.get("Stages")
            .and_then(Value::as_arr)
            .unwrap_or(&[])
            .iter()
            .map(|s| crate::math::narrow(s.as_obj().and_then(|o| o.get("HP")).and_then(Value::as_f64).unwrap_or(0.0)))
            .collect();
    if stages.is_empty() {
        warn_text(
            w,
            &format!("{} {}: no Stages (InitStage(0) would destroy it at BeginPlay: not ported)", d.cls, d.name),
        );
    }
    let solids = data.stage.solids.as_deref().unwrap_or(&[]);
    let ids: Vec<i32> = d.solids.clone().unwrap_or_default();
    let mut hurt = Vec::new();
    for &id in &ids {
        let Some(s) = usize::try_from(id).ok().and_then(|i| solids.get(i)) else { continue };
        if s.name.split('.').nth(1) != Some("DamageBox") {
            continue;
        }
        let (c, e) = (&s.center, &s.half_extent);
        // the box in sim axes: x = -UE Y, y = UE Z, depth = UE X (the levels place the walls unrotated)
        hurt.push(StageHurt::Box(WorldSphere {
            x: crate::math::narrow(-c[1]),
            y: crate::math::narrow(c[2]),
            z: Some(crate::math::narrow(c[0])),
            r: 0.0,
            bx: Some([
                crate::math::narrow(e[1]),
                0.0,
                0.0,
                0.0,
                crate::math::narrow(e[2]),
                0.0,
                0.0,
                0.0,
                crate::math::narrow(e[0]),
            ]),
            ..WorldSphere::default()
        }));
    }
    if hurt.is_empty() {
        warn_text(w, &format!("{} {}: no DamageBox solid in the stage data", d.cls, d.name));
    }
    let (s, l) = (&d.scale, &d.loc);
    let audio = ref_name(d.p.get("Ak_Damage_Environment"));
    let damage_audio = (!audio.is_empty()).then(|| match w.names.get(audio) {
        Some(n) => n,
        // A path's object name the data walk did not hold whole: interned at the load (a step never interns).
        None => Arc::make_mut(&mut w.names).intern(audio),
    });
    let geo = WallGeo {
        stages,
        damage_audio,
        audio_at: V2 {
            x: crate::math::narrow(-(l[1] + s[1] * f64::from(damage_audio_rel[1]))),
            y: crate::math::narrow(l[2] + s[2] * f64::from(damage_audio_rel[2])),
        },
        solids: ids,
        hurt,
    };
    let r = w.stage_run.as_mut().expect("a stage state");
    let actor = &mut r.actors[a.0 as usize];
    actor.x = crate::math::narrow(-l[1]);
    actor.y = crate::math::narrow(l[2]);
    WallPart {
        hp: geo.stages.first().copied().unwrap_or(0.0),
        stage: 0,
        destroyed: false,
        damage_montage: ref_name(d.p.get("DamageMontage")).to_owned(),
        destroy_montage: ref_name(d.p.get("DestroyMontage")).to_owned(),
        hit_at: -1,
        hit_montage: String::new(),
        broken_at: -1,
        off_at: -1,
        listeners: None,
        geo: Arc::new(geo),
    }
}

/// `wallHurt`: the DamageBox while its collision is on (off with the others at the Delay's
/// link), into `out` (cleared); false for none. The `StageActorEntry::hurt` hook's shape over a [`WallActor`].
pub fn wall_hurt(w: &World, a: WallActor, out: &mut Vec<StageHurt>) -> bool {
    out.clear();
    let p = wall(w, a);
    if p.off_at >= 0 {
        return false;
    }
    out.extend(p.geo.hurt.iter().cloned());
    true
}

/// `wallHurtShapes`: the DamageBox shapes whatever its collision (a class that switches it
/// itself: Batcave's car).
pub fn wall_hurt_shapes(w: &World, a: WallActor) -> &[StageHurt] {
    &wall(w, a).geo.hurt
}

/// `WallDestroy`: the class's DestroyDestructible (BPI_Destroy's call): its own override, or
/// [`wall_destroy_destructible`].
pub type WallDestroy = fn(&mut World, WallActor);

/// `WallVisuals`: the class's DamageVisuals (BPI_DamageVisuals' multicast), its own
/// override; [`wall_any_damage`] takes `None` for WallBP's.
pub type WallVisuals = fn(&mut World, WallActor);

/// `wallAnyDamage`: WallBP ReceiveAnyDamage (@870-1138). `destroy`: the class's
/// DestroyDestructible; `visuals`: the class's DamageVisuals (a DestructibleObjectBP child with the same
/// ReceiveAnyDamage and its own override: BatcaveWallDestructible @4273 -> @215 -> @53, its DamageVisuals @7001), `None`
/// for WallBP's.
pub fn wall_any_damage(w: &mut World, a: WallActor, dmg: Real, destroy: WallDestroy, visuals: Option<WallVisuals>) {
    let p = wall_mut(w, a);
    p.hp -= dmg;
    match visuals {
        Some(v) => v(w, a),
        None => damage_visuals(w, a),
    }
    let p = wall(w, a);
    if p.hp <= 0.0 {
        let next = p.stage + 1;
        init_stage(w, a, next, destroy);
    }
}

/// `damageVisuals`: BPI_DamageVisuals(null) -> DamageVisuals: the parent's DamageAudio post,
/// then DamageMontage.
// Deviation: the Env_Audio_Emit child actor's own AkComponent is played as a pooled one at its place (the port's
// PF_Post_Event_At_Location path, Tree Fort's pillar sound): its voice and RTPC state are the pool's.
fn damage_visuals(w: &mut World, a: WallActor) {
    let geo = Arc::clone(&wall(w, a).geo);
    if let Some(ev) = geo.damage_audio {
        let (x, y) = (Some(geo.audio_at.x), Some(geo.audio_at.y));
        w.events.push(Event::Sound { fighter: -1, ev, x, y, hit: None, pooled: true, key: None });
    }
    let tick = w.flow.tick;
    let p = wall_mut(w, a);
    p.hit_at = tick;
    p.hit_montage.clone_from(&p.damage_montage);
}

/// `initStage`: InitStage(Stage) (WallBP function InitStage @0-430; BatcaveWallDestructible's
/// @0-847 adds DestroyImmediately, false on every placed car, to the destroy test).
pub fn init_stage(w: &mut World, a: WallActor, stage: i32, destroy: WallDestroy) {
    let p = wall_mut(w, a);
    p.stage = stage;
    let hp = usize::try_from(stage).ok().and_then(|i| p.geo.stages.get(i)).copied();
    match hp {
        Some(hp) => p.hp = hp,
        None => {
            // BPI_Destroy (DestructibleObjectBP @383): ObjectDestroyed, then the class's DestroyDestructible @402
            broadcast(w, a, WallDelegate::ObjectDestroyed);
            destroy(w, a);
        }
    }
}

/// `wallDestroyDestructible`: WallBP DestroyDestructible (@2772 -> @2386 -> @15 -> @34).
pub fn wall_destroy_destructible(w: &mut World, a: WallActor) {
    let tick = w.flow.tick;
    let p = wall_mut(w, a);
    if p.destroyed {
        return;
    }
    p.destroyed = true;
    p.broken_at = tick;
    broadcast(w, a, WallDelegate::StructureDestroyed); // @15
    let owner = i32::try_from(placed_index(w, a)).expect("a placed index");
    stage_delay(w, owner, WALL_DELAY, 0.1, WALL_DELAY);
}

/// The components WallBP's Delay link turns off (@89 CollisionBox, @127 SkeletalMesh, @165 DamageBox, @203
/// CollisionBoxBottom); StaticMesh, StaticMesh1 and StaticMesh2 it only hides (@241-323), their collision left on.
const DELAY_OFF: [&str; 4] = ["CollisionBox", "SkeletalMesh", "DamageBox", "CollisionBoxBottom"];

/// `wallDelayed`: the Delay's link (@89-357): the four components' collision off
/// ([`DELAY_OFF`]: CollisionBoxBottom is NoCollision from the start and has no solid; the SkeletalMesh has no physics
/// asset, so no shape), the DestroyMontage's start (`off_at`). A solid of another component (a StaticMesh's) stays.
pub fn wall_delayed(w: &mut World, a: WallActor) {
    let geo = Arc::clone(&wall(w, a).geo);
    let data = Arc::clone(&w.data);
    let names = data.stage.solids.as_deref().unwrap_or(&[]);
    let n = w.stage_run.as_ref().expect("a stage state").solids.len();
    for &id in &geo.solids {
        let Ok(i) = u32::try_from(id) else { continue };
        let comp = names.get(i as usize).and_then(|s| s.name.split('.').nth(1));
        if (i as usize) < n && comp.is_some_and(|c| DELAY_OFF.contains(&c)) {
            stage_solid_enable(w, i, false);
        }
    }
    let tick = w.flow.tick;
    wall_mut(w, a).off_at = tick;
}

/// `wallSolidsEnable`: SetCollisionEnabled on every component of the actor with a solid in
/// the stage data (QueryAndPhysics / NoCollision).
pub fn wall_solids_enable(w: &mut World, a: WallActor, on: bool) {
    let geo = Arc::clone(&wall(w, a).geo);
    let n = w.stage_run.as_ref().expect("a stage state").solids.len();
    for &id in &geo.solids {
        if let Ok(i) = u32::try_from(id)
            && (i as usize) < n
        {
            stage_solid_enable(w, i, on);
        }
    }
}

/// `WallListenerFn`: a bound function, the listener's own handler (no argument: both delegate signatures are empty):
/// the listener (`self`) and the actor that broadcast.
pub type WallListenerFn = fn(&mut World, ActorRef, WallActor);

/// `wallDelegateFns`: a listener class enters its bound functions by
/// `"<Class>.<Function>"` (`"FartBomb_SpawnedActor_C.GroundDestroyed"`), called from its module's `register`; a name
/// entered twice is an error.
pub fn wall_delegate_fns(r: &mut Registry, defs: &[(&'static str, WallListenerFn)]) -> Result<(), RegistryError> {
    for &(k, f) in defs {
        r.wall_listeners.enter(k, f)?;
    }
    Ok(())
}

/// WallBP_C and its children in the build (the SuperStruct chains of the class jsons): the classes a cast to WallBP
/// passes. Every other [`wall_create`] actor is a DestructibleObjectBP child (BatcaveWallDestructible_C).
const WALL_BP: [&str; 4] =
    ["WallBP_C", "M009_DestructiblePlatformBP_C", "M009_DestructiblePlatformBP_Child_C", "DestructiblePlatformBP_C"];

/// `isWallBP`: the actor passes a cast to WallBP (WallBP_C, M009_DestructiblePlatformBP_C
/// and its child, DestructiblePlatformBP_C).
pub fn is_wall_bp(w: &World, a: ActorRef) -> bool {
    let ActorRef::Stage(i) = a else { return false };
    w.stage_run.as_ref().and_then(|r| r.actors.get(i as usize)).is_some_and(|x| WALL_BP.contains(&x.cls.name()))
}

/// `isDestructibleObject`: the actor as a DestructibleObjectBP child the World runs (one
/// holding a [`WallPart`]).
pub fn is_destructible_object(w: &World, a: ActorRef) -> Option<WallActor> {
    let ActorRef::Stage(i) = a else { return None };
    let x = w.stage_run.as_ref()?.actors.get(i as usize)?;
    wall_part(&x.own).map(|_| WallActor(i))
}

/// `groundDestructible`: a trace hit's HitActor cast to DestructibleObjectBP, the placed
/// stage actor whose solid (`StagePlacedData.solids`) the hit is; none for the stage block, its platforms and a
/// spawned actor's body.
pub fn ground_destructible(w: &World, hit: Option<&TraceHit>) -> Option<WallActor> {
    use crate::fighter::body::PlatformRef;
    let hit = hit?;
    let run = w.stage_run.as_ref()?;
    if hit.actor.is_some() {
        return None;
    }
    // the floor's solid (`floor.s.id`): a stage solid's box or polygon; a run-time solid's id is -1, met by no actor
    let id = match hit.floor? {
        PlatformRef::Solid(b) => i64::from(run.solids[run.boxes[b as usize].solid as usize].id),
        PlatformRef::Poly(i) => i64::from(run.solids[i as usize].id),
        _ => return None,
    };
    solid_destructible(w, id)
}

/// The placed stage actor whose solid (`StagePlacedData.solids`) has the data id `id`, cast to DestructibleObjectBP
/// (`groundDestructible`'s search): none for a solid no such actor holds.
pub fn solid_destructible(w: &World, id: i64) -> Option<WallActor> {
    let run = w.stage_run.as_ref()?;
    let placed = w.data.stage.placed.as_deref().unwrap_or(&[]);
    for (i, a) in run.actors.iter().enumerate() {
        let solids = placed.get(a.index as usize).and_then(|p| p.solids.as_deref());
        if solids.is_some_and(|s| s.iter().any(|&x| i64::from(x) == id)) {
            return wall_part(&a.own).map(|_| WallActor(u32::try_from(i).expect("a list index")));
        }
    }
    None
}

/// `bindGroundDestroyed`: the ground trace's bind, ObjectDestroyed += bind(`f`, `listener`)
/// on the actor it met cast to DestructibleObjectBP; a failed cast binds nothing.
pub fn bind_ground_destroyed(w: &mut World, hit: Option<&TraceHit>, listener: ActorRef, f: &'static str) {
    if let Some(a) = ground_destructible(w, hit) {
        wall_bind(w, a, WallDelegate::ObjectDestroyed, listener, f);
    }
}

/// `wallBind`: `Delegate += bind(f, listener)` (EX_AddMulticastDelegate: AddUnique, a pair
/// bound twice is called once).
pub fn wall_bind(w: &mut World, a: WallActor, d: WallDelegate, listener: ActorRef, f: &'static str) {
    if !w.registry.wall_listeners.contains(f) {
        return warn_parts(w, &["wall listener ", f, " is not entered (wall_delegate_fns): not bound"]);
    }
    if d == WallDelegate::StructureDestroyed && !is_wall_bp(w, ActorRef::Stage(a.0)) {
        let cls = w.stage_run.as_ref().map_or("", |r| r.actors[a.0 as usize].cls.name());
        return warn_parts(w, &[cls, " is no WallBP: StructureDestroyed not bound (", f, ")"]);
    }
    let l = WallListener { d, listener, f };
    let ls = wall_mut(w, a).listeners.get_or_insert_with(Vec::new);
    if !ls.contains(&l) {
        ls.push(l);
    }
}

/// `wallUnbind`: `Delegate -= bind(f, listener)` (EX_RemoveMulticastDelegate).
pub fn wall_unbind(w: &mut World, a: WallActor, d: WallDelegate, listener: ActorRef, f: &'static str) {
    let l = WallListener { d, listener, f };
    if let Some(ls) = &mut wall_mut(w, a).listeners {
        ls.retain(|x| *x != l);
    }
}

/// `broadcast`: the functions bound when it begins, in bind order; a listener that is gone
/// (dead) is skipped and dropped.
// Unverified: the order of a multicast's bound functions and its copy of the list are stock UE 4.26
// (FMulticastScriptDelegate::ProcessMulticastDelegate), as a fighter's jump listeners.
fn broadcast(w: &mut World, a: WallActor, d: WallDelegate) {
    let Some(ls) = wall(w, a).listeners.as_ref().filter(|ls| !ls.is_empty()).cloned() else { return };
    if ls.iter().any(|l| actor_dead(w, l.listener)) {
        let kept: Vec<WallListener> = ls.iter().copied().filter(|l| !actor_dead(w, l.listener)).collect();
        wall_mut(w, a).listeners = Some(kept);
    }
    let r: &'static Registry = w.registry;
    for l in ls {
        if l.d == d && !actor_dead(w, l.listener) {
            let f = *r.wall_listeners.get(l.f).expect("a bound wall listener is entered");
            f(w, l.listener, a);
        }
    }
}
