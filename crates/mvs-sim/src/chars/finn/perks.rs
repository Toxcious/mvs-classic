//! Finn's signature perks: Buff_Finn_GemOnCharge ([`GemVars`]),
//! Buff_Finn_GoingOutOfBusiness ([`SaleVars`]), FinnGemBoostBlast_C ([`GemBoostBlast`], a DealerActor),
//! FinnGemBoostUseProjectile_C (a plain projectile).
//!
//! Buff_Finn_GemBoost_C has no graph: its effects are FinnDashThrownTarget's CheckHasBuff branches (@1364
//! GemBoostPerkActive, the cost @2248, the blast @7321 and the two projectiles @7726 / @8010: gem.rs), so it has no
//! entry here. Perk_Finn_GemBoost is IsUnavailableOffline (not on the port's perk page). State: the buffs' own
//! (`ext`) and the blast actor's.
//!
//! Buff_Finn_GemOnCharge: ReceiveBeginPlay @26: the owner's HitBoxHandler.HitEnemyPlayer -> OnEnemyHit (the HitEnemy
//! signal, behind the hit). OnEnemyHit @749: LastHitInformation.AttackID != LastHitAttackID @828, then
//! LastHitAttackID = it; GetChargeValue(4) @1070; HighFivePercent == 1 and IsMovingOnGround and AttackAnimation in
//! HighFiveMontages @1453-1654: the neutral direction and speed; else GetChargeValue == 1 and AttackAnimation in
//! GroundedAttackMontages @1927-2068: the direction and speed by its index (0 Forward, 1 Up, 2 Down, 3 Neutral)
//! @2186 / @2405. Then NextGemFree = true @1722, the owner's CompSpawnActor(FinnDashThrownTarget_C, not attached,
//! GemTransform, LaunchDirection, LaunchSpeed, 4000) @1804 (gem.rs `spawn_gem`), NextGemFree = false @10. The gem's
//! BeginPlay reads NextGemFree off this buff (FinnDashThrownTarget @1812-1891: the throw is free).
// Deviation: LastHitInformation's AttackID and AttackAnimation are taken from the owner's active hit box whose hit
// set holds the victim (`active_hits`: attack_id, attack_name), the last one in the map; a hit no active hit box holds
// (a packet of a direct hit) is AttackID 0, AttackAnimation None (the packet's). The dealt pass hands the condition
// no AttackID (the port's packet is the hit box's data).
//
// Buff_Finn_GoingOutOfBusiness: ReceiveBeginPlay @1091: DamageManager.HitByEnemy -> OnHitByEnemy, the game mode's
// PlayerKnockedOut -> OnPlayerRungOut, Team Index = the owner's Destructible TeamIndex @1725. OnHitByEnemy @1790: not
// Activated and CurrentDamage >= DamageToActivate (100) @2010: Activated = true, SetSale(true), both delegates
// unbound (@2310, @2470): the sale stays on. OnPlayerRungOut @3314: not Activated -> Delay(0.0) @15 (-> @70: the
// loser index is the event's variable, so a second call inside the Delay changes the loser the resume reads; the
// Delay is not started again). @70: the loser's pawn is not the owner @363, BPI_IsDead @688, of Team Index @898:
// SetSale(true) @946, Delay(Duration 10) @961 (-> @1036: not Activated -> SetSale(false) @1075; a second start
// inside it is ignored). SetSale @2512: SaleActive = it; true: the pop text "Going Out Of Business!" (@2744-3267).
// So an ally's ring-out puts the shop on sale for 10 s and 100 damage taken puts it on sale for good.
// Deviation: BPI_IsDead is `dead > 0` one frame after the ring-out (the blast check's kill comes behind the signal).
//
// FinnGemBoostBlast (a SpawnedActorParent child): the parent's BeginPlay (HitBoxOn? true: its HitBox sphere, radius
// 400, HitBoxDamage 4 / 750 / 5, DirectionalKnockback, HitEachActorOnlyOnce), SetTeamColor (drawing), Delay(0.5) @358
// -> ReleaseToPool @15 (not poolable: destroyed). Spawned by the gem (@7321, a world transform at the gem).
// Deviation: its sphere against the other spawned actors is not tested (no DestroyProjectiles in its HitBoxDamage),
// as Rick's air flap.
//
// FinnGemBoostUseProjectile (a ParentProjectilePhysics child; its graph sets the team colour alone): the parent's
// whole, an empty Projectile kind. Spawned by the gem (@7726 / @8010: gem.rs).

