//! The items: their pool, the reach and pick-ups (Comp_ItemManager), the throws and uses, the items' pass,
//! FFAGameMode's random items (the item code and MapRandomActorSpawnArea).

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::ItemId;
use crate::actors::ProjectileId;
use crate::actors::item::{
    DefSrc, Item, ItemBox, ItemDef, ItemEnd, ThrowHitCall, flag, item_activate_parent, item_hooks, num, take_item_uses,
    take_throw_hits, text, truthy,
};
use crate::actors::{ActorClass, ItemClass};
use crate::combat::ChargeVals;
use crate::combat::buffs::BuffRef;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::math::ue::size2d;
use crate::math::{max, min, sign};
use crate::names::Name;
use crate::registry::ActorHooks;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::world::World;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::CapsuleShapeC;
use mvs_data::stage::ItemsData;
use mvs_data::value::{Props, Value};
use std::sync::Arc;

/// A data string's name in the match's [`crate::names::Names`] (interned at bind).
fn nm(w: &World, s: &str) -> Name {
    w.names.get(s).unwrap_or_else(|| panic!("a name the bind did not intern: {s}"))
}

/// A `reach` entry: a player's Comp_ItemManager ReachableItems and ClosestItem. An entry is an item, or a spawned
/// actor that is no BPI_Items (`trackReachableActor`).
#[derive(State, Debug, Default)]
pub struct Reach {
    pub items: Vec<ActorRef>,
    pub closest: Option<ActorRef>,
}

/// `ItemArea`: World.itemArea, in a match with items (`itemsOn`) from its reset on. Ticks are world
/// tick values, -1 for none.
#[derive(State, Debug, Default)]
pub struct ItemArea {
    /// FFAGameMode's StartNewMatch (PandaGameMode ReceiveBeginPlay's Delay(0.2)): its tick, -1 once run.
    pub new_match_at: i32,
    /// Items (StartTimeSpawnerServer @1208), duplicates kept.
    pub items: Vec<Name>,
    /// StartTime (@1264): GetGameTimeInSeconds at the start, float32 seconds.
    pub start: Real,
    /// ItemSpawnTimer: the tick it fires (-1: none, which ends the spawns).
    pub timer_at: i32,
    /// SpawnRandomItem's Delay(VFXToSpawnDelay) @4349: the tick it fires.
    pub pending_at: i32,
    /// RandomSpawnTransform's location Y, Z (`find_random_spawn_point` writes it; read again at @172).
    pub pending_y: Real,
    pub pending_z: Real,
}

// ---- helpers

fn item(w: &World, it: ItemId) -> &Item {
    w.actors.items.get(it).expect("a live item id")
}

fn item_mut(w: &mut World, it: ItemId) -> &mut Item {
    w.actors.items.get_mut(it).expect("a live item id")
}

/// `w.all`: the pawns, else the players.
fn all(w: &World) -> Vec<FighterId> {
    w.pawns.clone().unwrap_or_else(|| w.players.clone())
}

/// The fighter's character data.
fn char_index(w: &World, f: FighterId) -> Option<usize> {
    w.data.chars.get_index_of(w.names.str(w.fighter(f).ch))
}

/// `itemsDataOf(w)`: the items part a match reads, only in a match with items.
fn items_part(w: &World) -> Option<&ItemsData> {
    if crate::setup::items_on(&w.setup) { w.data.items.as_ref() } else { None }
}

fn idx(i: usize) -> u32 {
    u32::try_from(i).expect("a table index")
}

/// `actorDef(own, cls)` of an item class: the owner's character data, its peers' (every fighter of the match), the
/// stage's (`StageData.actors`), the items part's, then the perk parts (the owner's, its peers'); with no owner
/// `itemDef`'s rule for an item no one has picked up: the stage's, else the items part's.
pub fn resolve_def(w: &World, own: Option<FighterId>, cls: &str) -> Option<DefSrc> {
    let Some(f) = own else {
        if let Some(a) = w.data.stage.actors.as_ref().and_then(|m| m.get_index_of(cls)) {
            return Some(DefSrc::Stage(idx(a)));
        }
        return items_part(w).and_then(|d| d.actors.get_index_of(cls)).map(|a| DefSrc::Items(idx(a)));
    };
    let chars: Vec<usize> = std::iter::once(f).chain(all(w)).filter_map(|g| char_index(w, g)).collect();
    let in_char = |c: usize| {
        w.data.chars[c].fighter.actors.as_ref().and_then(|m| m.get_index_of(cls)).map(|a| DefSrc::Char(idx(c), idx(a)))
    };
    if let Some(s) = chars.iter().find_map(|&c| in_char(c)) {
        return Some(s);
    }
    if let Some(a) = w.data.stage.actors.as_ref().and_then(|m| m.get_index_of(cls)) {
        return Some(DefSrc::Stage(idx(a)));
    }
    if let Some(a) = items_part(w).and_then(|d| d.actors.get_index_of(cls)) {
        return Some(DefSrc::Items(idx(a)));
    }
    chars.iter().find_map(|&c| {
        let t = w.data.chars[c].perks.as_ref()?.actors.as_ref()?;
        t.get_index_of(cls).map(|a| DefSrc::Perk(idx(c), idx(a)))
    })
}

/// The class's registered item variant (`ACTORS.get(cls)?.item`), `None` for a class with no item entry.
fn item_class(w: &World, cls: &str) -> Option<ItemClass> {
    match w.registry.actors.get(cls) {
        Some(e) => match (e.class, &e.hooks) {
            (ActorClass::Item(c), ActorHooks::Item(_)) => Some(c),
            _ => None,
        },
        None => None,
    }
}

fn hooks(w: &World, it: ItemId) -> Option<&'static crate::registry::entries::ItemHooks> {
    item_hooks(w.registry, item(w, it).class)
}

/// The boxes an item's body meets besides the stage block (`World.blockers('item')`), with their marks, into `out`
/// (cleared first).
/// Unverified: the stage solids' marks (isStaticSolid, kinematicOf) are not read: every box is
/// taken as a moving platform's at rest, though `BoxMark::Solid` now names its solid; the six traced stages have no
/// marked box.
fn item_boxes(w: &mut World, out: &mut Vec<ItemBox>) {
    out.clear();
    let mut boxes = std::mem::take(&mut w.scratch.blockers);
    let mut marks = std::mem::take(&mut w.scratch.item_marks);
    w.blockers_marked_into(BlockKind::Item, &mut boxes, &mut marks);
    out.extend(boxes.iter().zip(&marks).map(|(&g, m)| ItemBox {
        g,
        is_static: false,
        kin: None,
        solid: match m {
            crate::world::spawned_platform::BoxMark::Solid(id) => Some(*id),
            _ => None,
        },
        poly: None,
    }));
    w.scratch.blockers = boxes;
    w.scratch.item_marks = marks;
    // the polygon solids of the channel after the boxes
    // Unverified: the body and the flight meet a polygon by its nearest point and its top in place of the box's clamp
    // and top, not PhysX's convex contact and sweep (`blocker_polys`).
    let mut polys = Vec::new();
    crate::fighter::traces::solids::blocker_polys(w, BlockKind::Item, &mut polys);
    let run = w.stage_run.as_ref();
    out.extend(polys.into_iter().map(|q| ItemBox {
        g: mvs_collision::Ground::default(),
        is_static: q.is_static,
        kin: None,
        solid: run.and_then(|r| r.solids.get(q.solid as usize)).map(|s| s.id),
        poly: Some(q),
    }));
}

/// The stage actor's own hit of each of the step's contacts with a stage solid (`stage_solid_hit_by`: the Tree Fort
/// trees' bounces of a spawned actor), behind the item's own ReceiveHit and OnComponentHit (DispatchBlockingHit
/// 142961f50: the other actor's dispatch runs after the mover's, a destroyed mover's too).
// Unverified: a lying body's contacts are dispatched as its OnComponentHit's (one per box per physics step while it
// touches), each one a hit on the solid's actor.
fn item_solid_hits(w: &mut World, id: ItemId, contacts: &[crate::actors::item::ItemContact]) {
    for c in contacts {
        let Some(sid) = c.solid else { continue };
        let i = w.stage_run.as_ref().and_then(|r| r.solids.iter().position(|s| s.id == sid));
        let Some(i) = i.and_then(|i| u32::try_from(i).ok()) else { continue };
        crate::world::stage::stage_solid_hit_by(w, ActorRef::Item(id), Some(i), c.nx, c.ny);
    }
}

fn sound(w: &mut World, fighter: i32, ev: Name, x: Real, y: Real) {
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
}

fn owner_fid(w: &World, it: &Item) -> i32 {
    it.owner.map_or(-1, |o| w.fighter(o).id)
}

/// A reachable entry's death and place (an item or a spawned actor that is no item).
fn entry_dead(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Item(i) => w.actors.items.get(i).is_none_or(|i| i.dead),
        ActorRef::Projectile(p) => w.actors.projectiles.get(p).is_none_or(|p| p.dead),
        _ => true,
    }
}

fn entry_xy(w: &World, a: ActorRef) -> (Real, Real) {
    match a {
        ActorRef::Item(i) => w.actors.items.get(i).map_or((0.0, 0.0), |i| (i.x, i.y)),
        ActorRef::Projectile(p) => w.actors.projectiles.get(p).map_or((0.0, 0.0), |p| (p.x, p.y)),
        _ => (0.0, 0.0),
    }
}

