//! What the World does with Morty's typed-list actors: the marker's activation, tick
//! (its recall) and destroy, the ship's tick, the mortar's seismic boom and its hurt box, the portal's tick and
//! destroy; the marker and the mortar aim as dealers. The actors' own state and ticks are in marker.rs, ship.rs,
//! mortar.rs and accelerator.rs; the world keeps their lists and makes the calls.
//!
//! The shared code reaches the marker and the mortar aim as dealers (`combat::hits` dealer: [`dealer`]).

use super::marker::{self, MarkerEnd};
use super::mortar;
use super::ship::{self, ShipEvent};
use crate::Real;
use crate::actors::AcceleratorId;
use crate::actors::ActorRef;
use crate::actors::MarkerId;
use crate::actors::MortarId;
use crate::actors::ProjectileClass;
use crate::actors::ShipId;
use crate::actors::projectile::{MakeOpts, ProjectileExt, hurt_y, make_projectile, speed_buff};
use crate::combat::HitPacket;
use crate::combat::elem::{charge_buff, charge_pct};
use crate::combat::hits::{
    DamageCall, Dealer, actor_hit_enemy, ally_hit, apply_projectile_hit, do_damage, hit_buffs_ally, hit_buffs_call,
    projectile_blocks, projectile_mirror,
};
use crate::combat::packet::{DataEnum, DataVec, OnHitBuffRow};
use crate::events::{Event, SoundKey};
use crate::fighter::anim::start_montage;
use crate::fighter::common::{actor_def, owner_team, warn_once};
use crate::fighter::control::{
    actor_spawn_ability, attack_dodged, dodge_over, enable_invulnerability, trigger_ability,
};
use crate::fighter::moves::{Impulse, V2, net_impulse, recall_move};
use crate::fighter::traces::solids::{QUERY_UNKNOWN, QUERY_WORLDSTATIC};
use crate::fighter::traces::{OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PROJECTILE_DESTRUCTIBLE, box_overlap_actors};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, facing, invulnerable, no_collision};
use crate::math::ue::{KINDA_SMALL_NUMBER, normal, size2d};
use crate::math::{max, min};
use crate::name;
use crate::registry::props::{flag, num, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{blast_box_loop, blast_timer_exit, spawn_box};
use crate::world::pawns::match_fighters;
use crate::world::stage_query::static_shapes;
use mvs_anim::montage::PROXY_NONE;
use mvs_data::value::Props;

fn mk(w: &World, m: MarkerId) -> &crate::actors::lists::Marker {
    w.actors.markers.get(m).expect("a live marker")
}

fn fid(w: &World, f: FighterId) -> i32 {
    w.fighter(f).id
}

fn is_dead(w: &World, f: FighterId) -> bool {
    w.fighter(f).life.dead != 0.0
}

fn team(w: &World, f: FighterId) -> i32 {
    w.fighter(f).team.team
}

fn sound(w: &mut World, f: FighterId, ev: crate::names::Name, at: Option<(Real, Real)>) {
    let fighter = fid(w, f);
    let (x, y) = (at.map(|a| a.0), at.map(|a| a.1));
    w.events.push(Event::Sound { fighter, ev, x, y, hit: None, pooled: false, key: None });
}

/// The class's `On Hit Buffs` rows (`p['On Hit Buffs']?.OnHitBuffs ?? []`).
fn on_hit_rows(w: &World, p: &Props) -> Vec<OnHitBuffRow> {
    OnHitBuffRow::rows(&w.names, obj(p, "On Hit Buffs").and_then(|o| o.get("OnHitBuffs")))
}

// ---- the marker's calls on its owner

/// `recallBegin`: C019Marker ActivateObject's calls on the owner (the rules are at the fighter's `capsule_off` and
/// `cant_die`).
pub fn recall_begin(w: &mut World, f: FighterId) {
    enable_invulnerability(w, f, true, false);
    trigger_ability(w, f, 2);
    enable_invulnerability(w, f, true, false);
    trigger_ability(w, f, 2);
    w.fighter_mut(f).capsule.capsule_off = true;
    w.fighter_mut(f).life.cant_die = true;
}

/// `recallEnd`: MoveAlongPath finished (ubergraph 15-1083): BPI_UpdateCantDieLIst(owner, false) on every blast box
/// (611); MoveStunBuff.K2_DestroyActor, whose ReceiveEndPlay calls NetImpulse((0, 10, 1000), XY and Z override,
/// type 0); K2_SetActorLocation(the safe location, teleport); LaunchCharacter((0, 0, 1), XY and Z override) (stock:
/// the pending launch of the next CMC tick, whose OnMovementUpdated then stages the NetImpulse for the tick after);
/// the capsule's collision back on (QueryAndPhysics); BPI_SetInvulnerability(false).
pub fn recall_end(w: &mut World, f: FighterId, x: Real, actor_y: Real) {
    w.fighter_mut(f).life.cant_die = false;
    net_impulse(w, f, -10.0, 1000.0, true, true, 0.0, false, 0.0);
    recall_move(w, f, x, actor_y);
    w.fighter_mut(f).moves.impulse = Some(Impulse { vx: 0.0, vy: 1.0, xy: true, z: true, kind: 0, d: None });
    w.fighter_mut(f).capsule.capsule_off = false;
    enable_invulnerability(w, f, false, false);
}

/// `markerEnded`: the marker's ReceiveEndPlay (ubergraph 6192), in its order: BPI_SetInvulnerability(false) (6354),
/// then the last pushed branch (7640): ToggleNavigationCollision(true) and, with MoveStunBuff still valid (7655), its
/// K2_DestroyActor (7694), whose ReceiveEndPlay sends NetImpulse(Impulse (0, 10, 1000), XY and Z override, type 0)
/// (Buff_GarnetStunnedMove 229). The buff lives from ActivateObject (9436) to the timeline's end, which destroys it
/// first (673): the span the capsule's collision is off, so that flag stands for it here. There is no teleport and
/// no LaunchCharacter on this path, and the can't-die entry stays.
/// The second BPI_SetInvulnerability(true) of the recall is not undone either: the EnableInvulnerability(false) at
/// 7257 is for AttachedFighter, a variable of the class that no graph writes (read at 7135-7257 and 7460-7503 only;
/// not exposed on spawn, no default), so it is never valid and the count stays at 1.
// Deviation: Buff_GarnetStunnedMove (the recall's one caller, which never calls its StartMoveServer): its graph's
// reached part, ReceiveEndPlay's NetImpulse, is here, its life is the fighter's `capsule_off`, not a buff actor.
pub fn marker_ended(w: &mut World, f: FighterId) {
    enable_invulnerability(w, f, false, false);
    let buff = w.fighter(f).capsule.capsule_off;
    w.fighter_mut(f).capsule.capsule_off = false;
    if buff {
        net_impulse(w, f, -10.0, 1000.0, true, true, 0.0, false, 0.0);
    }
}

/// `activateMarker`: C019Marker ActivateObject (ubergraph 8719): nothing for a dead owner or a second call; IsActive,
/// sfx_cha_C019_teleport_recall at the capsule, the route's last update and the timeline (`marker::activate`), then
/// the owner's side (`recall_begin`).
pub fn activate_marker(w: &mut World, m: MarkerId) {
    let x = mk(w, m);
    let (f, active, at) = (x.owner, x.active, (x.x, x.y));
    if is_dead(w, f) || active {
        return;
    }
    sound(w, f, name!("sfx_cha_C019_teleport_recall"), Some(at));
    marker::activate(w, m);
    recall_begin(w, f);
}

/// `markerTick`: the marker's actor tick. Delay(3.5) on a marker that is not active -> ActivateMontage:
/// ActivateObject, then PlayMontage(C019_Attack_Special_DownTeleport_Montage) on the instigator's mesh. The
/// timeline's end (ubergraph 15-1163): the owner's side (`recall_end`) at CheckTestLocations' location, EndVisuals,
/// DoKnockUpEnd and K2_DestroyActor.
/// DoKnockUpEnd (ubergraph 19870, 2620-3255): SphereOverlapActors(the owner's location, 360) for BP_FighterCharacter
/// actors; each one with a Comp_Actor_Destructible gets pfgDoDamage('Hit Box Damage' with ForceKnockback = another
/// team, 'On Hit Buffs') from the marker's own handler and a KnockupVFX. The hit has KnockBackMirror (pfgDoDamage
/// 11535): after the facing flip its X turns around for a victim behind the handler's owner, here the marker actor
/// at its spawn point.
/// The sphere's object types [3, 2, 8, 11, 16] are PhysicsBody, Pawn, DamageDestructible, ProjectileDestructible and
/// ItemDestructible (DefaultEngine.ini's channel order); of a fighter's bodies only the root capsule and the mesh are
/// Pawn objects.
/// pfgDoDamage for each (Comp_Pawn_HitBoxHandler): a dead actor is left (206); the ApplyBuffSelf? loop (842);
/// IgnoreActor, HitEachActorOnlyOnce (the marker's own handler, a fresh AttackTag list), HitOnlyOneFighter (off) and
/// DoLOSCheck (off) pass; an invulnerable actor leaves at 4608 for the dodge branch (7381), which needs
/// CanBeHitBy(the marker's team) and BaseKnockBack > 0: an enemy dodges, an invulnerable ally gets nothing; only then
/// the ally loop (5931) and the hit.
// Deviation: the overlap is the circle against the fighter's root capsule in the play plane (the mesh's physics
// bodies are not looked at), in fighter order (the query's own order is PhysX's); the IsInvulnerableToProjectiles /
// IsReflectingProjectiles test (4032-4598) is not ported.
pub fn marker_tick(w: &mut World, m: MarkerId) {
    let mut why = marker::step(w, m);
    let f = mk(w, m).owner;
    match why {
        Some(MarkerEnd::Auto) => {
            activate_marker(w, m);
            start_montage(w, f, name!("C019_Attack_Special_DownTeleport_Montage"), &PROXY_NONE, None);
            // The Delay(3.5) fires in the marker's tick: the timeline PlayFromStart starts there updates in this frame.
            if mk(w, m).moving && marker::timeline_tick(w, m) {
                why = Some(MarkerEnd::Done);
            }
        }
        Some(MarkerEnd::Life | MarkerEnd::Blast) => destroy_marker(w, m),
        _ => {}
    }
    if why != Some(MarkerEnd::Done) {
        return;
    }
    let gs = static_shapes(w, QUERY_WORLDSTATIC, name!("C019 marker CheckTestLocations"));
    let fighters = match_fighters(w);
    let q = marker::safe_location(w, m, gs.view(), &fighters);
    recall_end(w, f, q.x, q.y);
    let p = actor_def(w, ActorRef::Fighter(f), mk(w, m).cls).map(|d| d.p.clone()).unwrap_or_default();
    let buffs = on_hit_rows(w, &p);
    let me = ActorRef::Marker(m);
    for vic in match_fighters(w) {
        if is_dead(w, vic) {
            continue;
        }
        let half = capsule_half(w, vic) - cap_radius(w, vic);
        let (vy, fy) = (actor_y(w, vic), actor_y(w, f));
        let cy = max(vy - half, min(fy, vy + half));
        if size2d([w.fighter(vic).body.x - w.fighter(f).body.x, cy - fy]) > 360.0 + cap_radius(w, vic) {
            continue;
        }
        // KnockupVFX follows every pfgDoDamage call, the owner's own included.
        let id = fid(w, f);
        w.events.push(Event::MarkerBurst { fighter: id });
        // pfgDoDamage's first loop (842): an ApplyBuffSelf? entry that does not need the hit goes to the instigator on
        // every call (Buff_HealOverTime has CanOverride: each call replaces the last one and its BeginPlay heals at
        // once). The hit's own loop (5931): ApplyBuffHit? && ApplyBuffAlly? to a hit actor of the instigator's team
        // that is not the instigator, with Round(StackCount x the charge multiplier). `apply_projectile_hit` below
        // runs the first for its call.
        let hits = team(w, vic) != team(w, f) && !crate::combat::phases::no_hit_by(w, f, vic) && !invulnerable(w, vic);
        if !hits {
            hit_buffs_call(w, f, &buffs);
        }
        if !invulnerable(w, vic) {
            let mult = charge_buff(w, me); // the marker's BuffStackMultiplier
            hit_buffs_ally(w, f, vic, &buffs, mult);
            // NotifyAllyOfHit at the call's end (@21344): a team mate, not Morty himself (DamageAndLaunch between
            // refuses him: CanBeHitBy @718).
            if vic != f && team(w, vic) == team(w, f) {
                let mut hit = mk(w, m).hit.clone();
                hit.force_knockback = Some(true);
                let d = dealer(w, me);
                ally_hit(w, &d, vic, &hit);
            }
        }
        if team(w, vic) == team(w, f) || crate::combat::phases::no_hit_by(w, f, vic) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        let x = mk(w, m);
        let (attack_id, bkb, mfacing) = (x.attack_id, x.hit.base_knock_back.unwrap_or(0.0), x.facing);
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, bkb, tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        // The mirror's side (`projectile_mirror`): C019Marker writes no HitBoxHandlerFacingLeft, so its handler faces
        // right (the component's default) whatever the marker's facing.
        let hit = mk(w, m).hit.clone();
        let (dx, dy) = match &hit.knock_back_direction_vector {
            Some(v) => (v.x.unwrap_or(Real::NAN), v.y.unwrap_or(Real::NAN)),
            None => (1.0, 0.0),
        };
        let mut forced = hit.clone();
        forced.force_knockback = Some(true);
        let d = dealer(w, me);
        let (vx, vby) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
        let mx = projectile_mirror(w, &d, &hit, Some(vx), dx * mfacing);
        let pct = charge_pct(w, me);
        if apply_projectile_hit(w, &d, vic, &forced, &buffs, V2 { x: vx, y: vby }, Some(V2 { x: mx, y: dy }), &[], pct)
        {
            actor_hit_enemy(w, &d, vic);
        }
    }
    destroy_marker(w, m);
}

/// `damageMarker`: C019Marker ReceiveAnyDamage (ubergraph 8400): CurrentDamage += Damage; at MaxHealth (15) or more
/// K2_DestroyActor (ReceiveEndPlay's sound and owner calls), then sfx_cha_C019_teleport_end at the Capsule again.
/// What reaches it: the Capsule is a WorldDynamic body that overlaps Pawn only (the HitBox and HurtBox channels
/// default to Ignore, and the HitBox profile ignores WorldDynamic), so no hit box or projectile overlaps it; a sphere
/// query whose object types hold WorldDynamic finds it (C019_CookGrenade Detonate), and pfgDoDamage then takes the
/// fighter branch (its Comp_Actor_Destructible has IsProjectile false): DamageAndLaunch -> ApplyDamage.
// `!(a >= b)` is true for a NaN.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn damage_marker(w: &mut World, m: MarkerId, damage: Real) {
    let x = w.actors.markers.get_mut(m).expect("a live marker");
    x.current_damage += damage;
    let (cd, owner, cls, at) = (x.current_damage, x.owner, x.cls, (x.x, x.y));
    let max_health = actor_def(w, ActorRef::Fighter(owner), cls).and_then(|d| num(&d.p, "MaxHealth")).unwrap_or(0.0);
    if !(cd >= max_health) {
        return;
    }
    destroy_marker(w, m);
    sound(w, owner, name!("sfx_cha_C019_teleport_end"), Some(at));
}