use indexmap::IndexSet;

use super::gem::spawn_gem;
use super::state::finn_of;
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, WorldActor, WorldActorClass};
use crate::combat::BuffClass;
use crate::combat::HitPacket;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{BuffExt, BuffRef, buff_of, inst_view};
use crate::combat::elem::{charge_pct, instigator_charge};
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::phases::{alive, give_vbuf, los_blocked, no_hit, victim_pose};
use crate::combat::spawned_hits::class_rows;
use crate::events::{Event, PopKind};
use crate::fighter::common::{actor_data, latent_frames, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::moves::V2;
use crate::fighter::{FighterId, grounded, invulnerable};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{
    ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks, SizeHooks, WorldActorHooks,
};
use crate::registry::props::{buff, buff_mut, flag, num, num_in, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit_rounded, new_blast_bounds, spawn_box};
use crate::world::pawns::match_fighters;
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::{add_actor, comp_spawn_actor, new_world_actor};
use mvs_data::value::{Props, Value};

/// Buff_Finn_GemOnCharge's class.
pub const GEM_ON_CHARGE: &str = "Buff_Finn_GemOnCharge_C";

/// Buff_Finn_GoingOutOfBusiness's class.
pub const GOING_OUT_OF_BUSINESS: &str = "Buff_Finn_GoingOutOfBusiness_C";

/// Buff_Finn_GemBoost's class.
pub const GEM_BOOST: &str = "Buff_Finn_GemBoost_C";

/// FinnGemBoostBlast's class.
pub const GEM_BOOST_BLAST: &str = "FinnGemBoostBlast_C";

/// FinnGemBoostUseProjectile's class.
pub const GEM_BOOST_PROJECTILE: &str = "FinnGemBoostUseProjectile_C";

const GEM_ON_CHARGE_N: Name = name!("Buff_Finn_GemOnCharge_C");
const GOING_OUT_OF_BUSINESS_N: Name = name!("Buff_Finn_GoingOutOfBusiness_C");

/// Buff_Finn_GemOnCharge_C's extension (`{ lastHitAttackId, nextGemFree }`).
#[derive(State, Debug, Default)]
pub struct GemVars {
    /// LastHitAttackID (the class's 0).
    pub last_hit_attack_id: i32,
    /// NextGemFree.
    pub next_gem_free: bool,
}

/// Buff_Finn_GoingOutOfBusiness_C's extension (`SaleVars`).
#[derive(State, Debug, Default)]
pub struct SaleVars {
    /// Activated.
    pub activated: bool,
    /// SaleActive.
    pub sale_active: bool,
    /// Team Index.
    pub team: i32,
    /// The event's LoserIndex, as the pawn it names.
    pub loser: Option<FighterId>,
    /// Delay(0.0) @15: frames left, 0 none pending.
    pub ko_wait: Real,
    /// Delay(Duration) @961: frames left, 0 none pending.
    pub sale_wait: Real,
}

/// `GemBoostBlast` (FinnGemBoostBlast_C).
#[derive(State, Debug)]
pub struct GemBoostBlast {
    pub hit: HitPacket,
    pub attack_id: i32,
    pub facing: Real,
    /// HitBoxOn?.
    pub hit_on: bool,
    /// The HitBox sphere's radius (the root's scale is taken at the read).
    pub r: Real,
    /// HitEachActorOnlyOnce, by fighter id.
    pub hit_set: IndexSet<i32>,
    /// The age Delay(0.5) fires at.
    pub off_at: Real,
    /// Ticks, the spawn frame counted.
    pub age: i32,
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
    pub x: Real,
    pub y: Real,
}

impl Default for GemBoostBlast {
    fn default() -> Self {
        GemBoostBlast {
            hit: HitPacket::default(),
            attack_id: 0,
            facing: 0.0,
            hit_on: false,
            r: 0.0,
            hit_set: IndexSet::new(),
            off_at: 0.0,
            age: 0,
            blast_at: 0.1,
            blast_bounds: new_blast_bounds(),
            x: 0.0,
            y: 0.0,
        }
    }
}

/// The FinnGemBoostBlast_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &GemBoostBlast) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The FinnGemBoostBlast_C as a dealer (`DealerActor`): its place, facing and AttackID.
pub fn dealer_parts(w: &World, a: &WorldActor, s: &GemBoostBlast) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The FinnGemBoostBlast_C's HitBoxDamage packet (`hit`).
pub fn hit(s: &GemBoostBlast) -> &HitPacket {
    &s.hit
}

// ---- Buff_Finn_GemOnCharge

fn gem_vars(w: &mut World, b: BuffRef) -> Option<&mut GemVars> {
    let x = buff_mut(w, b)?;
    if !matches!(x.ext, BuffExt::GemOnCharge(_)) {
        x.ext = BuffExt::GemOnCharge(GemVars::default()); // `b.ext ??= { lastHitAttackId: 0, nextGemFree: false }`
    }
    match &mut x.ext {
        BuffExt::GemOnCharge(v) => Some(v),
        _ => None,
    }
}

/// The gem's BeginPlay read (FinnDashThrownTarget @1642-1891): the owner's Buff_Finn_GemOnCharge and its NextGemFree.
pub fn next_gem_free(w: &mut World, f: FighterId) -> bool {
    let Some(b) = buff_of(w, f, GEM_ON_CHARGE_N) else { return false };
    gem_vars(w, b).is_some_and(|v| v.next_gem_free)
}

/// LastHitInformation's AttackID and AttackAnimation for the hit on `vic` (see above).
fn last_hit_of(w: &World, f: FighterId, vic: FighterId) -> (i32, Name) {
    let vid = w.fighter(vic).id;
    let (mut id, mut anim) = (0, name!("None"));
    for h in w.fighter(f).montages.active_hits.values() {
        if !h.hit_set.contains(&vid) {
            continue;
        }
        id = h.attack_id;
        anim = h.attack_name;
    }
    (id, anim)
}

const DIRS: [&str; 4] = ["Forward", "Up", "Down", "Neutral"];

fn on_enemy_hit(w: &mut World, f: FighterId, b: BuffRef, vic: FighterId) {
    let (id, anim) = last_hit_of(w, f, vic);
    let Some(v) = gem_vars(w, b) else { return };
    if id == v.last_hit_attack_id {
        return; // @828
    }
    v.last_hit_attack_id = id;
    let anim = w.names.str(anim).to_owned();
    let charge = instigator_charge(w, f).map_or(0.0, |c| c.pct); // GetChargeValue(4) @1070
    let high_five = finn_of(w, f).is_some_and(|s| s.momentum.high_five_pct == 1.0) && grounded(w, f);
    let (dir, speed, transform) = {
        let d = inst_view(w, b);
        if !d.some() {
            return;
        }
        let listed = |k: &str| arr_pos(d.get(k), &anim);
        let dir_of = |name: &str| {
            let dir = d.obj(&format!("{name}AttackDirection"));
            (
                dir.and_then(|o| num(o, "Y")).unwrap_or(0.0),
                dir.and_then(|o| num(o, "Z")).unwrap_or(0.0),
                d.num(&format!("{name}AttackSpeed")).unwrap_or(0.0),
            )
        };
        let (dy, dz, speed) = if high_five && listed("HighFiveMontages").is_some() {
            dir_of("Neutral")
        } else {
            let i = if charge == 1.0 { listed("GroundedAttackMontages") } else { None };
            let Some(i) = i else { return }; // @2068
            // An index past the four directions reads `undefinedAttackDirection`: no direction, speed 0.
            DIRS.get(i).map_or((0.0, 0.0, 0.0), |n| dir_of(n))
        };
        ((dy, dz), speed, d.obj("GemTransform").cloned().unwrap_or_default())
    };
    if let Some(v) = gem_vars(w, b) {
        v.next_gem_free = true; // @1722
    }
    // GemTransform: the owner-relative transform, socket None
    let mut np = Props::new();
    np.insert("Transforms".into(), Value::Obj(transform));
    np.insert("Socket".into(), Value::Str("None".into()));
    let at = spawn_place(w, f, &np);
    spawn_gem(w, f, at.x, at.y, dir, speed, 4000.0, true); // @1804
    if let Some(v) = gem_vars(w, b) {
        v.next_gem_free = false; // @10
    }
}

/// The index of the string `s` in the data array `a` (`Array.indexOf`).
fn arr_pos(a: Option<&Value>, s: &str) -> Option<usize> {
    a.and_then(Value::as_arr)?.iter().position(|v| v.as_str() == Some(s))
}

fn gem_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    gem_vars(w, b);
}

