//! A fighter's hit shapes against the spawned actors, the items and the stage's placed actors (the melee tests of
//! the overlap code), and pfgDoDamage of a fighter's Comp_Pawn_HitBoxHandler on such a HitActor
//! from @15084 on (`fighterHitsProjectile`, `fighterHitsItem`, `fighterHitsActor`).

use crate::Real;
use crate::actors::ActorId;
use crate::actors::ActorRef;
use crate::actors::ItemId;
use crate::actors::ProjectileId;
use crate::actors::item as itm;
use crate::actors::projectile as prj;
use crate::combat::HitPacket;
use crate::combat::boxes::{
    self, CH_ALL, CH_DAMAGE, CH_HURT, CH_ITEM, CH_PAWN, CH_PROJ, WorldSphere, obj, seg_box_dist2, shape_over_box,
    shape_over_capsule,
};
use crate::combat::buffs as buff;
use crate::combat::elem;
use crate::combat::hit as fh;
use crate::combat::hits;
use crate::combat::phases::{self, Active, SphereKey};
use crate::combat::reflect;
use crate::combat::spawned_hits as sh;
use crate::consts as C;
use crate::fighter::FighterId;
use crate::fighter::anim;
use crate::fighter::common::{owner_team, warn_text};
use crate::fighter::moves::V2;
use crate::fighter::traces::body_info;
use crate::math::max;
use crate::math::to_int32;
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal, CondSignal};
use crate::world::World;
use crate::world::items;
use crate::world::spawning;
use crate::world::stage_actors as sa;

/// A Hitframe's hit-set key of a placed stage actor, less its index: apart from the fighters' ids, the spawned
/// actors' 1000 + id and the items' -1 - id.
const STAGE_HIT_KEY: i32 = -1_000_000_000;

/// The hit-set key of a projectile or a World.actors actor (`1000 + id`).
fn actor_key(seq: i32) -> i32 {
    1000 + seq
}

/// The hit-set key of an item (`-1 - id`).
fn item_key(seq: i32) -> i32 {
    -1 - seq
}

/// The sampled boxes of a test, sampled at the first Hitframe that needs them (`pose === undefined`);
/// `Some(None)`: the fighter has none.
type Pose = Option<Option<Vec<f32>>>;

fn pose_of<'p>(w: &World, att: FighterId, pose: &'p mut Pose) -> Option<&'p [f32]> {
    pose.get_or_insert_with(|| phases::sample(w, att)).as_deref()
}

/// A test's pose back to the World's pool.
fn put_pose(w: &World, pose: Pose) {
    if let Some(Some(p)) = pose {
        crate::fighter::anim::put_pose(w, p);
    }
}

/// The packet's KnockBackDirection, {X: 1, Y: 0} without one (a missing part is NaN).
fn kb_dir(q: &HitPacket) -> (Real, Real) {
    match &q.knock_back_direction {
        None => (1.0, 0.0),
        Some(d) => (d.x.unwrap_or(Real::NAN), d.y.unwrap_or(Real::NAN)),
    }
}

fn facing(w: &World, f: FighterId) -> Real {
    w.fighter(f).body.facing_dir
}

fn team(w: &World, f: FighterId) -> i32 {
    w.fighter(f).team.team
}

/// `entryStops`: pfgDoDamage of a fighter's Hitframe on a spawned actor, behind the hit-once record: the entry rows
/// (@842-2821), then the stops with the dealer in hitstun (@5257, IgnoreHitStun @4848) and HitOnlyOneFighter
/// (@3236-3288). True: the call goes on.
fn entry_stops(w: &mut World, att: FighterId, p: &HitPacket, met: &WorldSphere) -> bool {
    if !p.on_hit_buffs.is_empty() {
        hits::hit_buffs_call(w, att, &p.on_hit_buffs);
    }
    if hits::in_hitstun(w.fighter(att)) && !met.ignore_stun {
        return false;
    }
    !(p.hit_only_one_fighter.unwrap_or(false) && w.fighter(att).decay.has_hit_a_fighter)
}

/// `dealerHitPause`: the dealer's hit pause on a Hit on a victim with no Comp_Actor_Damage (@16603-17442:
/// HitPauseTime with the Hitframe's charge part, x the victim DamageManager's HitPauseMultiplier 1), x `scale` (a
/// stage actor's).
fn dealer_hit_pause(w: &mut World, att: FighterId, p: &HitPacket, charged_time: Real, scale: Option<Real>) {
    let raw = p.hit_pause_time.unwrap_or(0.0) + charged_time * p.charge_hit_pause_mult.unwrap_or(0.0);
    if raw.is_nan() || raw <= 0.0 || p.apply_hit_pause_to_damage_dealer == Some(false) {
        return;
    }
    let sec = match scale {
        Some(s) => raw * C::HIT_PAUSE_MULT * s,
        None => raw * C::HIT_PAUSE_MULT,
    };
    let frames = crate::real(fh::timer_count(sec));
    fh::dealt_hit_pause(w, att, frames, raw, p.queue_hit_pause_until_pose.unwrap_or(false), sec);
}

/// `hurtTarget`: a projectile a hit box can hit: IsProjectile on its Comp_Actor_Destructible.
fn hurt_target(w: &World, pr: ProjectileId) -> bool {
    sh::comp(w, ActorRef::Projectile(pr), "Comp_Actor_Destructible").is_some_and(|d| hits::truthy(d, "IsProjectile"))
}

/// `sphereMeets`: a sphere of a hit box (`hitSpheres`) against a sphere at (x, y) in the play plane of radius r.
fn sphere_meets(s: &WorldSphere, x: Real, y: Real, r: Real) -> bool {
    let rr = s.r + r;
    if s.bx.is_some() {
        return seg_box_dist2(s, x, y, 0.0, x, y, 0.0) <= rr * rr;
    }
    let (ex, ey, ez) = (s.x - x, s.y - y, s.z.unwrap_or(0.0));
    ex * ex + ey * ey + ez * ez <= rr * rr
}

/// `HURT_CH`: an object type as `hitSpheres`' channel.
fn hurt_ch(kind: i32) -> Option<u8> {
    match kind {
        obj::PAWN => Some(CH_PAWN),
        obj::DAMAGE_DESTRUCTIBLE => Some(CH_DAMAGE),
        obj::PROJECTILE_DESTRUCTIBLE => Some(CH_PROJ),
        obj::ITEM_DESTRUCTIBLE => Some(CH_ITEM),
        obj::HURT_BOX => Some(CH_HURT),
        _ => None,
    }
}

/// `actorHurtChannel`: the object type of a projectile's or a World.actors actor's HurtBox body as `hitSpheres`'
/// channel: the class's HurtBox component template; ProjectileBarrier's and its children's StopProjectiles
/// (ProjectileDestructible) where the class has no HurtBox.
/// Deviation: a class whose data has no such component, or whose body is of none of the five object types the HitBox
/// profile overlaps, keeps the rule from before the detectors: every hit shape meets it (CH_ALL).
fn actor_hurt_channel(w: &World, a: ActorRef) -> u8 {
    let c = sh::comp(w, a, "HurtBox").or_else(|| sh::comp(w, a, "StopProjectiles"));
    body_info(c, None).and_then(|i| hurt_ch(i.kind)).unwrap_or(CH_ALL)
}