/// `destroyMarker`: the marker's ReceiveEndPlay (ubergraph 6192): the owner's collision and invulnerability calls
/// and sfx_cha_C019_teleport_end at the capsule (the owner's calls not in the Lab reset's pass: an old pawn there).
pub fn destroy_marker(w: &mut World, m: MarkerId) {
    let x = w.actors.markers.get_mut(m).expect("a live marker");
    x.dead = true;
    let (owner, at) = (x.owner, (x.x, x.y));
    if !crate::world::flow::reset_pass(w) {
        marker_ended(w, owner);
    }
    sound(w, owner, name!("sfx_cha_C019_teleport_end"), Some(at));
}

/// `shipTick`: C019_Ship's actor tick (`ship::step`) and what its Delays do: sfx_cha_C019_spaceship_appear at the
/// actor with sfx_cha_C019_spaceship_loop, sfx_cha_C019_spaceship_death at the mesh, then
/// sfx_cha_C019_spaceship_loop_stop and K2_DestroyActor.
// Deviation: the loop is posted at the spawn point and does not follow the actor; SpawnedActorParent's EndPlay
// effects are not looked at for the ship (its DeathVFX is unset).
pub fn ship_tick(w: &mut World, sh: ShipId) {
    let gs = static_shapes(w, QUERY_UNKNOWN, name!("C019_Ship sensors"));
    let mut evs = Vec::new();
    ship::step(w, sh, gs.view(), &mut evs);
    let x = w.actors.ships.get(sh).expect("a live ship");
    let (f, at, mesh_dy) = (x.owner, (x.x, x.y), x.mesh_dy);
    for e in evs {
        match e {
            ShipEvent::Appear => {
                sound(w, f, name!("sfx_cha_C019_spaceship_appear"), Some(at));
                sound(w, f, name!("sfx_cha_C019_spaceship_loop"), Some(at));
            }
            ShipEvent::DeathSound => sound(w, f, name!("sfx_cha_C019_spaceship_death"), Some((at.0, at.1 + mesh_dy))),
            ShipEvent::Destroy => {
                sound(w, f, name!("sfx_cha_C019_spaceship_loop_stop"), None);
                w.actors.ships.get_mut(sh).expect("a live ship").dead = true;
            }
        }
    }
}

