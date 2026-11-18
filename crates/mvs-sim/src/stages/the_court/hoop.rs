//! The Court's two hoops (M010_Hoop_A_01_BP_C SJAM_Hoop_A_01_BP_2 HoopIndex 0,
//! SJAM_Hoop_A_01_BP2 HoopIndex 1).
//! Box: a static Floor backboard (a solid of the stage data). The other six components are Custom overlap sensors
//! (the class's templates: Pawn, ItemDestructible and ProjectileDestructible Overlap, the world, body, platform and
//! damage channels Ignore, the rest at the channel defaults: Item Overlap), fixed on the actor (no bone socket):
//! - HoopHitCollision's begin overlap @129 -> @10 (authority) HandleBasketballCollision(OtherActor): a free
//!   Item_Basketball (incl. subclasses) explodes (DoHoopExplosion with its LastHolder's TeamIndex, 0 through a None
//!   holder: CX-M010-6) @171, scores from its ThrownFromLocation @216 and is destroyed @270; else a
//!   BP_FighterCharacter whose HeldItem's class is exactly Item_Basketball_C scores from the hoop's own location
//!   (always 2) @656, the held ball is destroyed @688, ClearHeldItem @746, then explodes with his TeamIndex @833.
//! - DoHoopExplosion: HoopExplosionFXMulticast @113 (sfx_map_M010_ball_score at the sphere's centre,
//!   PS_Hoop_Explosion there, Basket_Anim_HR_Front @134-381), then for each overlapping BP_FighterCharacter (the list
//!   fetched again at each read @173-573) of another TeamIndex than the given one (@683): the packet below, the
//!   direction (fighter - centre) in (Y, Z) @753-895, ElementalBuffs [Buff_Elemental_Fire], its own handler's
//!   pfgDoDamage(null, the fighter) @2104 (the hoop the dealer: `apply_level_hit`).
//! - Handle Points Earned: CalculatePointsFromBasket (|initial.Y - the hoop actor's Y| >= 2500: 3, else 2),
//!   broadcast BasketScored(HoopIndex, points): the shot clock's HandlePointScored (`score`) and the hoop's own
//!   BasketScoredUIWrapper -> BasketScoredUIMulticast @1025-1943 (a pop text '+{points}', lifetime 2, scale 2, at
//!   (0, -400 x sign(scale X), 1200) on the hoop: the drawing's, from `M010Hoop::scored_at` / `points`).
//! - The backboard sensors (no filter): Top begin @428 Basket_Anim_WeightAdd + sfx_map_M010_backboard_weight_add,
//!   Top end @876 Basket_Anim_WeightLift + sfx_map_M010_backboard_weight_lift; Back begin @540
//!   Basket_Anim_HR_Behind, Bottom begin @652 Basket_Anim_HR_Underneath, Front begin @764 Basket_Anim_HR_Front, each
//!   + sfx_map_M010_backboard_shake. PlayAnimation(.., false) on the SkeletalMesh: the drawing plays
//!     `M010Hoop::anim` from `anim_at`.
//!
//! A free item's Capsule body (its own object type and response to WorldDynamic, the sensors' object type) is tested
//! in its move (`StageEntry::item_moved`, [`item_moved`]), sensor by sensor (HoopHit, Top, Back, Bottom, Front):
//! MoveComponentImpl 142971aa0 updates the overlaps before it dispatches the move's blocking hit, and
//! HandleBasketballCollision destroys the ball, so a ball that enters HoopHitCollision in the move that meets the
//! backboard scores from its ThrownFromLocation untouched, and its Capsule's ComponentHit (ResetBall) never runs.
//! The fighters' root capsules (object type Pawn), and the items again, are tested once a tick in the leftover pass
//! (`StageEntry::tick`, [`tick`]), after every move of the frame, in fighter order, then item order, sensor by sensor.
//! Deviation: the build fires a fighter's begin overlap inside his move (his tick group, in the order PhysX reports
//! it); here they come in the leftover pass, behind the items' of the frame. Deviation: an item is tested at the end
//! of its step, not over the move's sweep (a touch on the way to the blocking hit or through a sensor is missed).
//! Unverified: an idle (simulating) ball is taken in the same order as a flying one; the physics sync and the
//! rigid-body notify's order is not read. Deviation: a projectile's ProjectileDestructible body and a fighter's
//! other Pawn bodies (BaseFighter's Capsule) are not tested; nothing of a projectile reaches the scoring cast, the
//! backboards' anim and sound would. Unverified: a fighter out of play (dead > 0) or with his collision off
//! (noCollision) is taken as overlapping nothing.

