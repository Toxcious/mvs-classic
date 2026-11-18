//! Pawn_C019's own logic: the ten conditions (the cook grenade's out / behind / in front,
//! the mark, the Hammer and Cleaner perks' presence) and the notifies AnimNotify_C019_UpdateAim and
//! AnimNotify_C019_Ship_Spawner. `slotMix` is the state's (state.rs).

use super::state::{morty_mut, morty_of};
use crate::Real;
use crate::actors::ProjectileId;
use crate::combat::buffs::{buff_of, turn_off_hit_boxes};
use crate::combat::disguise::{copy_running, set_copy_aim};
use crate::combat::elem::instigator_charge;
use crate::events::NotifyRef;
use crate::fighter::anim::montages::{instance_data, state_ref};
use crate::fighter::anim::update::shown_uid;
use crate::fighter::common::peers;
use crate::fighter::match_side::{Foreign, SpawnRequest, notify_props};
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::ue::{SMALL_NUMBER, deg_atan2, safe_normal, size2d};
use crate::math::{max, min};
use crate::name;
use crate::registry::entries::{Condition, NotifyHandler};
use crate::registry::props::text;
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;

/// The hooks of this file in Morty's CharHooks (`mod.rs` has none beside state and slotMix, which `mod.rs` enters).
pub fn hooks(h: &mut CharHooks) {
    h.layer_held = Some(layer_held);
}

/// ZERO_ANIMWEIGHT_THRESH and FAnimNode_TwoWayBlend's B-alone alpha.
const ZERO_W: Real = 0.00001;
const ALPHA_ONE: Real = 0.99999;

/// `layerHeld` (CharHooks.layerHeld): C019_AnimBP's aim slots (`fighter::anim::slot_mix` blend_slots: AimCenter and
/// AimUp under a TwoWayBlend on the aim up, that and AimDown under one on the aim down) are a set every path to the
/// locomotion goes through, so a montage at full weight with tracks in some of them holds the layer while the
/// TwoWayBlends update none of the others (A updated under an alpha of 0.99999, B over 0.00001; a Slot without
/// bAlwaysUpdateSourcePose passes its source on only without a track at full weight: FAnimNode_Slot::Update_AnyThread
/// 1424945f0). C019_Attack_Special_F_Montage (AimCenter and AimUp) holds it with the aim not down. A montage with a
/// track in all three is the data's own set (`Montage.holds_source`).
// The layer under part of the set is held as well.
fn layer_held(w: &World, f: FighterId) -> bool {
    let Some(uid) = shown_uid(w, f) else { return false };
    let (Some(m), Some(s)) = (instance_data(&w.data, w, f, uid), state_ref(w, f, uid)) else { return false };
    let track = |n: &str| m.slots.iter().any(|t| t.slot == n && !t.segments.is_empty() && t.additive != Some(true));
    let (c, u, d) = (track("AimCenter"), track("AimUp"), track("AimDown"));
    if !(c || u || d) || (c && u && d) || !s.full_weight(m) {
        return false;
    }
    let ad = w.fighter(f).pose.aim_direction;
    let up = min(max(ad, 0.0), 1.0);
    let down = min(max(-ad, 0.0), 1.0);
    let updated = (down < ALPHA_ONE && ((up < ALPHA_ONE && !c) || (up > ZERO_W && !u))) || (down > ZERO_W && !d);
    !updated
}

/// LocalGrenades[0] of c019CookGrenadeOutProjectileCondition's Check (@5-855) for any fighter: the first
/// C019_CookGrenade of its own Comp_Spawner's SpawnedActors, else of its allies'. The graph asks nothing of the actor
/// but those two components, so the owner need not be a Morty (Arya's Disguise plays his neutral montages with Arya as
/// the owner). A Morty reads the list the world makes for him (`MortyState.team_grenades`); another fighter reads an
/// allied Morty's, which holds the same team's grenades, its own first.
// Simplification: the world lists the grenades for a Morty alone (the World's step), so a fighter that is not one and
// has no Morty on its team finds none. No ported path puts a C019_CookGrenade in such a fighter's spawner.
fn first_grenade(w: &World, f: FighterId) -> Option<ProjectileId> {
    if let Some(mo) = morty_of(w, f) {
        return mo.team_grenades.first().copied();
    }
    let team = w.fighter(f).team.team;
    for &p in peers(w) {
        if p == f || w.fighter(p).team.team != team {
            continue;
        }
        let Some(list) = morty_of(w, p).map(|m| &m.team_grenades) else { continue };
        if list.is_empty() {
            continue;
        }
        for &g in list {
            if w.actors.projectiles.get(g).is_some_and(|g| g.owner == Some(f)) {
                return Some(g);
            }
        }
        return list.first().copied();
    }
    None
}