/// SeismicBoom's object types: the column trace's and the box's.
const COLUMN_TYPES: [i32; 3] = [OBJ_PAWN, OBJ_PROJECTILE_DESTRUCTIBLE, OBJ_ITEM_DESTRUCTIBLE];
const BOX_TYPES: [i32; 2] = [OBJ_PAWN, OBJ_PROJECTILE_DESTRUCTIBLE];

/// SeismicBoom's hit, built in the graph @3530-5197 (`BOOM_HIT`), with its knockback direction where given.
fn boom_packet(dir: Option<(Real, Real)>) -> HitPacket {
    HitPacket {
        damage: Some(3.0),
        base_knock_back: Some(1400.0),
        knock_back_scaling: Some(10.0),
        hit_reaction: Some(DataEnum::Value(2)),
        hit_pause_time: Some(0.175),
        hit_stun_mult: Some(1.2),
        on_hit_audio: Some(name!("sfx_cha_C019_dirt_hit")),
        ignore_facing: Some(true),
        destroy_projectiles: Some(true),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(true),
        kbi_multiplier: Some(1.0),
        knock_back_direction_vector: dir.map(|(x, y)| DataVec { x: Some(x), y: Some(y), z: None }),
        ..HitPacket::default()
    }
}

/// `boomDir`: SeismicBoom's direction @1792-2486: d = Normal((the actor's location - Location) x (0, 25, 2)), d.Y in
/// [0, 0.1] becomes 0.1, in [-0.1, 0) -0.1 (UE Y: sim -x).
fn boom_dir(x: Real, y: Real, vx: Real, vy: Real) -> Real {
    let (ex, ey) = ((vx - x) * 25.0, (vy - y) * 2.0);
    let nx = normal([0.0, ex, ey], KINDA_SMALL_NUMBER)[1];
    if (-0.1..=0.0).contains(&nx) {
        -0.1
    } else if nx > 0.0 && nx <= 0.1 {
        0.1
    } else {
        nx
    }
}

