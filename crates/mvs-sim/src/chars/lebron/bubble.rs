//! LeBron's sass bubble and his fence: C016_SassBubble_C ([`SassBubble`], a projectile's
//! `ext`; its Box tested inside ActivateObject, [`box_hits`]) and C016ProjectileBarrier_C (a child of the shared
//! ProjectileBarrier, `actors/barrier.rs`, with the hooks [`BARRIER`]).
//!
//! Sources: c016_SassBubble (the neutral attack without the ball, C016_NoBall_Attack_Normal_Neutral_Montage),
//! C016ProjectileBarrier (a child of ProjectileBarrier: C016_Special_BoxOut_Montage, C016_Special_Burst_Montage),
//! Comp_Spawner (the attach, SetChargeVariables), the class defaults and component templates of the data.
//!
//! `SassBubble`'s getters `hitboxActive` / `hitCollision` (false) and its `activate` (no Delay) are in
//! `actors/projectile.rs`, by the `ext` variant.

use super::pawn;
use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, hurt_x, hurt_y, set_hitbox};
use crate::actors::{ActorClass, ActorId, ActorRef, ProjectileClass, ProjectileId, WorldActorClass};
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::{charge_buff, charge_pct};
use crate::combat::hits::{
    DamageCall, actor_ally_hit, actor_hit_enemy, apply_projectile_hit, dealer, do_damage, projectile_blocks,
};
use crate::combat::phases::{alive, give_vbuf, los_blocked, victim_pose};
use crate::combat::spawned_hits::{actor_body_types, class_rows, comp, projectile_hits_projectile, world_hooks};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{body_info, box_overlaps_actor};
use crate::fighter::{FighterId, actor_y, invulnerable};
use crate::math::max;
use crate::math::ue::size2d;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::num;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;

const BUBBLE: &str = "C016_SassBubble_C";
const FENCE: &str = "C016ProjectileBarrier_C";
const BALL: &str = "BasketBallProjectile_C";
const BALL_SHOT: &str = "BasketBallProjectile_Shot_C";
const BALL_PASS: &str = "BasketBallProjectile_Pass_C";

// A ParentProjectilePhysics child (EnableProjectileMovement false) attached to the owner's Root socket, 150 up, at
// scale 5 (Comp_Spawner HandleActorPostSpawn @10581 / @10515): the world's Projectile is the parent (the attach,
// Die With Instigator, the notify's DestroyAllActorsOnNotifyEnd).
// Its hit is dealt by `Box`, a box component with the tag HitBox and no collision in its template. The class's
// HitBox sphere has the tag too but bGenerateOverlapEvents false, and HitBox2D is not used (no
// Supports2DHitboxes): neither ever overlaps anything. So the parent's circle is off for good here (hitboxActive
// and hitCollision are false) and the Box is tested inside ActivateObject: as it has grown, and again at HitBoxScale.
// Unverified: UBoxComponent's default BoxExtent (the template names none).
const BOX_EXTENT: Real = 32.0;
/// Box.RelativeScale3D (0.9, 0.5, 0.5).
const BOX_SCALE_X: Real = 0.9;
const BOX_SCALE_YZ: Real = 0.5;
/// ReceiveTick @904: Box.RelativeLocation = (f x 50, 0, 0), in the root's scale.
const BOX_FORWARD: Real = 50.0;

/// `SassBubble extends Projectile`: what the class adds.
#[derive(State, Debug, Default)]
pub struct SassBubble {
    /// The Box's world scale along the owner's forward (X) and across the stage's depth (Y): the root's 5 times the
    /// template's (4.5, 2.5), then grown each tick. Z keeps its 2.5 (@686 passes the old Z).
    pub box_x: Real,
    pub box_y: Real,
    pub box_z: Real,
    /// The Box's relative X, set by the first tick (0 before it: the template has no location).
    pub box_fwd: Real,
    /// VSizeXY(GrowthScale) of the last tick that grew (0: GrowthScale's default).
    pub growth_len: Real,
    /// ParticleSystem1's world scale (the bubble that is drawn): 5 x 0.175, grown at half the rate.
    pub ps_scale: Real,
    /// Box.K2_DestroyComponent @1279.
    pub box_gone: bool,
}