use super::score::point_scored;
use crate::Real;
use crate::State;
use crate::actors::item::ItemDef;
use crate::actors::{ActorRef, ItemClass, ItemId, StageActorClass};
use crate::combat::HitPacket;
use crate::combat::packet::DataEnum;
use crate::events::{Event, SoundKey};
use crate::fighter::DodgeState;
use crate::fighter::control::{attack_dodged, dodge_over, dodge_tick};
use crate::fighter::moves::V2;
use crate::fighter::traces::{OBJ_ITEM, OBJ_ITEM_DESTRUCTIBLE, OBJ_PROJECTILE_DESTRUCTIBLE, OBJ_WORLD_DYNAMIC};
use crate::fighter::traces::{body_info, response_to};
use crate::math::max;
use crate::math::ue::size2d;
use crate::name;
use crate::names::Name;
use crate::registry::entries::StageActorEntry;
use crate::registry::props::all_fighters;
use crate::world::World;
use crate::world::stage_actors::{StageActor, StageActorOwn};

/// CalculatePointsFromBasket's LocalDistanceThreshold.
const THREE_POINT: Real = 2500.0;

/// `M010Hoop`: an M010_Hoop_A_01_BP_C in the match. What the drawing reads: `anim` (the SkeletalMesh's last
/// PlayAnimation, none before any) from `anim_at` (a world tick, -1 before), `scored_at` / `points` (the last
/// BasketScored: its pop text), `exploded_at` (the last HoopExplosionFXMulticast: PS_Hoop_Explosion at the sphere's
/// centre; its HR_Front is in `anim`).
#[derive(State, Debug, Default)]
pub struct M010Hoop {
    /// HoopIndex (the score key).
    pub hoop_index: i32,
    pub anim: Option<Name>,
    pub anim_at: i32,
    pub scored_at: i32,
    pub points: i32,
    pub exploded_at: i32,
    /// Its Comp_Pawn_HitBoxHandler dodge variables, across explosions (`fighter::control`).
    pub dodge: Option<DodgeState>,
    /// The sensors' overlaps now (a fighter's id, an item as -1 - its id), for the begin and end edges.
    pub in_hit: Vec<i32>,
    pub in_top: Vec<i32>,
    pub in_back: Vec<i32>,
    pub in_bottom: Vec<i32>,
    pub in_front: Vec<i32>,
}

/// The `StageActorEntry` of M010_Hoop_A_01_BP_C (`hoopKind`): a `create` and nothing else (no hurt shapes).
pub fn entry() -> StageActorEntry {
    StageActorEntry {
        class: StageActorClass::M010HoopA01BP,
        hurt_type: None,
        friendly_fire: false,
        can_take_damage: true,
        hit_pause_multiplier: None,
        surface: None,
        create: Some(create),
        any_damage: None,
        hit_by_enemy: None,
        hit_by_ally: None,
        knocked_back: None,
        hurt: None,
        sounds: &[],
        damage_disabled: None,
        invulnerable: None,
        current_damage: None,
        victim_pause: None,
        mesh: None,
        reset: None,
        solid_hit: None,
    }
}

fn create(w: &mut World, slot: u32) {
    let index = actor(w, slot).index;
    let hoop_index = {
        let d = &w.data.stage.placed.as_ref().expect("placed actors")[index as usize];
        d.p.get("HoopIndex").and_then(mvs_data::value::Value::as_f64).map_or(0, crate::math::dbl::to_int32)
    };
    let own = M010Hoop {
        hoop_index,
        anim: None,
        anim_at: -1,
        scored_at: -1,
        points: 0,
        exploded_at: -1,
        dodge: None,
        ..M010Hoop::default()
    };
    actor_mut(w, slot).own = StageActorOwn::M010Hoop(own);
}

fn actor(w: &World, slot: u32) -> &StageActor {
    &w.stage_run.as_ref().expect("a stage state").actors[slot as usize]
}

fn actor_mut(w: &mut World, slot: u32) -> &mut StageActor {
    &mut w.stage_run.as_mut().expect("a stage state").actors[slot as usize]
}

fn hoop_mut(w: &mut World, slot: u32) -> &mut M010Hoop {
    match &mut actor_mut(w, slot).own {
        StageActorOwn::M010Hoop(h) => h,
        _ => unreachable!("M010_Hoop_A_01_BP_C holds an M010Hoop"),
    }
}