/// `boomHit`: the hit as a packet (its X along the handler's facing side, `projectile_hit_dir`).
fn boom_hit(facing: Real, x: Real, y: Real, vx: Real, vy: Real) -> HitPacket {
    boom_packet(Some((boom_dir(x, y, vx, vy) * facing, 0.7)))
}

/// The Weakened row SeismicBoom's hit carries: Buff_Weakened x 2 on a hit enemy (duration 0).
fn boom_buffs() -> Vec<OnHitBuffRow> {
    vec![OnHitBuffRow {
        applied_buffs: name!("Buff_Weakened_C"),
        apply_buff_hit: true,
        apply_buff_enemy: true,
        stack_count: Some(2),
        buff_duration: Some(0.0),
        ..OnHitBuffRow::default()
    }]
}

/// `inBox` (seismicBoom's): the fighter's root capsule within the box (l, r, b, t) in the play plane.
fn in_box(w: &World, v: FighterId, l: Real, r: Real, b: Real, t: Real) -> bool {
    let half = capsule_half(w, v) - cap_radius(w, v);
    let (x, y) = (w.fighter(v).body.x, actor_y(w, v));
    let dx = max(max(l - x, 0.0), x - r);
    let dy = max(max(b - (y + half), 0.0), y - half - t);
    size2d([dx, dy]) <= cap_radius(w, v)
}

/// An actor's place (`o.x`, `o.y`) the boom's packet aims from.
fn place_of(w: &World, a: ActorRef) -> (Real, Real) {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or((0.0, 0.0), |p| (p.x, p.y)),
        ActorRef::Item(id) => w.actors.items.get(id).map_or((0.0, 0.0), |i| (i.x, i.y)),
        ActorRef::Actor(_) => crate::actors::world_actor::actor_place(w, a).map_or((0.0, 0.0), |(x, y, _)| (x, y)),
        _ => crate::unported!("morty actors seismicBoom: the place of an actor of this family", w, a),
    }
}