/// `Rect`: the Box in the play plane, its centre and half sizes; `hd` the half size across the stage's depth (the
/// Box's Y).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: Real,
    pub y: Real,
    pub hx: Real,
    pub hy: Real,
    pub hd: Real,
}

fn pr_ref(w: &World, pr: ProjectileId) -> &crate::actors::Projectile {
    w.actors.projectiles.get(pr).expect("a live projectile")
}

fn st(w: &World, pr: ProjectileId) -> &SassBubble {
    match &pr_ref(w, pr).ext {
        ProjectileExt::SassBubble(s) => s,
        _ => panic!("a sass bubble"),
    }
}

fn st_mut(w: &mut World, pr: ProjectileId) -> &mut SassBubble {
    match &mut w.actors.projectiles.get_mut(pr).expect("a live projectile").ext {
        ProjectileExt::SassBubble(s) => s,
        _ => panic!("a sass bubble"),
    }
}

/// `SassBubble.box()`: the Box in the play plane, as it has grown.
fn box_rect(w: &World, pr: ProjectileId) -> Rect {
    let (p, s) = (pr_ref(w, pr), st(w, pr));
    Rect {
        x: p.x + s.box_fwd * p.scale * p.facing,
        y: p.y,
        hx: BOX_EXTENT * s.box_x,
        hy: BOX_EXTENT * s.box_z,
        hd: BOX_EXTENT * s.box_y,
    }
}

/// `SassBubble.scaledBox()`: the Box behind ActivateHitbox's SetRelativeScale3D(HitBoxScale) (Comp_Pawn_HitBoxHandler
/// @16855): the relative scale under the root's, at the same relative location. `None` for an equal relative scale
/// (no transform update, so no second overlap update).
fn scaled_box(w: &World, pr: ProjectileId) -> Option<Rect> {
    let (p, s) = (pr_ref(w, pr), st(w, pr));
    let h = class_p(w, p).get("HitBoxScale").and_then(mvs_data::value::Value::as_obj);
    let axis = |k: &str| h.and_then(|h| num(h, k)).unwrap_or(1.0);
    let (sx, sz) = ((p.scale * axis("X")), (p.scale * axis("Z")));
    let sy = p.scale * axis("Y");
    if sx == s.box_x && sy == s.box_y && sz == s.box_z {
        return None;
    }
    Some(Rect {
        x: p.x + s.box_fwd * p.scale * p.facing,
        y: p.y,
        hx: BOX_EXTENT * sx,
        hy: BOX_EXTENT * sz,
        hd: BOX_EXTENT * sy,
    })
}

fn pr_dead(w: &World, pr: ProjectileId) -> bool {
    w.actors.projectiles.get(pr).is_none_or(|p| p.dead)
}