/// `reachOf`: a player's list is `World.actors.reach[id]`, a mode pawn's its own (`Items::item_reach`). The
/// list is taken out (left empty) for the caller to work on and put back by [`set_reach`].
fn take_reach(w: &mut World, f: FighterId) -> Option<(Vec<ActorRef>, Option<ActorRef>)> {
    let id = w.fighter(f).id;
    let player = usize::try_from(id).ok().filter(|&i| w.players.get(i) == Some(&f));
    match player {
        Some(i) => w.actors.reach.get_mut(i).map(|r| (std::mem::take(&mut r.items), r.closest)),
        None => w.fighter_mut(f).items.item_reach.as_mut().map(|r| (std::mem::take(&mut r.items), r.closest)),
    }
}

fn set_reach(w: &mut World, f: FighterId, items: Vec<ActorRef>, closest: Option<ActorRef>) {
    let id = w.fighter(f).id;
    let player = usize::try_from(id).ok().filter(|&i| w.players.get(i) == Some(&f));
    match player {
        Some(i) => {
            if let Some(r) = w.actors.reach.get_mut(i) {
                r.items = items;
                r.closest = closest;
            }
        }
        None => {
            if let Some(r) = w.fighter_mut(f).items.item_reach.as_mut() {
                r.items = items;
                r.closest = closest;
            }
        }
    }
}

// ---- the World's item passes

/// `World.step`: Item_Sandwich MulticastPickedUpByShaggy, one frame after the pickup (two Delay(0)): the grabber's
/// ThrowItem((0, 0, 0), not thrown) drops it (SFX_Drop), then ReleaseToPool.
pub fn destroy_pooled(w: &mut World) {
    let data = Arc::clone(&w.data);
    let mut i = 0;
    while let Some(id) = w.actors.items.nth(i) {
        i += 1;
        let it = item(w, id);
        if !it.pooled || it.dead {
            continue;
        }
        if let Some(ev) = text(it.def(&data).p, "SFX_Drop") {
            let (fighter, ev, x, y) = (owner_fid(w, it), nm(w, ev), it.x, it.y);
            sound(w, fighter, ev, x, y);
        }
        destroy_item(w, id);
    }
}

/// `itemManagerTick`: Comp_ItemManager ReceiveTick: a locally controlled owner (every fighter of a standalone world:
/// the AI controller is local there) calls BPI_SetPickupButtonIcon(IsMovingOnGround, 0) on every ReachableItems
/// entry, so an item the bot reaches first gets its icon from here, with action 0 (Attack). ReachableItems is filled
/// under HasAuthority alone and is not replicated, so on the client of a dedicated server the loop @2516 has nothing
/// to go over: a match's page drops every create this tick raises. The component ticks in TG_DuringPhysics.
pub fn item_manager_tick(w: &mut World) {
    let fs = crate::world::pawns::match_fighters_list(w);
    for &f in &fs {
        let Some((items, closest)) = take_reach(w, f) else { continue };
        // The loop's BPI_Items cast (@2903): a non-item entry gets nothing.
        for &c in &items {
            if let ActorRef::Item(it) = c {
                set_pickup_icon(w, f, it, 0);
            }
        }
        set_reach(w, f, items, closest);
    }
    crate::world::pawns::put_fighters(w, fs);
}

/// `World.step`: `f.pickupRequest` served by `pickUpClosestItem` (not a minion's).
pub fn pick_up_request(w: &mut World, f: FighterId) {
    let fd = w.fighter(f);
    if fd.life.is_minion || !fd.items.pickup_request {
        return;
    }
    w.fighter_mut(f).items.pickup_request = false;
    pick_up_closest_item(w, f);
}

/// `World.step`'s items pass: each item's doom, class step, granted buffs, its move (`Item::step`), the stage's
/// `stageItemMoved`, the blocking hit, `followItem`, the destroy rules, `itemHits`, the class tick and the reflect.
pub fn items_pass(w: &mut World) {
    let mut boxes = std::mem::take(&mut w.scratch.item_boxes);
    item_boxes(w, &mut boxes);
    let data = Arc::clone(&w.data);
    let mut contacts = std::mem::take(&mut w.scratch.item_contacts);
    let mut i = 0;
    while let Some(id) = w.actors.items.nth(i) {
        i += 1;
        let h = hooks(w, id);
        // A pending Delay ahead of DestroySpawnedActor (destroySpawnedActor), in a hand or not.
        if !item(w, id).dead && item_doom_tick(w, id) {
            continue;
        }
        // A registered class's own latent actions, in a hand or not; true is its K2_DestroyActor.
        if let Some(step) = h.and_then(|h| h.step)
            && !item(w, id).dead
            && step(w, id)
        {
            destroy_item(w, id);
        }
        // Its buff children's ticks and life timers, in a hand or not.
        let it = item(w, id);
        if !it.dead && it.elem.is_some() && it.owner.is_some() {
            crate::combat::elem::actor_buff_tick(w, ActorRef::Item(id));
        }
        let it = item(w, id);
        if it.holder >= 0 {
            // A reflect's pause Delays are the actor's own latent actions: they run in a hand too.
            if !it.dead && it.reflect.is_some() {
                crate::combat::reflect::reflect_tick(w, ActorRef::Item(id));
            }
            continue;
        }
        // Fastball's step override: never held yet, it is pinned to its spawner before and after the parent's step.
        let pinned = is_fastball(w, id) && crate::chars::creature::fastball::pinned(w, id);
        if pinned {
            crate::chars::creature::fastball::pin(w, id);
        }
        let ground = w.stage.ground;
        let accel = crate::actors::item::item_home_accel(w, id);
        let it = w.actors.items.get_mut(id).expect("a live item id");
        let d = it.def(&data);
        let st = it.step(id, d, &ground, &boxes, &mut contacts, accel, &mut w.scratch.throw_hits);
        if pinned {
            crate::chars::creature::fastball::pin(w, id);
        }
        // The move's overlaps with the stage's sensors (StageHooks.itemMoved), ahead of its blocking hit's dispatch; a
        // handler that destroyed the item leaves that hit undispatched.
        // Deviation: they come after Item.step, so behind the parent's ReceiveHit tail that the step runs
        // (`Item::spend`: the durability, the hit box, the bounce), which the build skips with the dispatch.
        if !item(w, id).dead && w.stage_run.is_some() {
            crate::world::stage::stage_item_moved(w, id);
            if item(w, id).dead {
                continue;
            }
        }
        let rh = if st.blocked { h.and_then(|h| h.receive_hit) } else { None };
        item_throw_hits(w); // a blocking hit's ItemThrowHit (ReceiveHit @4915)
        // A fighter welded to the item rides its move, ahead of the hit's End.
        crate::fighter::tick::follow_item(w, id);
        if st.end == Some(ItemEnd::Hit) {
            destroy_item(w, id);
            // The child's ReceiveHit after the parent's tail destroyed it.
            if let Some(rh) = rh {
                // (the step's contacts are handed to the hook through the scratch: the mine reads the hit normal)
                std::mem::swap(&mut w.scratch.item_contacts, &mut contacts);
                rh(w, id, true);
                std::mem::swap(&mut w.scratch.item_contacts, &mut contacts);
            }
            item_solid_hits(w, id, &contacts);
            continue;
        }
        // A hit the item outlived: the child's ReceiveHit, inside the move, ahead of the timers' destroy below.
        std::mem::swap(&mut w.scratch.item_contacts, &mut contacts);
        let mut kill = rh.is_some_and(|rh| rh(w, id, false));
        std::mem::swap(&mut w.scratch.item_contacts, &mut contacts);
        // OnActorHit (InternalDispatchBlockingHit 142800d00: after ReceiveHit, ahead of OnComponentHit): the handlers
        // bound to it (a tutorial minigame's), which may destroy the item; the solids' own hits still come.
        let mut hit_gone = false;
        if st.blocked && !kill && !item(w, id).dead {
            let at =
                crate::world::flow::DelegateList::Actor(ActorRef::Item(id), crate::world::flow::ActorList::ActorHit);
            crate::world::flow::broadcast_bound(w, at, &crate::world::flow::DelegateCall::ActorHit);
            hit_gone = item(w, id).dead;
        }
        // OnComponentHit of the step's contacts, behind ReceiveHit.
        if !kill
            && st.end.is_none()
            && !item(w, id).dead
            && let Some(ch) = h.and_then(|h| h.component_hit)
        {
            let mut k = 0;
            while k < contacts.len() && !kill && !item(w, id).dead {
                kill = ch(w, id, contacts[k]);
                k += 1;
            }
        }
        item_solid_hits(w, id, &contacts);
        if hit_gone {
            continue;
        }
        if let Some(end) = st.end {
            // DeathBlinkWarning's Finished (ItemActorParent ubergraph 10907): SFX_Destroy at the capsule, then
            // ReleaseToPool.
            let it = item(w, id);
            if end == ItemEnd::Life
                && let Some(ev) = text(it.def(&data).p, "SFX_Destroy")
            {
                let (fighter, ev, x, y) = (owner_fid(w, it), nm(w, ev), it.x, it.y);
                sound(w, fighter, ev, x, y);
            }
            destroy_item(w, id);
        } else if kill {
            destroy_item(w, id);
        } else if item(w, id).box_on() && item(w, id).owner.is_some() {
            // The overlaps the move began with World.actors components (ActorKind.spawnedMoved), ahead of its hits.
            crate::world::spawning::spawned_moved(w, ActorRef::Item(id));
            if !item(w, id).dead {
                crate::combat::phases::item_hits(w, id);
                item_throw_hits(w);
            }
        }
        // ReceiveTick (ItemKind.tick): after the move (its ProjectileMovement ticks first) and the hits.
        if let Some(tick) = h.and_then(|h| h.tick)
            && !item(w, id).dead
            && tick(w, id)
        {
            destroy_item(w, id);
        }
        if !item(w, id).dead && item(w, id).reflect.is_some() {
            crate::combat::reflect::reflect_tick(w, ActorRef::Item(id));
        }
    }
    w.scratch.item_boxes = boxes;
    w.scratch.item_contacts = contacts;
}