#[allow(clippy::too_many_arguments, reason = "the hook's shape")]
fn gem_dealt(
    _w: &mut World,
    _v: ActorRef,
    _o: FighterId,
    _d: FighterId,
    _h: &HitPacket,
    _b: BuffRef,
    _c: bool,
) -> bool {
    true // DealtDamageModCondition: LastHitInformation (last_hit_of) and the parent's true
}

fn gem_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s == BuffSignal::HitEnemy
        && let Some(v) = a.victim
    {
        on_enemy_hit(w, f, b, v);
    }
    false
}

// ---- Buff_Finn_GoingOutOfBusiness

fn sale(w: &World, b: BuffRef) -> Option<&SaleVars> {
    match &buff(w, b)?.ext {
        BuffExt::GoingOutOfBusiness(v) => Some(v),
        _ => None,
    }
}

fn sale_mut(w: &mut World, b: BuffRef) -> Option<&mut SaleVars> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::GoingOutOfBusiness(v) => Some(v),
        _ => None,
    }
}

/// SetSale @2512.
fn set_sale(w: &mut World, f: FighterId, b: BuffRef, on: bool) {
    if let Some(v) = sale_mut(w, b) {
        v.sale_active = on;
    }
    if on {
        let fighter = w.fighter(f).id;
        w.fighter_mut(f).events.push(Event::PopText {
            fighter,
            kind: PopKind::FinnSale,
            stacks: 0.0,
            gain: None,
            params: None,
        }); // @3267
    }
}