/// A hit shape's channels (`s.ch ?? CH_ALL`).
fn ch_of(s: &WorldSphere) -> i32 {
    s.ch.unwrap_or(i32::from(CH_ALL))
}

// ---- the projectiles and the World.actors with a HurtBox

/// `meleeProjectileTest`: a fighter's Hitframes over a projectile's hurt sphere
/// (IsProjectile on its Comp_Actor_Destructible, a grenade, or the sphere an actor buff added at its root) run
/// pfgDoDamage with the projectile as HitActor (`fighterHitsProjectile`); then over the HurtBox of an IsProjectile
/// actor of World.actors (its kind's `hurt_box`: `fighterHitsActor`). Each Hitframe hits an actor once (its hit
/// set, the actor's id + 1000); the entry rows and the stops (`entryStops`), then the dealer's dealt pass
/// (@5617-5904) ahead of the direction.
/// Not ported: the hitbox shapes are tested in the play plane; the direction is the Hitframe's with the dealer's
/// facing (no KnockBackMirror), BaseKnockBack without charge.
pub fn melee_projectile_test(w: &mut World, att: FighterId) {
    let mut pose: Pose = None;
    melee_projectile_test_with(w, att, &mut pose);
    put_pose(w, pose);
}

/// [`melee_projectile_test`] with the attacker's pose, sampled at its first use.
fn melee_projectile_test_with(w: &mut World, att: FighterId, pose: &mut Pose) {
    if !phases::alive(w, att) || w.fighter(att).montages.active_hits.is_empty() {
        return;
    }
    let mut n = 0;
    while let Some(pr) = w.actors.projectiles.nth(n) {
        n += 1;
        let prr = ActorRef::Projectile(pr);
        let p = w.actors.projectiles.get(pr).expect("a live projectile");
        if p.dead {
            continue;
        }
        let (hurt, grenade, has_elem) = (p.hurt_radius, p.grenade, p.elem.is_some());
        let hr = if hurt > 0.0 && (grenade || hurt_target(w, pr)) { hurt } else { 0.0 };
        // a buff's added sphere at the root
        let br = if has_elem { elem::actor_hurt_sphere(w, prr) } else { 0.0 };
        if hr <= 0.0 && br <= 0.0 {
            continue;
        }
        let key = actor_key(w.actors.projectiles.get(pr).expect("a live projectile").seq);
        let mut hc: Option<u8> = None;
        let hs = phases::take_actives(w, att);
        for h in hs.iter() {
            if sh::dead(w, prr) || phases::hit_set_has(w, att, h.key, key) {
                continue;
            }
            let Some(pv) = pose_of(w, att, pose) else {
                phases::give_actives(w, hs);
                return;
            };
            // The HurtBox on its class's object type, the buff's AdditionalHurtBox on ProjectileDestructible.
            let hcv = *hc.get_or_insert_with(|| if hr > 0.0 { actor_hurt_channel(w, prr) } else { 0 });
            let chan = hcv | if br > 0.0 { CH_PROJ } else { 0 };
            let mut spheres = phases::take_spheres(w);
            phases::hit_spheres_into(w, att, &h.p, pv, chan, 0.0, Real::INFINITY, None, &mut spheres);
            let pj = w.actors.projectiles.get(pr).expect("a live projectile");
            let over = spheres
                .iter()
                .find(|s| {
                    let c = ch_of(s);
                    (hr > 0.0 && (c & i32::from(hcv)) != 0 && sphere_meets(s, prj::hurt_x(pj), prj::hurt_y(pj), hr))
                        || (br > 0.0 && (c & i32::from(CH_PROJ)) != 0 && sphere_meets(s, pj.x, pj.y, br))
                })
                .cloned();
            phases::give_spheres(w, spheres);
            let Some(over) = over else { continue };
            phases::hit_set_add(w, att, h.key, key);
            if !entry_stops(w, att, &h.p, &over) {
                continue;
            }
            // @5617-5904: the dealer's dealt pass with the projectile as Victim, ahead of the direction.
            let q = buff::apply_dealt_damage_mods(w, att, prr, &h.p, true);
            let (kx, ky) = kb_dir(&q);
            let dx = kx * facing(w, att);
            // @15158-15462: ReflectProjectiles -> the dealer's Comp_ReflectProjectile.ReflectProjectile(HitActor, the
            // hit); reflect timing: ahead in the fighter pass, behind in detectProjectileHits.
            fighter_hits_projectile(w, att, pr, &q, dx, ky, true, h.charged);
        }
        phases::give_actives(w, hs);
    }
    let mut n = 0;
    while let Some(b) = w.actors.actors.nth(n) {
        n += 1;
        let bref = ActorRef::Actor(b);
        let x = w.actors.actors.get(b).expect("a live actor");
        if x.dead {
            continue;
        }
        let (cls, seq) = (x.cls, x.seq);
        let Some(hurt_box) = sh::world_hooks(w, cls).and_then(|k| k.hurt_box) else {
            let destructible = sh::comp(w, bref, "Comp_Actor_Destructible");
            if sh::comp(w, bref, "HurtBox").is_some() && destructible.is_some_and(|d| hits::truthy(d, "IsProjectile")) {
                let c = w.names.str(cls).to_owned();
                warn_text(
                    w,
                    &format!(
                        "{c}: an IsProjectile actor of World.actors with a HurtBox has no hurtBox entry (a fighter's hit box does not meet it)"
                    ),
                );
            }
            continue;
        };
        let key = actor_key(seq);
        let mut hb_ch: Option<u8> = None;
        let hs = phases::take_actives(w, att);
        for h in hs.iter() {
            if sh::dead(w, bref) || phases::hit_set_has(w, att, h.key, key) {
                continue;
            }
            let Some(hb) = hurt_box(w, b) else { break };
            let ch = *hb_ch.get_or_insert_with(|| actor_hurt_channel(w, bref));
            let Some(pv) = pose_of(w, att, pose) else {
                phases::give_actives(w, hs);
                return;
            };
            let met = phases::hit_sphere_find(w, att, &h.p, pv, ch, |s| {
                if hb.bx.is_some() { shape_over_box(s, &hb) } else { sphere_meets(s, hb.x, hb.y, hb.r) }
            });
            let Some(met) = met else { continue };
            phases::hit_set_add(w, att, h.key, key);
            if !entry_stops(w, att, &h.p, &met) {
                continue;
            }
            let q = buff::apply_dealt_damage_mods(w, att, bref, &h.p, true); // @5617-5904
            fighter_hits_actor(w, att, b, &q, V2 { x: met.x, y: met.y }, true, h.charged);
        }
        phases::give_actives(w, hs);
    }
}