/// `Spawning.doomTick` of an item: the frames left of a pending DestroySpawnedActor Delay; at 0 K2_DestroyActor.
fn item_doom_tick(w: &mut World, id: ItemId) -> bool {
    let it = item_mut(w, id);
    match it.doom {
        Some(d) if d > 0 => {
            it.doom = Some(d - 1);
            if d - 1 > 0 {
                return false;
            }
        }
        _ => return false,
    }
    if !item(w, id).dead {
        crate::world::spawning::destroy_spawned(w, ActorRef::Item(id));
    }
    true
}

/// `classTimers` and `itemAreaTimers`: the world timers of the registered classes, in the frame's timer pass after
/// the pawns': the items' (ItemKind.timer, list order), then the buffs' (BuffKind.timer, by fighter, then the
/// fighter's buff list). True from a hook: K2_DestroyActor.
/// Unverified: FTimerManager pops its timers by expire time; the port's order among the timers of one frame (the
/// pawns', the items', the buffs') is the list order, not read.
pub fn class_timers(w: &mut World) {
    let mut i = 0;
    while let Some(id) = w.actors.items.nth(i) {
        i += 1;
        if let Some(t) = hooks(w, id).and_then(|h| h.timer)
            && !item(w, id).dead
            && t(w, id)
        {
            destroy_item(w, id);
        }
    }
    // (`ps` is written once per step, ahead of this pass)
    let mut due = std::mem::take(&mut w.scratch.timer_serials);
    let mut k = 0;
    while let Some(&f) = w.scratch.ps.get(k) {
        k += 1;
        if w.fighter(f).buffs.buffs.is_empty() {
            continue;
        }
        due.clear();
        due.extend(
            w.fighter(f)
                .buffs
                .buffs
                .iter()
                .filter(|b| b.frames > 0.0 && buff_timer(w, b.cls).is_some())
                .map(|b| b.serial),
        );
        for &serial in &due {
            let Some(b) = w.fighter(f).buffs.buffs.iter().find(|b| b.serial == serial) else { continue };
            if b.frames <= 0.0 {
                continue;
            }
            let t = buff_timer(w, b.cls).expect("a timer hook");
            let r = BuffRef { fighter: f, serial };
            if t(w, f, r) {
                crate::combat::buffs::end_buff(w, f, r);
            }
        }
    }
    w.scratch.timer_serials = due;
    item_area_timers(w);
}

type BuffTimer = fn(&mut World, FighterId, BuffRef) -> bool;

fn buff_timer(w: &World, cls: Name) -> Option<BuffTimer> {
    w.registry.buffs.get(w.names.str(cls)).and_then(|e| e.hooks.timer)
}

/// `itemUses(w, null)` and `itemThrowHits` of the frame's tail: what was left of the frame's ItemThrowHit and
/// ActivateItem calls (an item hit by a caller with no drain of its own: Black Adam's field).
pub fn item_uses_unowned(w: &mut World) {
    item_uses(w, None);
    item_throw_hits(w);
}

/// `updateItemReach(w)` of the frame's tail.
pub fn update_item_reach_all(w: &mut World) {
    update_item_reach(w, None);
}

// ---- the call surface of the items

/// `itemThrowHits`: the frame's ItemThrowHit calls of the classes that override it, in
/// their order: the class's own part, dead or not (the build runs it inside the hit, ahead of the destroy).
pub fn item_throw_hits(w: &mut World) {
    let mut buf: Vec<ThrowHitCall> = Vec::new();
    loop {
        take_throw_hits(w, &mut buf);
        if buf.is_empty() {
            return;
        }
        for c in std::mem::take(&mut buf) {
            if let Some(th) =
                w.actors.items.get(c.it).and_then(|i| item_hooks(w.registry, i.class)).and_then(|h| h.throw_hit)
            {
                th(w, c.it, c.hit_player, c.enemy, c.vx, c.vy);
            }
        }
    }
}

/// `itemUses`: Comp_ItemManager.ActivateItem from AnimNotify_ItemUse by `f` (`None`:
/// every one left): the item's BPI_ActivateObject, its class's own or ItemActorParent's.
pub fn item_uses(w: &mut World, f: Option<FighterId>) {
    let mut buf = Vec::new();
    take_item_uses(w, f, &mut buf);
    if buf.is_empty() {
        return;
    }
    for (it, by) in buf {
        let Some(i) = w.actors.items.get(it) else { continue };
        if i.dead {
            continue;
        }
        let class = i.class;
        if let Some(own) = item_hooks(w.registry, class).and_then(|h| h.activate) {
            own(w, it, by);
        } else if crate::actors::item::PARENT_ACTIVATE.contains(&class) {
            item_activate_parent(w, it);
        } else {
            // its own graph's, no entry: nothing
            warn_text(w, &format!("ActivateObject not implemented ({})", class.name()));
        }
    }
    item_throw_hits(w);
}

/// `destroyItem`: the item's EndPlay (K2_DestroyActor from ReleaseToPool or
/// DestroySpawnedActor): the manager entries go (ItemActorParent ReceiveEndPlay), then SpawnedActorParent's handler
/// plays DeathVFX and 'Ak Death Location' unless BPI_SetDestroyEffectsBlocked blocked them (@15557).
pub fn destroy_item(w: &mut World, it: ItemId) {
    item_mut(w, it).dead = true;
    let reset = crate::world::flow::reset_pass(w);
    // ReceiveEndPlay with IsHeld: LastHolder's Comp_ItemManager.ClearHeldItem. Not in the Lab reset's pass: the
    // holder and the fighters in reach are old pawns there.
    if !reset {
        for f in all(w) {
            if w.fighter(f).items.held_item == Some(it) {
                w.fighter_mut(f).items.held_item = None;
            }
        }
        let reach: Vec<FighterId> = item(w, it).reach.iter().copied().collect();
        for f in reach {
            track_reachable(w, f, Some(ActorRef::Item(it)), false);
        }
    }
    let i = item(w, it);
    if i.input_ui {
        let seq = i.seq;
        w.events.push(Event::ItemUiExit { item: seq, fighter: None, actor: false });
    }
    // SpawnedActorParent's EndPlay @15512: the buff children end first (not in the reset's pass).
    if !reset && item(w, it).owner.is_some() {
        crate::combat::elem::end_actor_children(w, ActorRef::Item(it));
    }
    let data = Arc::clone(&w.data);
    let i = item(w, it);
    let p = i.def(&data).p;
    let (fid, x, y, facing, cls) = (owner_fid(w, i), i.x, i.y, i.facing, i.cls);
    if !i.destroy_fx_blocked {
        if let Some(ev) = text(p, "Ak Death Location") {
            let ev = nm(w, ev);
            sound(w, fid, ev, x, y);
        }
        let vfx = crate::world::spawning::death_vfx(w, cls, fid, p, x, y, facing);
        w.events.push(vfx);
    }
    // then the class's own ReceiveEndPlay (ItemKind.end)
    if let Some(end) = hooks(w, it).and_then(|h| h.end) {
        end(w, it);
    }
    // Then the native detach of the attached children (UWorld::DestroyActor): a fighter it carried.
    // Not in the Lab reset's pass.
    if !reset {
        let fs = all(w);
        crate::fighter::tick::detach_carried(w, &fs, it);
    }
}

/// `trackReachable`: Comp_ItemManager.TrackReachableItems: the first reachable item adds the PickUpOverride pack
/// (ItemReachable), the last one leaving removes it; ClosestItem is re-picked on every change. Item UI: an add hides
/// every reachable item's icon and shows the closest one's; a removal hides the removed item's icon, then does the
/// same over the rest (hiding it once more when none is left). With BlockPickUp (@77-107, @1509-1669) the call
/// neither adds nor removes: ReachableItems and the pack stay as they are, the item's icon is hidden (@1865) and the
/// closest re-picked. `Item::reach` is the item's side and follows the call either way. `it` `None`: the pawn's own
/// TrackReachableItems(null, false).
/// Unverified: a list frozen by BlockPickUp can keep an item that left the area, was picked up or died; the dead are
/// passed over (IsValid @637), the others are as the graph leaves them and not seen in the game.
fn track_reachable(w: &mut World, f: FighterId, it: Option<ActorRef>, add: bool) {
    let Some((mut items, _)) = take_reach(w, f) else {
        let key = w.names.str(w.fighter(f).ch).to_owned();
        warn_text(w, &format!("TrackReachableItems on a fighter with no Comp_ItemManager list (a minion): {key}"));
        return;
    };
    if let Some(ActorRef::Item(i)) = it
        && let Some(x) = w.actors.items.get_mut(i)
    {
        if add {
            x.reach.insert(f);
        } else {
            x.reach.shift_remove(&f);
        }
    }
    let block = w.fighter(f).items.block_pick_up;
    match it {
        Some(a) if add && !block => {
            if !items.contains(&a) {
                items.push(a);
            }
            if items.len() == 1 {
                w.fighter_mut(f).items.item_reachable = true; // MulticastOverridePack(true) @340
            }
        }
        _ => {
            if !block && !items.is_empty() {
                if let Some(k) = it.and_then(|a| items.iter().position(|c| *c == a)) {
                    items.remove(k);
                }
                if items.is_empty() {
                    w.fighter_mut(f).items.item_reachable = false; // MulticastOverridePack(false) @1850
                }
            }
            if let Some(a) = it {
                set_item_ui(w, f, a, true);
            }
        }
    }
    // `live`: the entries not dead (set_item_ui kills none and leaves the list alone)
    for &c in &items {
        if !entry_dead(w, c) {
            set_item_ui(w, f, c, true);
        }
    }
    let closest = nearest_item(w, f, &items);
    set_reach(w, f, items, closest);
    match (closest, it) {
        (Some(c), _) => set_item_ui(w, f, c, false),
        (None, Some(a)) => set_item_ui(w, f, a, true),
        (None, None) => {}
    }
}

