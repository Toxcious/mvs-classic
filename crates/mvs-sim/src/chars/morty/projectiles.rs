//! Morty's projectiles: the cook grenades (C019_CookGrenade_C, its NoGrav and Split
//! children: [`CookGrenade`], a projectile's `ext`, with their detonation, blast and hit-by), the blaster's charge
//! shot (C019BlasterChargeProjectile_C, retargeting the closest grenade), the splitters and their followups, and the
//! whip (C019_WhipProjectile_C, a projectile of the tether family with its tipper).
//!
//! Morty's ParentProjectilePhysics children (the grenade, the gun's shots, the whip): the registry entries and what
//! each class's graph does through the world. The classes' own variables and movement are in `actors::projectile`,
//! but for the grenade's IgnorePlatformsCheck ([`CookGrenade`]).
//!
//! C019_CookGrenade Detonate Server(BombCount), behind a once-gate: Detonate, Detonate Client, K2_DestroyActor.
//! BombCount is 0 except from C019BlasterChargeProjectile (the count of grenades that shot has set off).
//! Detonate: GetBlastRadius ([`cook_radius`]) = 100 x BombCount + (BombCount == 0 ? 100 : 0) + BlastRadius 225, or the
//! perk Buff_C019_Cook's radius (UseCookDuration / UseMaxRadius, `perks.rs`); SphereOverlapActors at the actor
//! (object types Pawn, WorldDynamic and two game channels); a fighter of the grenade's team
//! (Comp_Actor_Destructible.TeamIndex) is skipped, and one whose actor is |dY| >= radius away. d = the unit vector from
//! the grenade to the fighter's actor location in (Y, Z); LocalLaunchVelocity = (BaseKB x d.Y, BaseKB x (d.Z < 0 ?
//! Lerp(1, 0.25, |d.Z|) : Lerp(1, 1.25, d.Z))); the hit: Damage 5 + 2 x BombCount, BaseKnockBack = BaseKB x the same
//! Lerp of the launch normal's Z, KnockBackScaling 26 + min(BombCount, 3), direction = the launch normal with
//! IgnoreFacing, HitReaction 4 for a fighter at or above the grenade (else 3), HitPauseTime 0.1, HitStunMult 1,
//! OnHitAudio sfx_cha_C019_grenade_hit, ForceKnockback, DoLOSCheck = the actor is not AttachedTo (nothing attaches the
//! grenade) and has no DestructibleEnvironment tag (Detonate 4882-5640): true for every fighter; dealt through the
//! grenade's own Comp_Pawn_HitBoxHandler.pfgDoDamage with On Hit Buffs, whose line-of-sight trace (`los_blocked`:
//! ProjectileExplode, from the handler's owner, the grenade, to the victim's actor location, before the invulnerability
//! checks) drops the hit behind the stage block. Platforms and ship pieces do not block it.
//! Detonate Client: SpawnEmitterAtLocation(PS_C019_GrenadeExplosion, location, the mesh's rotation, scale radius / 333)
//! with Team, the fuse loop stopped (ExecuteActionOnPlayingID Stop, 200 ms) and sfx_cha_C019_grenade_explode at the
//! location.
// Deviation: the overlap is the blast circle against the fighter's root capsule in the play plane (hurtbox components
// on the queried channels are not checked); the emitter's rotation is 0 (the mesh's is not passed on); the loop stops
// with the port's default fade; the attached case is not hit. The other actors: [`blast_actors`].

// The negated comparisons are `!(a < b)`, true for a NaN too.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::Real;
use crate::State;
use crate::actors::projectile::{
    CollisionResponse, MakeOpts, Projectile, ProjectileExt, TetherParams, class_p, hitbox_active, launch_from_hit,
    make_projectile, set_platform_response, yaw_x,
};
use crate::actors::world_actor::{actor_owner, actor_p, actor_place};
use crate::actors::{ActorClass, ActorRef, ProjectileId};
use crate::combat::elem::{charge_pct, end_actor_children};
use crate::combat::hits::{DamageCall, actor_hit_enemy, apply_projectile_hit, dealer, do_damage, projectile_blocks};
use crate::combat::phases::los_blocked;
use crate::combat::spawned_hits::class_rows;
use crate::combat::{DataEnum, DataVec, HitPacket};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_def, actor_is_a, warn_text};
use crate::fighter::control::{actor_spawn_ability, attack_dodged, dodge_over};
use crate::fighter::match_side::{SpawnRequest, TutorialEvent};
use crate::fighter::moves::V2;
use crate::fighter::traces::sphere_overlap_actors;
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, invulnerable};
use crate::math::ue::{KINDA_SMALL_NUMBER, SMALL_NUMBER, normal, safe_normal, size2d};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks, ReflectHooks};
use crate::registry::props::{flag, num, num_in, obj};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::{reset_pass, tutorial_event};
use crate::world::pawns::match_fighters;
use crate::world::spawning::{destroy_projectile, list_projectile, spawn_overlap};
use mvs_data::value::{Props, Value};

/// `OBJ.Pawn`, `OBJ.WorldDynamic`, `OBJ.DamageDestructible`: Detonate's loop over the non-fighter actors of the sphere.
const BLAST_TYPES: [Real; 3] = [2.0, 1.0, 8.0];

/// `CookGrenade extends Projectile`: what the class adds. Every grenade is made as this class (the kind's `make`, the
/// mortar's split grenades).
#[derive(State, Debug, Default)]
pub struct CookGrenade {
    /// C019_CookGrenade's own variable IgnorePlatformsCheck (false in the class defaults; the children
    /// C019_CookGrenade_NoGrav and c019_CookGrenade_Split have no graph of their own): the latch of its ReceiveTick's
    /// writes of the Capsule's Platform response.
    pub ignore_platforms_check: bool,
    /// UseCookDuration / UseMaxRadius (false in the class defaults), written by ModifyBlastRadius from the perk buff
    /// Buff_C019_Cook (`perks.rs`) alone. Written to the record only while true.
    pub use_cook_duration: bool,
    pub use_max_radius: bool,
}

fn grenade_of(p: &Projectile) -> Option<&CookGrenade> {
    match &p.ext {
        ProjectileExt::CookGrenade(g) => Some(g),
        _ => None,
    }
}