/// `fighterHitsActor`: pfgDoDamage of fighter `att`'s handler with the actor `b` of
/// World.actors as HitActor and the packet `q` (the dealt pass made), from @15084 on: a ReflectProjectiles hit's
/// reflect fails (@15158, on to @15491); without DestroyProjectiles HitByAlly / HitByEnemy alone (@19122, which
/// SpawnedActorParent ignores); else DamageAndLaunch (`actorDamageAndLaunch`) and on a Hit the dealer's hit pause, the
/// Hit's rows (@18293-19121), HitNonPlayer and HitAnything (@22812 -> @22088 -> @22208). Also the call of a buff's
/// graph on the owner's handler (Buff_DodgeReflect @1115). `at`: LocalHitLocation; `hit_box`: the call has a
/// HitBox; `charged_time`: the Hitframe's, for the dealer's hit pause.
pub fn fighter_hits_actor(
    w: &mut World,
    att: FighterId,
    b: ActorId,
    q: &HitPacket,
    at: V2,
    hit_box: bool,
    charged_time: Real,
) {
    let bref = ActorRef::Actor(b);
    let x = w.actors.actors.get(b).expect("a live actor");
    let (cls, b_owner) = (x.cls, x.owner);
    // NotifyAllyOfHit, at the end of the call in the build: a team mate's actor takes no damage (CanBeHitBy), so
    // nothing of the steps below comes between.
    hits::fighter_ally_actor_hit(w, att, bref, Some(b_owner), q);
    if q.reflect_projectiles.unwrap_or(false) {
        reflect::reflect_world_actor(w, b); // @15158: Success false, on to @15491
    }
    if !q.destroy_projectiles.unwrap_or(false) {
        return; // @19122: HitByAlly / HitByEnemy alone, which SpawnedActorParent ignores
    }
    let dmg = q.damage.unwrap_or(0.0) * w.fighter(att).charge.charge_dmg_mult;
    let t = team(w, att);
    if !sh::actor_damage_and_launch(w, b, t, q, dmg, ActorRef::Fighter(att), att, at, true) {
        return; // CanBeHitBy refused (a DamageDisabled barrier): no ApplyDamage, no ReceiveAnyDamage
    }
    if dmg != 0.0 && sh::world_hooks(w, cls).and_then(|k| k.any_damage).is_none() {
        let c = w.names.str(cls).to_owned();
        warn_text(w, &format!("{c}: ReceiveAnyDamage not ported (a fighter's DestroyProjectiles hit)"));
    }
    dealer_hit_pause(w, att, q, charged_time, None); // @16603-17442
    if !sh::dead(w, bref) && !q.on_hit_buffs.is_empty() {
        let mult = w.fighter(att).charge.charge_buff_mult;
        elem::hit_actor_rows_typed(w, att, bref, &q.on_hit_buffs, mult); // the Hit's rows @18293-19121
    }
    anim::cond_signal(w, att, CondSignal::HitNonPlayer); // DamageAndLaunch's Hit -> HitNonPlayer @22812
    hits::fighter_hit_anything(w, att, bref, q, hit_box); // @22812 -> @22088 -> HitAnything @22208
}

/// `fighterHitsProjectile`: pfgDoDamage of fighter `att`'s handler with the projectile
/// `pr` as HitActor and the packet `p`, from @15084 on: `dx`, `dy` the packet's knockback direction in the sim's
/// axes (the facing applied). Also the call of a buff's graph on the owner's handler (Buff_DodgeReflect @1115).
/// `hit_box`: the call has a hit box (HitAnything's HitBoxInfo.HitBox); `charged_time`: the Hitframe's, for the
/// dealer's hit pause on a Hit. NotifyAllyOfHit (@8400 -> @21344) at the end, whichever way the call ended, with the
/// actor's team as it is then: a projectile the call reflected is of the reflector's team.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn fighter_hits_projectile(
    w: &mut World,
    att: FighterId,
    pr: ProjectileId,
    p: &HitPacket,
    dx: Real,
    dy: Real,
    hit_box: bool,
    charged_time: Real,
) {
    hits_projectile(w, att, pr, p, dx, dy, hit_box, charged_time);
    let owner = w.actors.projectiles.get(pr).and_then(|x| x.owner);
    hits::fighter_ally_actor_hit(w, att, ActorRef::Projectile(pr), owner, p);
}

#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
fn hits_projectile(
    w: &mut World,
    att: FighterId,
    pr: ProjectileId,
    p: &HitPacket,
    dx: Real,
    dy: Real,
    hit_box: bool,
    charged_time: Real,
) {
    let prr = ActorRef::Projectile(pr);
    // @15158-15462, gated on the HitActor's IsProjectile (@15084-15144): a success broadcasts the handler's
    // ReflectedProjectile and ends the call, a failure goes on to @15491.
    if p.reflect_projectiles.unwrap_or(false)
        && hits::dealer_is_projectile(w, &hits::dealer(w, prr))
        && reflect::reflect_projectile(w, att, prr, V2 { x: -dx, y: dy }, None)
    {
        if let Some(h) = w.fighter(att).hooks.char.reflected_projectile {
            h(w, att, prr); // @15462
        }
        return;
    }
    let destroy = p.destroy_projectiles.unwrap_or(false);
    let mut ally = team(w, att) == owner_team(w, prr);
    // DamageAndLaunch's CanBeHitBy (@659): DamageDisabled, FriendlyFire; false @718 -> @8047.
    if destroy {
        let mut d = sh::can_be_hit_by_d(sh::comp(w, prr, "Comp_Actor_Destructible"));
        if let Some(dd) = w.actors.projectiles.get(pr).expect("a live projectile").damage_disabled {
            let ff = d.as_ref().is_some_and(|d| d.friendly_fire);
            d = Some(sa::CanBeHitByD { damage_disabled: dd, friendly_fire: ff });
        }
        ally = !sa::can_be_hit_by(team(w, att), owner_team(w, prr), d);
    }
    let has_elem = |w: &World| w.actors.projectiles.get(pr).is_some_and(|x| x.elem.is_some());
    if !ally && destroy {
        // DamageAndLaunch's taken pass on the actor's buffs (@1878) ahead of ApplyDamage @2591.
        let dmg = p.damage.unwrap_or(0.0) * w.fighter(att).charge.charge_dmg_mult;
        // DamageAndLaunch's Hit -> HitNonPlayer @22812, after the destroy in the build; the signal writes the
        // condition objects alone.
        anim::cond_signal(w, att, CondSignal::HitNonPlayer);
        let taken = if has_elem(w) { elem::actor_taken_damage(w, prr, dmg, att) } else { dmg };
        if sh::projectile_any_damage(w, pr, taken, 0, ActorRef::Fighter(att)) {
            spawning::destroy_projectile(w, pr, false);
            crate::fighter::tick::controller_rumble(w, att, p.controller_rumble); // @3589-3668
            dealer_hit_pause(w, att, p, charged_time, None); // @16603-17442: the Hit stands
            hits::fighter_hit_anything(w, att, prr, p, hit_box); // @22812 -> @22088 -> HitAnything @22208
            return;
        }
    }
    // The allowed hit's rumble on the attacker (@3589-3668), ahead of HitByEnemy @3723.
    if !ally && destroy {
        crate::fighter::tick::controller_rumble(w, att, p.controller_rumble);
    }
    // HitByAlly / HitByEnemy's Attacker: the dealer (Comp_Pawn_HitBoxHandler @19466-19486, DamageManager @8047).
    let bkb = p.base_knock_back.unwrap_or(0.0);
    hits::hit_by(w, pr, ally, dx, dy, bkb, -1, Some(ActorRef::Fighter(att)));
    // HitByEnemy's other bindings: the actor's buffs.
    if !ally && !sh::dead(w, prr) && has_elem(w) {
        let arg = ActorBuffArg { by: att, kx: -dx, ky: dy };
        elem::actor_buff_signal(w, prr, ActorBuffSignal::HitByEnemy, Some(&arg));
    }
    // A hit CanBeHitBy refused with ForceKnockback (@8516 goto 3222): the allowed hit's tail after its HitByAlly,
    // without ApplyDamage: the attacker's rumble (@3589-3668), HitByEnemy @3723 (the class's and the actor's buffs),
    // Hit false (no pause, no rows, no HitAnything). The reaction's light-hit sound is not made, as on the allowed hit.
    if ally && destroy && p.force_knockback.unwrap_or(false) {
        crate::fighter::tick::controller_rumble(w, att, p.controller_rumble);
        if !sh::dead(w, prr) {
            hits::hit_by(w, pr, false, dx, dy, bkb, -1, Some(ActorRef::Fighter(att)));
        }
        if !sh::dead(w, prr) && has_elem(w) {
            let arg = ActorBuffArg { by: att, kx: -dx, ky: dy };
            elem::actor_buff_signal(w, prr, ActorBuffSignal::HitByEnemy, Some(&arg));
        }
        return;
    }
    if !ally && destroy {
        dealer_hit_pause(w, att, p, charged_time, None); // @16603-17442
    }
    // Hit: the rows for the HitActor (@18293-19121).
    if !ally && destroy && !sh::dead(w, prr) && !p.on_hit_buffs.is_empty() {
        let mult = w.fighter(att).charge.charge_buff_mult;
        elem::hit_actor_rows_typed(w, att, prr, &p.on_hit_buffs, mult);
    }
    if !ally && destroy {
        hits::fighter_hit_anything(w, att, prr, p, hit_box); // @22812 -> @22088 -> HitAnything @22208
    }
}