/// `setItemUi`: SetItemUiVisibility (Comp_ItemManager @1051-1962; a reliable NetClient RPC called by
/// TrackReachableItems, which runs under the owner's HasAuthority @25-67): only where the owner's controller is a
/// PlayerController and locally controlled @1146-1309 (a bot has an AIController). BPI_SetPickupButtonIcon
/// (IsMovingOnGround, Pickup Item Input Action For Player), then BPI_HideItemUI(hide). Which human is local differs
/// per client, so the event names the fighter and the UI takes its own. A non-item entry: the branch @1963-2028
/// calls the actor's BPI_NonItemSetPickupIcon (@2339) and BPI_NonItemHidePickupUI (@2469) with the same two values.
fn set_item_ui(w: &mut World, f: FighterId, it: ActorRef, hide: bool) {
    let fd = w.fighter(f);
    if fd.team.bot_controlled {
        return;
    }
    let (fid, on_ground) = (fd.id, fd.body.on_floor);
    match it {
        ActorRef::Item(i) => {
            set_pickup_icon(w, f, i, 10);
            let seq = item(w, i).seq;
            w.events.push(Event::ItemUi { item: seq, fighter: fid, hide, on_ground, actor: false });
        }
        ActorRef::Projectile(p) => {
            let Some(pr) = w.actors.projectiles.get(p) else { return };
            let label = match w.registry.actors.get(w.names.str(pr.cls)).map(|e| &e.hooks) {
                Some(ActorHooks::Projectile(h)) => h.pickup_label,
                _ => false,
            };
            if !label {
                return;
            }
            // A class whose overrides are the item's InputUI form (ProjectileKind.pickupLabel:
            // C023A_Car_SpawnedActor SetPickupIcon @3604-4472, @5525-6091) raises the same events, marked `actor`:
            // the latch is the car shell's (CarActor.inputUi).
            let seq = pr.seq;
            let made = match w
                .actors
                .projectiles
                .get_mut(p)
                .and_then(|pr| crate::chars::gizmo::car::car_of_mut(&mut pr.ext))
            {
                Some(car) => !std::mem::replace(&mut car.input_ui, true),
                None => {
                    let cls = w.names.str(w.actors.projectiles.get(p).map_or(Name::NONE, |pr| pr.cls)).to_owned();
                    return warn_text(
                        w,
                        &format!("ProjectileKind.pickupLabel on a class with no inputUi latch: {cls}"),
                    );
                }
            };
            if made {
                w.events.push(Event::ItemUiCreate { item: seq, fighter: fid, action: 10, on_ground, actor: true });
            }
            w.events.push(Event::ItemUi { item: seq, fighter: fid, hide, on_ground, actor: true });
        }
        _ => {}
    }
}

/// `setPickupIcon`: ItemActorParent SetPickupIcon: PopInputLabel only while InputUI is not valid.
/// A gone item a reach list still names (`Actors::gone_items`: a destroy while the fighter's BlockPickUp was set, or
/// in the Lab reset's pass, leaves it there) is its kept copy.
fn set_pickup_icon(w: &mut World, f: FighterId, it: ItemId, action: u8) {
    let i = match w.actors.items.get_mut(it) {
        Some(i) => i,
        None => match w.actors.gone_items.iter_mut().find(|(g, _)| *g == it) {
            Some((_, i)) => i,
            None => panic!("item {it:?} of a reach list is neither live nor kept in gone_items"),
        },
    };
    if i.input_ui {
        return;
    }
    i.input_ui = true;
    let seq = i.seq;
    let fd = w.fighter(f);
    let (fid, on_ground) = (fd.id, fd.body.on_floor);
    w.events.push(Event::ItemUiCreate { item: seq, fighter: fid, action, on_ground, actor: false });
}

/// `trackReachableActor`: Comp_ItemManager.TrackReachableItems(actor, add) called by a spawned actor's own graph (a
/// class that is no BPI_Items: C023A_Car_SpawnedActor's DriverCheck overlaps @4543 / @5274 -> @18189).
pub fn track_reachable_actor(w: &mut World, f: FighterId, pr: ProjectileId, add: bool) {
    track_reachable(w, f, Some(ActorRef::Projectile(pr)), add);
}

/// `nearestItem` over the entries of `items` that are not dead (a `live` filter).
fn nearest_item(w: &World, f: FighterId, items: &[ActorRef]) -> Option<ActorRef> {
    let fd = w.fighter(f);
    let mut out = None;
    let mut best = 0.0;
    for &c in items.iter().filter(|&&c| !entry_dead(w, c)) {
        let (x, y) = entry_xy(w, c);
        let d = size2d([x - fd.body.x, y - fd.body.y]);
        if out.is_some() && d >= best {
            continue;
        }
        out = Some(c);
        best = d;
    }
    out
}

/// `updateItemReach`: PickupArea begin / end overlap with a fighter's root capsule:
/// begin needs !IsHeld (CanBePickedUp and CanBePickedUpByOwner are on, RestrictedToTeam is off) and plays
/// SFX_Pickup_Area at the item. Tested at the end of a move, as UpdateOverlaps does: for one fighter after its CMC tick
/// moved the capsule, and for all after the items' physics step. A ring-out fighter still overlaps: its death
/// (ubergraph 19141) leaves the capsule where it left the blast box until the respawn.
pub fn update_item_reach(w: &mut World, only: Option<FighterId>) {
    let fs = match only {
        Some(f) => {
            let mut v = crate::world::pawns::take_fighters(w);
            v.push(f);
            v
        }
        None => crate::world::pawns::match_fighters_list(w),
    };
    for &f in &fs {
        if !w.fighter(f).items.reach_recheck {
            continue;
        }
        w.fighter_mut(f).items.reach_recheck = false;
        track_reachable(w, f, None, false);
    }
    let data = Arc::clone(&w.data);
    let enter = crate::name!("sfx_shared_item_generic_enter");
    let mut i = 0;
    while let Some(id) = w.actors.items.nth(i) {
        i += 1;
        // The class's own overlap components (ItemKind.overlaps).
        if let Some(own) = hooks(w, id).and_then(|h| h.overlaps)
            && !item(w, id).dead
            && item(w, id).holder < 0
            && own(w, id, only)
        {
            destroy_item(w, id);
        }
        let it = item(w, id);
        if it.dead || it.held {
            continue;
        }
        for &f in &fs {
            // ItemActorParent's PickupArea BeginOverlap (2384): with RestrictedToTeam only a fighter of the item's
            // Comp_Actor_Destructible team (the spawner's, then the last grabber's) is tracked; without
            // CanBePickedUpByOwner the Creator (the spawner) is not (@2752 -> @3242).
            let it = item(w, id);
            let d = it.def(&data);
            let fd = w.fighter(f);
            let (half, rad, ay) =
                (crate::fighter::capsule_half(w, f), crate::fighter::cap_radius(w, f), crate::fighter::actor_y(w, f));
            let team = it.owner.map(|o| w.fighter(o).team.team);
            let on = it.pickup_on()
                && it.overlaps_fighter(d, fd.body.x, ay, half, rad)
                && !(truthy(d.p, "RestrictedToTeam") && Some(fd.team.team) != team)
                && !(flag(d.p, "CanBePickedUpByOwner") == Some(false) && Some(f) == it.spawner);
            if on == it.reach.contains(&f) {
                continue;
            }
            let (fid, x, y) = (fd.id, it.x, it.y);
            track_reachable(w, f, Some(ActorRef::Item(id)), on);
            if on {
                sound(w, fid, enter, x, y);
            }
        }
    }
    crate::world::pawns::put_fighters(w, fs);
}

/// `pickUpClosestItem`: Comp_ItemManager.PickUpClosestItem -> EquipOnSpawn -> the item's
/// BPI_PickUp.
pub fn pick_up_closest_item(w: &mut World, f: FighterId) {
    let (items, closest) = take_reach(w, f).expect("a Comp_ItemManager list");
    let it = match closest {
        Some(c) if !entry_dead(w, c) && items.contains(&c) => Some(c),
        _ => nearest_item(w, f, &items),
    };
    set_reach(w, f, items, closest);
    match it {
        None => {}
        Some(ActorRef::Item(i)) => equip_on_spawn(w, f, i),
        Some(ActorRef::Projectile(p)) => {
            // EquipOnSpawn on an actor that is no BPI_Items (@251 -> @1767-1932): a BPI_SpawnedActor's
            // BPI_NonItemPickup(owner), then OnItemPickedUp; no HeldItem, no pack.
            let cls = w.names.str(w.actors.projectiles.get(p).expect("a live projectile").cls).to_owned();
            let pick = match w.registry.actors.get(&cls).map(|e| &e.hooks) {
                Some(ActorHooks::Projectile(h)) => h.non_item_pickup,
                _ => None,
            };
            match pick {
                None => warn_text(w, &format!("BPI_NonItemPickup not implemented ({cls})")),
                Some(pick) => pick(w, p, f),
            }
            // The OnItemPickedUp broadcast of a non-item: PickupHooks takes items.
            let key = w.names.str(w.fighter(f).ch).to_owned();
            if w.registry.pickups.contains(&key) {
                warn_text(w, &format!("OnItemPickedUp with a non-item ({cls}) not given to {key}"));
            }
        }
        Some(_) => {}
    }
}