/// `seismicBoom`: C019_MortarAim SeismicBoom(Location) after its Delay(0.05) (ubergraph 5338-..., 665-5337):
/// HitActors cleared, Seismic Boom Visuals(Location, Location + (0, 0, 500)): sfx_cha_C019_dirt_impact at the
/// location and the emitters (the mortarBoom event). Then BoxTraceMultiForObjects from Location + (0, 0, 500) down to
/// Location, half extent (75, 75, 0), object types Pawn, ProjectileDestructible and ItemDestructible, ignoring self:
/// every hit actor once; then BoxOverlapActors at Location + (0, 0, 25), extent ('Blast Radius' 175, 175, 'Blast
/// Height' 50), types Pawn and ProjectileDestructible, ignoring the first set: appended.
/// Each actor in that order. A C019_CookGrenade that is not a fighter: nothing when IsSplit, else two
/// c019_CookGrenade_Split_C from the owner's spawner and Disarm. Any other: d = Normal((its actor location -
/// Location) x (0, 25, 2)); the hit, built in the graph (`boom_packet`); OnHitBuffs: Buff_Weakened x 2 on a hit
/// enemy (duration 0), and an ally entry with no buff class; dealt through the actor's own pfgDoDamage.
/// No team test between @1617 and @5233: a team mate (the owner but) takes pfgDoDamage too, whose DamageAndLaunch
/// fails CanBeHitBy (no damage, ForceKnockback false) and whose NotifyAllyOfHit @21344 runs (`ally_hit`), behind the
/// dodge branch's pop for an invulnerable one (@7381: CanBeHitBy false). The owner is the handler's Instigator:
/// NotifyAllyOfHit skips him and nothing else of the call shows.
/// An actor that is no fighter and no C019_CookGrenade (@2786 -> @1728) takes the same rule and hit through
/// `do_damage`: a projectile (its HurtBox, ProjectileDestructible) is destroyed through its ReceiveAnyDamage
/// (DestroyProjectiles), a thrown item (its Sphere, ItemDestructible: the trace's types alone) is damaged.
// Deviation: the two boxes are tested against the fighters' root capsules in the play plane, in fighter order within
// each set; an invulnerable enemy is taken to dodge it as with an overlapping hitbox; the other actors (the gather's
// bodies of the boxes, `box_overlap_actors`: the trace's sweep as the box it covers) after the fighters, then the
// grenades (their HurtBox sphere against the boxes), whose splits first move in the next frame. A C019Marker's bodies
// (WorldDynamic) are of none of the queried types.
pub fn seismic_boom(w: &mut World, mo: MortarId) {
    let x = w.actors.mortars.get(mo).expect("a live mortar");
    let (f, bx, by, ax, ay, cls, attack_id) = (x.owner, x.boom_x, x.boom_y, x.x, x.y, x.cls, x.attack_id);
    let (x, y) = (bx, by);
    let id = fid(w, f);
    sound(w, f, name!("sfx_cha_C019_dirt_impact"), Some((x, y)));
    let fc = facing(w, f);
    w.events.push(Event::MortarBoom { fighter: id, x, y, ax, ay, facing: fc });
    let live: Vec<FighterId> =
        match_fighters(w).into_iter().filter(|&v| !is_dead(w, v) && !no_collision(w, v)).collect();
    let column: Vec<FighterId> =
        live.iter().copied().filter(|&v| in_box(w, v, x - 75.0, x + 75.0, y, y + 500.0)).collect();
    let p = actor_def(w, ActorRef::Fighter(f), cls).map(|d| d.p.clone()).unwrap_or_default();
    let (br, bh) = (num(&p, "Blast Radius").unwrap_or(0.0), num(&p, "Blast Height").unwrap_or(0.0));
    let mut targets = column.clone();
    targets.extend(
        live.iter()
            .copied()
            .filter(|v| !column.contains(v) && in_box(w, *v, x - br, x + br, y + 25.0 - bh, y + 25.0 + bh)),
    );
    let buffs = boom_buffs();
    let me = ActorRef::Mortar(mo);
    let mut others: Vec<ActorRef> = Vec::new();
    let col_types = COLUMN_TYPES.map(crate::real);
    for v in box_overlap_actors(w, x, y + 250.0, 75.0, 250.0, 75.0, &col_types, &[Some(me)], -1.0) {
        if !matches!(v, ActorRef::Fighter(_)) {
            others.push(v);
        }
    }
    let box_types = BOX_TYPES.map(crate::real);
    for v in box_overlap_actors(w, x, y + bh / 2.0, br, bh, br, &box_types, &[Some(me)], -1.0) {
        if !matches!(v, ActorRef::Fighter(_)) && !others.contains(&v) {
            others.push(v);
        }
    }
    for vic in targets {
        if crate::combat::phases::no_hit_by(w, f, vic) || vic == f {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        let (vx, vay) = (w.fighter(vic).body.x, actor_y(w, vic));
        if team(w, vic) == team(w, f) {
            if !invulnerable(w, vic) {
                let mf = w.actors.mortars.get(mo).expect("a live mortar").facing;
                let d = dealer(w, me);
                ally_hit(w, &d, vic, &boom_hit(mf, x, y, vx, vay));
            }
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, 1400.0, tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        let nx = boom_dir(x, y, vx, vay);
        let d = dealer(w, me);
        let vby = w.fighter(vic).body.y;
        let pct = charge_pct(w, me);
        let hit = boom_packet(None);
        if apply_projectile_hit(w, &d, vic, &hit, &buffs, V2 { x: vx, y: vby }, Some(V2 { x: nx, y: 0.7 }), &[], pct) {
            actor_hit_enemy(w, &d, vic);
        }
    }
    for v in others {
        let grenade = match v {
            ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some_and(|p| p.grenade),
            _ => false,
        };
        let mo_dead = w.actors.mortars.get(mo).is_none_or(|m| m.dead);
        if crate::combat::spawned_hits::dead(w, v) || grenade || mo_dead {
            continue;
        }
        let (ox, oy) = place_of(w, v);
        let mf = w.actors.mortars.get(mo).expect("a live mortar").facing;
        let mut hit = boom_hit(mf, x, y, ox, oy);
        hit.on_hit_buffs = boom_buffs();
        do_damage(w, me, v, &hit, &DamageCall { hit_box: false, comp: None, clank: false, rows: None });
    }
    split_grenades(w, f, x, y, br, bh);
}

/// seismicBoom's grenades: a C019_CookGrenade of any team: nothing for a split one (IsSplit); else CompSpawnActor(
/// c019_CookGrenade_Split_C) twice on the owner's spawner with WorldTransforms at the grenade's location, directions
/// (0, 0.5, 1) and (0, -0.5, 1) (mirrored by the owner's facing as in every CompSpawnActor), InitialSpeed 1200,
/// MaxSpeed 3000; then Disarm (DetonateOnDeath = false, K2_DestroyActor). The split class: IsSplit, GrenadeLifetime
/// 1, TriggerAbilityOnSpawnIndex false. The call (c019_MortarAim ubergraph 3304, on the spawner of the mortar's owner
/// @2913-2933) passes AttackID 0: the owner's attack count at the boom (Comp_Spawner HandleActorPostSpawn
/// 2444-2747), not the mortar's AttackID.
fn split_grenades(w: &mut World, f: FighterId, x: Real, y: Real, br: Real, bh: Real) {
    let split_cls = name!("c019_CookGrenade_Split_C");
    let split = actor_def(w, ActorRef::Fighter(f), split_cls).map(|d| d.p.clone());
    let near = |g: &crate::actors::Projectile, l: Real, r: Real, b: Real, t: Real| {
        let hy = hurt_y(g);
        size2d([max(max(l - g.x, 0.0), g.x - r), max(max(b - hy, 0.0), hy - t)]) <= g.hurt_radius && g.hurt_radius > 0.0
    };
    let ids: Vec<_> = w.actors.projectiles.iter().map(|(id, _)| id).collect();
    for gid in ids {
        let Some(g) = w.actors.projectiles.get(gid) else { continue };
        if !g.grenade || g.dead {
            continue;
        }
        let is_split = crate::actors::projectile::class_def(w, g).is_some_and(|d| flag(&d.p, "IsSplit"));
        if is_split {
            continue;
        }
        if !near(g, x - 75.0, x + 75.0, y, y + 500.0) && !near(g, x - br, x + br, y + 25.0 - bh, y + 25.0 + bh) {
            continue;
        }
        let (gx, gy) = (g.x, g.y);
        if let Some(sp) = &split {
            for dy in [0.5, -0.5] {
                actor_spawn_ability(w, f, sp);
                let mut np = Props::new();
                let mut d = Props::new();
                d.insert("X".into(), mvs_data::value::Value::Num(0.0));
                d.insert("Y".into(), mvs_data::value::Value::Num(dy));
                d.insert("Z".into(), mvs_data::value::Value::Num(1.0));
                np.insert("InitialMovementDirection".into(), mvs_data::value::Value::Obj(d));
                np.insert("InitialSpeed".into(), mvs_data::value::Value::Num(1200.0));
                np.insert("MaxSpeed".into(), mvs_data::value::Value::Num(3000.0));
                let count = w.fighter(f).decay.attack_count;
                let class = ProjectileClass::C019CookGrenadeSplit;
                let at = Some(V2 { x: gx, y: gy });
                let mut s = make_projectile(w, split_cls, class, Some(f), count, &np, at, MakeOpts::default());
                // `new CookGrenade(...)`: the Projectile with the class's own fields at their defaults.
                s.ext = ProjectileExt::CookGrenade(super::projectiles::CookGrenade::default());
                let sid = w.actors.projectiles.insert(s);
                crate::world::spawning::list_projectile(w, sid);
            }
        } else {
            warn_once(w, name!("spawned actor not implemented (c019_CookGrenade_Split_C)"));
        }
        w.actors.projectiles.get_mut(gid).expect("a live grenade").disarmed = true;
        crate::world::spawning::destroy_projectile(w, gid, false);
    }
}

/// `mortarHurt`: C019_MortarAim's Hurtbox (`mortar::hurt_touches`) under a spawned actor's or a thrown item's
/// hitbox: pfgDoDamage at the overlap's begin, as between projectiles (projectileHitsProjectile). The mortar is a
/// SpawnedActorParent whose Destructible has IsProjectile; it cannot clank (HitBoxOn? false, no CanProjectileClank),
/// so the hit needs DestroyProjectiles (Comp_Pawn_HitBoxHandler 15491), without which only HitByAlly / HitByEnemy
/// are broadcast (SpawnedActorParent's: nothing). DamageAndLaunch then needs CanBeHitBy: another team and not
/// DamageDisabled, which the mortar's Destructible is until ActivateObject (c019_MortarAim 9281). The mortar has no
/// ReceiveAnyDamage and no health; the dealer's HitNonPlayer follows (a sandwich or a splitter shot ends there). An
/// overlap that began before the activation is not run again. Each begin runs the call's entry rows first (the
/// dealer's OnHitBuffs rows that need no hit, `spawned_hits::entry_rows`), whatever the gates after them.
// Deviation: tested in the play plane once a step with the other spawned-actor overlaps; a fighter's melee hitbox on
// it takes the same path and needs DestroyProjectiles too, which no ported Hitframe sets.
pub fn mortar_hurt(w: &mut World, a: ActorRef, on: bool) {
    if w.actors.mortars.is_empty() {
        return;
    }
    let (ax, ay, r, hit) = match a {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id).expect("a live projectile");
            (p.x, p.y, p.hit_radius, p.hit.clone())
        }
        ActorRef::Item(id) => {
            let i = w.actors.items.get(id).expect("a live item");
            (i.x, i.y, crate::actors::item::item_hit_radius(w, id), i.hit.clone())
        }
        _ => crate::unported!("morty actors mortarHurt of a dealer of this family", w, a, on),
    };
    let ids: Vec<MortarId> = w.actors.mortars.iter().map(|(id, _)| id).collect();
    for mo in ids {
        let m = w.actors.mortars.get_mut(mo).expect("a listed mortar");
        if !on || !mortar::hurt_touches(m, ax, ay, r) {
            m.overlapped_by.shift_remove(&a);
            continue;
        }
        if !m.overlapped_by.insert(a) {
            continue;
        }
        // pfgDoDamage's entry rows (@842-2821), ahead of every gate below.
        if !crate::combat::spawned_hits::entry_rows(w, a, ActorRef::Mortar(mo)) {
            if crate::combat::spawned_hits::dead(w, a) {
                return;
            }
            continue;
        }
        let m = w.actors.mortars.get(mo).expect("a listed mortar");
        let (owner, active, attack_type) = (m.owner, m.active, m.attack_type);
        if hit.hit_each_actor_only_once.unwrap_or(false) {
            let seen = match a {
                ActorRef::Projectile(id) => {
                    &mut w.actors.projectiles.get_mut(id).expect("a live projectile").hit_actors
                }
                ActorRef::Item(id) => &mut w.actors.items.get_mut(id).expect("a live item").hit_actors,
                _ => unreachable!("a projectile or an item"),
            };
            if !seen.insert(ActorRef::Mortar(mo)) {
                continue;
            }
        }
        if !hit.destroy_projectiles.unwrap_or(false) || owner_team(w, a) == team(w, owner) || !active {
            continue;
        }
        crate::combat::spawned_hits::dealer_hit_non_player(w, a, crate::real(attack_type), ActorRef::Mortar(mo));
    }
}