// ---- the items

/// `meleeItemTest`: a fighter's Hitframes over an owned item's Sphere (profile
/// ItemDestructible, on while its hurt radius is above 0) or the sphere a buff added at its root out of a hand (the
/// same centre) run pfgDoDamage with the item as HitActor (`fighterHitsItem`). The hit-once record (key -1 - the
/// item's id) and the entry rows come first, then the stops (`entryStops`) and the dealt pass (@5617-5904).
/// Deviation: tested at the end of the fighter's update (beside meleeProjectileTest) and in each overlaps pass
/// after the projectiles moved, at the item's last position (the items move later in the step), items in list order.
/// Deviation: the first overlapping entry of the active Hitframes takes the item; meleeTest takes the first box in
/// attach order, so a SweetSpotHitframe's normal set wins on an item here (it matters for Item_C017_Art alone: the
/// other ported items die to any damage).
pub fn melee_item_test(w: &mut World, att: FighterId) {
    let mut pose: Pose = None;
    melee_item_test_with(w, att, &mut pose);
    put_pose(w, pose);
}

/// [`melee_item_test`] with the attacker's pose, sampled at its first use.
fn melee_item_test_with(w: &mut World, att: FighterId, pose: &mut Pose) {
    if !phases::alive(w, att) || w.fighter(att).montages.active_hits.is_empty() || w.actors.items.is_empty() {
        return;
    }
    let mut n = 0;
    while let Some(it) = w.actors.items.nth(n) {
        n += 1;
        let ir = ActorRef::Item(it);
        if sh::dead(w, ir) {
            continue;
        }
        // (a stage item no one has picked up has no Sphere on)
        if !itm::is_owned(w, it) {
            continue;
        }
        let i = w.actors.items.get(it).expect("a live item");
        let (has_elem, holder, seq) = (i.elem.is_some(), i.holder, i.seq);
        let br = if has_elem && holder < 0 { elem::actor_hurt_sphere(w, ir) } else { 0.0 };
        let hurt = itm::item_hurt_radius(w, it);
        let hr = max(hurt, br);
        if hr <= 0.0 {
            continue;
        }
        let key = item_key(seq);
        let hs = phases::take_actives(w, att);
        for h in hs.iter() {
            if sh::dead(w, ir) || phases::hit_set_has(w, att, h.key, key) {
                continue;
            }
            let Some(pv) = pose_of(w, att, pose) else {
                phases::give_actives(w, hs);
                return;
            };
            // The Sphere (ItemDestructible) and the buff's sphere (ProjectileDestructible: AdditionalHurtBox) at one
            // centre.
            let chan = CH_ITEM | if br > 0.0 { CH_PROJ } else { 0 };
            let i = w.actors.items.get(it).expect("a live item");
            let (x, y) = (i.x, i.y);
            let over = phases::hit_sphere_find(w, att, &h.p, pv, chan, |s| {
                let c = ch_of(s);
                ((c & i32::from(CH_ITEM)) != 0 && hurt > 0.0 && sphere_meets(s, x, y, hurt))
                    || ((c & i32::from(CH_PROJ)) != 0 && br > 0.0 && sphere_meets(s, x, y, br))
            });
            let Some(over) = over else { continue };
            phases::hit_set_add(w, att, h.key, key);
            if !entry_stops(w, att, &h.p, &over) {
                continue;
            }
            let q = buff::apply_dealt_damage_mods(w, att, ir, &h.p, true); // @5617-5904
            let (dx, dy) = kb_dir(&q);
            let kx = -dx * facing(w, att); // the packet's X (UE Y), as meleeProjectileTest
            fighter_hits_item(w, att, it, &q, kx, dy, h.charged, true, V2 { x: over.x, y: over.y });
        }
        phases::give_actives(w, hs);
    }
}

/// `fighterHitsItem`: pfgDoDamage of fighter `att`'s handler with the item `it` as
/// HitActor and the packet `p`, from @15084 on: `kx`, `ky` the packet's KnockBackDirectionVector as the caller made it
/// (UE Y, Z), `charged_time` the Hitframe's for the dealer's hit pause. Also a buff graph's call on the owner's
/// handler (Buff_DodgeReflect @1115). `hit_box`: as fighterHitsProjectile's; `at`: where the hit box met the item
/// (the item's place for the buff's call), for the light hit's sound. NotifyAllyOfHit at the end.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn fighter_hits_item(
    w: &mut World,
    att: FighterId,
    it: ItemId,
    p: &HitPacket,
    kx: Real,
    ky: Real,
    charged_time: Real,
    hit_box: bool,
    at: V2,
) {
    hits_item(w, att, it, p, kx, ky, charged_time, hit_box, at);
    let owner = w.actors.items.get(it).and_then(|i| i.owner);
    hits::fighter_ally_actor_hit(w, att, ActorRef::Item(it), owner, p);
}