/// `boxHits`: the Box against everything it overlaps when UpdateHitbox turns it on (@1265 -> SpawnedActorParent @15092
/// -> HitBoxesOn @8531: every component with a tag of HitBoxTags -> the handler's ActivateHitbox), by the handler's
/// OnComponentBeginOverlap. Two overlap updates inside that call: SetCollisionEnabled(3) @16817 with the Box as it
/// has grown, then SetRelativeScale3D(HitBoxScale) @16855, which writes (1, 1, 1) over the grown relative scale
/// (0.9 k^n, 0.5 k^n, 0.5): half sizes 160 each under the root's 5, narrower than a long charge's Box and twice as
/// tall. What both hold is hit by the first (HitEachActorOnlyOnce). Each update meets:
/// - the components of the world's actors that a HitBox-type component overlapping ProjectileDestructible begins an
///   overlap with (`hit_box_on`): an enemy ProjectileBarrier's StopProjectiles (C016ProjectileBarrier: its begin
///   destroys the bubble, DestroyOnBlocked, and hands its owner his ball) and WW_ShieldActor's BlockCollision
///   (Blocked: the bubble's destroy waits for the parent's Delay(0.0), so the hits below still come); then the Box's
///   own pfgDoDamage on each actor whose bodies it begins to overlap (`box_actor_hits`: VelmaEvidence's Destructible,
///   a shield's BlockCollision: the Buff_ArmorPack row to a team mate's);
/// - the other team's fighters (HitBoxDamage with the actor's charge values; DoLOSCheck, the invulnerable test,
///   HitEachActorOnlyOnce), at HitLocation = the Box's K2_GetComponentLocation (Comp_Pawn_HitBoxHandler @8448), its
///   centre ahead of the actor;
/// - a team mate (`actor_ally_hit`: the Buff_ArmorPack row, then NotifyAllyOfHit);
/// - the projectiles' HurtBoxes (DestroyProjectiles; a clank with CanProjectileClank on both);
/// - the thrown items' Sphere (ItemDestructible, which the Box overlaps: `do_damage`, as a projectile's hit box).
///
/// The owner's own hurtboxes overlap too and nothing follows (no row of his without a hit).
/// Public for Velma_SassBubblev2, whose Box is the same component template, grown and
/// turned on the same way by its own graph.
// Unverified: the order of one overlap update's begin events (ComponentOverlapMulti's result order) is not read: the
// actors' components first, then the fighters, the projectiles and the items, each in list order.
// Unverified: that no begin overlap of the Box is dispatched once a barrier's K2_DestroyActor ended the bubble inside
// the update (UpdateOverlapsImpl 14297ec80's loop over the new overlaps against a pending-kill component is not read).
// Deviation: the Box is a 3D box (160 deep at the play plane) against 3D hurt capsules; tested in the plane.
// Deviation: of the world's actors a hit box can damage only the bodies of the traces are tested (`box_actor_hits`):
// the pawn bodies, hurt boxes and damage boxes of the classes without such bodies are not.
pub fn box_hits(w: &mut World, pr: ProjectileId, b: Rect) {
    let me = ActorRef::Projectile(pr);
    let shape = WorldSphere {
        x: b.x,
        y: b.y,
        z: Some(0.0),
        r: 0.0,
        bx: Some([b.hx, 0.0, 0.0, 0.0, b.hy, 0.0, 0.0, 0.0, b.hd]),
        ..WorldSphere::default()
    };
    let mut i = 0;
    while let Some(a) = w.actors.actors.nth(i) {
        i += 1;
        if w.actors.actors.get(a).is_some_and(|x| !x.dead)
            && let Some(ActorEntry { hooks: ActorHooks::World(h), .. }) =
                crate::actors::world_actor::actor_entry(w, ActorRef::Actor(a))
            && let Some(hit_box_on) = h.hit_box_on
        {
            hit_box_on(w, a, pr, name!("Box"), &shape);
        }
        if pr_dead(w, pr) {
            return;
        }
        box_actor_hits(w, pr, a, b);
        if pr_dead(w, pr) {
            return;
        }
    }
    let Some(f) = pr_ref(w, pr).owner else { return };
    let rows = class_rows(w, me);
    for o in match_fighters(w) {
        let oid = w.fighter(o).id;
        if o == f || !alive(w, o) || crate::combat::phases::no_hit_by(w, f, o) || pr_ref(w, pr).hit_set.contains(&oid) {
            continue;
        }
        let Some(vp) = victim_pose(w, o) else { continue };
        let met = crate::combat::hit::hurt::box_hurt_hit(w, o, b.x, b.y, b.hx, b.hy, &vp);
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        let (px, py, los, attack_id, each_once, bkb) = {
            let p = pr_ref(w, pr);
            (
                p.x,
                p.y,
                p.hit.do_los_check.unwrap_or(false),
                p.attack_id,
                p.hit.hit_each_actor_only_once.unwrap_or(false),
                p.hit.base_knock_back.unwrap_or(0.0),
            )
        };
        let (ox, oy) = (w.fighter(o).body.x, w.fighter(o).body.y);
        if los && los_blocked(w, px, py, ox, oy) {
            continue;
        }
        if projectile_blocks(w, &dealer(w, me), o) {
            continue;
        }
        let ally = w.fighter(o).team.team == w.fighter(f).team.team;
        if invulnerable(w, o) {
            if !ally {
                let tick = w.flow.tick;
                attack_dodged(w, me, o, attack_id, bkb, tick, None, false);
            }
            continue;
        }
        dodge_over(w, me, o);
        if each_once {
            w.actors.projectiles.get_mut(pr).expect("a live projectile").hit_set.insert(oid);
        }
        if ally {
            // The ally rows and NotifyAllyOfHit (`actor_ally_hit`).
            let (d, hit) = (dealer(w, me), pr_ref(w, pr).hit.clone());
            actor_ally_hit(w, &d, o, &hit, &rows, charge_buff(w, me));
            continue;
        }
        // The Buff_C016_Ball_Conditional row (ApplyBuffSelf?, RequireHitToApplyBuffToSelf) goes to the Instigator
        // behind this hit (`hit_buffs_hit`).
        let hit = pr_ref(w, pr).hit.clone();
        let d = dealer(w, me);
        if apply_projectile_hit(w, &d, o, &hit, &rows, V2 { x: b.x, y: b.y }, None, &[], charge_pct(w, me)) {
            actor_hit_enemy(w, &dealer(w, me), o);
        }
    }
    let mut i = 0;
    while let Some(o) = w.actors.projectiles.nth(i) {
        i += 1;
        if o == pr {
            continue;
        }
        let op = pr_ref(w, o);
        if op.dead || op.hurt_radius <= 0.0 {
            continue;
        }
        let r = op.hurt_radius;
        let dx = max((hurt_x(op) - b.x).abs() - b.hx, 0.0);
        let dy = max((hurt_y(op) - b.y).abs() - b.hy, 0.0);
        if dx * dx + dy * dy > r * r {
            continue;
        }
        projectile_hits_projectile(w, pr, o, false);
        if pr_dead(w, pr) {
            return;
        }
    }
    let mut i = 0;
    while let Some(it) = w.actors.items.nth(i) {
        i += 1;
        let item = w.actors.items.get(it).expect("a live item");
        let r = item.hurt_radius();
        if item.dead || r <= 0.0 {
            continue;
        }
        let dx = max((item.x - b.x).abs() - b.hx, 0.0);
        let dy = max((item.y - b.y).abs() - b.hy, 0.0);
        if dx * dx + dy * dy > r * r {
            continue;
        }
        let hit = pr_ref(w, pr).hit.clone();
        do_damage(
            w,
            me,
            ActorRef::Item(it),
            &hit,
            &DamageCall { hit_box: true, comp: Some(name!("Box")), clank: false, rows: None },
        );
        if pr_dead(w, pr) {
            return;
        }
    }
}