/// c019CookGrenadeOutProjectileCondition's Check on f: 0 without a grenade (@855-911, Behind left as it was), else 1
/// with Behind false and 2 with Behind true (@925-1516, from the first grenade). The children read Behind right after
/// the parent's Check and only where that Check found a grenade (NoneOrInFront @29-125, OutBehind @29, OutInFront
/// @29-58), so the value kept from an earlier call is never seen and the result carries it. A Morty still keeps it in
/// `MortyState.cook_grenade_behind` (part of his state hash).
fn cook_grenade_out(w: &mut World, f: FighterId) -> u8 {
    let Some(g) = first_grenade(w, f).and_then(|g| w.actors.projectiles.get(g)) else { return 0 };
    let gx = g.x;
    let fx = w.fighter(f).body.x;
    let behind = if facing(w, f) < 0.0 { gx >= fx } else { gx <= fx };
    if let Some(mo) = morty_mut(w, f) {
        mo.cook_grenade_behind = behind;
    }
    if behind { 2 } else { 1 }
}

fn out(w: &mut World, f: FighterId) -> bool {
    cook_grenade_out(w, f) != 0
}
fn not_out(w: &mut World, f: FighterId) -> bool {
    cook_grenade_out(w, f) == 0
}
fn none_or_in_front(w: &mut World, f: FighterId) -> bool {
    cook_grenade_out(w, f) != 2
}
fn out_behind(w: &mut World, f: FighterId) -> bool {
    cook_grenade_out(w, f) == 2
}
fn out_in_front(w: &mut World, f: FighterId) -> bool {
    cook_grenade_out(w, f) == 1
}

/// C019MarkProjectileCondition Check @0-301: a C019Marker among the actor's own Comp_Spawner SpawnedActors, of any
/// actor (no cast): false for a fighter that is not a Morty (nothing else spawns the marker), and the Not child
/// (@0-58) is its negation, true there.
fn mark(w: &mut World, f: FighterId) -> bool {
    morty_of(w, f).is_some_and(|m| m.has_marker)
}
fn not_mark(w: &mut World, f: FighterId) -> bool {
    !mark(w, f)
}

/// C019HammerPerkEquipped / C019CleanerPerkEquipped Check @79-228: the actor's BPI_CheckIfHasBuff(Buff_C019_Hammer_C
/// / Buff_C019_Cleaner_C)'s hasBuff (Comp_State_Manager CheckHasBuff: a live buff actor of the class), false without
/// the fighter interface @252; C019HammerPerkNotEquipped @79-255 its Not, true without the interface (Not(false)
/// @279-324). The buffs are the perks' markers (`perks.rs`), given by Comp_Pawn_PerkManager.
// Unverified: IsBuffOfClass is not read (as C021BubblePerkCondition, chars/black_adam): the exact class.
fn hammer(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_C019_Hammer_C")).is_some()
}
fn cleaner(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, name!("Buff_C019_Cleaner_C")).is_some()
}
fn not_hammer(w: &mut World, f: FighterId) -> bool {
    !hammer(w, f)
}