/// The lerp of Detonate's launch: 1 to 0.25 below the horizontal, 1 to 1.25 above.
fn blast_lerp(z: Real) -> Real {
    if z < 0.0 { 1.0 + (0.25 - 1.0) * z.abs() } else { 1.0 + 0.25 * z }
}

/// GetBlastRadius @0-685 without its BombCount terms (`detonate` adds them): SelectFloat(PerkMaxBlastRadius,
/// SelectFloat(Lerp(PerkMinBlastRadius, PerkMaxBlastRadius, (GrenadeLifetime - remaining) / GrenadeLifetime),
/// BlastRadius, UseCookDuration), UseMaxRadius) @401-511, float32; the alpha is not clamped. remaining @123-258: the
/// DetonateTimer's K2_GetTimerRemainingTimeHandle when the handle is valid and the timer active, else 0.
/// The timer (K2_SetTimer(DetonateTimerFunction, GrenadeLifetime) in BeginPlay, Projectile's fuse) has counted one step
/// a frame since the spawn frame's timer tick (the port's model of its firing, `actors::projectile::step`: it fires at
/// age n + 1 with n the first count whose time passes GrenadeLifetime): ExpireTime - InternalTime = GrenadeLifetime -
/// (age - 1) x DeltaSeconds (K2_GetTimerRemainingTimeHandle). Detonate from the timer runs while it executes, 0 (the
/// same facts row), which the clamp gives (that count is past the lifetime); that is the full 550.
// Unverified: an early Detonate (ReceiveAnyDamage, the Blaster's charge shot, the mortar) reads the count as of the
// grenade's last step, whatever the call's place in the frame (one step, 1/60 of the 400 range over 6 s, ~1.1). An
// attached grenade's paused fuse is not modelled (nothing attaches it).
pub fn cook_radius(w: &World, pr: &Projectile) -> Real {
    let p = class_p(w, pr);
    let g = grenade_of(pr);
    let get = |k: &str| num(p, k).unwrap_or(0.0);
    if g.is_some_and(|g| g.use_max_radius) {
        return get("PerkMaxBlastRadius");
    }
    if !g.is_some_and(|g| g.use_cook_duration) {
        return get("BlastRadius");
    }
    let life = get("GrenadeLifetime");
    let left = max(0.0, life - crate::real(max_i(0, pr.age - 1)) * DELTA_SECONDS);
    let alpha = (life - left) / life;
    let (lo, hi) = (get("PerkMinBlastRadius"), get("PerkMaxBlastRadius"));
    lo + (alpha * (hi - lo))
}

fn max_i(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

/// `detonate`: Detonate for BombCount `bomb`. `ignore`: SphereOverlapActors' ActorsToIgnore, [BlockingActor]
/// (Detonate @547: the fighter whose projectile armor blocked the grenade, set by its Blocked event; or the
/// Steven_ShieldActor whose BlockBox blocked it, which matches no fighter, so Steven himself can be hit).
pub fn detonate(w: &mut World, id: ProjectileId, bomb: i32, ignore: Option<ActorRef>) {
    let me = ActorRef::Projectile(id);
    let Some(p) = w.actors.projectiles.get_mut(id) else { return };
    p.detonated = true;
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let Some(owner) = p.owner else { return };
    let (px, py, cls, seq, attack_id) = (p.x, p.y, p.cls, p.seq, p.attack_id);
    let fid = w.fighter(owner).id;
    let team = w.fighter(owner).team.team;
    let bomb_r = crate::real(bomb);
    let radius = (100.0 * bomb_r + cook_radius(w, p)) + if bomb == 0 { 100.0 } else { 0.0 };
    let kb = num(class_p(w, p), "BaseKB").unwrap_or(0.0);
    for vic in match_fighters(w) {
        if reset_pass(w) {
            break; // (not in the Lab reset's pass: an old pawn there)
        }
        let v = w.fighter(vic);
        if v.team.team == team
            || v.life.dead > 0.0
            || crate::combat::phases::no_hit_by_team(w, team, vic)
            || ignore == Some(ActorRef::Fighter(vic))
        {
            continue;
        }
        let (vx, vy) = (v.body.x, actor_y(w, vic));
        let (rad, half) = (cap_radius(w, vic), capsule_half(w, vic) - cap_radius(w, vic));
        let cy = max(vy - half, min(py, vy + half));
        if size2d([vx - px, cy - py]) > radius + rad {
            continue;
        }
        if !(((vx - px) / radius).abs() < 1.0) {
            continue;
        }
        if los_blocked(w, px, py, vx, vy) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        // GetDirectionUnitVector (@2464), Normal(LocalLaunchVelocity, 0.0001) (@5042)
        let u = safe_normal([0.0, vx - px, vy - py], SMALL_NUMBER);
        let (lx, ly) = (kb * u[1], kb * blast_lerp(u[2]));
        let n = normal([0.0, lx, ly], KINDA_SMALL_NUMBER);
        let (nx, ny) = (n[1], n[2]);
        let hit = HitPacket {
            damage: Some(5.0 + 2.0 * bomb_r),
            base_knock_back: Some(kb * blast_lerp(min(max(ny, -1.0), 1.0))),
            knock_back_scaling: Some(26.0 + crate::real(bomb.min(3))),
            hit_reaction: Some(DataEnum::Value(if vy >= py { 4 } else { 3 })),
            hit_pause_time: Some(0.1),
            hit_stun_mult: Some(1.0),
            on_hit_audio: Some(name!("sfx_cha_C019_grenade_hit")),
            ignore_facing: Some(true),
            force_knockback: Some(true),
            do_los_check: Some(true),
            play_on_hit_effects: Some(true),
            apply_hit_pause_to_damage_dealer: Some(true),
            kbi_multiplier: Some(1.0),
            ..HitPacket::default()
        };
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, hit.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        let d = dealer(w, me);
        let cp = charge_pct(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &[], V2 { x: px, y: py }, Some(V2 { x: nx, y: ny }), &[], cp) {
            actor_hit_enemy(w, &d, vic);
        }
    }
    if !reset_pass(w) {
        blast_actors(w, id, radius, bomb, ignore);
    }
    // Another team's C019Marker (its Capsule and spline mesh are WorldDynamic bodies, which the trace gather does not
    // hold for a marker) takes the same hit as damage.
    // Deviation: the hit's feedback on a marker (hit sound, effect, the dealer's hit events) is not ported; the damage
    // is taken without the dealer's damage modifiers.
    let mut k = 0;
    while let Some(m) = w.actors.markers.nth(k) {
        k += 1;
        let Some(mk) = w.actors.markers.get(m) else { continue };
        let (mx, my, mr, dead, mo) = (mk.x, mk.y, mk.cap_radius, mk.dead, mk.owner);
        if dead || w.fighter(mo).team.team == team {
            continue;
        }
        if size2d([mx - px, my - py]) > radius + mr {
            continue;
        }
        if !(((mx - px) / radius).abs() < 1.0) {
            continue;
        }
        if los_blocked(w, px, py, mx, my) {
            continue;
        }
        crate::chars::morty::actors::damage_marker(w, m, 5.0 + 2.0 * bomb_r);
    }
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_C019_GrenadeExplosion")),
        x: px,
        y: py,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: radius / 333.0,
        tint: None,
        to: None,
    });
    w.events.push(Event::SoundStop {
        fighter: fid,
        key: SoundKey { tag: name!("proj"), n: seq },
        fade: None,
        ev: None,
        curve: None,
        cut: false,
    });
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C019_grenade_explode"),
        x: Some(px),
        y: Some(py),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// Detonate's loop on the actors of the sphere that are no BaseFighter (@1294 -> @3687): SphereOverlapActors @625 with