/// IsSaleActive, for Finn_Shop (@5755-6018): the owner's Buff_Finn_GoingOutOfBusiness and its SaleActive.
pub fn is_sale_active(w: &World, f: FighterId) -> bool {
    buff_of(w, f, GOING_OUT_OF_BUSINESS_N).and_then(|b| sale(w, b)).is_some_and(|v| v.sale_active)
}

fn sale_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    let team = w.fighter(f).team.team;
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::GoingOutOfBusiness(SaleVars { team, ..SaleVars::default() });
    }
}

fn sale_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if sale(w, b).is_none_or(|v| v.activated) {
        return false; // @1790 / @3314 (and unbound after the activation)
    }
    if s == BuffSignal::HitByEnemy {
        let to = inst_view(w, b).num("DamageToActivate").unwrap_or(0.0);
        if w.fighter(f).stats.damage >= to {
            if let Some(v) = sale_mut(w, b) {
                v.activated = true; // @2080
            }
            set_sale(w, f, b, true);
        }
    } else if s == BuffSignal::PlayerKnockedOut
        && let Some(victim) = a.victim
        && let Some(v) = sale_mut(w, b)
    {
        v.loser = Some(victim);
        if v.ko_wait == 0.0 {
            v.ko_wait = latent_frames(0.0);
        }
    }
    false
}