#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
fn hits_item(
    w: &mut World,
    att: FighterId,
    it: ItemId,
    p: &HitPacket,
    kx: Real,
    ky: Real,
    charged_time: Real,
    hit_box: bool,
    at: V2,
) {
    let ir = ActorRef::Item(it);
    let is_proj = itm::item_is_projectile(w, it);
    // @15158-15462 for an IsProjectile item (reached by no data today). Reflect timing: ahead in both passes (the
    // items move after detectProjectileHits).
    if is_proj
        && p.reflect_projectiles.unwrap_or(false)
        && reflect::reflect_projectile(w, att, ir, V2 { x: kx, y: ky }, None)
    {
        if let Some(h) = w.fighter(att).hooks.char.reflected_projectile {
            h(w, att, ir); // @15462: the handler's ReflectedProjectile
        }
        return;
    }
    let owner = w.actors.items.get(it).and_then(|i| i.owner).expect("an owned item");
    let owner_t = team(w, owner);
    let enemy = team(w, att) != owner_t;
    let has_elem = |w: &World| w.actors.items.get(it).is_some_and(|i| i.elem.is_some());
    let arg = ActorBuffArg { by: att, kx, ky };
    // @19122: HitByAlly / HitByEnemy alone; the enemy's reaches the item's buffs.
    if is_proj && !p.destroy_projectiles.unwrap_or(false) {
        if enemy && has_elem(w) {
            elem::actor_buff_signal(w, ir, ActorBuffSignal::HitByEnemy, Some(&arg));
        }
        return;
    }
    let d = sh::can_be_hit_by_d(sh::comp(w, ir, "Comp_Actor_Destructible"));
    if !sa::can_be_hit_by(team(w, att), owner_t, d) {
        // @659 -> @718: HitByAlly @8047 (no listener on an item), Hit false. With ForceKnockback (@8516 goto 3222)
        // the allowed hit's tail without ApplyDamage: the light path's PlayHitSound (@3468), the attacker's rumble
        // (@3589-3668), HitByEnemy @3723 (the item's buffs).
        if p.force_knockback.unwrap_or(false) {
            let dmg = p.damage.unwrap_or(0.0) * w.fighter(att).charge.charge_dmg_mult;
            let inst = w.fighter(owner).id;
            sh::light_hit_sound(w, p, dmg, att, at, true, inst);
            crate::fighter::tick::controller_rumble(w, att, p.controller_rumble);
            if !sh::dead(w, ir) && has_elem(w) {
                elem::actor_buff_signal(w, ir, ActorBuffSignal::HitByEnemy, Some(&arg));
            }
        }
        return;
    }
    // DamageAndLaunch's taken pass on the item's buffs (@1878) ahead of ApplyDamage, then its HitByEnemy.
    let mut dmg = p.damage.unwrap_or(0.0) * w.fighter(att).charge.charge_dmg_mult; // ItemHealth: float32
    if has_elem(w) {
        dmg = elem::actor_taken_damage(w, ir, dmg, att);
    }
    let gone = itm::item_receive_damage(w, it, dmg, false);
    items::item_throw_hits(w); // its ItemThrowHit (ReceiveAnyDamage @8495), inside ApplyDamage
    if gone && !sh::dead(w, ir) {
        items::destroy_item(w, it);
    }
    // AActor::TakeDamage: ReceiveAnyDamage (its destroy above included), then OnTakeAnyDamage with the pawn as the
    // causer.
    crate::world::flow::broadcast_take_any_damage(w, ir, dmg, Some(ActorRef::Fighter(att)));
    // The light path's PlayHitSound (@3468; no victim pause: the item has no Comp_Actor_Damage, @4012), the
    // attacker's rumble (@3589-3668), then HitByEnemy @3723.
    let inst = w.fighter(owner).id;
    sh::light_hit_sound(w, p, dmg, att, at, true, inst);
    crate::fighter::tick::controller_rumble(w, att, p.controller_rumble); // @3589-3668
    if !gone && !sh::dead(w, ir) && has_elem(w) {
        elem::actor_buff_signal(w, ir, ActorBuffSignal::HitByEnemy, Some(&arg));
    }
    dealer_hit_pause(w, att, p, charged_time, None);
    if !sh::dead(w, ir) && !p.on_hit_buffs.is_empty() {
        let mult = w.fighter(att).charge.charge_buff_mult;
        elem::hit_actor_rows_typed(w, att, ir, &p.on_hit_buffs, mult); // @18293-19121
    }
    anim::cond_signal(w, att, CondSignal::HitNonPlayer); // DamageAndLaunch's Hit -> HitNonPlayer @22812
    hits::fighter_hit_anything(w, att, ir, p, hit_box); // @22812 -> @22088 -> HitAnything @22208
}

// ---- the spheres actor buffs add to themselves

/// The buff `i` of actor `a`'s Comp_State_Manager, `None` past the list's end.
fn elem_buff(w: &World, a: ActorRef, i: usize) -> Option<&elem::ElemBuff> {
    let e = match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id)?.elem.as_ref(),
        ActorRef::Item(id) => w.actors.items.get(id)?.elem.as_ref(),
        _ => None,
    };
    e?.buffs.get(i)
}

fn buff_count(w: &World, a: ActorRef) -> usize {
    let e = match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).and_then(|x| x.elem.as_ref()),
        ActorRef::Item(id) => w.actors.items.get(id).and_then(|x| x.elem.as_ref()),
        _ => None,
    };
    e.map_or(0, |e| e.buffs.len())
}

fn buff_index(i: usize) -> u32 {
    u32::try_from(i).expect("a buff list of fewer than 2^32 entries")
}

/// The actor buff hooks of the buff `i` of `a`.
fn buff_hooks(w: &World, a: ActorRef, i: usize) -> Option<crate::registry::entries::ActorBuffHooks> {
    let b = elem_buff(w, a, i)?;
    w.registry.buffs.get(b.class.name(&w.names))?.actor
}