/// object types [2 Pawn, 1 WorldDynamic, 8 DamageDestructible, 18 HurtBox] and ActorsToIgnore [BlockingActor]; @991-1103
/// the actor is not the grenade and its Comp_Actor_Destructible.TeamIndex is not the grenade's. LocalIsAboveOrAttached =
/// its Z >= the grenade's @3909; a SpawnedActorParent @3928 whose BPI_GetProjectileAttackType is 1 (Heavy) with
/// HitBoxOn? @4083 is skipped; then @1728-2168 |dY| / radius < 1 (or a DestructibleEnvironment tag: no actor of the port
/// has one), the launch of the fighter loop and HitReaction 4 above (else 3) @2992 / @4158, PlayerData @3072 failing for
/// a non-fighter (-> @4183), the hit @5176-6221 and pfgDoDamage(null, the actor, the hit, On Hit Buffs) @6318
/// (`do_damage`: an IsProjectile actor without DestroyProjectiles gets its HitByEnemy alone, a grenade's launch).
/// Of the port's bodies those types meet an item's PickupArea, BugsBunnyRocketUpProjectile's Box, the mortar aim's
/// Hurtbox, Wonder Woman's evade shield (OverlapAllDynamic) and the actors' Pawn bodies; a grenade's and a sandwich's
/// bodies (PhysicsBody, ProjectileDestructible) are of none of them.
// Deviation: after the fighters (one loop in the build, in the overlap's order), in the gather's order; an item no one
// has held (no team in the port) is left out; HurtBox is not asked here: of the gathered bodies only the fighters' are
// of that type (a class's own HurtBox body, C018AirFlag's Sphere, is not entered in the gather).
// Unverified: DoLOSCheck (no AttachedTo, no tag: true) as the fighter loop's losBlocked from the grenade to the actor.
fn blast_actors(w: &mut World, id: ProjectileId, radius: Real, bomb: i32, ignore: Option<ActorRef>) {
    let me = ActorRef::Projectile(id);
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let Some(owner) = p.owner else { return };
    let (px, py) = (p.x, p.y);
    let kb = num(class_p(w, p), "BaseKB").unwrap_or(0.0);
    let team = w.fighter(owner).team.team;
    let bomb_r = crate::real(bomb);
    let ign: Vec<Option<ActorRef>> = ignore.into_iter().map(Some).collect();
    for v in sphere_overlap_actors(w, px, py, radius, &BLAST_TYPES, &ign, -1.0) {
        if v == me || matches!(v, ActorRef::Fighter(_)) || w.actors.projectiles.get(id).is_none_or(|p| p.dead) {
            continue;
        }
        let Some((ox, oy, dead)) = actor_place(w, v) else { continue };
        let Some(oo) = actor_owner(w, v) else { continue };
        if dead || w.fighter(oo).team.team == team {
            continue;
        }
        // (ItemActorParent is a SpawnedActorParent child)
        let cls = crate::actors::world_actor::actor_cls(w, v).unwrap_or(Name::NONE);
        let sap = matches!(v, ActorRef::Item(_)) || {
            let r = actor_is_a(w, ActorRef::Fighter(oo), cls, name!("SpawnedActorParent_C"));
            if r.is_none() {
                let m = format!(
                    "{}: the class of {} is not in the data (taken as no SpawnedActorParent)",
                    w.names.str(w.actors.projectiles.get(id).map_or(Name::NONE, |p| p.cls)),
                    w.names.str(cls)
                );
                warn_text(w, &m);
            }
            r.unwrap_or(false)
        };
        if sap && num(actor_p(w, v), "ProjectileAttackType").unwrap_or(0.0) == 1.0 && spawned_hit_box_on(w, v) {
            continue;
        }
        if !(((ox - px) / radius).abs() < 1.0) {
            continue;
        }
        if los_blocked(w, px, py, ox, oy) {
            continue;
        }
        // GetDirectionUnitVector (@2464), Normal(LocalLaunchVelocity, 0.0001) (@5042)
        let u = safe_normal([0.0, ox - px, oy - py], SMALL_NUMBER);
        let (lx, ly) = (kb * u[1], kb * blast_lerp(u[2]));
        let n = normal([0.0, lx, ly], KINDA_SMALL_NUMBER);
        let (nx, ny) = (n[1], n[2]);
        // (the packet's X is along the dealer's forward in the port, World.projectileHitDir: the launch's sim x times
        // yawX)
        let yaw = w.actors.projectiles.get(id).map_or(1.0, yaw_x);
        let hit = HitPacket {
            damage: Some(5.0 + 2.0 * bomb_r),
            base_knock_back: Some(kb * blast_lerp(min(max(ny, -1.0), 1.0))),
            knock_back_scaling: Some(26.0 + crate::real(bomb.min(3))),
            knock_back_direction_vector: Some(DataVec { x: Some(nx * yaw), y: Some(ny), z: None }),
            hit_reaction: Some(DataEnum::Value(if oy >= py { 4 } else { 3 })),
            hit_pause_time: Some(0.1),
            hit_stun_mult: Some(1.0),
            on_hit_audio: Some(name!("sfx_cha_C019_grenade_hit")),
            ignore_facing: Some(true),
            force_knockback: Some(true),
            do_los_check: Some(true),
            play_on_hit_effects: Some(true),
            apply_hit_pause_to_damage_dealer: Some(true),
            kbi_multiplier: Some(1.0),
            hit_each_actor_only_once: Some(true),
            on_hit_buffs: class_rows(w, me),
            ..HitPacket::default()
        };
        do_damage(w, me, v, &hit, &DamageCall { hit_box: false, comp: None, clank: false, rows: None });
    }
}