fn sale_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(v) = sale_mut(w, b) else { return false };
    // Delay(Duration) -> @1036.
    let mut off = false;
    if v.sale_wait > 0.0 {
        v.sale_wait -= 1.0;
        off = v.sale_wait == 0.0 && !v.activated;
    }
    if off {
        set_sale(w, f, b, false);
    }
    // Delay(0.0) -> @70.
    let Some(v) = sale_mut(w, b) else { return false };
    if v.ko_wait > 0.0 {
        v.ko_wait -= 1.0;
        if v.ko_wait == 0.0 {
            let (loser, team) = (v.loser, v.team);
            if let Some(l) = loser
                && l != f
                && w.fighter(l).life.dead > 0.0
                && w.fighter(l).team.team == team
            {
                set_sale(w, f, b, true); // @946
                let duration = inst_view(w, b).num("Duration").unwrap_or(0.0);
                if let Some(v) = sale_mut(w, b)
                    && v.sale_wait == 0.0
                {
                    v.sale_wait = latent_frames(duration); // @961
                }
            }
        }
    }
    false
}

// ---- FinnGemBoostBlast

fn blast_of(w: &World, id: ActorId) -> Option<&GemBoostBlast> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::GemBoostBlast(s) => Some(s),
        _ => None,
    }
}

fn blast_mut(w: &mut World, id: ActorId) -> Option<&mut GemBoostBlast> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::GemBoostBlast(s) => Some(s),
        _ => None,
    }
}

/// The HitBox sphere where it is now: the hit tests and the drawing.
fn blast_shape(w: &World, id: ActorId) -> Option<WorldSphere> {
    let a = w.actors.actors.get(id)?;
    let s = blast_of(w, id)?;
    Some(WorldSphere { x: s.x, y: s.y, r: s.r * a.size_scale.unwrap_or(1.0), ..WorldSphere::default() })
}

/// `hitShapes`: the HitBox (object type HitBox) while it is on.
fn blast_hit_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    let Some(s) = blast_of(w, id) else { return false };
    if !s.hit_on || w.actors.actors.get(id).is_none_or(|a| a.dead) {
        return false;
    }
    match blast_shape(w, id) {
        Some(sp) => {
            out.push(sp);
            true
        }
        None => false,
    }
}

fn blast_step(w: &mut World, id: ActorId) {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(s) = blast_mut(w, id) else { return };
    s.age += 1;
    let dead = if crate::real(s.age) == s.off_at {
        true // Delay(0.5) -> ReleaseToPool
    } else {
        blast_timer_exit_rounded(doe, &mut s.blast_at, &s.blast_bounds, crate::real(s.age - 1), s.x, s.y)
    };
    if dead && let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// The HitBox sphere against the enemies' hurt shapes, as Rick's air flap.
fn blast_hits(w: &mut World, id: ActorId) {
    let me = ActorRef::Actor(id);
    let Some(s) = blast_of(w, id) else { return };
    if !s.hit_on {
        return;
    }
    let (attack_id, hit, sx, sy) = (s.attack_id, s.hit.clone(), s.x, s.y);
    let Some(shape) = blast_shape(w, id) else { return };
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let team = w.fighter(f).team.team;
    for vic in match_fighters(w) {
        let vid = w.fighter(vic).id;
        let Some(s) = blast_of(w, id) else { return };
        if w.fighter(vic).team.team == team || !alive(w, vic) || no_hit(w, vic) || s.hit_set.contains(&vid) {
            continue;
        }
        let Some(vp) = victim_pose(w, vic) else { continue };
        let met = crate::combat::hit::hurt_hit(w, vic, std::slice::from_ref(&shape), &vp).is_some();
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        let (vx, vy) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
        if hit.do_los_check.unwrap_or(false) && los_blocked(w, sx, sy, vx, vy) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        if hit.hit_each_actor_only_once.unwrap_or(false)
            && let Some(s) = blast_mut(w, id)
        {
            s.hit_set.insert(vid);
        }
        let rows = class_rows(w, me);
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &rows, V2 { x: sx, y: sy }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &d, vic);
        }
    }
}