/// AnimNotify_C019_UpdateAim Received_NotifyBegin: BPI_FindClosestGrenade (Pawn_C019 FindClosestGrenade: the closest
/// live C019_CookGrenade of this fighter among its and its allies' spawned actors, by GetDistanceTo) ->
/// BPI_SetAimDirection(angle / 90), the angle DegAtan2 of the unit direction's Z over its Y towards the grenade's side
/// (the elevation, whichever way he faces); without one BPI_SetAimDirection(0).
// Deviation: Buff_C019ProjectileOption's buff actors (no source for the buff is ported) are left out.
// An owner that does not implement BPI_C019 (@42-80: any pawn but Pawn_C019, as Arya under Disguise) goes straight to
// BPI_SetAimDirection(0) (@1756-1877).
fn update_aim_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    let Some(mo) = morty_of(w, f) else {
        // the notify's MeshComp anim instance: a Disguise's second mesh when its montage runs there
        if copy_running(w, f) {
            set_copy_aim(w, f, 0.0);
        } else {
            w.fighter_mut(f).pose.aim_direction = 0.0;
        }
        return;
    };
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let mut best: Option<(Real, Real)> = None;
    let mut bd = 1_000_000.0;
    for &g in &mo.team_grenades {
        let Some(g) = w.actors.projectiles.get(g) else { continue };
        if g.owner != Some(f) || g.dead {
            continue;
        }
        let d = size2d([g.x - fx, g.y - fy]);
        if d < bd {
            bd = d;
            best = Some((g.x, g.y));
        }
    }
    // GetDirectionUnitVector(Morty, grenade) (@983), DegAtan2(Z, +-Y) (@1151, @1361: the Y toward the grenade's
    // side), NormalizeToRange(a, -90, 90) x 2 - 1 (@1563-1652)
    let v = best.map_or(0.0, |(bx, by)| {
        let u = safe_normal([0.0, bx - fx, by - fy], SMALL_NUMBER);
        ((deg_atan2(u[2], u[1].abs()) + 90.0) / 180.0) * 2.0 - 1.0
    });
    w.fighter_mut(f).pose.aim_direction = v;
}

/// AnimNotify_C019_Ship_Spawner Received_NotifyBegin: the spawn point (Ship's constructor), then CompSpawnActor as
/// AnimNotify_Spawner (neither AimOn nor StoreAimInfo is set on Morty's two up specials). The owner's TurnOffHitboxCheck
/// @3787 true ends the begin @3846 (`turn_off_hit_boxes`).
fn ship_spawner_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    if !w.fighter(f).buffs.buffs.is_empty() && turn_off_hit_boxes(w, f) {
        return;
    }
    let cls = text(notify_props(w, f, n), "ActorClass")
        .map_or(crate::names::Name::NONE, |s| crate::registry::props::name(w, s));
    let attack_id = w.fighter(f).decay.attack_count;
    let charge = instigator_charge(w, f);
    let r = SpawnRequest {
        cls,
        n,
        attack_id,
        foreign: Foreign::None,
        copy: None,
        charge,
        aim: None,
        props: None,
        instigator: None,
        origin: None,
    };
    w.fighter_mut(f).requests.spawn_requests.push(r);
}

/// This file's registry entries (the ten conditions, the two notifies).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    // c019CookGrenadeOutProjectileCondition: a C019_CookGrenade among the SpawnedActors of the fighter's or its allies'
    // Comp_Spawner; it also writes Behind from the first one found (facing left: actor Y >= grenade Y, else grenade Y
    // >= actor Y) and leaves Behind as it was when there is none. Its children combine the two.
    for (name, f) in [
        ("C019CookGrenadeOutProjectileCondition_C", out as crate::registry::entries::CondFn),
        ("C019CookGrenadeNotOutProjectileCondition_C", not_out),
        ("c019CookGrenadeNoneOrInFrontProjectileCondition_C", none_or_in_front),
        ("c019CookGrenadeOutBehindProjectileCondition_C", out_behind),
        ("c019CookGrenadeOutInFrontProjectileCondition_C", out_in_front),
        ("C019MarkProjectileCondition_C", mark),
        ("c019MarkProjectileNotCondition_C", not_mark),
        ("C019HammerPerkEquipped_C", hammer),
        ("C019CleanerPerkEquipped_C", cleaner),
        ("C019HammerPerkNotEquipped_C", not_hammer),
    ] {
        r.conditions.enter(name, Condition::Fn(f))?;
    }
    r.notifies.enter(
        "AnimNotify_C019_UpdateAim_C",
        NotifyHandler { begin: Some(update_aim_begin), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_C019_Ship_Spawner_C",
        NotifyHandler { begin: Some(ship_spawner_begin), ..NotifyHandler::default() },
    )
}