/// HitBoxOn? as the actor holds it: a projectile's hit box, an item's ThrowHitBox, else the class default.
fn spawned_hit_box_on(w: &World, o: ActorRef) -> bool {
    match o {
        ActorRef::Projectile(i) => w.actors.projectiles.get(i).is_some_and(hitbox_active),
        ActorRef::Item(i) => w.actors.items.get(i).is_some_and(|i| i.box_on()),
        o => flag(actor_p(w, o), "HitBoxOn?"),
    }
}

/// C019_CookGrenade OnHitByEnemy(Hit, Attacker): nothing when the attacker is a spawned actor whose
/// BPI_GetProjectileAttackType is Light (2); else the launch. OnHitByAlly: only with CanTurnOnHitBox; the hitbox goes on
/// until the next bounce (`launch_from_hit`), then the same launch. The launch (ubergraph 631): the hit's
/// KnockBackDirectionVector as (Y, Z), (0, 1) when it is zero, normalised, x FMax(BaseKnockBack, 2000) ->
/// LaunchFromHitMulticast, which also posts sfx_cha_C019_grenade_bounce on the actor.
/// OnHitByAlly's HitBoxOn? = true and UpdateHitbox @8475 (HitBoxesOn -> ActivateHitbox's SetCollisionEnabled) update
/// the box's overlaps at once, at the grenade's place then (a melee hit's: ahead of its move this frame), ahead of the
/// launch's sound: what it already overlaps is dealt inside the ally's hit (`spawn_overlap`, the same test as a
/// spawn's), and HitEachActorOnlyOnce keeps it from hitting that fighter again where the move ends.
#[allow(clippy::too_many_arguments, reason = "the hook's parameters")]
fn grenade_hit_by(
    w: &mut World,
    id: ProjectileId,
    ally: bool,
    dx: Real,
    dy: Real,
    bkb: Real,
    attacker_type: i32,
    _attacker: Option<ActorRef>,
) {
    let Some(g) = w.actors.projectiles.get_mut(id) else { return };
    if !g.grenade || g.dead {
        return;
    }
    if if ally { !g.can_turn_on_hit_box } else { attacker_type == 2 } {
        return;
    }
    let dy = if dx == 0.0 && dy == 0.0 { 1.0 } else { dy };
    let n = normal([0.0, dx, dy], KINDA_SMALL_NUMBER); // @944
    let speed = max(bkb, 2000.0);
    launch_from_hit(g, n[1] * speed, n[2] * speed, ally);
    let (fid, x, y) = (g.owner_id, g.x, g.y);
    if ally {
        spawn_overlap(w, id);
    }
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C019_grenade_bounce"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// C019_CookGrenade ReceiveTick, its part about platforms (ubergraph 4995, @5083-5337), in the actor's tick, so behind
/// the ProjectileMovement's tick of the frame (the kind's step after the projectile's); the class data has
/// PrimaryActorTick.TickInterval 0.1, every 6th frame of the fixed step, the same frames as the tick's other parts in
/// the shared step (unverified: the first in the spawn frame).
/// GetVelocity() @5083 (the root's ComponentVelocity: the movement's velocity at the end of its tick) with Z > 0 @5161:
/// unless IgnorePlatformsCheck @5209, IgnorePlatformsCheck = true @5224 and Capsule.SetCollisionResponseToChannel(20,
/// 0) @5235; else, only with IgnorePlatformsCheck @5276, IgnorePlatformsCheck = false @5286 and
/// Capsule.SetCollisionResponseToChannel(20, 2) @5297. So the response is written on a change of the test's result
/// alone, and what is written is used by the next frame's move: a grenade that turned down inside a platform is pushed
/// out of it there (`safe_move`).
fn grenade_tick(w: &mut World, id: ProjectileId) -> bool {
    let Some(p) = w.actors.projectiles.get_mut(id) else { return false };
    if (p.age - 1) % 6 != 0 {
        return false;
    }
    let ProjectileExt::CookGrenade(g) = &mut p.ext else {
        let m = format!(
            "C019_CookGrenade: not made as a CookGrenade, its Platform response is not written ({})",
            w.names.str(p.cls)
        );
        warn_text(w, &m);
        return false;
    };
    if p.vy > 0.0 {
        if g.ignore_platforms_check {
            return false;
        }
        g.ignore_platforms_check = true;
        set_platform_response(p, CollisionResponse::Ignore);
    } else if g.ignore_platforms_check {
        g.ignore_platforms_check = false;
        set_platform_response(p, CollisionResponse::Block);
    }
    false
}

fn grenade_make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::CookGrenade(CookGrenade::default())
}

fn grenade_moved(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    if p.dead {
        return;
    }
    let (fid, age, seq, x, y) = (p.owner_id, p.age, p.seq, p.x, p.y);
    if age == p.loop_tick {
        w.events.push(Event::SoundStart {
            fighter: fid,
            ev: name!("sfx_cha_C019_grenade_loop"),
            key: SoundKey { tag: name!("proj"), n: seq },
        });
    }
    if age == p.tell_tick {
        w.events.push(Event::Sound {
            fighter: fid,
            ev: name!("sfx_cha_C019_grenade_tell"),
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: None,
        });
    }
}