/// The portal's Box in the play plane (`Accelerator.box`).
fn accel_box(w: &World, ac: AcceleratorId) -> (Real, Real, Real, Real) {
    let x = w.actors.accelerators.get(ac).expect("a live accelerator");
    let c = crate::fighter::common::actor_components(w, ActorRef::Fighter(x.owner), x.cls);
    let b = c.and_then(|c| obj(c, "Box"));
    let g = |k: &str, a: &str| b.and_then(|b| obj(b, k)).and_then(|o| num(o, a)).unwrap_or(0.0);
    let (cx, cy) = (x.x + g("RelativeLocation", "X") * x.facing, x.y + g("RelativeLocation", "Z"));
    let (ex, ez) = (g("BoxExtent", "X"), g("BoxExtent", "Z"));
    (cx - ex, cx + ex, cy - ez, cy + ez)
}

/// `acceleratorTick`: ProjectileAccelerator's Box begin overlaps (accelerator.rs) and its lifetime, behind
/// SpawnedActorParent's blast-box loop (its first tick) and CheckBlastBoxTimer.
// Deviation: the BeginPlay loop runs in the actor's first tick, in its spawn frame behind the loop sound World's spawn
// posts; the build's DestroySpawnedActor comes inside the parent's BeginPlay, ahead of the class's loop sound, which
// ends with the actor.
pub fn accelerator_tick(w: &mut World, ac: AcceleratorId) {
    let x = w.actors.accelerators.get(ac).expect("a live accelerator");
    let p = actor_def(w, ActorRef::Fighter(x.owner), x.cls).map(|d| d.p.clone()).unwrap_or_default();
    let (doe, force) = (flag(&p, "DestroyOnExitedArena?"), flag(&p, "ForceSpawnPositionInsideBlastBox"));
    let sb = spawn_box(w);
    let x = w.actors.accelerators.get_mut(ac).expect("a live accelerator");
    let out = if x.age == 0 {
        let (mut px, mut py) = (x.x, x.y);
        let out = blast_box_loop(&sb, &mut px, &mut py, None, force, &mut x.blast_bounds, doe);
        (x.x, x.y) = (px, py);
        out
    } else {
        blast_timer_exit(doe, &mut x.blast_at, &x.blast_bounds, crate::real(x.age), x.x, x.y)
    };
    if out {
        destroy_accelerator(w, ac);
        return;
    }
    let (left, right, bottom, top) = accel_box(w, ac);
    let owner = w.actors.accelerators.get(ac).expect("a live accelerator").owner;
    let ids: Vec<_> = w.actors.projectiles.iter().map(|(id, _)| id).collect();
    for pid in ids {
        let Some(pr) = w.actors.projectiles.get(pid) else { continue };
        let (hx, hy) = (crate::actors::projectile::hurt_x(pr), hurt_y(pr));
        let over = !pr.dead
            && pr.hurt_radius > 0.0
            && size2d([max(max(left - hx, 0.0), hx - right), max(max(bottom - hy, 0.0), hy - top)]) <= pr.hurt_radius;
        let seq = pr.seq;
        let inside = &mut w.actors.accelerators.get_mut(ac).expect("a live accelerator").inside;
        if !over {
            inside.shift_remove(&seq);
            continue;
        }
        if !inside.insert(seq) {
            continue;
        }
        if owner_team(w, ActorRef::Projectile(pid)) != team(w, owner) {
            continue;
        }
        let p = w.actors.projectiles.get_mut(pid).expect("a live projectile");
        if !p.sped_up {
            speed_buff(p, 2.0);
        }
        let (px, py) = (p.x, p.y);
        let id = fid(w, owner);
        w.events.push(Event::AcceleratorBuff { fighter: id, x: px, y: py });
        sound(w, owner, name!("sfx_cha_C019_portal_buff"), Some((px, py)));
    }
    if super::accelerator::step(w, ac) {
        destroy_accelerator(w, ac);
    }
}