/// `dodgeOf`'s slot for a hoop (`fighter::control::dodge_slot`'s hook for a stage actor).
pub fn dodge_slot(w: &mut World, slot: u32) -> Option<&mut Option<DodgeState>> {
    let run = w.stage_run.as_mut()?;
    match &mut run.actors.get_mut(slot as usize)?.own {
        StageActorOwn::M010Hoop(h) => Some(&mut h.dodge),
        _ => None,
    }
}

/// A sensor's world shape (sim x = -UE Y, y = UE Z), from the hoop's StageData.solids by component name; static.
#[derive(Clone, Copy, Debug)]
struct Bx {
    x: Real,
    y: Real,
    hx: Real,
    hy: Real,
}

struct Geo {
    hit: Option<Bx>,
    top: Option<Bx>,
    back: Option<Bx>,
    bottom: Option<Bx>,
    front: Option<Bx>,
    /// HoopExplosionCollision: the centre and the radius.
    ex: Option<(Real, Real, Real)>,
}

fn geo(w: &World, index: u32) -> Geo {
    let sd = &w.data.stage;
    let d = &sd.placed.as_ref().expect("placed actors")[index as usize];
    let all = sd.solids.as_deref().unwrap_or(&[]);
    let find = |comp: &str| {
        let suffix = format!(".{comp}");
        d.solids
            .iter()
            .flatten()
            .filter_map(|&i| usize::try_from(i).ok().and_then(|i| all.get(i)))
            .find(|s| s.name.ends_with(&suffix))
    };
    let bx = |comp: &str| {
        find(comp).map(|s| Bx {
            x: crate::math::narrow(-s.center[1]),
            y: crate::math::narrow(s.center[2]),
            hx: crate::math::narrow(s.half_extent[1]),
            hy: crate::math::narrow(s.half_extent[2]),
        })
    };
    Geo {
        hit: bx("HoopHitCollision"),
        top: bx("BackboardTopCollision"),
        back: bx("BackboardBackCollision"),
        bottom: bx("BackboardBottomCollision"),
        front: bx("BackboardFrontCollision"),
        ex: find("HoopExplosionCollision").map(|s| {
            (
                crate::math::narrow(-s.center[1]),
                crate::math::narrow(s.center[2]),
                crate::math::narrow(s.radius.unwrap_or(s.half_extent[0])),
            )
        }),
    }
}

/// An upright capsule (centre x, y, radius r, half height h) against a box on the play plane.
fn capsule_in_box(x: Real, y: Real, r: Real, h: Real, b: &Bx) -> bool {
    let half = max(h - r, 0.0);
    let dx = max(max(b.x - b.hx - x, 0.0), x - (b.x + b.hx));
    let dy = max(max(b.y - b.hy - (y + half), 0.0), y - half - (b.y + b.hy));
    size2d([dx, dy]) < r
}

/// An upright capsule against a sphere on the play plane.
fn capsule_in_sphere(x: Real, y: Real, r: Real, h: Real, cx: Real, cy: Real, cr: Real) -> bool {
    let half = max(h - r, 0.0);
    let dy = max(max(cy - (y + half), 0.0), y - half - cy);
    size2d([cx - x, dy]) < r + cr
}

/// The sensors overlap an item's Capsule when its object type is one they overlap (Item, ItemDestructible,
/// ProjectileDestructible) and it does not ignore WorldDynamic (the sensors' type), with its query collision on.
fn item_meets(w: &World, it: ItemId) -> bool {
    let Some(i) = w.actors.items.get(it) else { return false };
    if i.dead || i.holder >= 0 {
        return false;
    }
    let Some(src) = crate::world::items::item_def_src(w, it) else { return false };
    let cap = ItemDef::at(&w.data, src).comp("Capsule");
    let Some(bi) = body_info(cap, None) else { return false };
    bi.on
        && [OBJ_ITEM, OBJ_ITEM_DESTRUCTIBLE, OBJ_PROJECTILE_DESTRUCTIBLE].contains(&bi.kind)
        && response_to(cap, crate::real(OBJ_WORLD_DYNAMIC)) != name!("ECR_Ignore")
}

/// One fighter's root capsule as the five sensors of a [`tick`] read it.
struct CapRead {
    id: i32,
    x: Real,
    y: Real,
    r: Real,
    h: Real,
}