/// C019_CookGrenade Blocked (ubergraph 4696, no parent call): BlockingActor = the actor, Detonate Server(0), which ends
/// in K2_DestroyActor.
fn grenade_blocked(w: &mut World, id: ProjectileId, by: ActorRef) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    if p.detonated || p.dead {
        return;
    }
    detonate(w, id, 0, Some(by));
    if w.actors.projectiles.get(id).is_some_and(|p| !p.dead) {
        destroy_projectile(w, id, false);
    }
}

/// C019_CookGrenade ReceiveDestroyed: DetonateOnDeath -> Detonate Server (also the fuse timer's and the arena exit's
/// call; it ends in K2_DestroyActor).
/// No ReceiveEndPlay of its own: the parent's EndPlay (@15512 DestroyChildrenWithIgnore) runs before ReceiveDestroyed
/// (native), so the buff children (the passive rows kept) are gone before this hit's HitEnemyPlayer
/// (`actor_hit_enemy`).
fn grenade_destroyed(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    if !flag(class_p(w, p), "DetonateOnDeath") || p.disarmed || p.detonated {
        return;
    }
    if !reset_pass(w) {
        end_actor_children(w, ActorRef::Projectile(id));
    }
    detonate(w, id, 0, None);
}

fn grenade_kind() -> ProjectileHooks {
    ProjectileHooks {
        make: Some(grenade_make),
        step: Some(grenade_tick),
        moved: Some(grenade_moved),
        hit_by: Some(grenade_hit_by),
        blocked: Some(grenade_blocked),
        destroyed: Some(grenade_destroyed),
        ..ProjectileHooks::default()
    }
}

// ---- C019BlasterChargeProjectile ----

/// C019BlasterChargeProjectile FindClosestTarget: the closest C019_CookGrenade (children included) of the shot's own
/// instigator that is not being destroyed, among the SpawnedActors of the owner's and its allies' spawners.
pub fn closest_grenade(w: &World, id: ProjectileId) -> Option<ProjectileId> {
    let pr = w.actors.projectiles.get(id)?;
    let mut best = None;
    let mut bd = 1_000_000.0;
    for (gid, g) in w.actors.projectiles.iter() {
        if !g.grenade || g.dead || g.owner != pr.owner {
            continue;
        }
        let d = size2d([g.x - pr.x, g.y - pr.y]);
        if d < bd {
            bd = d;
            best = Some(gid);
        }
    }
    best
}

/// AimActorEnd (bound to the target's end): unbind, FindClosestTarget again -> the next grenade, or fly on along
/// unit(TravelStart -> location).
fn charge_shot_retarget(w: &mut World, id: ProjectileId) {
    if let Some(p) = w.actors.projectiles.get_mut(id) {
        p.aim_bound = false;
    }
    match closest_grenade(w, id) {
        Some(g) => crate::actors::projectile::aim_at_grenade(w, id, g),
        None => crate::actors::projectile::fly_on(w, id),
    }
}

/// Whether a projectile is gone (dead, or out of the arena).
fn gone(w: &World, g: ProjectileId) -> bool {
    w.actors.projectiles.get(g).is_none_or(|p| p.dead)
}

/// C019BlasterChargeProjectile per frame: the target's end, then MoveToTimeline. When the timeline finishes (ubergraph
/// 664): the target's Detonate Server(BombCount), whose K2_DestroyActor runs AimActorEnd at once (the next target's
/// MoveToCookedGrenade still sees the old BombCount), then UpdateVisualsMulticast, BombCount + 1 and the hit update.
/// The timeline is not stopped when the target ends early and no other grenade is out, so it still finishes on the
/// flying shot and raises BombCount and the hit (the dead target's once-gate keeps its Detonate Server from doing
/// anything).
// Deviation: AimActorEnd is bound to the target's OnEndPlay (ubergraph 5432 -> 2216) and runs inside the target's
// K2_DestroyActor; here a target that ended is noticed in the shot's own tick. Nothing moves the shot in between, so
// the next target's timeline starts from the same place and ticks first in the same frame; the differences are the next
// target's place (read up to a frame later) and, for a target that ended after the shot's tick, the fly-on's first move
// (a frame late; whether a ProjectileMovement activated in the middle of a frame moves in that frame is not read).
fn charge_shot_tick(w: &mut World, id: ProjectileId) -> bool {
    let Some(p) = w.actors.projectiles.get(id) else { return false };
    if p.aim_bound && p.aim_at.is_some_and(|g| gone(w, g)) {
        charge_shot_retarget(w, id);
    }
    if !crate::actors::projectile::timeline_tick(w, id) {
        return false;
    }
    let Some(p) = w.actors.projectiles.get(id) else { return false };
    let (aim, bombs, bound) = (p.aim_at, p.bomb_count, p.aim_bound);
    if let Some(g) = aim
        && !gone(w, g)
    {
        detonate(w, g, bombs, None);
        destroy_projectile(w, g, false);
        if bound {
            charge_shot_retarget(w, id);
        }
    }
    if let Some(p) = w.actors.projectiles.get_mut(id) {
        crate::actors::projectile::bomb_arrived(p);
    }
    false
}

/// C019BlasterChargeProjectile ReceiveBeginPlay, then the spawner's BPI_SetInitialMovement.
fn charge_shot_begin(w: &mut World, id: ProjectileId) {
    match closest_grenade(w, id) {
        Some(g) => crate::actors::projectile::aim_at_grenade(w, id, g),
        None => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                crate::actors::projectile::start_moving(p);
            }
        }
    }
    crate::actors::projectile::initial_movement(w, id);
}

/// C019BlasterChargeProjectile HitEnemyPlayer (no parent call): HitVFX = SpawnEmitterAtLocation(PS_Blaster_Impact, the
/// hit player's actor location, scale 1) with Team.
fn charge_shot_hit_enemy(w: &mut World, id: ProjectileId, vic: FighterId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let (cls, fid) = (p.cls, p.owner_id);
    let (x, y) = (w.fighter(vic).body.x, actor_y(w, vic));
    w.events.push(impact_event(cls, fid, x, y));
}

/// A `projectileDeath` of PS_Blaster_Impact at scale 1.
fn impact_event(cls: Name, fighter: i32, x: Real, y: Real) -> Event {
    Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(name!("PS_Blaster_Impact")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    }
}