/// The Box's begin of an overlap with the actor `a`'s bodies of the traces (its kind's `bodies`), as a hit box dealer's
/// pass over the actors that are no fighter (`combat::hits::hit_box_actor_hits`): the bodies of the object types the
/// Box's template answers (`actor_body_types`: HurtBox among them), kept where the body's template answers the Box's
/// type (HitBox); HitEachActorOnlyOnce's track (the projectile's `hit_actors`, which its HitBox sphere shares:
/// Velma_SassBubblev2), DoLOSCheck from the Box to the actor (@3298), then pfgDoDamage with the handler's rows
/// (`do_damage`: the class's On Hit Buffs, so the Buff_ArmorPack row on a team mate's actor through the ally rows
/// @5931-6835).
// Unverified: the Box's two overlap updates of one activation keep no overlap list between them here: an actor both
// meet is dealt to once by HitEachActorOnlyOnce (true in both classes' HitBoxDamage), not by the overlap going on.
fn box_actor_hits(w: &mut World, pr: ProjectileId, a: ActorId, b: Rect) {
    let (me, ar) = (ActorRef::Projectile(pr), ActorRef::Actor(a));
    let Some(x) = w.actors.actors.get(a) else { return };
    if x.dead || world_hooks(w, x.cls).and_then(|h| h.bodies).is_none() {
        return;
    }
    let hit = pr_ref(w, pr).hit.clone();
    let each_once = hit.hit_each_actor_only_once.unwrap_or(false);
    if each_once && pr_ref(w, pr).hit_actors.contains(&ar) {
        return;
    }
    let box_comp = comp(w, me, "Box");
    let types = actor_body_types(box_comp);
    let responder = body_info(box_comp, None).map_or(7, |i| i.kind); // OBJ.HitBox
    if types.is_empty()
        || !box_overlaps_actor(w, b.x, b.y, b.hx, b.hy, b.hd, &types, &[Some(me)], crate::real(responder), ar)
    {
        return;
    }
    if hit.do_los_check.unwrap_or(false)
        && let Some((vx, vy, _)) = crate::actors::world_actor::actor_place(w, ar)
        && los_blocked(w, b.x, b.y, vx, vy)
    {
        return;
    }
    if each_once {
        w.actors.projectiles.get_mut(pr).expect("a live projectile").hit_actors.insert(ar);
    }
    do_damage(w, me, ar, &hit, &DamageCall { hit_box: true, comp: Some(name!("Box")), clank: false, rows: None });
}