/// `equipOnSpawn`: Comp_ItemManager.EquipOnSpawn(it) on f's manager, from PickUpClosestItem or
/// EquipOnSpawnMulticast (`spawn_and_equip_item`): physics off, the reach entries of every manager removed,
/// SFX_Pickup at the item, collision off. Item_Sandwich then checks the grabber: a Shaggy (BPI_GetFighterCharacter 3)
/// gets MulticastPickedUpByShaggy, which shows and equips the RockMesh again; the item leaves the hand a frame later
/// (`destroy_pooled`). Any other fighter keeps it (HeldItem, the hold override pack).
fn equip_on_spawn(w: &mut World, f: FighterId, it: ItemId) {
    let h = hooks(w, it);
    let key = w.names.str(w.fighter(f).ch).to_owned();
    let picked = w.registry.pickups.get(&key).map(|p| p.picked_up);
    // A class's PickUp that skips ItemActorParent's (ItemKind.pickUpOverride).
    if let Some(o) = h.and_then(|h| h.pick_up_override)
        && o(w, it, f)
    {
        if let Some(p) = picked {
            p(w, f, it);
        }
        return;
    }
    let fid = w.fighter(f).id;
    item_mut(w, it).pick_up(f, fid);
    // Fastball's override, past the parent's.
    if is_fastball(w, it) {
        crate::chars::creature::fastball::pick_up(w, it, f);
    }
    let reach: Vec<FighterId> = item(w, it).reach.iter().copied().collect();
    for g in reach {
        track_reachable(w, g, Some(ActorRef::Item(it)), false);
    }
    let data = Arc::clone(&w.data);
    let i = item(w, it);
    let p = i.def(&data).p;
    if let Some(ev) = text(p, "SFX_Pickup") {
        let (ev, x, y) = (nm(w, ev), i.x, i.y);
        sound(w, fid, ev, x, y);
    }
    // Comp_ItemManager.EquipOnSpawn returns after HeldItem with UseOnPickup (LocalUseonPickup, 1729): no hold
    // override pack, so the port's held item (which stands for that pack) stays unset.
    let used = truthy(p, "UseOnPickup");
    let shaggy = item(w, it).class == ItemClass::ItemSandwich
        && w.data.chars.get(&key).and_then(|c| text(&c.fighter.pawn, "HydraCharacterData")) == Some("character_shaggy");
    if shaggy {
        item_mut(w, it).pooled = true;
        if crate::chars::shaggy::state::shaggy_of(w, f).is_some() {
            // ShaggyState.rock = true: the name added when it is not there.
            if !crate::fighter::has_equipped(w, f, crate::name!("rockmesh")) {
                let rock = crate::name!("RockMesh");
                crate::fighter::equip_add(w, f, rock);
            }
        }
    } else if !used {
        w.fighter_mut(f).items.held_item = Some(it);
        // ConstructHoldOverridePack @2467: HoldingItemOverride as it is at the pick-up
        let pack = crate::fighter::anim::nav_hold(w, f);
        w.fighter_mut(f).anim.hold_pack = pack;
    }
    // ItemActorParent PickUp (2699-2709): with UseOnPickup the class's ActivateOnPickup.
    if used {
        match h.and_then(|h| h.picked_up) {
            None => warn_text(w, &format!("ActivateOnPickup not implemented ({})", item(w, it).class.name())),
            Some(pu) => {
                if pu(w, it, f) {
                    destroy_item(w, it);
                }
            }
        }
    }
    // the class's own PickUp
    if let Some(g) = h.and_then(|h| h.grabbed)
        && !item(w, it).dead
    {
        g(w, it, f);
    }
    // EquipOnSpawn's OnItemPickedUp broadcast (1692): the grabbing pawn's own handler.
    if let Some(p) = picked {
        p(w, f, it);
    }
}

/// `spawnItemAtDeath`: an ItemActorParent child as pr's ActorClassSpawnedAtDeath: at pr's
/// place and pitch, facing its flight, with the death spawn's (0, 200) plus half the owner's velocity as
/// HandleActorPostSpawn's impulse, the rest as `spawn_item`'s. late: in the world's timer tick.
pub fn spawn_item_at_death(w: &mut World, pr: ProjectileId, cls: Name, late: bool) -> Option<ItemId> {
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    let cls_s = w.names.str(cls).to_owned();
    let o = p.owner;
    let src = resolve_def(w, o, &cls_s);
    let class = item_class(w, &cls_s);
    let (Some(o), Some(src), Some(class)) = (o, src, class) else {
        warn_text(w, &format!("spawned actor not implemented ({cls_s})"));
        return None;
    };
    let (px, py, pitch, pvx, pfacing, pscale, pcls) = (p.x, p.y, p.pitch, p.vx, p.facing, p.scale, p.cls);
    let pcls = w.names.str(pcls).to_owned();
    let pdef = resolve_def(w, Some(o), &pcls);
    let data = Arc::clone(&w.data);
    let pp = pdef.map_or_else(|| ItemDef::at(&data, DefSrc::None), |s| ItemDef::at(&data, s)).p;
    let fd = w.fighter(o);
    let facing = sign(if pvx != 0.0 && !pvx.is_nan() { pvx } else { pfacing });
    let (vx, vy) = (fd.body.vx / 2.0, 200.0 + fd.body.vy / 2.0);
    let sb = crate::world::blast::spawn_box(w);
    let mut it =
        Item::make(w, cls, class, Some(o), src, ItemDef::at(&data, src), px, py, pitch, facing, vx, vy, &sb, late);
    if let Some(sub) = item_hooks(w.registry, class).and_then(|h| h.sub) {
        it.ext = sub();
    }
    // The spawn transform's scale is the item root's: SpawnedActorParent @16629 SelectTransform(GetTransform,
    // MakeTransform(.., SpawnedActorAtDeathTransform's scale), SpawnDeathActorWithThisActorsTransform).
    it.scale = if truthy(pp, "SpawnDeathActorWithThisActorsTransform") {
        pscale
    } else {
        pp.get("SpawnedActorAtDeathTransform")
            .and_then(Value::as_obj)
            .and_then(|t| t.get("Scale3D"))
            .and_then(Value::as_obj)
            .and_then(|s| num(s, "X"))
            .unwrap_or(1.0)
    };
    let charge = crate::combat::elem::instigator_charge(w, o);
    let id = w.actors.items.insert(it);
    Some(add_item(w, id, Some(o), charge, true))
}

/// `spawnItem`: an ItemActorParent child of a registered class spawned through f's
/// Comp_Spawner.CompSpawnActor, by a notify or by another actor's graph: at (x, y) with the launch velocity (direction
/// x InitialSpeed, already mirrored by f's facing). The item has no projectile movement and simulates physics, so
/// HandleActorPostSpawn (10992-11329) adds that velocity plus half of f's as an impulse. late: spawned in the
/// world's timer tick. charge: f's charge values at the CompSpawnActor call (the default: `instigatorCharge(f)`).
#[allow(clippy::too_many_arguments, reason = "the call keeps the build function's parameters")]
pub fn spawn_item(
    w: &mut World,
    cls: Name,
    f: FighterId,
    x: Real,
    y: Real,
    vx: Real,
    vy: Real,
    late: bool,
    charge: Option<ChargeVals>,
) -> Option<ItemId> {
    let cls_s = w.names.str(cls).to_owned();
    let (Some(src), Some(class)) = (resolve_def(w, Some(f), &cls_s), item_class(w, &cls_s)) else {
        warn_text(w, &format!("spawned actor not implemented ({cls_s})"));
        return None;
    };
    let data = Arc::clone(&w.data);
    let fd = w.fighter(f);
    let (facing, fvx, fvy) = (fd.body.facing_dir, fd.body.vx, fd.body.vy);
    let sb = crate::world::blast::spawn_box(w);
    let mut it = Item::make(
        w,
        cls,
        class,
        Some(f),
        src,
        ItemDef::at(&data, src),
        x,
        y,
        0.0,
        facing,
        vx + fvx / 2.0,
        vy + fvy / 2.0,
        &sb,
        late,
    );
    if let Some(sub) = item_hooks(w.registry, class).and_then(|h| h.sub) {
        it.ext = sub(); // the class's own variables (ItemKind.sub)
    }
    let id = w.actors.items.insert(it);
    Some(add_item(w, id, Some(f), charge, true))
}

/// `spawnStageItem`: a stage's level script's BeginDeferredActorSpawnFromClass of an item class (The Court's ball:
/// Map_M010 @15-553): at (x, y), identity rotation, no owner and no Instigator, collision handling 1 (AlwaysSpawn: no
/// FindTeleportSpot push-out), then FinishSpawning: the class's defaults from StageData.actors, at rest. No fighter's
/// SpawnedActors lists it, no charge comes, no NumberOfActorsAllowed count. Unverified: the facing of an identity
/// rotation is taken as 1.
pub fn spawn_stage_item(w: &mut World, cls: Name, x: Real, y: Real) -> Option<ItemId> {
    let cls_s = w.names.str(cls).to_owned();
    let src = w.data.stage.actors.as_ref().and_then(|m| m.get_index_of(&cls_s)).map(|a| DefSrc::Stage(idx(a)));
    let (Some(src), Some(class)) = (src, item_class(w, &cls_s)) else {
        warn_text(w, &format!("stage-spawned actor not implemented ({cls_s})"));
        return None;
    };
    let data = Arc::clone(&w.data);
    let sb = crate::world::blast::spawn_box(w);
    let mut it = Item::make(w, cls, class, None, src, ItemDef::at(&data, src), x, y, 0.0, 1.0, 0.0, 0.0, &sb, false);
    if let Some(sub) = item_hooks(w.registry, class).and_then(|h| h.sub) {
        it.ext = sub();
    }
    let id = w.actors.items.insert(it);
    let id = add_item(w, id, None, None, false);
    // A fighter of the match to look the class up through while it has no owner, set after the spawn so its
    // components' BeginPlay stays unported (`add_item`).
    let look = all(w).first().copied();
    item_mut(w, id).lookup_fighter = look;
    Some(id)
}