/// C019BlasterChargeProjectile ReceiveEndPlay / OnReleasedToPool (ubergraph 3511): an AimAtActor that is still alive
/// and within 80 u is detonated with the shot's BombCount.
fn charge_shot_destroyed(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let (aim, bombs, px, py) = (p.aim_at, p.bomb_count, p.x, p.y);
    let Some(g) = aim else { return };
    let Some(a) = w.actors.projectiles.get(g) else { return };
    if a.dead || size2d([a.x - px, a.y - py]) > 80.0 {
        return;
    }
    detonate(w, g, bombs, None);
    destroy_projectile(w, g, false);
    if let Some(p) = w.actors.projectiles.get_mut(id) {
        p.bomb_count += 1;
    }
}

/// C019BlasterChargeProjectile ReceiveEndPlay / OnReleasedToPool: SpawnEmitterAtLocation(PS_Blaster_Impact, location,
/// scale 1) with Team and sfx_cha_C020_blaster_hit at the location (SFX_Charged, which picks
/// sfx_cha_C020_blaster_charged_hit, is only set by SetFullChargeTrail, which nothing calls).
fn charge_shot_end(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let (cls, fid, x, y) = (p.cls, p.owner_id, p.x, p.y);
    w.events.push(impact_event(cls, fid, x, y));
    w.events.push(sound_at(fid, name!("sfx_cha_C020_blaster_hit"), x, y));
}

/// A `sound` event at a place.
fn sound_at(fighter: i32, ev: Name, x: Real, y: Real) -> Event {
    Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None }
}

/// MulticastReflect @4068: the parent's, then @1549 with ParticleSystem valid @1511 ParticleSystem.Activate(true): the
/// drawing's (not drawn on a reflect). Nothing of the sim.
fn reflect_multicast(_w: &mut World, _a: ActorRef, _by: FighterId, _pause: bool) {}

/// A `reflected` override the graph has with nothing to do.
fn reflected_nothing(_w: &mut World, _a: ActorRef) {}

fn charge_shot_kind() -> ProjectileHooks {
    ProjectileHooks {
        begin: Some(charge_shot_begin),
        step: Some(charge_shot_tick),
        hit_enemy: Some(charge_shot_hit_enemy),
        destroyed: Some(charge_shot_destroyed),
        end: Some(charge_shot_end),
        reflect: ReflectHooks { multicast: Some(reflect_multicast), ..ReflectHooks::default() },
        ..ProjectileHooks::default()
    }
}

// ---- C019_SplitterProjectile (and its children), C019_SplitterFollowup (and its child) ----

/// The C019 shots' own BeginPlay / EndPlay / Destroyed handlers.
/// C019_SplitterFollowup ReceiveBeginPlay: sfx_cha_C019_splitter_cast and sfx_cha_C019_splitter_loop on the actor.
/// ReceiveEndPlay: sfx_cha_C019_splitter_death at the actor's location and sfx_cha_C019_splitter_loop_stop.
/// C019_SplitterProjectile (and its children) ReceiveEndPlay: sfx_cha_C019_gun_death at the location;
/// ReceiveDestroyed: HitVFX = SpawnEmitterAtLocation(PS_Blaster_Impact, actor location, scale 1) with Team.
// Unverified: every end of these actors is taken as a real destroy (ReceiveDestroyed runs; the classes also have the
// pool handlers InitFromPool / OnReleasedToPool, whose use by Comp_Spawner is not decoded), and the loop is stopped by
// its key with the port's default fade instead of the stop event's own action.
fn followup_begin(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let (fid, seq, x, y) = (p.owner_id, p.seq, p.x, p.y);
    w.events.push(sound_at(fid, name!("sfx_cha_C019_splitter_cast"), x, y));
    w.events.push(Event::SoundStart {
        fighter: fid,
        ev: name!("sfx_cha_C019_splitter_loop"),
        key: SoundKey { tag: name!("proj"), n: seq },
    });
}

fn followup_end(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let (fid, seq, x, y) = (p.owner_id, p.seq, p.x, p.y);
    w.events.push(sound_at(fid, name!("sfx_cha_C019_splitter_death"), x, y));
    // (sfx_cha_C019_splitter_loop_stop: a Stop action with a 150 ms fade)
    w.events.push(Event::SoundStop {
        fighter: fid,
        key: SoundKey { tag: name!("proj"), n: seq },
        fade: Some(0.15),
        ev: None,
        curve: None,
        cut: false,
    });
}

/// c019_SplitterFollowup Reflected @1252 (the child has no graph of its own): the parent's (empty),
/// K2_PauseTimerHandle(LifetimeTimer), Delay(0.375) (latent link 1282 -> 15), K2_UnPauseTimerHandle. Nothing writes
/// LifetimeTimer (xref writes: none): both calls do nothing.
fn followup_kind() -> ProjectileHooks {
    ProjectileHooks {
        begin: Some(followup_begin),
        end: Some(followup_end),
        reflect: ReflectHooks { reflected: Some(reflected_nothing), ..ReflectHooks::default() },
        ..ProjectileHooks::default()
    }
}