// ---- C016_SassBubble

fn make(_w: &World, _f: FighterId, _r: &SpawnRequest) -> ProjectileExt {
    ProjectileExt::SassBubble(SassBubble::default())
}

fn begin(w: &mut World, pr: ProjectileId) {
    let scale = pr_ref(w, pr).scale;
    let s = st_mut(w, pr);
    s.box_x = scale * BOX_SCALE_X;
    s.box_y = scale * BOX_SCALE_YZ;
    s.box_z = s.box_y;
    s.ps_scale = scale * 0.175;
}

/// ReceiveTick @115 (the parent's first), while not Activated: f = 1 + DeltaSeconds x GrowthValue; GrowthScale = the
/// Box's world scale x f @420-560; Box.SetWorldScale3D(GrowthScale.X, GrowthScale.Y, the old Z) @686;
/// Box.K2_SetRelativeLocation((f x 50, 0, 0)) @904 (from f alone: it does not add up); ParticleSystem1's world scale
/// x (1 + DeltaSeconds x GrowthValue x 0.5) @1182.
// Unverified: the actor's delta is taken as the world's (it is attached to the owner but has its own time dilation).
fn step(w: &mut World, pr: ProjectileId) -> bool {
    if pr_ref(w, pr).activated {
        return false;
    }
    let g = num(class_p(w, pr_ref(w, pr)), "GrowthValue").unwrap_or(0.0);
    let k = 1.0 + (DELTA_SECONDS * g);
    let s = st_mut(w, pr);
    s.box_x *= k;
    s.box_y *= k;
    s.growth_len = size2d([s.box_x, s.box_y]);
    s.box_fwd = k * BOX_FORWARD;
    s.ps_scale *= 1.0 + ((DELTA_SECONDS * g) * 0.5);
    false
}