/// `spawnWorldItem`: BeginDeferredActorSpawnFromClass(class, (x, y), collision handling 3, the area as the owner) +
/// FinishSpawningActor of an item class by a world actor (MapRandomActorSpawnArea SpawnRandomItem @370 / @610): no
/// fighter owner and no Instigator, no spawner's list, no charge, no NumberOfActorsAllowed count. PostActorConstruction
/// with handling 3 runs FindTeleportSpot at the place and destroys the actor before BeginPlay when it finds no spot:
/// `None` then. Unverified: the facing of the area's zero rotation is taken as 1. Deviation: FindTeleportSpot's try
/// while spawning (8 permutations) is the begun-play one (findTeleportSpotAt), as `push_out_item`'s.
pub fn spawn_world_item(w: &mut World, cls: Name, src: DefSrc, x: Real, y: Real) -> Option<ItemId> {
    let cls_s = w.names.str(cls).to_owned();
    let class = item_class(w, &cls_s)?;
    let data = Arc::clone(&w.data);
    let a = ItemDef::at(&data, src);
    let sb = crate::world::blast::spawn_box(w);
    let mut it = Item::make(w, cls, class, None, src, a, x, y, 0.0, 1.0, 0.0, 0.0, &sb, false);
    if let Some(sub) = item_hooks(w.registry, class).and_then(|h| h.sub) {
        it.ext = sub();
    }
    it.lookup_fighter = all(w).first().copied();
    if let Some(cap) = a.comp("Capsule") {
        let body = cap.get("BodyInstance").and_then(Value::as_obj);
        let what = crate::name!("spawn spot test");
        if let Some(boxes) = crate::world::spawning::root_body_blockers(w, body, cls, what) {
            let [ux, uy] = crate::math::ue::turn_by_pitch(it.pitch, [0.0, 1.0]);
            let shape = crate::world::spawning::capsule_shape(Some(capsule_c(cap)), it.scale, ux * it.facing, uy);
            let at = crate::world::spawning::find_teleport_spot_at_in(boxes.view(), x, y, &shape);
            if !at.ok {
                return None;
            }
            it.x = at.x;
            it.prev_x = at.x;
            it.y = at.y;
            it.prev_y = at.y;
        }
    }
    let id = w.actors.items.insert(it);
    Some(add_item(w, id, None, None, false))
}

fn capsule_c(cap: &Props) -> CapsuleShapeC {
    CapsuleShapeC { capsule_radius: num(cap, "CapsuleRadius"), capsule_half_height: num(cap, "CapsuleHalfHeight") }
}

/// The class defaults `itemDef` gives: its owner's (`actorDef`: the fighters' data, then the stage's, then the items
/// part's), for an item no one has picked up the stage's, else the items part's.
pub fn item_def_src(w: &World, it: ItemId) -> Option<DefSrc> {
    let i = item(w, it);
    resolve_def(w, i.owner, w.names.str(i.cls))
}

/// `itemDef`: the class defaults through [`item_def_src`] (their `components` by
/// `ItemDef::at` over the same source).
pub fn item_def(w: &World, it: ItemId) -> Option<&Props> {
    item_def_src(w, it).map(|s| ItemDef::at(&w.data, s).p)
}

/// `addItem`: the rest of the spawn of an item made by `spawn_item` or by its class
/// (ItemKind.make), already in the World's list: the push-out (`push_out_item`), its number, the charge, the lists,
/// the passive buffs, NumberOfActorsAllowed and the class's BeginPlay. `f` `None`: a stage item (no spawner's list).
/// `push_out` false: a spawn whose collision handling runs no FindTeleportSpot (the default: `!!f`).
pub fn add_item(w: &mut World, it: ItemId, f: Option<FighterId>, charge: Option<ChargeVals>, push_out: bool) -> ItemId {
    if push_out {
        push_out_item(w, it); // PostActorConstruction's FindTeleportSpot, ahead of BeginPlay
    }
    let seq = w.actors.item_seq;
    w.actors.item_seq += 1;
    item_mut(w, it).seq = seq;
    crate::combat::elem::receive_charge(w, ActorRef::Item(it), charge);
    if let Some(f) = f {
        w.fighter_mut(f).items.spawned_actors.push(ActorRef::Item(it));
    }
    // its components' BeginPlay. Not ported: a stage item's (no fighter to look its class up
    // through: `spawn_stage_item`).
    if f.is_some() || item(w, it).lookup_fighter.is_some() {
        crate::combat::elem::spawn_passive_buffs(w, ActorRef::Item(it));
    }
    // The BeginPlay blast-box loop's @15066 (Item's constructor): the class's own BeginPlay goes on, then the destroy.
    let doomed = item(w, it).dead;
    item_mut(w, it).dead = false;
    if let Some(f) = f {
        // the parent's, ahead of the class's own part
        crate::world::spawning::actors_allowed(w, ActorRef::Item(it), f);
    }
    let begun = hooks(w, it).and_then(|h| h.begin).is_some_and(|b| b(w, it));
    if (begun || doomed) && !item(w, it).dead {
        destroy_item(w, it);
    }
    it
}

/// `pushOutItem`: FindTeleportSpot at an item's spawn: every ported item comes through its spawner's
/// Comp_Spawner.CompSpawnActor (collision handling 2, @9054 / @9531). The root Capsule at its template scale, its axis
/// tilted by the spawn pitch as the item's own capsule tests, out of what its profile blocks (rootBodyBlockers: Item,
/// the stage block and World.blockers('item')). Nothing written where it is clear.
/// Deviation: the constructor's ForceSpawnPositionInsideBlastBox clamp (BeginPlay, after the push-out in the build)
/// comes first here; it moves only a place outside the blast box, which no stage box reaches. An item spawned attached
/// (Comp_Spawner @3913: BPI_Items, EquipOnSpawnMulticast) is not ported: no notify of the ported data spawns one.
fn push_out_item(w: &mut World, it: ItemId) {
    let Some(src) = item_def_src(w, it) else { return };
    let data = Arc::clone(&w.data);
    let Some(cap) = ItemDef::at(&data, src).comp("Capsule") else { return };
    let body = cap.get("BodyInstance").and_then(Value::as_obj);
    let (cls, x, y, scale, pitch, facing) = {
        let i = item(w, it);
        (i.cls, i.x, i.y, i.scale, i.pitch, i.facing)
    };
    let what = crate::name!("spawn push-out");
    let Some(boxes) = crate::world::spawning::root_body_blockers(w, body, cls, what) else { return };
    let [ux, uy] = crate::math::ue::turn_by_pitch(pitch, [0.0, 1.0]);
    let shape = crate::world::spawning::capsule_shape(Some(capsule_c(cap)), scale, ux * facing, uy);
    let at = crate::world::spawning::find_teleport_spot_shape_in(boxes.view(), x, y, &shape);
    if at.x == x && at.y == y {
        return;
    }
    let i = item_mut(w, it);
    i.x = at.x;
    i.prev_x = at.x;
    i.y = at.y;
    i.prev_y = at.y;
}

/// `spawnAndEquipItem`: Comp_ItemManager SpawnAndEquipItem(cls) on f's manager (ubergraph @3520-4180; Item_Mystery's
/// call @410): a valid HeldItem is dropped first, ThrowItem((0, 0, 0)) @3658; then
/// BeginDeferredActorSpawnFromClass(cls, MakeTransform((0, 0, 0)), collision handling 0, owner f) @3777, Instigator = f
/// @3827, FinishSpawningActor at (0, 0, 0) @3935; the actor's BPI_UpdateHitbox @4143 and EquipOnSpawnMulticast(it)
/// @4180, which EquipOnSpawn's SetHitBoxOn(false) follows: the hit box ends off either way.
/// Unverified: collision handling 0 (Undefined) is taken as the class default AlwaysSpawn (no FindTeleportSpot). The
/// facing is f's.
pub fn spawn_and_equip_item(w: &mut World, f: FighterId, cls: Name) -> Option<ItemId> {
    if w.fighter(f).items.held_item.is_some() {
        drop_held_item(w, f);
    }
    let cls_s = w.names.str(cls).to_owned();
    let (Some(src), Some(class)) = (resolve_def(w, Some(f), &cls_s), item_class(w, &cls_s)) else {
        warn_text(w, &format!("SpawnAndEquipItem: item class not implemented ({cls_s})"));
        return None;
    };
    let data = Arc::clone(&w.data);
    let facing = w.fighter(f).body.facing_dir;
    let sb = crate::world::blast::spawn_box(w);
    let mut it =
        Item::make(w, cls, class, Some(f), src, ItemDef::at(&data, src), 0.0, 0.0, 0.0, facing, 0.0, 0.0, &sb, false);
    if let Some(sub) = item_hooks(w.registry, class).and_then(|h| h.sub) {
        it.ext = sub();
    }
    let charge = crate::combat::elem::instigator_charge(w, f);
    let id = w.actors.items.insert(it);
    let id = add_item(w, id, Some(f), charge, false);
    if item(w, id).dead {
        return None; // (IsValid @3973)
    }
    equip_on_spawn(w, f, id);
    Some(id)
}

/// `dropHeldItem`: Comp_ItemManager.ThrowItem((0, 0, 0)) called from outside the fighter's own tick (Item_Basketball's
/// MulticastThrowItem from its DropItem): f's held item is dropped where it is held, released at once.
pub fn drop_held_item(w: &mut World, f: FighterId) {
    crate::fighter::match_side::throw_item(w, f, 0.0, 0.0, None);
    release_items(w, f);
}