/// C019_SplitterProjectile after Delay(DelayBeforeSplit) (ubergraph 682): SplitVisuals, and with CanSplit the
/// instigator's Comp_Spawner.CompSpawnActor(Spiltter) twice, at the actor's location + OffsetProjectile1 / 2 (809 /
/// 1139: world axes with WorldTransforms = true, so no facing flip, and world Y is sim -x: the ordinary offsets, Y 50
/// both, are 50 towards sim -x whatever the facing; rotation 0) with Projectile 1 / 2 Movement Direction (mirrored by
/// the instigator's facing at the split, as in every CompSpawnActor: HandleActorPostSpawn 5786-5890) and speed 3000;
/// then K2_DestroyActor (no death effects).
/// SplitVisuals (ubergraph 4029): SpawnEmitterAtLocation(ImpactVFX, actor location, rotation 0, SplitterVisualsScale)
/// with Team = 0.6 for a local player, else the instigator's colour index (the same pick as a DeathVFX's).
/// Both calls pass AttackID 0 (the 13th argument, @918 / @1248), which HandleActorPostSpawn replaces with the attack
/// count of the spawner's owner at that moment (Comp_Spawner HandleActorPostSpawn 2444-2747: BPI_GetPlayerAttackCount
/// of the pawn's PlayerState; set on the actor at 9104, or InitFromPool 3399): the instigator's count at the split, not
/// the splitter's own AttackID.
/// The follow-ups tick in the frame of the split (the splitter's Delay fires in its own tick; an actor spawned inside
/// an actor tick ticks in that frame): the World's step steps them.
pub fn split_projectile(w: &mut World, id: ProjectileId) {
    let Some(pr) = w.actors.projectiles.get(id) else { return };
    let Some(owner) = pr.owner else { return };
    let (px, py, cls, fid) = (pr.x, pr.y, pr.cls, pr.owner_id);
    let p = class_p(w, pr).clone();
    if let Some(vfx) = p.get("ImpactVFX").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        let vfx = crate::registry::props::name(w, vfx);
        w.events.push(Event::ProjectileDeath {
            cls,
            fighter: fid,
            vfx: Some(vfx),
            x: px,
            y: py,
            ox: 0.0,
            rot: [0.0, 0.0, 0.0],
            scale: num_in(&p, "SplitterVisualsScale", "X").unwrap_or(1.0),
            tint: None,
            to: None,
        });
    }
    let Some(sp) = p.get("Spiltter").and_then(Value::as_str).filter(|s| !s.is_empty()) else { return };
    let sp = crate::registry::props::name(w, sp);
    let def = actor_def(w, ActorRef::Fighter(owner), sp).map(|a| a.p.clone());
    if !flag(&p, "CanSplit") {
        return;
    }
    let Some(ap) = def else { return };
    let class = w.registry.actors.get(w.names.str(sp)).map(|e| e.class);
    let Some(ActorClass::Projectile(class)) = class else {
        let m = format!("spawned actor not implemented ({})", w.names.str(sp));
        warn_text(w, &m);
        return;
    };
    for k in [1, 2] {
        let o = obj(&p, &format!("OffsetProjectile{k}")).cloned().unwrap_or_default();
        let d = obj(&p, &format!("Projectile {k} Movement Direction")).cloned().unwrap_or_default();
        actor_spawn_ability(w, owner, &ap);
        let mut n = Props::new();
        n.insert("InitialMovementDirection".into(), Value::Obj(d));
        n.insert("InitialSpeed".into(), Value::Num(3000.0));
        n.insert("MaxSpeed".into(), Value::Num(3000.0));
        let at = V2 { x: px - num(&o, "Y").unwrap_or(0.0), y: py + num(&o, "Z").unwrap_or(0.0) };
        let count = w.fighter(owner).decay.attack_count;
        let made = make_projectile(w, sp, class, Some(owner), count, &n, Some(at), MakeOpts::default());
        let fu = w.actors.projectiles.insert(made);
        list_projectile(w, fu);
    }
}

/// C019_SplitterProjectile OnProjectileBounce (@2689, bound to ProjectileMovement.OnProjectileBounce in BeginPlay
/// @327-350): the bounce sound on the actor, inside the move, for every impact.
fn splitter_bounce(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, _ivx: Real, _ivy: Real) -> bool {
    if let Some(p) = w.actors.projectiles.get(id) {
        let e = sound_at(p.owner_id, name!("sfx_cha_C019_gun_bounce"), p.x, p.y);
        w.events.push(e);
    }
    false
}

fn splitter_end(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    let (cls, fid, x, y) = (p.cls, p.owner_id, p.x, p.y);
    w.events.push(sound_at(fid, name!("sfx_cha_C019_gun_death"), x, y));
    w.events.push(impact_event(cls, fid, x, y));
}

/// c019_SplitterProjectile Reflected @3225 (its children have no graph of their own): the parent's (empty),
/// K2_PauseTimerHandle(LifetimeTimer), Delay(0.375) (latent link 3255 -> 2663), K2_UnPauseTimerHandle. Nothing writes
/// LifetimeTimer (xref writes: none): both calls do nothing.
fn splitter_kind() -> ProjectileHooks {
    ProjectileHooks {
        bounce_impact: Some(splitter_bounce),
        split: Some(split_projectile),
        end: Some(splitter_end),
        reflect: ReflectHooks { reflected: Some(reflected_nothing), ..ReflectHooks::default() },
        ..ProjectileHooks::default()
    }
}

// ---- C019_WhipProjectile ----