/// What the five sensors of one [`tick`] read of the fighters (a root capsule, or off: dead or no collision: none)
/// and of the items (whether the item can meet a sensor), read once for them; an edge's handler can move, hit or
/// destroy what they read, so it drops them (`done` false).
#[derive(Default)]
struct Read {
    fighters: Vec<CapRead>,
    items: Vec<(ItemId, bool)>,
    done: bool,
}

impl Read {
    fn read(&mut self, w: &World) {
        self.fighters.clear();
        for f in all_fighters(w) {
            if !(w.fighter(f).life.dead <= 0.0 && !crate::fighter::no_collision(w, f)) {
                continue;
            }
            self.fighters.push(CapRead {
                id: w.fighter(f).id,
                x: w.fighter(f).body.x,
                y: crate::fighter::actor_y(w, f),
                r: crate::fighter::cap_radius(w, f),
                h: crate::fighter::capsule_half(w, f),
            });
        }
        self.items.clear();
        for id in w.actors.items.ids() {
            self.items.push((id, item_meets(w, id)));
        }
        self.done = true;
    }
}

/// The five sensors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sens {
    Hit,
    Top,
    Back,
    Bottom,
    Front,
}

const SENSORS: [Sens; 5] = [Sens::Hit, Sens::Top, Sens::Back, Sens::Bottom, Sens::Front];

fn box_of(g: &Geo, s: Sens) -> Option<Bx> {
    match s {
        Sens::Hit => g.hit,
        Sens::Top => g.top,
        Sens::Back => g.back,
        Sens::Bottom => g.bottom,
        Sens::Front => g.front,
    }
}

fn list_of(w: &mut World, slot: u32, s: Sens) -> &mut Vec<i32> {
    let h = hoop_mut(w, slot);
    match s {
        Sens::Hit => &mut h.in_hit,
        Sens::Top => &mut h.in_top,
        Sens::Back => &mut h.in_back,
        Sens::Bottom => &mut h.in_bottom,
        Sens::Front => &mut h.in_front,
    }
}

/// The begin edge of a sensor (`begin` closures): `key` is the fighter or item that began it.
fn on_begin(w: &mut World, slot: u32, s: Sens, key: i32) {
    match s {
        Sens::Hit => handle_basketball(w, slot, key),
        Sens::Top => play(w, slot, name!("Basket_Anim_WeightAdd"), Some(name!("sfx_map_M010_backboard_weight_add"))),
        Sens::Back => play(w, slot, name!("Basket_Anim_HR_Behind"), Some(name!("sfx_map_M010_backboard_shake"))),
        Sens::Bottom => play(w, slot, name!("Basket_Anim_HR_Underneath"), Some(name!("sfx_map_M010_backboard_shake"))),
        Sens::Front => play(w, slot, name!("Basket_Anim_HR_Front"), Some(name!("sfx_map_M010_backboard_shake"))),
    }
}

/// The end edge of a sensor: only the Top one has a handler.
fn on_end(w: &mut World, slot: u32, s: Sens) {
    if s == Sens::Top {
        play(w, slot, name!("Basket_Anim_WeightLift"), Some(name!("sfx_map_M010_backboard_weight_lift")));
    }
}

/// One sensor's overlaps now against its list: the begin edges (fighters, then items) and the end edges are called,
/// the list is the new set.
fn sensor(w: &mut World, slot: u32, g: &Geo, s: Sens, rd: &mut Read) {
    let mut now: Vec<i32> = Vec::new();
    if let Some(b) = box_of(g, s) {
        if !rd.done {
            rd.read(w);
        }
        for c in &rd.fighters {
            if capsule_in_box(c.x, c.y, c.r, c.h, &b) {
                now.push(c.id);
            }
        }
        for &(id, on) in &rd.items {
            let Some(it) = w.actors.items.get(id) else { continue };
            if on && capsule_in_box(it.x, it.y, it.capsule_radius(), it.capsule_half_height(), &b) {
                now.push(-1 - it.seq);
            }
        }
    }
    let list = list_of(w, slot, s);
    if *list == now {
        return; // no edge
    }
    let was = std::mem::replace(list, now.clone());
    rd.done = false;
    if s == Sens::Top {
        for k in &was {
            if !now.contains(k) {
                on_end(w, slot, s);
            }
        }
    }
    for k in &now {
        if !was.contains(k) {
            on_begin(w, slot, s, *k);
        }
    }
}