/// ActivateObject @1317, once per call of the owner's AnimNotify_ActivateSpawned (the montage has one; its
/// UpdateCharge has Comp_Spawner copy the owner's charge values into the actor first, @3727, in the world's activate
/// loop): Activated @1228, SetVisuals(true) @1239, HitBoxOn? = true @1254, UpdateHitbox @1265, Box.K2_DestroyComponent
/// @1279.
/// SetVisuals @1366: the text emitter at the Instigator's location + (80, 0, 150) (80 toward the camera) with his
/// rotation: PS_C016_Text02 at scale 1 while VSizeXY(GrowthScale) < 10, else PS_C016_Text at (1.5, 1.25, 1.25)
/// (Unverified: the event carries one scale, 1.5; the drawing may read the name); then FX_Blast:
/// sfx_cha_C016_neutral_cast at the actor (both sides of its 13.472 test post the same event).
/// The Box's destroy ends its overlaps (UPrimitiveComponent::OnComponentDestroyed -> ClearComponentOverlaps). No graph
/// of the reachable set has a handler with an effect for an end overlap of a hit box (the handler binds the begin
/// alone; Buff_ElementalParent's end bodies are empty), and the port keeps no overlap of the Box: it is tested inside
/// this call and gone.
fn activate(w: &mut World, pr: ProjectileId) {
    if st(w, pr).box_gone {
        return;
    }
    let Some(f) = pr_ref(w, pr).owner else { return };
    let big = st(w, pr).growth_len >= 10.0;
    let (cls, px, py) = {
        let p = pr_ref(w, pr);
        (p.cls, p.x, p.y)
    };
    let (fid, fx, fy) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(if big { name!("PS_C016_Text") } else { name!("PS_C016_Text02") }),
        x: fx,
        y: fy + 150.0,
        ox: 0.0,
        rot: [0.0; 3],
        scale: if big { 1.5 } else { 1.0 },
        tint: None,
        to: None,
    });
    w.events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C016_neutral_cast"),
        x: Some(px),
        y: Some(py),
        hit: None,
        pooled: false,
        key: None,
    });
    // the HitBoxOn? variable (BPI_CanProjectileClank reads it); the getters stay false
    set_hitbox(w.actors.projectiles.get_mut(pr).expect("a live projectile"), true);
    let b = box_rect(w, pr);
    box_hits(w, pr, b);
    if let Some(scaled) = scaled_box(w, pr)
        && !pr_dead(w, pr)
    {
        box_hits(w, pr, scaled);
    }
    st_mut(w, pr).box_gone = true;
}

fn bubble_kind() -> ProjectileHooks {
    ProjectileHooks {
        make: Some(make),
        begin: Some(begin),
        step: Some(step),
        activate: Some(activate),
        ..ProjectileHooks::default()
    }
}

// ---- C016ProjectileBarrier

/// `isBall`: the ShouldActorBeBlockedAndDestroyed of C016ProjectileBarrier tests the actor as a BasketBallProjectile
/// by a soft reference @57 or by ClassIsChildOf @202: any team's ball, his own included. The port has no class tree:
/// the two children his data has are named here (ball.rs enters each as a kind of its own, with or without
/// `parents`), and a child entered later passes by its entry's `parents`.
pub fn is_ball(w: &World, cls: Name) -> bool {
    let s = w.names.str(cls);
    matches!(s, BALL | BALL_SHOT | BALL_PASS) || w.registry.actors.get(s).is_some_and(|e| e.parents.contains(&BALL))
}

/// ShouldActorBeBlockedAndDestroyed (C016ProjectileBarrier): the parent's result @0, or any team's ball. The rest is
/// the parent's graph (`actors/barrier.rs`: BPI_Blocked, the block sound and PS_HitImpact_Directional, the other's
/// destroy, then the owner's BPI_GetBallBack behind BlockedBufforMontage, for whatever was blocked).
fn blocks(w: &World, a: ActorId, o: ActorRef) -> bool {
    crate::actors::barrier::barrier_blocks(w, a, o)
        || crate::actors::world_actor::actor_cls(w, o).is_some_and(|c| is_ball(w, c))
}

/// The fence's getBallBack, BaseFighter-side: C016's BPI_GetBallBack (`pawn::get_ball_back`: the ball buff and the
/// receive montage). The blocked ball itself is destroyed by the barrier (the ball's own end play: ball.rs). Another
/// ball of his still in flight is left alone, as in the build.
fn fence_ball_back(w: &mut World, _a: ActorId, owner: FighterId) {
    pawn::get_ball_back(w, owner);
}

/// C016ProjectileBarrier's hooks over the shared barrier's (`projectileBarrier({ blocks, getBallBack })`).
pub const BARRIER: crate::actors::barrier::BarrierHooks =
    crate::actors::barrier::BarrierHooks { blocks: Some(blocks), get_ball_back: Some(fence_ball_back), begin: None };

/// This file's registry entries (C016_SassBubble_C, C016ProjectileBarrier_C with `barrier::entry`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        BUBBLE,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::C016SassBubble), ActorHooks::Projectile(bubble_kind())),
    )?;
    r.actors.enter(FENCE, crate::actors::barrier::entry(WorldActorClass::C016ProjectileBarrier))
}