/// `destroyAccelerator`: SpawnedActorParent DestroySpawnedActor for the portal: 'Ak Death Location' and the DeathVFX
/// at the actor; the loop posted on the actor ends with it.
pub fn destroy_accelerator(w: &mut World, ac: AcceleratorId) {
    let x = w.actors.accelerators.get_mut(ac).expect("a live accelerator");
    x.dead = true;
    let (owner, seq, cls, at) = (x.owner, x.seq, x.cls, (x.x, x.y));
    let id = fid(w, owner);
    let key = SoundKey { tag: name!("accel"), n: seq };
    w.events.push(Event::SoundStop { fighter: id, key, fade: None, ev: None, curve: None, cut: false });
    let ev = actor_def(w, ActorRef::Fighter(owner), cls)
        .and_then(|d| crate::registry::props::text(&d.p, "Ak Death Location"))
        .filter(|s| !s.is_empty())
        .map(|s| crate::fighter::common::lit(w, s));
    if let Some(ev) = ev {
        sound(w, owner, ev, Some(at));
    }
    w.events.push(Event::AcceleratorEnd { fighter: id, x: at.0, y: at.1 });
}

/// A marker or a mortar aim as a dealer (the `pr` of the hits: its place, facing, AttackID, owner). Neither
/// moves (vx = vy = 0), nor has a yaw, a hit velocity or HasHitAFighter.
pub fn dealer(w: &World, a: ActorRef) -> Dealer {
    let (cls, owner, x, y, facing, has_elem, charged, attack_id, dead, age) = match a {
        ActorRef::Marker(id) => {
            let m = w.actors.markers.get(id).expect("a live marker");
            (m.cls, m.owner, m.x, m.y, m.facing, m.elem.is_some(), m.charge.is_some(), m.attack_id, m.dead, m.age)
        }
        ActorRef::Mortar(id) => {
            let m = w.actors.mortars.get(id).expect("a live mortar");
            (m.cls, m.owner, m.x, m.y, m.facing, m.elem.is_some(), m.charge.is_some(), m.attack_id, m.dead, m.age)
        }
        _ => unreachable!("a marker or a mortar aim: {a:?}"),
    };
    Dealer {
        a: Some(a),
        cls,
        owner: Some(owner),
        lookup_fighter: None,
        inst: None,
        x,
        y,
        vx: 0.0,
        vy: 0.0,
        facing,
        yaw_x: None,
        hit_velocity: None,
        is_item: false,
        has_elem,
        charged,
        attack_id,
        dead,
        age,
        has_hit_a_fighter: false,
    }
}