/// `meleeHitboxSphereTest`: a fighter's HitBox-type components against the spheres
/// actor buffs added to themselves (the buff kind's `hitbox_sphere`: Buff_GarnetStopProjectile's AdditionalHurtBox,
/// profile ProjectileDestructible, overlapping the HitBox channel alone): the BeginOverlap the class bound, sent as
/// HitboxOverlap to that buff (the class tests the fighter's team) once per begin of an overlap. The components: the
/// Hitframes' hit shapes while their collision is on (a dealer in hitstun too: the overlap does not deal) and the
/// pawn's always-on HitBox spheres (while the pawn's collision is on), each overlapping ProjectileDestructible. No
/// damage: the sphere's actor (the buff) has no Comp_Actor_Destructible. On the projectiles then the items, in list
/// order.
/// Deviation: the engine begins an overlap inside the move that makes it (the pawn's, a component's turned on); here
/// at the frame's two hit tests, as the hit boxes are; an overlap that begins and ends between two tests is not seen.
pub fn melee_hitbox_sphere_test(w: &mut World, att: FighterId) {
    let mut pose: Pose = None;
    for pass in 0..2 {
        let mut n = 0;
        loop {
            let a = if pass == 0 {
                w.actors.projectiles.nth(n).map(ActorRef::Projectile)
            } else {
                w.actors.items.nth(n).map(ActorRef::Item)
            };
            let Some(a) = a else { break };
            n += 1;
            if sh::dead(w, a) || buff_count(w, a) == 0 {
                continue;
            }
            if let ActorRef::Item(it) = a
                && !itm::is_owned(w, it)
            {
                continue;
            }
            let mut i = 0;
            while i < buff_count(w, a) {
                let k = buff_hooks(w, a, i);
                let at = buff_index(i);
                i += 1;
                let Some(k) = k else { continue };
                let Some(hs) = k.hitbox_sphere else { continue };
                let Some(sp) = hs(w, a, at) else { continue };
                if sp[2].is_nan() || sp[2] <= 0.0 || elem::hitbox_sphere_keys(w, a, at).is_none() {
                    continue;
                }
                if pose.is_none() {
                    let none = !phases::alive(w, att) || w.fighter(att).montages.active_hits.is_empty();
                    pose = Some(if none { None } else { phases::sample(w, att) });
                }
                let pv = pose.as_ref().and_then(Option::as_deref);
                // (the buff the signal ends is the one it went to, wherever its handler moved the list)
                let serial = elem_buff(w, a, i - 1).expect("a buff of the list").serial;
                if hitbox_sphere_overlaps(w, att, sp, a, at, pv)
                    && let Some(on) = k.on
                    && on(w, a, at, ActorBuffSignal::HitboxOverlap, Some(&ActorBuffArg { by: att, kx: 0.0, ky: 0.0 }))
                {
                    elem::end_actor_buff(w, a, serial);
                }
                if sh::dead(w, a) {
                    break;
                }
            }
        }
    }
    put_pose(w, pose);
}

/// `hitboxSphereBegin`: the overlaps a sphere of the buff kind's `hitbox_sphere` holds
/// when its class adds it (the buff's begin, after its keys are made): written into the keys with no begin sent,
/// for every pawn, minions too, as UpdateOverlaps @1378 records them.
pub fn hitbox_sphere_begin(w: &mut World, a: ActorRef, i: u32) {
    let at = usize::try_from(i).expect("a buff index");
    let Some(hs) = buff_hooks(w, a, at).and_then(|k| k.hitbox_sphere) else { return };
    if elem::hitbox_sphere_keys(w, a, i).is_none() {
        return;
    }
    let Some(sp) = hs(w, a, i) else { return };
    if sp[2].is_nan() || sp[2] <= 0.0 {
        return;
    }
    let mut nf = 0;
    while let Some(f) = hits::fighter_at(w, nf) {
        nf += 1;
        let none = !phases::alive(w, f) || w.fighter(f).montages.active_hits.is_empty();
        let pose = if none { None } else { phases::sample(w, f) };
        hitbox_sphere_overlaps(w, f, sp, a, i, pose.as_deref());
        put_pose(w, Some(pose));
    }
}

/// `hitboxSphereOverlaps`: `att`'s components met by the sphere `sp` (x, y, r) now, against the keys of the buff `i`
/// of `a`: true when one began (its key added); the keys of `att`'s components no longer met are taken out. `pose`:
/// `att`'s sampled boxes (`None`: no hit shape on).
fn hitbox_sphere_overlaps(
    w: &mut World,
    att: FighterId,
    sp: [Real; 3],
    a: ActorRef,
    i: u32,
    pose: Option<&[f32]>,
) -> bool {
    let aid = w.fighter(att).id;
    let mut now: Vec<SphereKey> = Vec::new();
    if phases::alive(w, att) {
        if let Some(pose) = pose {
            let hs = phases::take_actives(w, att);
            for h in hs.iter() {
                let met = phases::hit_sphere_find(w, att, &h.p, pose, CH_PROJ, |q| {
                    (ch_of(q) & i32::from(CH_PROJ)) != 0 && sphere_meets(q, sp[0], sp[1], sp[2])
                });
                if met.is_some() {
                    now.push(SphereKey::Hit(aid, h.key));
                }
            }
            phases::give_actives(w, hs);
        }
        if !phases::actor_collision_off(w, att) {
            let comps = boxes::pawn_hit_spheres(w, att);
            for (k, c) in comps.iter().enumerate() {
                if !c.types.contains(&obj::PROJECTILE_DESTRUCTIBLE) {
                    continue;
                }
                let s = pawn_sphere(w, att, c);
                if sphere_meets(&s, sp[0], sp[1], sp[2]) {
                    now.push(SphereKey::Pawn(aid, u32::try_from(k).expect("a pawn sphere index")));
                }
            }
        }
    }
    let Some(keys) = elem::hitbox_sphere_keys(w, a, i) else { return false };
    let mine = |k: &SphereKey| match *k {
        SphereKey::Hit(f, _) | SphereKey::Pawn(f, _) => f == aid,
    };
    keys.retain(|k| !mine(k) || now.contains(k));
    let mut begun = false;
    let mut dropped = false;
    for k in now {
        dropped |= keys.is_full() && !keys.contains(&k);
        if keys.insert(k) {
            begun = true;
        }
    }
    if dropped {
        crate::fighter::common::warn_text(
            w,
            "a buff's hitbox sphere holds more overlaps than SPHERE_KEYS: the key is dropped (combat::phases)",
        );
    }
    begun
}

/// `pawnSpherePlace` of a fighter: the component's world sphere in the play plane.
fn pawn_sphere(w: &World, f: FighterId, d: &boxes::PawnHitSphere) -> WorldSphere {
    let fm = w.fighter(f);
    let (xy, sz) = match fm.body.actor_scale {
        Some(a) => (a[0], a[1]),
        None => (fm.body.size_scale, fm.body.size_scale),
    };
    let ix = &crate::fighter::common::data_of(w, f).boxes;
    let [x, y, r] = boxes::pawn_sphere_place(
        hits::mesh_wx(fm),
        hits::mesh_wy(fm),
        xy,
        sz,
        crate::math::narrow(ix.mesh_scale),
        crate::math::narrow(ix.mesh_z),
        d,
    );
    WorldSphere { x, y, z: Some(0.0), r, ..WorldSphere::default() }
}

// ---- the damage boxes, the World.actors pawns and the stage's actors

/// The shared tail of `meleeDamageBoxTest`, `meleeActorTest` and `meleeStageActorTest`: the hit-once record, the
/// entry rows and the stops; true: the call goes on.
fn record_and_stops(w: &mut World, att: FighterId, h: &Active, key: i32, met: &WorldSphere) -> bool {
    phases::hit_set_add(w, att, h.key, key);
    entry_stops(w, att, &h.p, met)
}