/// `spawnGemBoostBlast`: FinnDashThrownTarget's CompSpawnActor(FinnGemBoostBlast_C, ..., world transform at (x, y))
/// @7321, for gem.rs: the spawner `f`, the facing its transform gives. GrantBuffsFromInstigator and the list, the
/// parent's BeginPlay blast-box loop, then AfterSpawnedActor.
pub fn spawn_gem_boost_blast(w: &mut World, f: FighterId, x: Real, y: Real, facing: Real) -> Option<ActorId> {
    let data = w.data.clone();
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(f), name!("FinnGemBoostBlast_C")) else {
        warn_text(w, &format!("spawned actor not exported ({GEM_BOOST_BLAST})"));
        return None;
    };
    let none = Props::new();
    let mut s = GemBoostBlast {
        hit: HitPacket::from_props(&w.names, obj(&d.p, "HitBoxDamage").unwrap_or(&none), None),
        attack_id: w.fighter(f).decay.attack_count,
        facing,
        hit_on: flag(&d.p, "HitBoxOn?"),
        r: (obj(&d.components, "HitBox").and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0)
            * num_in(&d.p, "HitBoxScale", "X").unwrap_or(1.0)),
        off_at: latent_frames(0.5),
        x,
        y,
        ..GemBoostBlast::default()
    };
    let sb = spawn_box(w);
    let dead = blast_box_loop(
        &sb,
        &mut s.x,
        &mut s.y,
        None,
        flag(&d.p, "ForceSpawnPositionInsideBlastBox"),
        &mut s.blast_bounds,
        flag(&d.p, "DestroyOnExitedArena?"),
    );
    let actor = new_world_actor(
        w,
        name!("FinnGemBoostBlast_C"),
        WorldActorClass::FinnGemBoostBlast,
        f,
        dead,
        WorldActorState::GemBoostBlast(s),
    );
    let id = w.actors.actors.insert(actor);
    let charge = instigator_charge(w, f);
    if dead {
        add_actor(w, id, charge, true); // listed, dead: no AfterSpawnedActor
    } else {
        comp_spawn_actor(w, id, charge, f);
    }
    Some(id)
}

/// This file's registry entries (the two perk buffs, FinnGemBoostBlast_C, FinnGemBoostUseProjectile_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        GEM_ON_CHARGE,
        BuffEntry::new(
            BuffClass::BuffFinnGemOnCharge,
            BuffHooks { begin: Some(gem_begin), dealt_cond: Some(gem_dealt), on: Some(gem_on), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter(
        GOING_OUT_OF_BUSINESS,
        BuffEntry::new(
            BuffClass::BuffFinnGoingOutOfBusiness,
            BuffHooks { begin: Some(sale_begin), on: Some(sale_on), tick: Some(sale_tick), ..BuffHooks::default() },
        ),
    )?;
    r.actors.enter(
        GEM_BOOST_BLAST,
        ActorEntry::new(
            ActorClass::World(WorldActorClass::FinnGemBoostBlast),
            ActorHooks::World(WorldActorHooks {
                size: Some(SizeHooks::default()),
                hit_shapes: Some(blast_hit_shapes),
                step: Some(blast_step),
                hits: Some(blast_hits),
                ..WorldActorHooks::default()
            }),
        ),
    )?;
    r.actors.enter(
        GEM_BOOST_PROJECTILE,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::FinnGemBoostUseProjectile),
            ActorHooks::Projectile(ProjectileHooks::default()),
        ),
    )
}