/// `releaseItems`: Comp_ItemManager.ThrowItem from the velocity on. IsThrown = the direction is not zero. V0 =
/// direction x ThrowSpeedModifier x ThrowSpeed; on each axis the fighter's velocity (at the call) is added when V0 is
/// not zero there and both have the same sign test (> 0); the item gets that velocity with MaxSpeed 3 x its size; the
/// projectile speed buffs (@2314: InitialSpeed and MaxSpeed through ApplyProjectileSpeedBuffsToInputs, the velocity
/// Normal(v) x the buffed size). Thrown: SFX_Throw on the item; dropped: SFX_Drop and physics from rest.
/// Unverified: the item is taken to move in the frame of its release.
pub fn release_items(w: &mut World, f: FighterId) {
    let data = Arc::clone(&w.data);
    let throw = Some(crate::name!("sfx_shared_item_generic_throw"));
    let mut k = 0;
    while let Some(r) = w.fighter(f).items.item_releases.get(k).cloned() {
        k += 1;
        let it = r.it;
        let Some(i) = w.actors.items.get(it) else { continue };
        if i.dead {
            continue;
        }
        let p = i.def(&data).p;
        let thrown = r.dx.abs() > 1e-4 || r.dy.abs() > 1e-4;
        let throw_speed = crate::fighter::common::char_data(w, w.fighter(f).ch).def.throw_speed;
        let sp = num(p, "ThrowSpeedModifier").unwrap_or(1.0) * crate::math::narrow(throw_speed);
        // The sign tests are on UE's Y (= -x) and Z.
        let (v0x, v0y) = (r.dx * sp, r.dy * sp);
        let vx = if v0x != 0.0 && (-v0x > 0.0) == (-r.vx > 0.0) { v0x + r.vx } else { v0x };
        let vy = if v0y != 0.0 && (v0y > 0.0) == (r.vy > 0.0) { v0y + r.vy } else { v0y };
        let vs = size2d([vx, vy]);
        let bs = crate::fighter::moves::projectile_speed_buffed(w, f, vs);
        if bs == vs {
            let d = ItemDef::at(&data, item(w, it).src);
            item_mut(w, it).release(d, r.x, r.y, vx, vy, thrown, r.attack_id, None);
        } else {
            let ms = crate::fighter::moves::projectile_speed_buffed(w, f, vs * 3.0);
            let d = ItemDef::at(&data, item(w, it).src);
            // Normal(velocity, 0.0001) x the buffed speed
            let u = crate::math::ue::normal([0.0, vx, vy], crate::math::ue::KINDA_SMALL_NUMBER);
            item_mut(w, it).release(d, r.x, r.y, u[1] * bs, u[2] * bs, thrown, r.attack_id, Some(ms));
        }
        // Fastball's override, past the parent's.
        if is_fastball(w, it) {
            crate::chars::creature::fastball::release(w, it, thrown);
        }
        if thrown && let Some(t) = hooks(w, it).and_then(|h| h.thrown) {
            t(w, it); // the class's own Thrown
        }
        crate::fighter::tick::follow_item(w, it); // the item's K2_SetActorTransform carries a welded fighter
        // `p.SFX_Throw ?? generic`: an empty name is kept, and plays nothing (as an empty SFX_Drop)
        let ev = match (thrown, text(p, if thrown { "SFX_Throw" } else { "SFX_Drop" })) {
            (_, Some("")) => None,
            (_, Some(s)) => Some(nm(w, s)),
            (true, None) => throw,
            (false, None) => None,
        };
        if let Some(ev) = ev {
            let (fid, x, y) = (w.fighter(f).id, item(w, it).x, item(w, it).y);
            sound(w, fid, ev, x, y);
        }
        // ThrowItem's OnItemThrown(HeldItem) @3092, after BPI_ReleaseFromFighter @2835 and only for a thrown direction
        // (@3078): the buffs that bound it.
        if thrown {
            let arg = BuffSignalArg { actor: Some(ActorRef::Item(it)), ..BuffSignalArg::default() };
            crate::combat::buffs::buff_signal(w, f, BuffSignal::ItemThrown, arg);
        }
    }
    w.fighter_mut(f).items.item_releases.clear();
}

/// `itemBoxActivated`: an item's hit box turned on by its class outside the item's step (C023B_DroppedBomb's
/// Detonate: HitBoxOn? true and UpdateHitbox). SetCollisionEnabled updates the overlaps at once, so the fighters the
/// box already overlaps are hit inside the call.
pub fn item_box_activated(w: &mut World, it: ItemId) {
    let i = item(w, it);
    if i.dead || !i.box_on() || i.owner.is_none() || i.holder >= 0 {
        return;
    }
    crate::combat::phases::item_hits(w, it);
    item_throw_hits(w);
}

// ---- MapRandomActorSpawnArea_C and FFAGameMode's random items

/// `MapSpawnArea`: a placed MapRandomActorSpawnArea: its Box's world bounds
/// (GetComponentBounds: Origin and BoxExtent), its actor location (K2_GetActorLocation: root) and
/// BigObjectSpawnXLocations, UE axes.
#[derive(Clone, PartialEq, Debug)]
pub struct MapSpawnArea {
    pub level: Name,
    pub actor: Name,
    pub origin: [Real; 3],
    pub extent: [Real; 3],
    pub root: [Real; 3],
    pub big_object_x: Vec<Real>,
}

// The class defaults of the timers (Default__MapRandomActorSpawnArea_C; no placed area of the build overrides one).
const MAX_ITEM_SPAWN_TIME: Real = 12.5;
const MIN_ITEM_SPAWN_TIME: Real = 5.0;
const TIME_TILL_FASTEST_SPAWN: Real = 150.0;
const VFX_TO_SPAWN_DELAY: Real = 0.4;

fn v3(v: &[f64]) -> [Real; 3] {
    [0, 1, 2].map(|i| crate::math::narrow(v.get(i).copied().unwrap_or(0.0)))
}

/// `actorOfClass`: UGameplayStatics::GetActorOfClass(MapRandomActorSpawnArea_C): the
/// stage's area (the bound data's spawn areas of the stage's map), or `None` with a warning. With two the first of
/// the manifest's order is taken (unverified).
pub fn actor_of_class(w: &World) -> Option<MapSpawnArea> {
    let Some(a) = w.data.spawn_areas.first() else {
        warn_text(
            w,
            &format!("MapRandomActorSpawnArea: the stage {} has none (GetActorOfClass gives None)", w.data.stage.name),
        );
        return None;
    };
    Some(MapSpawnArea {
        level: w.names.get(&a.level).unwrap_or(Name::NONE),
        actor: w.names.get(&a.actor).unwrap_or(Name::NONE),
        origin: v3(&a.origin),
        extent: v3(&a.extent),
        root: v3(&a.root),
        big_object_x: a.big_object_x.iter().map(|&x| crate::math::narrow(x)).collect(),
    })
}

/// `findRandomSpawnPoint`: FindRandomSpawnPoint (ubergraph @4083-4238): GetComponentBounds(Box) ->
/// RandomPointInBoundingBox(Origin, BoxExtent) -> MakeTransform(that point, rot(0, 0, 0), (1, 1, 1)). The location in
/// UE axes, float32 as the engine's FVector holds it. RandomPointInBoundingBox = FMath::RandPointInBox: one FRandRange
/// per axis, Min + (Max - Min) x FRand. Deviation: the draws are the world's stream (World.randomFloat), not
/// FMath::FRand's, and are taken X, Y, Z in that order. X is drawn though the sim plane has none: it keeps the count
/// of draws. RandomSpawnTransform is the area's member: `World.item_area` holds it in a match with items.
pub fn find_random_spawn_point(w: &mut World, area: &MapSpawnArea) -> [Real; 3] {
    let mut axis = |i: usize| {
        let lo = area.origin[i] - area.extent[i];
        let hi = area.origin[i] + area.extent[i];
        lo + ((hi - lo) * w.random_float())
    };
    let x = axis(0);
    let y = axis(1);
    let z = axis(2);
    if let Some(ar) = w.item_area.as_mut() {
        ar.pending_y = y;
        ar.pending_z = z;
    }
    [x, y, z]
}

/// `newItemArea`: World.reset, in a match with items. In FreeForAll (FFAGameMode's
/// StartNewMatch) the game mode's ReceiveBeginPlay starts its Delay(0.2) before the first step (counted from the next
/// one); in another mode only a mutator starts the spawner, and the area waits with no timer.
/// Unverified: a Lab-style reset of such a match (tests only) starts it again from the reset, as a new level's.
pub fn new_item_area(w: &World) -> ItemArea {
    let new_match_at = if crate::world::flow::ffa_items_on(&w.setup) {
        w.flow.tick + crate::math::to_int32(crate::fighter::common::latent_frames(0.2))
    } else {
        -1
    };
    ItemArea {
        new_match_at,
        items: Vec::new(),
        start: 0.0,
        timer_at: -1,
        pending_at: -1,
        pending_y: 0.0,
        pending_z: 0.0,
    }
}

/// GetGameTimeInSeconds: the world clock (every fighter's `time`).
fn game_time(w: &World) -> Real {
    all(w).first().map_or(0.0, |&f| w.fighter(f).clock.time)
}

/// `itemAreaLatent`: the area's latent actions and the game mode's, in the world's
/// leftover latent pass (ahead of the stage's). Unverified: the game mode and the area are taken ahead of the stage's
/// objects, the mode first.
pub fn item_area_latent(w: &mut World) {
    let tick = w.flow.tick;
    let Some(ar) = w.item_area.as_mut() else { return };
    if ar.new_match_at >= 0 && tick >= ar.new_match_at {
        ar.new_match_at = -1;
        start_new_match(w);
    }
    let Some(ar) = w.item_area.as_mut() else { return };
    if ar.pending_at >= 0 && tick >= ar.pending_at {
        ar.pending_at = -1;
        spawn_pending(w);
    }
}