/// `meleeDamageBoxTest`: a fighter's Hitframes over a spawned actor's own BoxComponent
/// on the HitBox channel (the projectile kind's `damage_box`: SeismicReactor's DamageBox, object type Pawn) run
/// pfgDoDamage with the actor as HitActor: its Comp_Actor_Destructible keeps IsProjectile false, so @15084 goes to
/// DamageAndLaunch @15630: CanBeHitBy (another team): ApplyDamage of the Damage x the dealer's charge multiplier (no
/// call for 0) -> the class's ReceiveAnyDamage; either team: HitByEnemy / HitByAlly; then, another team's alone,
/// the dealer's hit pause, the Hit's rows, HitNonPlayer and HitAnything. The hit-once record is the Hitframe's hit
/// set, key 1000 + the actor's id.
/// Deviation: tested where meleeItemTest is, at the actor's last place; the box's depth is the actor's (0).
pub fn melee_damage_box_test(w: &mut World, att: FighterId) {
    let mut pose: Pose = None;
    melee_damage_box_test_with(w, att, &mut pose);
    put_pose(w, pose);
}

/// [`melee_damage_box_test`] with the attacker's pose, sampled at its first use.
fn melee_damage_box_test_with(w: &mut World, att: FighterId, pose: &mut Pose) {
    if !phases::alive(w, att) || w.fighter(att).montages.active_hits.is_empty() {
        return;
    }
    let mut n = 0;
    while let Some(pr) = w.actors.projectiles.nth(n) {
        n += 1;
        let prr = ActorRef::Projectile(pr);
        if sh::dead(w, prr) {
            continue;
        }
        let x = w.actors.projectiles.get(pr).expect("a live projectile");
        let (cls, seq) = (x.cls, x.seq);
        let kind = sh::projectile_hooks(w, cls);
        let Some(db) = kind.and_then(|k| k.damage_box).and_then(|f| f(w, pr)) else { continue };
        let kind = kind.expect("a kind with a damage box");
        let key = actor_key(seq);
        let hs = phases::take_actives(w, att);
        for h in hs.iter() {
            if sh::dead(w, prr) || phases::hit_set_has(w, att, h.key, key) {
                continue;
            }
            let Some(pv) = pose_of(w, att, pose) else {
                phases::give_actives(w, hs);
                return;
            };
            let met = phases::hit_sphere_find(w, att, &h.p, pv, CH_DAMAGE, |s| shape_over_box(s, &db));
            let Some(met) = met else { continue };
            if !record_and_stops(w, att, h, key, &met) {
                continue;
            }
            // CanBeHitBy @659: another team takes the damage; either team then gets DamageAndLaunch's HitByEnemy
            // @3723 / HitByAlly @8047 broadcast (a victim ApplyDamage destroyed gets none), the direction the
            // packet's KnockBackDirection with X along the attacker's facing. Between ApplyDamage and HitByEnemy the
            // attacker's rumble (@3668). A refused hit with ForceKnockback (@8516 goto 3222) runs that tail after its
            // HitByAlly, Hit false.
            let d = sh::can_be_hit_by_d(sh::comp(w, prr, "Comp_Actor_Destructible"));
            let ally = !sa::can_be_hit_by(team(w, att), owner_team(w, prr), d); // @659
            let q = buff::apply_dealt_damage_mods(w, att, prr, &h.p, true); // @5617-5904
            let dmg = q.damage.unwrap_or(0.0) * w.fighter(att).charge.charge_dmg_mult;
            if !ally
                && dmg != 0.0
                && let Some(any) = kind.any_damage
                && any(w, pr, dmg, ActorRef::Fighter(att))
            {
                spawning::destroy_projectile(w, pr, false);
            }
            let (kx, ky) = kb_dir(&q);
            let dx = kx * facing(w, att);
            let bkb = q.base_knock_back.unwrap_or(0.0);
            let by = Some(ActorRef::Fighter(att));
            if ally && !sh::dead(w, prr) {
                hits::hit_by(w, pr, true, dx, ky, bkb, -1, by); // @8047
            }
            if ally && !q.force_knockback.unwrap_or(false) {
                continue;
            }
            crate::fighter::tick::controller_rumble(w, att, q.controller_rumble); // @3668
            if !sh::dead(w, prr) {
                hits::hit_by(w, pr, false, dx, ky, bkb, -1, by); // @3723
            }
            if ally {
                continue;
            }
            dealer_hit_pause(w, att, &q, h.charged, None);
            if !sh::dead(w, prr) && !h.p.on_hit_buffs.is_empty() {
                let mult = w.fighter(att).charge.charge_buff_mult;
                elem::hit_actor_rows_typed(w, att, prr, &h.p.on_hit_buffs, mult); // the Hit's rows @18293-19121
            }
            anim::cond_signal(w, att, CondSignal::HitNonPlayer); // @22812, after the hit pause
            hits::fighter_hit_anything(w, att, prr, &q, true); // @22812 -> @22088 -> HitAnything @22208
        }
        phases::give_actives(w, hs);
    }
}

/// `meleeActorTest`: a fighter's Hitframes over a World.actors pawn's root capsule
/// (the kind's `pawn_body`: ItemHelper_BananaGuard's CollisionCylinder, profile Pawn) or its own HitBox-overlapping
/// boxes (the kind's `damage_box`: Steven_ShieldActor's Hurtbox and BlockBox, each by its component's object type)
/// run pfgDoDamage with the actor as HitActor, as meleeDamageBoxTest: the hit-once record (key 1000 + the actor's
/// id), the entry rows and the stops, the dealt pass; no IsProjectile, so @15084 -> DamageAndLaunch @15630
/// (`actorDamageAndLaunch`). On a Hit the attacker's hit pause, the Hit's rows (@18293-19011), HitNonPlayer and
/// HitAnything.
/// Deviation: tested where meleeDamageBoxTest is, at the actor's last place, its capsule on the play plane.
/// None of the elemental calls (@21835, @22365, @22840) applies to a non-fighter HitActor.
pub fn melee_actor_test(w: &mut World, att: FighterId) {
    let mut pose: Pose = None;
    melee_actor_test_with(w, att, &mut pose);
    put_pose(w, pose);
}