fn item_of(w: &World, key: i32) -> Option<ItemId> {
    if key >= 0 {
        return None;
    }
    w.actors.items.iter().find(|(_, i)| i.seq == -1 - key && !i.dead).map(|(id, _)| id)
}

fn play(w: &mut World, slot: u32, anim: Name, ev: Option<Name>) {
    let tick = w.flow.tick;
    let (index, x, y) = {
        let a = actor(w, slot);
        (a.index, a.x, a.y)
    };
    let h = hoop_mut(w, slot);
    h.anim = Some(anim);
    h.anim_at = tick;
    if let Some(ev) = ev {
        let key = Some(SoundKey { tag: name!("m010Hoop"), n: i32::try_from(index).expect("a placed index") });
        w.events.push(Event::Sound { fighter: -1, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key });
    }
}

/// `hoopTick`: one hoop's overlaps in the leftover pass (`StageEntry::tick`).
pub fn tick(w: &mut World, slot: u32) {
    if actor(w, slot).dead {
        return;
    }
    let g = geo(w, actor(w, slot).index);
    let mut rd = Read::default();
    // no pre-check: five boxes against a few fighters and items a tick, and any capsule height reaches them
    for s in SENSORS {
        sensor(w, slot, &g, s, &mut rd);
    }
    // the handler's ReceiveTick: the DODGED text of a fighter the last explosion no longer covers. Unverified: its
    // place behind the sensors is not read
    let t = w.flow.tick;
    let mut events = std::mem::take(&mut w.events);
    dodge_tick(w, ActorRef::Stage(slot), t, &mut events);
    w.events = events;
}

/// `hoopItemMoved`: one hoop's overlaps with a free item's Capsule after its move (`StageEntry::item_moved`), ahead
/// of the move's blocking hit; the lists are [`tick`]'s, so the leftover pass meets no edge of this item again.
pub fn item_moved(w: &mut World, slot: u32, it: ItemId) {
    if actor(w, slot).dead {
        return;
    }
    let g = geo(w, actor(w, slot).index);
    let on = item_meets(w, it);
    for (n, s) in SENSORS.into_iter().enumerate() {
        item_sensor(w, slot, it, on, box_of(&g, s), s);
        // the handler destroyed it: its EndPlay, not this move, ends its other overlaps ([`tick`])
        if n == 0 && w.actors.items.get(it).is_none_or(|i| i.dead) {
            return;
        }
    }
}

/// One sensor's edge for one item: its key joins or leaves the list, with the begin or end call.
fn item_sensor(w: &mut World, slot: u32, it: ItemId, on: bool, b: Option<Bx>, s: Sens) {
    let i = w.actors.items.get(it).expect("an item");
    let key = -1 - i.seq;
    let now = on && b.is_some_and(|b| capsule_in_box(i.x, i.y, i.capsule_radius(), i.capsule_half_height(), &b));
    let list = list_of(w, slot, s);
    let at = list.iter().position(|&k| k == key);
    if now == at.is_some() {
        return;
    }
    if now {
        list.push(key);
        on_begin(w, slot, s, key);
    } else if let Some(at) = at {
        list.remove(at);
        on_end(w, slot, s);
    }
}

/// `handleBasketball`: HandleBasketballCollision(OverlappingActor).
fn handle_basketball(w: &mut World, slot: u32, key: i32) {
    if let Some(it) = item_of(w, key) {
        // (no subclass of Item_Basketball_C is in the build's data)
        if w.actors.items.get(it).expect("an item").class != ItemClass::ItemBasketball {
            return;
        }
        // LastHolder (Basketball.last_holder, written at pickup only: a reflect's new Instigator is not it); a ball no
        // one has held reads TeamIndex through a None context: 0 (CX-M010-6: ProcessContextOpcode clears the r-value)
        let (lh, thrown_x) = match &w.actors.items.get(it).expect("an item").ext {
            crate::actors::item::ItemExt::Basketball(b) => (b.last_holder, b.thrown_x),
            _ => (-1, 0.0),
        };
        let holder = if lh >= 0 { w.by_id(lh) } else { None };
        let team = holder.map_or(0, |f| w.fighter(f).team.team);
        explode(w, slot, team);
        points_earned(w, slot, thrown_x);
        if !w.actors.items.get(it).is_none_or(|i| i.dead) {
            crate::world::items::destroy_item(w, it);
        }
        return;
    }
    let Some(f) = (if key >= 0 { w.by_id(key) } else { None }) else { return };
    let Some(held) = w.fighter(f).items.held_item else { return };
    let ok = w.actors.items.get(held).is_some_and(|i| !i.dead && i.class == ItemClass::ItemBasketball);
    if !ok {
        return;
    }
    let hx = actor(w, slot).x;
    points_earned(w, slot, hx); // the hoop's own location: 2
    crate::world::items::destroy_item(w, held); // K2_DestroyActor; its EndPlay's ClearHeldItem, then ClearHeldItem @746
    let team = w.fighter(f).team.team;
    explode(w, slot, team);
}