/// This file's registry entries: `buffGraphsPorted('Buff_GarnetStunnedMove_C')` (its reached part is
/// [`marker_ended`]'s and [`recall_end`]'s NetImpulse).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buff_graphs_ported.enter("Buff_GarnetStunnedMove_C", ())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boom_dir_pushes_a_near_vertical_direction_out_to_a_tenth() {
        assert_eq!(boom_dir(0.0, 0.0, 0.0, 100.0), -0.1);
        assert_eq!(boom_dir(0.0, 0.0, 0.1, 100.0), 0.1);
        assert_eq!(boom_dir(0.0, 0.0, -0.1, 100.0), -0.1);
        let far = boom_dir(0.0, 0.0, 100.0, 0.0);
        assert_eq!(far, 1.0);
        assert_eq!(boom_dir(0.0, 0.0, 0.0, 0.0), -0.1);
    }

    #[test]
    fn boom_hit_turns_the_direction_by_the_facing() {
        let h = boom_hit(-1.0, 0.0, 0.0, 100.0, 0.0);
        let v = h.knock_back_direction_vector.expect("a direction");
        assert_eq!((v.x, v.y), (Some(-1.0), Some(0.7)));
        assert_eq!(h.damage, Some(3.0));
        assert!(boom_packet(None).knock_back_direction_vector.is_none());
    }
}