/// [`melee_actor_test`] with the attacker's pose, sampled at its first use.
fn melee_actor_test_with(w: &mut World, att: FighterId, pose: &mut Pose) {
    if !phases::alive(w, att) || w.fighter(att).montages.active_hits.is_empty() {
        return;
    }
    let mut n = 0;
    while let Some(b) = w.actors.actors.nth(n) {
        n += 1;
        let bref = ActorRef::Actor(b);
        let x = w.actors.actors.get(b).expect("a live actor");
        if x.dead {
            continue;
        }
        let (cls, seq) = (x.cls, x.seq);
        let kind = sh::world_hooks(w, cls);
        let body = kind.and_then(|k| k.pawn_body).and_then(|f| f(w, b));
        let dbs = if body.is_some() { &[][..] } else { kind.map_or(&[][..], |k| k.damage_box) };
        if body.is_none() && dbs.is_empty() {
            continue;
        }
        let key = actor_key(seq);
        let hs = phases::take_actives(w, att);
        for h in hs.iter() {
            if sh::dead(w, bref) || phases::hit_set_has(w, att, h.key, key) {
                continue;
            }
            let Some(pv) = pose_of(w, att, pose) else {
                phases::give_actives(w, hs);
                return;
            };
            let mut met = None;
            if let Some(body) = &body {
                met = phases::hit_sphere_find(w, att, &h.p, pv, CH_PAWN, |s| shape_over_capsule(s, body));
            } else {
                for d in dbs {
                    let Some(db) = (d.at)(w, b) else { continue };
                    let ch = body_info(sh::comp(w, bref, d.comp), None).and_then(|i| hurt_ch(i.kind)).unwrap_or(CH_ALL);
                    met = phases::hit_sphere_find(w, att, &h.p, pv, ch, |s| shape_over_box(s, &db));
                    if met.is_some() {
                        break;
                    }
                }
            }
            let Some(met) = met else { continue };
            if !record_and_stops(w, att, h, key, &met) {
                continue;
            }
            let q = buff::apply_dealt_damage_mods(w, att, bref, &h.p, true); // @5617-5904
            let dmg = q.damage.unwrap_or(0.0) * w.fighter(att).charge.charge_dmg_mult;
            // (the attacker's rumble, DamageAndLaunch @3668, is actorDamageAndLaunch's)
            let t = team(w, att);
            let at = V2 { x: met.x, y: met.y };
            if !sh::actor_damage_and_launch(w, b, t, &q, dmg, ActorRef::Fighter(att), att, at, true) {
                continue;
            }
            dealer_hit_pause(w, att, &q, h.charged, None);
            if !sh::dead(w, bref) && !h.p.on_hit_buffs.is_empty() {
                let mult = w.fighter(att).charge.charge_buff_mult;
                elem::hit_actor_rows_typed(w, att, bref, &h.p.on_hit_buffs, mult); // the Hit's rows @18293-19121
            }
            anim::cond_signal(w, att, CondSignal::HitNonPlayer); // @22812, after the hit pause
            hits::fighter_hit_anything(w, att, bref, &q, true); // @22812 -> @22088 -> HitAnything @22208
        }
        phases::give_actives(w, hs);
    }
}

/// `meleeStageActorTest`: a fighter's Hitframes over a placed stage actor's hurt
/// shape (`stageHurt`, its object type the kind's `hurt_type`) run pfgDoDamage with the actor as HitActor, as
/// meleeActorTest: the hit-once record (key STAGE_HIT_KEY - the actor's index), the entry rows, the stops (a hit on a
/// stage actor does not set HasHitAFighter: it is set for a BPI_BaseFighter alone), IsVulnerableTo (@4608), the
/// dealt pass, then DamageAndLaunch (`stageDamageAndLaunch`). On a Hit the attacker's rumble and hit pause
/// (@16603-17442: HitPauseTime x the victim's HitPauseMultiplier, x its CurrentDamage tier with a
/// Comp_Actor_Damage), then HitNonPlayer.
/// Deviation: tested where meleeActorTest is, at the actor's place now, its capsule on the play plane.
pub fn melee_stage_actor_test(w: &mut World, att: FighterId) {
    let mut pose: Pose = None;
    melee_stage_actor_test_with(w, att, &mut pose);
    put_pose(w, pose);
}

/// [`melee_stage_actor_test`] with the attacker's pose, sampled at its first use.
fn melee_stage_actor_test_with(w: &mut World, att: FighterId, pose: &mut Pose) {
    if w.stage_run.is_none() || !phases::alive(w, att) || w.fighter(att).montages.active_hits.is_empty() {
        return;
    }
    let mut body = Vec::new();
    let mut i = 0;
    while i < sh::stage_actor_count(w) {
        let bref = ActorRef::Stage(i);
        let kind = sh::stage_kind(w, i);
        i += 1;
        let Some(kind) = kind else { continue };
        let Some(hb_ch) = sa::stage_hurt(w, kind, bref, &mut body) else { continue };
        let key = STAGE_HIT_KEY - sa::stage_actor_index(w, i - 1);
        let ch = u8::try_from(to_int32(hb_ch)).expect("a channel mask");
        let hs = phases::take_actives(w, att);
        for h in hs.iter() {
            if sa::stage_actor_dead(w, i - 1) || phases::hit_set_has(w, att, h.key, key) {
                continue;
            }
            let Some(pv) = pose_of(w, att, pose) else {
                phases::give_actives(w, hs);
                return;
            };
            let met = phases::hit_sphere_find(w, att, &h.p, pv, ch, |s| sa::stage_hurt_over(s, &body));
            let Some(met) = met else { continue };
            if !record_and_stops(w, att, h, key, &met) {
                continue;
            }
            if !sa::stage_vulnerable(w, kind, bref) {
                continue; // @4608
            }
            let q = buff::apply_dealt_damage_mods(w, att, bref, &h.p, true); // @5617-5904
            let dmg = q.damage.unwrap_or(0.0) * w.fighter(att).charge.charge_dmg_mult;
            let t = team(w, att);
            let at = V2 { x: met.x, y: met.y };
            if !sa::stage_damage_and_launch(w, bref, t, &q, dmg, ActorRef::Fighter(att), att, at, true) {
                continue; // (its rumble @3668 too)
            }
            let raw = q.hit_pause_time.unwrap_or(0.0) + h.charged * q.charge_hit_pause_mult.unwrap_or(0.0);
            if raw > 0.0 && q.apply_hit_pause_to_damage_dealer != Some(false) {
                let scale = sa::stage_hit_pause_scale(w, kind, bref);
                dealer_hit_pause(w, att, &q, h.charged, Some(scale));
            }
            if !sa::stage_actor_dead(w, i - 1) && !h.p.on_hit_buffs.is_empty() {
                sa::stage_hit_actor_rows_typed(w, bref, &h.p.on_hit_buffs); // the Hit's rows (refused: warned)
            }
            anim::cond_signal(w, att, CondSignal::HitNonPlayer); // @22812, after the hit pause
            hits::fighter_hit_anything(w, att, bref, &q, true); // @22812 -> @22088 -> HitAnything @22208
        }
        phases::give_actives(w, hs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_set_keys_as_the_ts() {
        assert_eq!(actor_key(3), 1003);
        assert_eq!(item_key(0), -1);
        assert_eq!(item_key(4), -5);
        assert_eq!(STAGE_HIT_KEY - 2, -1_000_000_002);
    }

    #[test]
    fn a_sphere_meets_a_sphere_in_the_plane_and_in_depth() {
        let s = WorldSphere { x: 0.0, y: 0.0, z: None, r: 10.0, ..WorldSphere::default() };
        assert!(sphere_meets(&s, 15.0, 0.0, 5.0));
        assert!(!sphere_meets(&s, 15.1, 0.0, 5.0));
        let deep = WorldSphere { z: Some(12.0), ..s.clone() };
        assert!(!sphere_meets(&deep, 0.0, 0.0, 1.0));
        assert!(sphere_meets(&deep, 0.0, 0.0, 2.0));
    }

    #[test]
    fn hurt_channels() {
        assert_eq!(hurt_ch(obj::PROJECTILE_DESTRUCTIBLE), Some(CH_PROJ));
        assert_eq!(hurt_ch(obj::HURT_BOX), Some(CH_HURT));
        assert_eq!(hurt_ch(obj::HIT_BOX), None);
        assert_eq!(hurt_ch(obj::WORLD_STATIC), None);
    }
}