/// C019_WhipProjectile ActivateLassoTipper (ubergraph 12315, LassoTipperActive is set in BeginPlay):
/// SphereOverlapActors(the actor's location, 175, object type Pawn, BP_FighterCharacter) and pfgDoDamage(
/// LassoTipperDamage) through the whip's hitbox handler for each (CanBeHitBy: the other team only). The hit has
/// HitOnlyOneFighter. The self hit on the instigator that follows the first hit (Damage 0, BaseKnockBack 3000) fails
/// CanBeHitBy (same team), and BPI_AddWhipTargetForVisuals is empty on Morty.
// Deviation: the sphere is tested against the fighter's root capsule in the plane; the fighters are taken in index
// order and the first one hit ends the loop; the knockback's side is the actor's yaw, still the outgoing direction (the
// return velocity has not moved it yet).
pub fn whip_tipper(w: &mut World, id: ProjectileId) {
    let me = ActorRef::Projectile(id);
    let Some(pr) = w.actors.projectiles.get(id) else { return };
    let Some(owner) = pr.owner else { return };
    let (px, py, attack_id) = (pr.x, pr.y, pr.attack_id);
    let team = w.fighter(owner).team.team;
    let lasso = obj(class_p(w, pr), "LassoTipperDamage").cloned().unwrap_or_default();
    let hit = HitPacket::from_props(&w.names, &lasso, None);
    for vic in match_fighters(w) {
        let v = w.fighter(vic);
        if v.team.team == team || v.life.dead > 0.0 || crate::combat::phases::no_hit_by_team(w, team, vic) {
            continue;
        }
        let (vx, vy) = (v.body.x, actor_y(w, vic));
        let h = capsule_half(w, vic) - cap_radius(w, vic);
        let dx = px - vx;
        let dy = py - max(vy - h, min(py, vy + h));
        if size2d([dx, dy]) > 175.0 + cap_radius(w, vic) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let tick = w.flow.tick;
            attack_dodged(w, me, vic, attack_id, num(&lasso, "BaseKnockBack").unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        // HitOnlyOneFighter ends the loop on a hit only (a failed one leaves it on).
        let d = dealer(w, me);
        let cp = charge_pct(w, me);
        if apply_projectile_hit(w, &d, vic, &hit, &[], V2 { x: d.x, y: d.y }, None, &[], cp) {
            actor_hit_enemy(w, &d, vic);
            break;
        }
    }
}

/// The class's values of the tether family (`actors::projectile` whip_tick has the offsets): BeginPlay's Delay(0.2);
/// ReturnMovementServer's MaxSpeed 6000 and velocity x 6000 towards the instigator mesh's socket 'Attach Point NameL'
/// (the spawner notify's socket); the HitBox sphere deals as the parent hitbox (no HitBox2D); SpawnSnapEffect at the
/// Delay.
fn whip_opts(_w: &World, _f: FighterId, _r: &SpawnRequest) -> MakeOpts {
    MakeOpts {
        tether: Some(TetherParams {
            travel: 0.2,
            return_speed: 6000.0,
            return_max_speed: Some(6000.0),
            socket: None,
            hit_sphere: Some(name!("HitBox")),
            snap: true,
        }),
        ..MakeOpts::default()
    }
}

/// ReceiveBeginPlay's last link (push_flow 9516 @8696): BPI_ProcessTutorialEvent(UniqueCharacterMechanic0) @9612.
fn whip_begin(w: &mut World, _id: ProjectileId) {
    tutorial_event(w, TutorialEvent::UniqueCharacterMechanic0);
}

fn whip_moved(w: &mut World, id: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(id) else { return };
    if p.dead {
        return;
    }
    let Some(owner) = p.owner else { return };
    // C019_WhipProjectile OwnerKnockedBack (bound to the owner's KnockedBack in BeginPlay): K2_DestroyActor.
    // Deviation: taken at the actor's next tick, not inside the hit.
    // SpawnSnapEffect (13131): SpawnEmitterAtLocation(PS_C019_NAN_ReturnBurst, the Cylinder's location (the root's),
    // rotation 0, scale 0.75) with Team = SelectFloat(0.6, the colour index, IsLocalOnlinePlayer), the DeathVFX's rule.
    let (cls, fid, x, y, snapped, due, kb_seq) = (p.cls, p.owner_id, p.x, p.y, p.snapped, p.tipper_due, p.kb_seq);
    if snapped {
        w.events.push(Event::ProjectileDeath {
            cls,
            fighter: fid,
            vfx: Some(name!("PS_C019_NAN_ReturnBurst")),
            x,
            y,
            ox: 0.0,
            rot: [0.0, 0.0, 0.0],
            scale: 0.75,
            tint: None,
            to: None,
        });
    }
    if w.fighter(owner).attackers.knocked_back_count != kb_seq {
        destroy_projectile(w, id, false);
    } else if due {
        whip_tipper(w, id);
    }
}

fn whip_kind() -> ProjectileHooks {
    ProjectileHooks {
        make_opts: Some(whip_opts),
        begin: Some(whip_begin),
        moved: Some(whip_moved),
        ..ProjectileHooks::default()
    }
}

/// This file's registry entries (the ten projectile classes' kinds). The follow-up classes are named by the splitters'
/// Spiltter alone (no notify spawns one).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use crate::actors::ProjectileClass as C;
    let entry = |class: C, hooks: ProjectileHooks, foreign: bool| {
        let mut e = ActorEntry::new(ActorClass::Projectile(class), ActorHooks::Projectile(hooks));
        e.foreign_owner = foreign;
        e
    };
    // BPI_ReverseMoveDirection @0 -> ubergraph @6501 -> @5737-5789 (ProjectileMovement.IsActive) -> @4873-4945:
    // Velocity x (1, -1, 1) (`combat::reverse`; the children take it as data children).
    r.actors.enter("C019_CookGrenade_C", entry(C::C019CookGrenade, grenade_kind(), false))?;
    r.actors.enter("C019_CookGrenade_NoGrav_C", entry(C::C019CookGrenadeNoGrav, grenade_kind(), false))?;
    r.actors.enter("c019_CookGrenade_Split_C", entry(C::C019CookGrenadeSplit, grenade_kind(), false))?;
    // foreignOwner (the copied gun neutrals, ground and air): these three read the owner's generic fields alone
    // (charge, facing, velocity, use2D, attackCount, boxes, team, id, ability slot 0) and the shot's target is a
    // grenade of its own owner; no MortyState.
    r.actors.enter("C019BlasterChargeProjectile_C", entry(C::C019BlasterChargeProjectile, charge_shot_kind(), true))?;
    r.actors.enter("C019_SplitterProjectile_C", entry(C::C019SplitterProjectile, splitter_kind(), true))?;
    r.actors.enter(
        "C019_SplitterProjectile_OnCooldown_C",
        entry(C::C019SplitterProjectileOnCooldown, splitter_kind(), true),
    )?;
    r.actors.enter(
        "c019_HorizontalSplitterProjectile_C",
        entry(C::C019HorizontalSplitterProjectile, splitter_kind(), false),
    )?;
    r.actors.enter("C019_SplitterFollowup_C", entry(C::C019SplitterFollowup, followup_kind(), false))?;
    r.actors.enter("c019_SplitterFollowup_Child_C", entry(C::C019SplitterFollowupChild, followup_kind(), false))?;
    r.actors.enter("C019_WhipProjectile_C", entry(C::C019WhipProjectile, whip_kind(), false))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blast_lerp_ends() {
        assert_eq!(blast_lerp(0.0), 1.0);
        assert_eq!(blast_lerp(-1.0), 0.25);
        assert_eq!(blast_lerp(1.0), 1.25);
    }

    #[test]
    fn default_grenade_has_no_perk_radius() {
        let g = CookGrenade::default();
        assert!(!g.ignore_platforms_check && !g.use_cook_duration && !g.use_max_radius);
    }

    #[test]
    fn max_i_picks_larger() {
        assert_eq!(max_i(0, -1), 0);
        assert_eq!(max_i(3, 2), 3);
    }
}