/// Handle Points Earned(HoopIndex, initial location): the points, BasketScored.
fn points_earned(w: &mut World, slot: u32, from_x: Real) {
    // |initial - hoop| on UE Y: the sim's x negated on both sides
    let points = if ((from_x - actor(w, slot).x).abs()) >= THREE_POINT { 3 } else { 2 };
    let tick = w.flow.tick;
    let h = hoop_mut(w, slot);
    h.scored_at = tick;
    h.points = points;
    let hoop_index = h.hoop_index;
    point_scored(w, hoop_index, points);
}

/// DoHoopExplosion's packet (@961-1971; the fields left out are false / 0 / None).
fn packet() -> HitPacket {
    HitPacket {
        sparse: true,
        damage: Some(10.0),
        base_knock_back: Some(1500.0),
        knock_back_scaling: Some(17.0),
        hit_reaction: Some(DataEnum::Value(0)),
        hit_pause_time: Some(0.2),
        hit_stun_mult: Some(1.0),
        ignore_facing: Some(true),
        attack_id: Some(0),
        destroy_projectiles: Some(true),
        play_on_hit_effects: Some(true),
        apply_hit_pause_to_damage_dealer: Some(false),
        attack_decay_type: Some(DataEnum::Value(5)),
        additive_kb_mode: Some(DataEnum::Value(1)),
        kb_scaling_starting_dmg: Some(0.0),
        hit_each_actor_only_once: Some(false),
        account_for_velocity_with_vacuum_knockback: Some(true),
        ignore_kbi: Some(false),
        kbi_multiplier: Some(1.0),
        ..HitPacket::default()
    }
}

/// `explode`: DoHoopExplosion(OwnerTeamIndex).
fn explode(w: &mut World, slot: u32, owner_team: i32) {
    let ex = geo(w, actor(w, slot).index).ex;
    let tick = w.flow.tick;
    {
        let h = hoop_mut(w, slot);
        h.exploded_at = tick;
    }
    if let Some((x, y, _)) = ex {
        w.events.push(Event::Sound {
            fighter: -1,
            ev: name!("sfx_map_M010_ball_score"),
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: true,
            key: None,
        });
    }
    {
        let h = hoop_mut(w, slot);
        h.anim = Some(name!("Basket_Anim_HR_Front"));
        h.anim_at = tick;
    }
    let Some((cx, cy, cr)) = ex else { return };
    let host = ActorRef::Stage(slot);
    let pkt = packet();
    for vic in all_fighters(w) {
        let (fx, fy) = (w.fighter(vic).body.x, crate::fighter::actor_y(w, vic));
        let (r, h) = (crate::fighter::cap_radius(w, vic), crate::fighter::capsule_half(w, vic));
        if w.fighter(vic).life.dead > 0.0
            || crate::fighter::no_collision(w, vic)
            || !capsule_in_sphere(fx, fy, r, h, cx, cy, cr)
        {
            continue;
        }
        if w.fighter(vic).team.team == owner_team {
            continue;
        }
        if crate::fighter::no_hit(w, vic) {
            continue;
        }
        if crate::fighter::invulnerable(w, vic) {
            attack_dodged(w, host, vic, 0, 1500.0, tick, None, false);
            continue;
        }
        dodge_over(w, host, vic);
        // KnockBackDirectionVector = (Y, Z) of (fighter - centre) in UE (IgnoreFacing): in sim (x, y)
        let dir = V2 { x: fx - cx, y: fy - cy };
        crate::combat::hits::apply_level_hit(
            w,
            host,
            name!("M010_Hoop_A_01_BP_C"),
            vic,
            &pkt,
            V2 { x: fx, y: fy },
            dir,
            &[name!("Buff_Elemental_Fire_C")],
        );
    }
}