/// `itemAreaTimers`: the area's ItemSpawnTimer, in the timer pass (behind the
/// classes' timers, ahead of the stage's; Unverified order). A one-shot: gone before its event runs.
pub fn item_area_timers(w: &mut World) {
    let tick = w.flow.tick;
    let Some(ar) = w.item_area.as_mut() else { return };
    if ar.timer_at < 0 || tick < ar.timer_at {
        return;
    }
    ar.timer_at = -1;
    spawn_random_item(w);
}

/// FFAGameMode StartNewMatch @2755 (behind the parent's): GetAllActorsOfClass(MapRandomActorSpawnArea)[0] @2776; valid
/// @2860: StartTimeSpawnerServer(Items) @2954, then SpawnCenterActor(BugsBunnyNeutralSafe) @3054.
fn start_new_match(w: &mut World) {
    let Some(area) = actor_of_class(w) else { return };
    let Some(d) = items_part(w) else {
        warn_text(w, "MapRandomActorSpawnArea: the match has no items part (GameData.items)");
        return;
    };
    let items: Vec<Name> = d.ffa_items.iter().map(|s| nm(w, s)).collect();
    let center = nm(w, &d.center_actor);
    // the timer set in the leftover pass: pending until this frame's timer pass, which does not count the frame
    let tick = crate::real(w.flow.tick);
    start_time_spawner_server(w, &items, tick);
    spawn_center_actor(w, &area, center);
}

/// `startTimeSpawnerServer`: StartTimeSpawnerServer @1167: Items, StartTime
/// (GetGameTimeInSeconds), then (@1291-2672) the blast box's bounds, which the spawn does not read; ItemSpawnTimer =
/// K2_SetTimerDelegate(SpawnRandomItem, MaxItemSpawnTime, no loop) @2699. A timer set before the manager's tick of a
/// frame is pending until that frame's timer pass and counts from the frame after it: `active_from` is the tick of
/// the timer pass that makes it active.
pub fn start_time_spawner_server(w: &mut World, items: &[Name], active_from: Real) {
    let start = game_time(w);
    let at = active_from + crate::combat::hit::timer_ticks(Some(MAX_ITEM_SPAWN_TIME));
    let Some(ar) = w.item_area.as_mut() else { return };
    ar.items = items.to_vec();
    ar.start = start;
    ar.timer_at = crate::math::to_int32(at);
}

/// `spawnCenterActor`: SpawnCenterActor @2894-3824: per BigObjectSpawnXLocations
/// entry, BeginDeferredActorSpawnFromClass(class, the actor location + (0, entry, 0) + (0, 0, BoxExtent.Z), handling 0,
/// owner self) @3301 and FinishSpawningActor @3745 (spawnWorldProjectile). The class's
/// ProjectileMovement has no InitialSpeed, so it starts at rest and falls.
pub fn spawn_center_actor(w: &mut World, area: &MapSpawnArea, cls: Name) {
    let cls_s = w.names.str(cls).to_owned();
    let has = items_part(w).is_some_and(|d| d.actors.contains_key(&cls_s));
    let ok = match w.registry.actors.get(&cls_s) {
        Some(e) => matches!(e.hooks, ActorHooks::Projectile(_)) && e.ownerless,
        None => false,
    };
    if !has || !ok {
        warn_text(w, &format!("MapRandomActorSpawnArea: SpawnCenterActor of {cls_s} not ported (skipped)"));
        return;
    }
    let Some(look) = all(w).first().copied() else { return };
    for &bx in &area.big_object_x {
        let y = area.root[1] + bx;
        let z = area.root[2] + area.extent[2];
        // (sim x = -UE Y; 0 - y keeps +0)
        let pr = crate::actors::projectile::make_world_at_rest(w, cls, look, 0.0 - y, z);
        crate::world::spawning::spawn_world_projectile(w, pr);
    }
}

/// `spawnRandomItem`: SpawnRandomItem @4266: FindRandomSpawnPoint (three draws), MulticastPreSpawnVFX(the location)
/// @4326 (PS_ItemSpawn at scale 2 @4473: the itemFx event, at the location's Y and Z), Delay(VFXToSpawnDelay) @4349 on
/// the area, started in the timer pass: counted from the next frame.
fn spawn_random_item(w: &mut World) {
    let Some(area) = actor_of_class(w) else { return };
    let at = find_random_spawn_point(w, &area);
    let (cls, vfx) = (crate::name!("MapRandomActorSpawnArea_C"), crate::name!("PS_ItemSpawn"));
    w.events.push(Event::ItemFx { cls, item: -1, fighter: -1, vfx, x: 0.0 - at[1], y: at[2], scale: 2.0 });
    let at = w.flow.tick + crate::math::to_int32(crate::fighter::common::latent_frames(VFX_TO_SPAWN_DELAY));
    if let Some(ar) = w.item_area.as_mut() {
        ar.pending_at = at;
    }
}

/// `spawnPending`: the Delay's link @15: Array_Random(Items), IsValidClass @103, Array_Random again @113 (the class
/// spawned), BeginDeferredActorSpawnFromClass(that class, (0, Y, Z), handling 3, owner self) @370,
/// FinishSpawningActor @610 (`spawn_world_item`), BPI_UpdateHitbox @779, then the re-arm @816-1139:
/// K2_SetTimerDelegate(SpawnRandomItem, Lerp(MaxItemSpawnTime, MinItemSpawnTime, FClamp((GetGameTimeInSeconds -
/// StartTime) / TimeTillFastestSpawn, 0, 1)), no loop), set in the leftover pass. The two draws are the world's
/// stream (Deviation: FMath::RandRange's stream). Deviation: a class not ported for a spawn with no owner is not
/// spawned, with one warning: its draws and the re-arm run as for a spawn.
/// BPI_UpdateHitbox -> SpawnedActorParent @15092: HitBoxOn? of the class default; with it true (Item_DrumStick) the
/// item falls with its hit box on until its first contact with the stage spends it.
/// Unverified: what that box meets is not ported: a box of an item no fighter has picked up hits nothing.
fn spawn_pending(w: &mut World) {
    let Some(ar) = w.item_area.as_ref() else { return };
    let n = ar.items.len();
    if n == 0 {
        return;
    }
    let nr = crate::real(u32::try_from(n).expect("a short list"));
    w.random_int(nr); // @15: every entry of Items is a class (the items part's bind), so IsValidClass passes
    let k = w.random_int(nr);
    let ar = w.item_area.as_ref().expect("the area");
    let cls = ar.items[usize::try_from(crate::math::to_int32(k)).expect("an index")];
    let (py, pz, start) = (ar.pending_y, ar.pending_z, ar.start);
    let cls_s = w.names.str(cls).to_owned();
    let src = items_part(w).and_then(|d| d.actors.get_index_of(&cls_s)).map(|a| DefSrc::Items(idx(a)));
    let ok = match w.registry.actors.get(&cls_s) {
        Some(e) => matches!(e.hooks, ActorHooks::Item(_)) && e.ownerless,
        None => false,
    };
    match (src, ok) {
        (Some(src), true) => {
            let it = spawn_world_item(w, cls, src, 0.0 - py, pz);
            let data = Arc::clone(&w.data);
            if let Some(it) = it
                && !item(w, it).dead
                && truthy(ItemDef::at(&data, src).p, "HitBoxOn?")
            {
                item_mut(w, it).hit_on = true;
            }
        }
        _ => warn_text(
            w,
            &format!("MapRandomActorSpawnArea: item class {cls_s} not ported for a spawn with no owner (skipped)"),
        ),
    }
    let alpha = min(max((game_time(w) - start) / TIME_TILL_FASTEST_SPAWN, 0.0), 1.0);
    let rate = MAX_ITEM_SPAWN_TIME + (alpha * (MIN_ITEM_SPAWN_TIME - MAX_ITEM_SPAWN_TIME));
    // (the port's own Chaos Wheel runs the spawner at a rate of its own)
    let rate = crate::world::wheel::item_rate(w).unwrap_or(rate);
    let at = w.flow.tick + crate::math::to_int32(crate::combat::hit::timer_ticks(Some(rate)));
    if let Some(ar) = w.item_area.as_mut() {
        ar.timer_at = at;
    }
}

/// `itemDef(w, it)?.components`: the item class's component templates.
pub fn item_components(w: &World, it: ItemId) -> Option<&mvs_data::value::Props> {
    item_def_src(w, it).map(|s| ItemDef::at(&w.data, s).components)
}

/// The item is a Fastball, whose pickUp / release / step overrides the world calls.
fn is_fastball(w: &World, it: ItemId) -> bool {
    matches!(item(w, it).ext, crate::actors::item::ItemExt::Fastball(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_data::GameData;

    // The oracle's sandwich (a probe of spawnItem at the fighter's place + (0, 100)): PickupArea radius 250, the
    // pickup sphere at the root.
    #[test]
    fn the_sandwich_pickup_area() {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: skipped", dir.display());
            return;
        };
        let data = GameData::load(&store, &["shaggy"], "trainingroomsmall").expect("the gamedata kind loads");
        let c = data.chars.get_index_of("shaggy").expect("shaggy");
        let a = data.chars[c].fighter.actors.as_ref().and_then(|m| m.get_index_of("Item_Sandwich_C"));
        let src = DefSrc::Char(idx(c), idx(a.expect("the sandwich's defaults")));
        let d = ItemDef::at(&data, src);
        let mut it = Item::new(Name::NONE, ItemClass::ItemSandwich, None, 0.0, 1.0, 0.0, 0.0, false);
        it.x = -800.0;
        it.y = 350.0;
        assert_eq!(it.pickup_radius(d), 250.0);
        assert_eq!((it.pickup_x(d), it.pickup_y(d)), (-800.0, 350.0));
        assert_eq!((it.capsule_radius(), it.capsule_half_height()), (22.0, 44.0));
    }
}
