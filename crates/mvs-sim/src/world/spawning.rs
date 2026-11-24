//! Spawned actors: the owners' spawn, activate and despawn requests, the actor families' passes and their ends
//! (the spawn code and `World.step`'s spawn passes).

use crate::Real;
use crate::actors::projectile::{
    self, Blockers, MakeOpts, ProjectileExt, ReflectState, StepEnd, hitbox_active, instigator, make_projectile, spawner,
};
use crate::actors::world_actor::{WorldActorState, actor_doom, actor_p, set_actor_doom};
use crate::actors::{
    ActorClass, ActorId, ActorRef, ItemId, MortarId, Projectile, ProjectileId, WorldActor, WorldActorClass,
};
use crate::combat::ChargeVals;
use crate::combat::elem;
use crate::events::{Event, Fid, SoundKey};
use crate::fighter::common::{actor_def, actor_is_a, actor_sound_from, warn_text};
use crate::fighter::match_side::{ActivateMode, ActivateRequest, DespawnRequest, Foreign, SpawnRequest};
use crate::fighter::moves::V2;
use crate::fighter::{FighterId, PlatformRef};
use crate::math;
use crate::name;
use crate::names::Name;
use crate::registry::ActorEntry;
use crate::registry::entries::{ActorHooks, ItemHooks, ProjectileHooks, WorldActorHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::world::World;
use crate::world::flow::{ActorList, DelegateCall, DelegateList, PawnList, broadcast_bound};
use crate::world::spawn_spot::{num, obj, truthy};
use crate::world::spawned_platform::BlockKind;
use crate::world::stage_query::StageShapes;
use mvs_collision::Ground;
use mvs_data::value::{Props, Value};

use crate::consts::{SOLID_DETECT, SOLID_ITEM, SOLID_NAV, SOLID_NOPLATFORM};
use crate::fighter::traces::solids::QUERY_UNKNOWN;

/// `Actors::gone_mortars`, ahead of the removal of the dead mortar aims: the entries a projectile's or an item's
/// `hit_actors` still names stay, and each dead mortar aim one names joins them.
fn keep_gone_mortars(w: &mut World) {
    let named = |w: &World, id: MortarId| {
        let r = ActorRef::Mortar(id);
        w.actors.projectiles.iter().any(|(_, p)| p.hit_actors.contains(&r))
            || w.actors.items.iter().any(|(_, i)| i.hit_actors.contains(&r))
    };
    let mut gone = std::mem::take(&mut w.actors.gone_mortars);
    gone.retain(|(id, _)| named(w, *id));
    for (id, m) in w.actors.mortars.iter() {
        if m.dead && named(w, id) {
            gone.push((id, m.clone()));
        }
    }
    w.actors.gone_mortars = gone;
}

/// `Actors::gone_actors`, ahead of the removal of the dead world actors: the entries a projectile's or an item's
/// `hit_actors` still names stay, and each dead world actor one names joins them.
fn keep_gone_actors(w: &mut World) {
    let named = |w: &World, id: ActorId| {
        let r = ActorRef::Actor(id);
        w.actors.projectiles.iter().any(|(_, p)| p.hit_actors.contains(&r))
            || w.actors.items.iter().any(|(_, i)| i.hit_actors.contains(&r))
            || crate::chars::tom_and_jerry::names_actor(w, id)
    };
    let mut gone = std::mem::take(&mut w.actors.gone_actors);
    gone.retain(|(id, _)| named(w, *id));
    let dead: Vec<ActorId> =
        w.actors.actors.iter().filter(|(id, a)| a.dead && named(w, *id)).map(|(id, _)| id).collect();
    for id in dead {
        if let Some(a) = w.actors.actors.get(id) {
            gone.push((id, a.clone()));
        }
    }
    w.actors.gone_actors = gone;
}

/// `Actors::gone_items`, ahead of the removal of the dead items: the entries a Finn aimer or a set (a projectile's or
/// an item's `hitActors`, an ice pellet's `touching`, a Bugs tunnel's `projInside`, a Superman laser's `once` / `over`,
/// a fighter's reach list) still names stay, and each dead item one names joins them (the kept so the per-tick trace's
/// record writes it whole).
fn keep_gone_items(w: &mut World) {
    let named = |w: &World, id: ItemId| {
        let r = ActorRef::Item(id);
        crate::chars::finn::aimer::names_item(w, id)
            || w.actors.projectiles.iter().any(|(_, p)| {
                p.hit_actors.contains(&r)
                    || matches!(&p.ext, crate::actors::ProjectileExt::IcePellet(s) if s.touching.contains(&r))
            })
            || w.actors.items.iter().any(|(_, i)| i.hit_actors.contains(&r))
            || w.actors.actors.iter().any(
                |(_, a)| matches!(&a.state, crate::actors::WorldActorState::Tunnel(t) if t.proj_inside.contains(&r)),
            )
            || w.actors.actors.iter().any(|(_, a)| {
                matches!(&a.state, crate::actors::WorldActorState::LaserDamage(d) if d.once.contains(&r) || d.over.contains(&r))
            })
            // a fighter's Comp_ItemManager list (ReachableItems): a destroy with BlockPickUp set leaves the item there
            || w.actors.reach.iter().any(|x| x.items.contains(&r))
            || w.fighters.iter().any(|(_, f)| f.items.item_reach.as_ref().is_some_and(|x| x.items.contains(&r)))
    };
    let mut gone = std::mem::take(&mut w.actors.gone_items);
    gone.retain(|(id, _)| named(w, *id));
    let dead: Vec<ItemId> = w.actors.items.iter().filter(|(id, i)| i.dead && named(w, *id)).map(|(id, _)| id).collect();
    for id in dead {
        if let Some(i) = w.actors.items.get(id) {
            gone.push((id, i.clone()));
        }
    }
    w.actors.gone_items = gone;
}

/// `Actors::gone_boxes`, ahead of the removal of the dead projectiles: the entries a fighter's base (`base`, `walkBase`,
/// `sweptBase`) still names stay, and each dead projectile with boxes one names joins them.
fn keep_gone_boxes(w: &mut World) {
    let named = |w: &World, id: ProjectileId| {
        w.fighters.iter().any(|(_, f)| {
            [f.platforms.base, f.platforms.walk_base, f.platforms.swept_base]
                .iter()
                .any(|b| matches!(b, Some(PlatformRef::Spawned(ActorRef::Projectile(p), _)) if *p == id))
        })
    };
    let mut gone = std::mem::take(&mut w.actors.gone_boxes);
    gone.retain(|(id, ..)| named(w, *id));
    let dead: Vec<ProjectileId> = w
        .actors
        .projectiles
        .iter()
        .filter(|(id, p)| p.dead && p.solids.is_some() && named(w, *id))
        .map(|(id, _)| id)
        .collect();
    for id in dead {
        if let Some(p) = w.actors.projectiles.get(id) {
            gone.push((id, p.seq, p.solids.clone().unwrap_or_default()));
        }
    }
    w.actors.gone_boxes = gone;
}

/// SpawnedActorParent_C: the parent whose BeginPlay grants the instigator's buffs and limits the count.
const PARENT: Name = name!("SpawnedActorParent_C");

// ---- `World.step`'s passes

/// `World.step`: Pawn_C019's `teamGrenades` and `hasMarker`, ahead of the frame.
/// c019CookGrenadeOutProjectileCondition: the C019_CookGrenade actors in the SpawnedActors of the fighter's
/// Comp_Spawner, then of its allies' (spawn order), as they are before this frame's projectile movement.
pub fn morty_lists(w: &mut World) {
    if w.players.iter().all(|&f| crate::chars::morty::morty_of(w, f).is_none()) {
        return;
    }
    let any_grenade = w.actors.projectiles.iter().any(|(_, g)| g.grenade);
    for f in w.players.clone() {
        if crate::chars::morty::morty_of(w, f).is_none() {
            continue;
        }
        let has_marker = w.actors.markers.iter().any(|(_, m)| m.owner == f && !m.dead);
        if !any_grenade {
            crate::chars::morty::set_lists(w, f, &[], has_marker);
            continue;
        }
        let team = w.fighter(f).team.team;
        let live = |g: &Projectile| g.owner.is_some() && g.grenade && !g.dead;
        let mut own: Vec<ProjectileId> =
            w.actors.projectiles.iter().filter(|(_, g)| live(g) && g.owner == Some(f)).map(|(id, _)| id).collect();
        own.extend(
            w.actors
                .projectiles
                .iter()
                .filter(|(_, g)| {
                    live(g) && g.owner != Some(f) && g.owner.is_some_and(|o| w.fighter(o).team.team == team)
                })
                .map(|(id, _)| id),
        );
        crate::chars::morty::set_lists(w, f, &own, has_marker);
    }
}

/// `shipTick` of every ship ahead of the fighters. A ship ticks in the frame it spawns
/// (`lists_pass`, TG_NewlySpawned) and from then on ahead of the fighters.
pub fn ship_ticks(w: &mut World) {
    let mut i = 0;
    while let Some(sh) = w.actors.ships.nth(i) {
        i += 1;
        crate::chars::morty::ship_tick(w, sh);
    }
}

/// `World.step`'s `platforms`: `platformsOfFrame` when actors, projectiles, stage
/// solids or run-time solids are there, else the ships'. Written to `scratch.platforms`, which each fighter of the
/// physics passes takes (`tick::platforms_of_frame`).
pub fn platforms_of_frame(w: &mut World) {
    let built = !w.actors.actors.is_empty()
        || !w.actors.projectiles.is_empty()
        || w.stage_run.as_ref().is_some_and(|r| !r.solids.is_empty())
        || w.runtime_solids.is_some();
    let mut out = std::mem::take(&mut w.scratch.platforms);
    out.clear();
    out.extend(w.actors.ships.ids().map(|s| PlatformRef::Spawned(ActorRef::Ship(s), 0)));
    if built {
        frame_platforms(w, &mut out);
    }
    w.scratch.platforms = out;
    w.scratch.platforms_built = built;
}

/// `platformsOfFrame`'s list past the ships: the stage's boxes, the live actors' solids, the projectiles' boxes
/// (placed where each stands), the run-time Floor boxes, the stage solids' polygons.
fn frame_platforms(w: &mut World, out: &mut Vec<PlatformRef>) {
    let n = w.stage_run.as_ref().map_or(0, |r| r.boxes.len());
    out.extend((0..n).map(|i| PlatformRef::Solid(u32::try_from(i).expect("a box index"))));
    // The live actors' `solids`.
    crate::world::spawned_platform::actor_platforms(w, out);
    // The projectiles' boxes, placed here where each projectile stands.
    for i in 0..w.actors.projectiles.len() {
        let id = w.actors.projectiles.nth(i).expect("an index under the length");
        let pr = w.actors.projectiles.get(id).expect("a live projectile id");
        let k = match &pr.solids {
            Some(s) if !pr.dead => s.len(),
            _ => continue,
        };
        for b in 0..k {
            crate::world::spawned_platform::projectile_box_follow(w, id, b);
            out.push(PlatformRef::Spawned(ActorRef::Projectile(id), u32::try_from(b).expect("a box index")));
        }
    }
    // the run-time Floor boxes, after the stage's (a pass-through platform is in the stage's platforms)
    for s in w.runtime_solids.iter().flatten() {
        if s.solid == crate::world::stage::RuntimeBox::Floor && !s.destroyed {
            out.push(PlatformRef::Runtime(s.spec));
        }
    }
    // `stageSolids(w).fighters`: the stage solids' polygons, none without stage solids.
    if w.stage_run.as_ref().is_some_and(|r| !r.solids.is_empty()) {
        let s = crate::fighter::traces::stage_solids(w);
        out.extend(s.fighters.iter().map(|p| PlatformRef::Poly(p.solid)));
    }
}

/// `World.step`'s request pass of one owner: `itemUses` (AnimNotify_ItemUse's ActivateItem calls of f's tick, ahead
/// of its spawns), the spawn requests, the activate requests, the despawn requests, `applyNearbyBuffs`,
/// `applyAllyBuffs`, `dealHoldHits`, `consumeRequests` (a hold's DestroyAttachedConsumableActors), `aimerTick`.
/// Spawned actors tick in TG_NewlySpawned, so a projectile moves in the frame it was spawned.
pub fn serve_requests(w: &mut World, f: FighterId) {
    crate::world::items::item_uses(w, Some(f));
    // (a request a spawn's BeginPlay adds joins the list, as a `for of` over it does)
    let mut i = 0;
    while let Some(r) = w.fighter(f).requests.spawn_requests.get(i).cloned() {
        i += 1;
        serve_spawn(w, f, &r);
    }
    w.fighter_mut(f).requests.spawn_requests.clear();
    serve_activates(w, f);
    serve_despawns(w, f);
    if !w.fighter(f).requests.nearby_buff_requests.is_empty() {
        apply_nearby_buffs(w, f);
    }
    if !w.fighter(f).requests.ally_buff_requests.is_empty() {
        apply_ally_buffs(w, f);
    }
    if !w.fighter(f).holds.hold_hits.is_empty() {
        crate::combat::holds::deal_hold_hits(w, f);
    }
    crate::combat::holds::consume_requests(w);
    crate::fighter::match_side::aimer_tick(w, f);
}

/// The registry entry of a spawned actor class.
fn entry(w: &World, cls: Name) -> Option<&'static ActorEntry> {
    let reg: &'static crate::registry::Registry = w.registry;
    reg.actors.get(w.names.str(cls))
}

/// `ACTORS.get(cls)?.projectile`.
fn proj_hooks(w: &World, cls: Name) -> Option<ProjectileHooks> {
    match entry(w, cls)?.hooks {
        ActorHooks::Projectile(h) => Some(h),
        _ => None,
    }
}

/// `ACTORS.get(cls)?.item`.
fn item_hooks(w: &World, cls: Name) -> Option<ItemHooks> {
    match entry(w, cls)?.hooks {
        ActorHooks::Item(h) => Some(h),
        _ => None,
    }
}

/// The world actor part of `ACTORS.get(cls)`.
fn world_hooks(w: &World, cls: Name) -> Option<WorldActorHooks> {
    match entry(w, cls)?.hooks {
        ActorHooks::World(h) => Some(h),
        _ => None,
    }
}

/// An entry with an empty `projectile` part and no other part than `parents` (`Object.keys(ck.projectile).length ===
/// 0 && Object.keys(ck).every((k) => k === 'projectile' || k === 'parents')`): a ParentProjectilePhysics child with
/// no graph of its own.
fn plain_projectile(e: &ActorEntry) -> bool {
    let ActorHooks::Projectile(h) = e.hooks else { return false };
    let empty = h.make.is_none()
        && h.construct.is_none()
        && h.bodies.is_none()
        && h.make_world.is_none()
        && h.begin.is_none()
        && h.receive_hit.is_none()
        && h.damage_box.is_none()
        && h.step.is_none()
        && h.physics.is_none()
        && !h.own_tick
        && h.bounce_impact.is_none()
        && h.moved.is_none()
        && h.split.is_none()
        && h.overlap.is_none()
        && h.non_item_pickup.is_none()
        && !h.pickup_label
        && h.hitbox_begin_overlap.is_none()
        && h.reflecting_projectiles.is_none()
        && h.hit_enemy.is_none()
        && h.can_mark.is_none()
        && h.hit_by.is_none()
        && h.move_command_completed.is_none()
        && h.hit_non_player.is_none()
        && h.hit_anything.is_none()
        && h.hit_ally.is_none()
        && h.blocked.is_none()
        && h.activate.is_none()
        && h.any_damage.is_none()
        && h.destroyed.is_none()
        && h.spawns_at_death.is_none()
        && h.end.is_none();
    // Unverified: the `reflect` part's hooks are taken as unset (ReflectHooks is not compared).
    empty
        && !e.foreign_owner
        && !e.ownerless
        && !e.keep_at_lab_reset
        && !e.spawned_meet_bodies
        && !e.calls_parent_hit_anything
        && e.elemental.is_none()
        && e.carry.is_none()
        && e.mesh.is_none()
        && e.hazard.is_none()
        && e.follower.is_none()
}

/// `r.p`: the notify's properties of a spawn request, with a ProjectileAimer's values over them
/// (activateAimer: AimOn false, InitialMovementDirection (0, y, z), InitialSpeed, MaxSpeed).
pub(crate) fn request_props(w: &World, f: FighterId, r: &SpawnRequest) -> Props {
    if let Some(p) = &r.props {
        return (**p).clone();
    }
    let data = w.data.clone();
    let mut p = crate::fighter::anim::notify_data(w, &data, f, r.n).p.clone();
    if let Some(a) = r.aim {
        p.insert("AimOn".into(), Value::Bool(false));
        let mut d = Props::new();
        d.insert("X".into(), Value::Num(0.0));
        d.insert("Y".into(), Value::Num(f64::from(a.y)));
        d.insert("Z".into(), Value::Num(f64::from(a.z)));
        p.insert("InitialMovementDirection".into(), Value::Obj(d));
        p.insert("InitialSpeed".into(), Value::Num(f64::from(a.initial_speed)));
        p.insert("MaxSpeed".into(), Value::Num(f64::from(a.max_speed)));
    }
    p
}

/// `x.id = w.actorSeq++`.
fn next_seq(w: &mut World) -> i32 {
    let s = w.actors.actor_seq;
    w.actors.actor_seq += 1;
    s
}

/// The projectile `pr`.
fn pm(w: &mut World, pr: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(pr).expect("a live projectile id")
}

/// The projectile `pr`, read.
fn pm_ref(w: &World, pr: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(pr).expect("a live projectile id")
}

/// `x.reflect?.unlisted`: a reflected actor left its owner's list (RemoveActorListServer).
fn unlisted(r: &Option<ReflectState>) -> bool {
    r.as_ref().is_some_and(|r| r.unlisted)
}

/// One spawn request of the owner `f`.
fn serve_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) {
    let ap = actor_def(w, ActorRef::Fighter(f), r.cls).map(|a| a.p.clone());
    let np = request_props(w, f, r);
    let ent = entry(w, r.cls);
    // A Spawner of a Disguise's copied montage (r.foreign: the class is the victim's, the owner is the wearer), or
    // Taz's burp of an actor he ate (the class is another kit's, the owner is Taz). The build has no rule here: the
    // notify runs the wearer's conditions and Comp_Spawner (AnimNotify_ConditionalSpawner @219, AnimNotify_Spawner
    // @620) for any class. The port's own: a class's code is written against its own pawn as the owner, so it
    // spawns only where that code was read against another one: the entry's `foreign_owner` mark, or a
    // ParentProjectilePhysics child with no graph of its own (an empty projectile entry). Every other class and an
    // attached actor (the attach is to the owner's own mesh: not read for the second one) are refused, by name.
    // `r.copy` is the other thing: the socket was read on the second mesh (Comp_Spawner @651-720).
    if ap.is_some() && r.foreign != Foreign::None {
        let plain = ent.is_some_and(plain_projectile);
        if !(ent.is_some_and(|e| e.foreign_owner) || plain) || truthy(&np, "AttachToActor") {
            let what =
                if r.foreign == Foreign::Burp { "burp of an eaten actor" } else { "spawn from a disguise montage" };
            let msg = format!("{what} refused ({})", w.names.str(r.cls));
            warn_text(w, &msg);
            return;
        }
    }
    let Some(ap) = ap else {
        let msg = format!("spawned actor not implemented ({})", w.names.str(r.cls));
        warn_text(w, &msg);
        return;
    };
    crate::fighter::control::actor_spawn_ability(w, f, &ap);
    // Each actor's SpawnedActorParent BeginPlay copies the owner's charge values of the notify (receiveCharge),
    // ahead of its first hit test. C019_Ship, C019AimPunch and ProjectileAccelerator deal no hit: the port keeps no
    // values on them.
    let fid = w.fighter(f).id;
    if r.cls == name!("C019Marker_C") {
        let mut m = crate::chars::morty::new_marker(w, f, r);
        m.seq = next_seq(w);
        let id = w.actors.markers.insert(m);
        elem::receive_charge(w, ActorRef::Marker(id), r.charge);
        // SpawnedActorParent BeginPlay's GrantBuffsFromInstigator @9233.
        elem::grant_actor_buffs(w, ActorRef::Marker(id));
        return;
    }
    if r.cls == name!("C019_Ship_C") {
        let mut sh = crate::chars::morty::new_ship(w, f, r);
        sh.seq = next_seq(w);
        let (x, y) = (sh.x, sh.y);
        let id = w.actors.ships.insert(sh);
        // its Comp_EdgeTrace (SpawnedPlatformParent's)
        crate::fighter::traces::edge_begin_play(w, ActorRef::Ship(id));
        // C019_Ship ReceiveBeginPlay.
        let ev = name!("sfx_cha_C019_portal_cast");
        w.events.push(Event::Sound { fighter: fid, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
        return;
    }
    if r.cls == name!("C019_MortarAim_C") {
        let mut mo = crate::chars::morty::new_mortar(w, f, r);
        mo.seq = next_seq(w);
        let id = w.actors.mortars.insert(mo);
        elem::receive_charge(w, ActorRef::Mortar(id), r.charge);
        // SpawnedActorParent BeginPlay's GrantBuffsFromInstigator @9233.
        elem::grant_actor_buffs(w, ActorRef::Mortar(id));
        return;
    }
    if let Some(e) = ent
        && let ActorHooks::World(h) = e.hooks
        && let Some(spawn) = h.spawn
    {
        // The parent's BeginPlay, the child's, then AfterSpawnedActor (`comp_spawn_actor`).
        let ActorClass::World(class) = e.class else { unreachable!("a world actor entry of another family") };
        let (state, dead) = spawn(w, f, r);
        let x = new_world_actor(w, r.cls, class, f, dead, state);
        let id = w.actors.actors.insert(x);
        if !dead && let Some(s) = h.spawned {
            s(w, id);
        }
        comp_spawn_actor(w, id, r.charge, f);
        return;
    }
    if let Some(ih) = item_hooks(w, r.cls) {
        // A registered ItemActorParent child. CompSpawnActor with socket None: the attach actor's location - (0, 0,
        // 100) plus the notify's translation (as Projectile's constructor), the direction mirrored by the owner's
        // facing.
        if np.get("Socket").and_then(Value::as_str).is_some_and(|s| s != "None" && !s.is_empty()) {
            let msg = format!("item spawn at a socket not implemented ({})", w.names.str(r.cls));
            warn_text(w, &msg);
        }
        let tr = obj(&np, "Transforms");
        let t = tr.and_then(|t| obj(t, "Translation"));
        let d = obj(&np, "InitialMovementDirection");
        let sp = num(&np, "InitialSpeed").unwrap_or(0.0);
        let facing = crate::fighter::facing(w, f);
        let ix = w.fighter(f).body.x + t.and_then(|t| num(t, "Y")).unwrap_or(0.0) * facing;
        let iy = crate::fighter::actor_y(w, f) - 100.0 + t.and_then(|t| num(t, "Z")).unwrap_or(0.0);
        // The spawn transform's Scale3D is the root's world scale (Item.scale); a class with a make takes it there.
        let scale = tr.and_then(|t| obj(t, "Scale3D")).and_then(|s| num(s, "X")).unwrap_or(1.0);
        if ih.make.is_none() && scale != 1.0 {
            let msg = format!("item spawn scale not implemented ({})", w.names.str(r.cls));
            warn_text(w, &msg);
        }
        // A class with its ProjectileMovement on makes its own Item (ItemHooks::make).
        let it = match ih.make {
            Some(make) => {
                let made = make(w, f, r, ix, iy);
                Some(crate::world::items::add_item(w, made, Some(f), r.charge, true))
            }
            None => {
                let dy = d.and_then(|d| num(d, "Y")).unwrap_or(0.0);
                let dz = d.and_then(|d| num(d, "Z")).unwrap_or(0.0);
                let (vx, vy) = (dy * sp * facing, dz * sp);
                crate::world::items::spawn_item(w, r.cls, f, ix, iy, vx, vy, false, r.charge)
            }
        };
        // GrantBuffsFromInstigator, AfterSpawnedActor
        item_spawned(w, it, f);
        return;
    }
    if r.cls == name!("C019AimPunch_C") {
        let mut ap = crate::chars::morty::new_aim_punch(w, f, r);
        ap.seq = next_seq(w);
        w.actors.aim_punches.insert(ap);
        return;
    }
    if r.cls == name!("ProjectileAccelerator_C") {
        let mut ac = crate::chars::morty::new_accelerator(w, f, r);
        let seq = next_seq(w);
        ac.seq = seq;
        w.actors.accelerators.insert(ac);
        // ReceiveBeginPlay.
        let key = SoundKey { tag: name!("accel"), n: seq };
        w.events.push(Event::SoundStart { fighter: fid, ev: name!("sfx_cha_C019_portal_loop"), key });
        return;
    }
    let class = match ent.map(|e| e.class) {
        Some(ActorClass::Projectile(c)) => c,
        _ if r.cls == name!("C017_CannonShot_C") => {
            // (Iron Giant's code makes its shots itself, `new Projectile`: no ported notify asks for one)
            crate::unported!("spawning: a spawn request of C017_CannonShot_C (a plain Projectile)", r.cls)
        }
        _ => {
            let msg = format!("spawned actor not implemented ({})", w.names.str(r.cls));
            warn_text(w, &msg);
            return;
        }
    };
    let hooks = proj_hooks(w, r.cls);
    // A registered projectile class (a ParentProjectilePhysics child, or a SpawnedActorParent child with a
    // ProjectileMovement such as Superman's ice pellet) may bring its own state beside the constructor's
    // (ProjectileHooks::make).
    // The class's own constructor options (ProjectileHooks::make_opts, a subclass's `make`), else the base ones.
    let mut opts = hooks.and_then(|h| h.make_opts).map_or_else(MakeOpts::default, |mo| mo(w, f, r));
    opts.by_fighter = r.copy.is_some();
    let mut p = make_projectile(w, r.cls, class, Some(f), r.attack_id, &np, r.copy.flatten(), opts);
    if let Some(i) = r.instigator
        && i != f
    {
        p.inst_override = crate::actors::projectile::OwnerOverride::Fighter(i);
    }
    if let Some(make) = hooks.and_then(|h| h.make) {
        p.ext = make(w, f, r);
    }
    if let Some(construct) = hooks.and_then(|h| h.construct) {
        construct(w, &mut p);
    }
    // PostActorConstruction's FindTeleportSpot, ahead of BeginPlay and of the list
    if !p.attached {
        let own = ActorRef::Fighter(p.owner.or(p.lookup_fighter).expect("a projectile its owner's spawner made"));
        if let Some((x, y)) = push_out_spot(w, own, r.cls, p.spawn_scale.unwrap_or(p.scale), p.x, p.y) {
            (p.x, p.prev_x, p.y, p.prev_y) = (x, x, y, y);
        }
    }
    let pr = w.actors.projectiles.insert(p);
    pm(w, pr).seq = next_seq(w);
    elem::receive_charge(w, ActorRef::Projectile(pr), r.charge);
    w.fighter_mut(f).items.spawned_actors.push(ActorRef::Projectile(pr));
    // the passive buffs, GrantBuffsFromInstigator, the blast-box loop, NumberOfActorsAllowed
    let doomed = parent_begin_play(w, pr);
    if let Some(begin) = hooks.and_then(|h| h.begin) {
        begin(w, pr);
    }
    if end_doomed(w, pr, doomed) {
        return;
    }
    spawn_begins(w, pr);
    if pm_ref(w, pr).dead {
        return;
    }
    after_spawned_actor(w, f, ActorRef::Projectile(pr));
    spawn_overlap(w, pr);
}

/// A world actor around the class state its kind made (the `SimActor` fields at the class's construction):
/// numbered by `add_actor`, no charge, no buffs.
pub(crate) fn new_world_actor(
    w: &World,
    cls: Name,
    class: WorldActorClass,
    owner: FighterId,
    dead: bool,
    state: WorldActorState,
) -> WorldActor {
    WorldActor {
        cls,
        class,
        seq: -1,
        dead,
        owner,
        owner_id: w.fighter(owner).id,
        charge: None,
        elem: None,
        lifetime_pause: None,
        size_scale: None,
        plat: None,
        dodge: None,
        doom: None,
        mobility: None,
        stationary_count: None,
        state,
    }
}

/// The owner's activate requests: an UpdateCharge activation (`c.charge` set): Comp_Spawner SetChargeVariables @3727
/// on each actor it picked, then its BPI_ActivateObject @3610.
fn serve_activates(w: &mut World, f: FighterId) {
    // (each loop walks the live list by index, as a `for of` does: a request an activation adds joins the loops)
    let req = |w: &World, i: usize| w.fighter(f).requests.activate_requests.get(i).cloned();
    let mut i = 0;
    while let Some(c) = req(w, i) {
        i += 1;
        for x in pick_spawned(w, ListKind::Projectiles, f, &c) {
            let ActorRef::Projectile(pr) = x else { continue };
            if c.charge.is_some() {
                elem::set_charge_variables(w, x, c.charge);
            }
            // ActivateActors @3285: BPI_SetActivationConditions per row (SpawnedActorParent @4520 Array_Set), ahead
            // of ActivateObject @3610 (ActorKind.setCondition).
            // The electric chains', the Garnet marker's and the fishing lure's setCondition are direct calls
            // (`chars::black_adam::chain::set_condition`, `chars::garnet::marker::set_condition`,
            // `chars::tom_and_jerry::fishing::set_condition`).
            // Unverified: ProjectileHooks has no setCondition: another projectile
            // class with condition rows is warned.
            let is_chain = w.actors.projectiles.get(pr).is_some_and(|p| matches!(p.ext, ProjectileExt::Chain(_)));
            let is_marker =
                w.actors.projectiles.get(pr).is_some_and(|p| matches!(p.ext, ProjectileExt::GarnetMarker2(_)));
            let is_meeseeks =
                w.actors.projectiles.get(pr).is_some_and(|p| matches!(p.ext, ProjectileExt::MeeseeksSummon(_)));
            let is_lure = w.actors.projectiles.get(pr).is_some_and(|p| matches!(p.ext, ProjectileExt::Lure(_)));
            if is_lure {
                for &(n, v) in &c.conds {
                    crate::chars::tom_and_jerry::fishing::set_condition(w, pr, n, v);
                }
            } else if is_meeseeks {
                for &(n, v) in &c.conds {
                    crate::chars::rick::meeseeks::set_condition(w, pr, n, v);
                }
            } else if is_chain {
                for &(n, v) in &c.conds {
                    crate::chars::black_adam::chain::set_condition(w, pr, n, v);
                }
            } else if is_marker {
                for &(n, v) in &c.conds {
                    crate::chars::garnet::marker::set_condition(w, pr, n, v);
                }
            } else if !c.conds.is_empty() && proj_hooks(w, c.cls).is_some() {
                let msg = format!("setCondition of a projectile class not ported ({})", w.names.str(c.cls));
                warn_text(w, &msg);
            }
            activate_actor(w, pr);
        }
    }
    let mut i = 0;
    while let Some(c) = req(w, i) {
        i += 1;
        for x in pick_spawned(w, ListKind::Markers, f, &c) {
            let ActorRef::Marker(m) = x else { continue };
            if c.charge.is_some() {
                elem::set_charge_variables(w, x, c.charge);
            }
            crate::chars::morty::activate_marker(w, m);
        }
    }
    let mut i = 0;
    while let Some(c) = req(w, i) {
        i += 1;
        for x in pick_spawned(w, ListKind::Mortars, f, &c) {
            let ActorRef::Mortar(mo) = x else { continue };
            if c.charge.is_some() {
                elem::set_charge_variables(w, x, c.charge);
            }
            crate::chars::morty::mortar_activate(w, mo);
        }
    }
    let mut i = 0;
    while let Some(c) = req(w, i) {
        i += 1;
        for x in pick_spawned(w, ListKind::Actors, f, &c) {
            let ActorRef::Actor(id) = x else { continue };
            if c.charge.is_some() {
                elem::set_charge_variables(w, x, c.charge);
            }
            let h = world_hooks(w, c.cls);
            if let Some(set) = h.and_then(|h| h.set_condition) {
                for &(n, v) in &c.conds {
                    set(w, id, n, v);
                }
            }
            if let Some(act) = h.and_then(|h| h.activate) {
                act(w, id);
            }
        }
    }
    let mut i = 0;
    while let Some(c) = req(w, i) {
        i += 1;
        for x in pick_spawned(w, ListKind::AimPunches, f, &c) {
            let ActorRef::AimPunch(ap) = x else { continue };
            // C019AimPunch ActivateObject: the owner's K2_SetActorLocation (no sweep).
            let boxes = crate::world::stage_query::static_shapes(w, SOLID_NOPLATFORM, name!("C019AimPunch trace 1"));
            if let Some(to) = crate::chars::morty::aim_punch_activate(w, ap, boxes.view()) {
                crate::fighter::moves::teleport_move(w, f, to.x, to.y);
            }
        }
    }
    w.fighter_mut(f).requests.activate_requests.clear();
}

/// The owner's despawn requests. A reflected actor left its owner's list (RemoveActorListServer),
/// in every loop on SpawnedActors.
pub(crate) fn serve_despawns(w: &mut World, f: FighterId) {
    // (each loop walks the live request list and actor lists by index, as a `for of` does)
    let all = |w: &World, i: usize| match w.fighter(f).requests.despawn_requests.get(i) {
        None => None,
        Some(&DespawnRequest::All(c)) => Some(Some(c)),
        Some(DespawnRequest::Ends { .. }) => Some(None),
    };
    let mut k = 0;
    while let Some(c) = all(w, k) {
        k += 1;
        let Some(c) = c else { continue };
        let mut i = 0;
        while let Some(pr) = w.actors.projectiles.nth(i) {
            i += 1;
            let p = pm_ref(w, pr);
            if spawner(p) == Some(f) && is_a(w, p.cls, c) && !p.dead && !unlisted(&p.reflect) {
                destroy_projectile(w, pr, false);
            }
        }
    }
    let mut k = 0;
    while let Some(c) = all(w, k) {
        k += 1;
        let Some(c) = c else { continue };
        let mut i = 0;
        while let Some(id) = w.actors.mortars.nth(i) {
            i += 1;
            let m = w.actors.mortars.get(id).expect("a live mortar id");
            if m.owner == f && is_a(w, m.cls, c) {
                w.actors.mortars.get_mut(id).expect("a live mortar id").dead = true;
            }
        }
    }
    let mut k = 0;
    while let Some(c) = all(w, k) {
        k += 1;
        let Some(c) = c else { continue };
        let mut i = 0;
        while let Some(id) = w.actors.actors.nth(i) {
            i += 1;
            let x = w.actors.actors.get(id).expect("a live actor id");
            if x.owner == f && is_a(w, x.cls, c) {
                w.actors.actors.get_mut(id).expect("a live actor id").dead = true;
            }
        }
    }
    let mut k = 0;
    while let Some(c) = all(w, k) {
        k += 1;
        let Some(c) = c else { continue };
        let mut i = 0;
        while let Some(id) = w.actors.aim_punches.nth(i) {
            i += 1;
            let a = w.actors.aim_punches.get(id).expect("a live aim punch id");
            if a.owner == f && is_a(w, a.cls, c) {
                w.actors.aim_punches.get_mut(id).expect("a live aim punch id").dead = true;
            }
        }
    }
    // An item is in its spawner's SpawnedActors (its owner turns to whoever picked it up last).
    let mut k = 0;
    while let Some(c) = all(w, k) {
        k += 1;
        let Some(c) = c else { continue };
        let mut i = 0;
        while let Some(it) = w.actors.items.nth(i) {
            i += 1;
            let x = w.actors.items.get(it).expect("a live item id");
            if x.spawner == Some(f) && is_a(w, x.cls, c) && !x.dead && !unlisted(&x.reflect) {
                crate::world::items::destroy_item(w, it);
            }
        }
    }
    let mut k = 0;
    while let Some(c) = w.fighter(f).requests.despawn_requests.get(k) {
        k += 1;
        if let &DespawnRequest::Ends { cls, last, first } = c {
            if last {
                despawn_one(w, f, cls, true);
            }
            if first {
                despawn_one(w, f, cls, false);
            }
        }
    }
    w.fighter_mut(f).requests.despawn_requests.clear();
}

/// `applyNearbyBuffs`: AnimNotify_ApplyBuffNearbyPlayer Received_NotifyBegin: every player's pawn (GetAllPlayers)
/// that is not the owner, with BuffAlly on the owner's TeamIndex, within Distance of the owner (actor locations)
/// and, with MustFacePlayer, on the side the owner faces while facing the other way, gets SpawnBuffActor(Buff, the
/// pawn, the owner, BuffStackCount, BuffDuration). BuffOwner (the buff on the owner too) and BuffEnemies (the other
/// pawns as well) are off on every C019 notify.
/// Deviation: a pawn waiting for its respawn is left out (GetAllPlayers still lists it).
fn apply_nearby_buffs(w: &mut World, f: FighterId) {
    let data = w.data.clone();
    let mut i = 0;
    while let Some(&n) = w.fighter(f).requests.nearby_buff_requests.get(i) {
        i += 1;
        let p = &crate::fighter::anim::notify_data(w, &data, f, n).p;
        if truthy(p, "BuffOwner") || truthy(p, "BuffEnemies") {
            warn_text(w, "ApplyBuffNearbyPlayer: BuffOwner / BuffEnemies not ported");
        }
        let os = crate::world::pawns::match_fighters_list(w);
        for &o in &os {
            let ft = w.fighter(f).team.team;
            if o == f || w.fighter(o).life.dead != 0.0 || w.fighter(o).team.team != ft || !truthy(p, "BuffAlly") {
                continue;
            }
            let (ox, fx) = (w.fighter(o).body.x, w.fighter(f).body.x);
            let dist = math::ue::size2d([ox - fx, crate::fighter::actor_y(w, o) - crate::fighter::actor_y(w, f)]);
            if dist > num(p, "Distance").unwrap_or(100.0) {
                continue;
            }
            let (of, ff) = (crate::fighter::facing(w, o), crate::fighter::facing(w, f));
            if truthy(p, "MustFacePlayer") && !(of != ff && (if ff < 0.0 { ox <= fx } else { ox >= fx })) {
                continue;
            }
            // Unverified: a notify without a Buff is assumed to spawn nothing.
            let Some(buff) = p.get("Buff").and_then(Value::as_str).filter(|s| !s.is_empty()) else { continue };
            let buff = w.lit(buff);
            let dur = num(p, "BuffDuration").unwrap_or(0.0);
            let stacks = math::to_int32(num(p, "BuffStackCount").unwrap_or(0.0));
            crate::combat::buffs::spawn_buff(w, o, buff, dur, Some(stacks), Some(f), true, None);
        }
        crate::world::pawns::put_fighters(w, os);
    }
    w.fighter_mut(f).requests.nearby_buff_requests.clear();
}

/// `applyAllyBuffs`: AnimNotify_ApplyBuff Received_NotifyBegin without BuffSelf and with BuffAllAllies
/// (@999-2015): every BP_FighterCharacter (GetAllActorsOfClass) that is not the owner and has the owner's TeamIndex
/// gets SpawnBuffActor(Buff, that fighter, the owner, Stacks, BuffDuration, BreakIfBlocked). Shaggy's RageBuilder
/// recoveries (ground and air): Buff_ScoobySnack, 5 stacks, 5 s.
/// Unverified: fighters in the sim's order (GetAllActorsOfClass has the level's actor order); a fighter waiting for
/// its respawn is left out; applied after the owner's tick, as the nearby buffs; with DestroyBuffsOnNotifyEnd (no
/// notify of the data) the ally's actor would be filed under the owner's notify, which is not ported.
// Unverified: every ally gets it, so in the port's own big matches a team-wide grant (this one, Taz's HealAllies, Finn's
// ArmorBuns, the ice perk's gray health, the dodge perk's cooldowns, the passive buffs' BuffAllies rows) is worth
// more to a team of three or four than the beta's 2v2 ever saw; kept as the build's rule.
fn apply_ally_buffs(w: &mut World, f: FighterId) {
    let data = w.data.clone();
    let mut i = 0;
    while let Some(&n) = w.fighter(f).requests.ally_buff_requests.get(i) {
        i += 1;
        let p = &crate::fighter::anim::notify_data(w, &data, f, n).p;
        if truthy(p, "DestroyBuffsOnNotifyEnd") {
            warn_text(w, "ApplyBuff: BuffAllAllies with DestroyBuffsOnNotifyEnd not ported");
        }
        let all = crate::world::pawns::all_list(w);
        for &o in &all {
            if o == f || w.fighter(o).life.dead != 0.0 || w.fighter(o).team.team != w.fighter(f).team.team {
                continue;
            }
            // Unverified: a notify without a Buff is assumed to spawn nothing.
            let Some(buff) = p.get("Buff").and_then(Value::as_str).filter(|s| !s.is_empty()) else { continue };
            let buff = w.lit(buff);
            let dur = num(p, "BuffDuration").unwrap_or(1.0);
            let stacks = math::to_int32(num(p, "Stacks").unwrap_or(0.0));
            crate::combat::buffs::spawn_buff(w, o, buff, dur, Some(stacks), Some(f), true, None);
        }
        crate::world::pawns::put_fighters(w, all);
    }
    w.fighter_mut(f).requests.ally_buff_requests.clear();
}

/// `World.step`'s projectiles pass: each projectile's move, kind hooks, doom, reflect, mesh tick, physics and
/// granted buffs, then the splits and the projectiles born in the pass.
/// Deviation: a ship's pieces are met where they stand after the ship's move of this frame. The pieces are not swept
/// (the ship's root has no collision), so one can come to overlap a capsule: a projectile that dies on the
/// environment then dies in its next move, a bouncing one (the grenade, the charge shot) is first moved out of the
/// piece by SafeMoveUpdatedComponent's rule (Projectile.safeMove, read in the exe) and an item is pushed out of the
/// box; the physics body of an item in penetration is not read.
pub fn projectiles_pass(w: &mut World) {
    let mut all = std::mem::take(&mut w.scratch.blockers);
    w.blockers_into(BlockKind::Nav, &mut all);
    // `staticOf(platforms)` of a root that ignores Platform (`Projectile.platBoxes`): the same list for every
    // projectile of the pass
    let mut statics = Vec::new();
    crate::fighter::traces::statics_among(w, &all, &mut statics);
    // the polygon solids of the same channel, and those of object type WorldStatic for a root that ignores Platform
    let mut polys = Vec::new();
    crate::fighter::traces::solids::blocker_polys(w, BlockKind::Nav, &mut polys);
    let static_polys: Vec<_> = polys.iter().filter(|q| q.is_static).cloned().collect();
    let ground = w.stage.ground;
    let b = &w.stage.blast;
    let blast = Ground {
        left: f64::from(b.left),
        right: f64::from(b.right),
        bottom: f64::from(b.bottom),
        top: f64::from(b.top),
    };
    let mut splits = Vec::new();
    let mut hit_boxes = crate::world::stage_query::StageShapes::default();
    // (a projectile spawned in the pass joins it, as a `for of` over the list does)
    let mut i = 0;
    while let Some(pr) = w.actors.projectiles.nth(i) {
        i += 1;
        // Its own hit pause (CustomTimeDilation 0.005): the actor and its components held, its buff children not.
        if crate::combat::spawned_hits::projectile_pause_tick(w, pr) {
            if !pm_ref(w, pr).dead && pm_ref(w, pr).elem.is_some() {
                elem::actor_buff_tick(w, ActorRef::Projectile(pr));
            }
            // The pause's end: OnHitPauseEnded -> BPI_HitPauseEnd @5176 -> DestroySpawnedActor(false).
            // Unverified: run at the end of the pause's last held frame (the HitPauseTimer is a world timer).
            if let Some(p) = w.actors.projectiles.get_mut(pr)
                && p.hit_pause.is_none()
                && p.destroy_after_pause
            {
                p.destroy_after_pause = false;
                destroy_spawned_actor(w, ActorRef::Projectile(pr), 0.0);
            }
            continue;
        }
        // SpawnedActorParent Blocked's Delay(0.0) -> DestroySpawnedActor(false) (World.blocked), a latent action of
        // the actor's tick.
        let (cls, dead, blocked) = {
            let p = pm_ref(w, pr);
            (p.cls, p.dead, p.blocked_destroy)
        };
        if blocked && !dead {
            destroy_projectile(w, pr, false);
            continue;
        }
        // MaxLifetime and the blast-box check are timers: those deaths fall in the world's timer tick. The movement
        // component's tick, with the class's OnProjectileBounce handlers inside it (projectile_bounce). A registered
        // child class's handlers of the move and its tick; a true from step is its K2_DestroyActor. The actor's
        // tick (`step`) comes behind its movement component's, as in the build (bTickBeforeOwner): what it writes
        // for the move is used by the next frame's move.
        let kind = proj_hooks(w, cls);
        let hb = match kind.and_then(|k| k.receive_hit) {
            Some(rh) if !dead => {
                (rh.boxes)(w, pr, &mut hit_boxes);
                Some(hit_boxes.view())
            }
            _ => None,
        };
        let plats = Blockers { all: &all, statics: &statics, polys: &polys, static_polys: &static_polys };
        let why = projectile::step(w, pr, &ground, &blast, &plats, true, hb);
        if let Some(moved) = kind.and_then(|k| k.moved) {
            moved(w, pr);
        }
        match why {
            Some(StepEnd::Split) => splits.push(pr),
            Some(why) => destroy_projectile(w, pr, why != StepEnd::Env),
            None => {}
        }
        // The overlaps the move began with World.actors components (ActorKind.projectileMoved).
        if !pm_ref(w, pr).dead {
            projectile_moved(w, pr);
        }
        // ParentProjectilePhysics' ReceiveTick (@796 -> @10-166, AimDownProjectileMovement: aimDownTick), ahead of
        // the child's own tick, whose latent actions may turn the hit box on at the look-at's rotation
        // (C021_Homing_Projectile @233).
        // Deviation: the port meets the hits of the move behind both ticks (detectProjectileHits), so a hit the
        // build meets inside the move reads the look-at of the velocity after the move, not the rotation the move
        // turned to (bRotationFollowsVelocity: the velocity it started with), and a velocity a hit writes (a stick's
        // StopSimulating) is looked at in the next frame, not in this one.
        if !pm_ref(w, pr).dead && !kind.is_some_and(|k| k.own_tick) {
            projectile::aim_down_tick(w, pr);
        }
        // A pending Delay ahead of DestroySpawnedActor (destroy_spawned_actor), a latent action of the actor's tick.
        if !pm_ref(w, pr).dead && doom_tick(w, ActorRef::Projectile(pr)) {
            continue;
        }
        if !pm_ref(w, pr).dead
            && let Some(step) = kind.and_then(|k| k.step)
            && step(w, pr)
        {
            destroy_projectile(w, pr, true);
        }
        // A reflect's pause, its latent actions in the actor's tick: ahead of the mesh's tick,
        // which its Montage_SetPlayRate (SpawnedActorParent @1560 / @2942) reaches in the same frame, and of the
        // physics step, which its SetMobilityToStationary comes before in the build.
        if !pm_ref(w, pr).dead && pm_ref(w, pr).reflect.is_some() {
            crate::combat::reflect::reflect_tick(w, ActorRef::Projectile(pr));
        }
        // Its skeletal mesh's tick (TG_PrePhysics, behind the actor's), then its simulating
        // body's step.
        if !pm_ref(w, pr).dead && crate::actors::actor_montage::has_anim(w, ActorRef::Projectile(pr)) {
            crate::actors::actor_montage::mesh_tick(w, ActorRef::Projectile(pr));
        }
        if !pm_ref(w, pr).dead
            && let Some(physics) = kind.and_then(|k| k.physics)
        {
            physics(w, pr);
        }
        // Its buff children's ticks and life timers.
        if !pm_ref(w, pr).dead && pm_ref(w, pr).elem.is_some() {
            elem::actor_buff_tick(w, ActorRef::Projectile(pr));
        }
    }
    let born = w.actors.projectiles.len();
    for pr in splits {
        if let Some(split) = proj_hooks(w, pm_ref(w, pr).cls).and_then(|k| k.split) {
            split(w, pr);
        }
    }
    // The follow-ups are spawned inside the splitter's tick (its Delay): they tick in this frame.
    let mut i = born;
    while let Some(fu) = w.actors.projectiles.nth(i) {
        i += 1;
        let plats = Blockers { all: &all, statics: &statics, polys: &polys, static_polys: &static_polys };
        match projectile::step(w, fu, &ground, &blast, &plats, true, None) {
            Some(why) => destroy_projectile(w, fu, why != StepEnd::Env),
            None => {
                projectile_moved(w, fu);
                // its actor tick, behind that move
                if !pm_ref(w, fu).dead && !proj_hooks(w, pm_ref(w, fu).cls).is_some_and(|k| k.own_tick) {
                    projectile::aim_down_tick(w, fu);
                }
            }
        }
    }
    w.scratch.blockers = all;
}

/// `World.step`'s passes of the other families: markers, the new ships, mortars, the registered actors (`step`, mesh
/// tick, `late`, granted buffs, `spawnedMoved`), aim punches, accelerators. Each list is walked by place, so an
/// entry the pass adds is met in it (as a `for of` does).
pub fn lists_pass(w: &mut World) {
    let mut i = 0;
    while let Some(m) = w.actors.markers.nth(i) {
        i += 1;
        if !w.actors.markers.get(m).expect("a live marker").dead {
            crate::chars::morty::marker_tick(w, m);
        }
        // its granted buffs
        if w.actors.markers.get(m).is_some_and(|x| !x.dead && x.elem.is_some()) {
            elem::actor_buff_tick(w, ActorRef::Marker(m));
        }
    }
    let mut i = 0;
    while let Some(sh) = w.actors.ships.nth(i) {
        i += 1;
        if w.actors.ships.get(sh).expect("a live ship").age == 0 {
            crate::chars::morty::ship_tick(w, sh);
        }
    }
    let mut i = 0;
    while let Some(mo) = w.actors.mortars.nth(i) {
        i += 1;
        if !w.actors.mortars.get(mo).expect("a live mortar").dead {
            let mut boxes = StageShapes { boxes: std::mem::take(&mut w.scratch.blockers), polys: Vec::new() };
            w.blocker_shapes_into(BlockKind::Trace, &mut boxes);
            let boom = crate::chars::morty::mortar_step(w, mo, boxes.view());
            w.scratch.blockers = boxes.boxes;
            if boom {
                crate::chars::morty::seismic_boom(w, mo);
            }
        }
        // its granted buffs
        if w.actors.mortars.get(mo).is_some_and(|x| !x.dead && x.elem.is_some()) {
            elem::actor_buff_tick(w, ActorRef::Mortar(mo));
        }
    }
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        let live = |w: &World| w.actors.actors.get(x).is_some_and(|a| !a.dead);
        if !live(w) || doom_tick(w, ActorRef::Actor(x)) {
            continue;
        }
        let k = world_hooks(w, w.actors.actors.get(x).expect("a live actor").cls);
        if let Some(step) = k.and_then(|k| k.step) {
            step(w, x);
        }
        // Its skeletal mesh's tick, behind the actor's own, then what the kind takes after
        // it.
        if live(w) && crate::actors::actor_montage::has_anim(w, ActorRef::Actor(x)) {
            crate::actors::actor_montage::mesh_tick(w, ActorRef::Actor(x));
        }
        if live(w)
            && let Some(late) = k.and_then(|k| k.late)
        {
            late(w, x);
        }
        // Its buff children's ticks and life timers: the perk buffs it was granted.
        if w.actors.actors.get(x).is_some_and(|a| !a.dead && a.elem.is_some()) {
            elem::actor_buff_tick(w, ActorRef::Actor(x));
        }
        // The overlaps its tick began with the other World.actors components (ActorKind.spawnedMoved).
        if live(w) {
            spawned_moved(w, ActorRef::Actor(x));
        }
    }
    let mut i = 0;
    while let Some(ap) = w.actors.aim_punches.nth(i) {
        i += 1;
        if !w.actors.aim_punches.get(ap).expect("a live aim punch").dead {
            crate::chars::morty::aim_punch_step(w, ap);
        }
    }
    let mut i = 0;
    while let Some(ac) = w.actors.accelerators.nth(i) {
        i += 1;
        if !w.actors.accelerators.get(ac).expect("a live accelerator").dead && !doom_tick(w, ActorRef::Accelerator(ac))
        {
            crate::chars::morty::accelerator_tick(w, ac);
        }
    }
}

/// `World.step`'s ends of the dead markers, ships, mortars, registered actors, aim punches and accelerators (their
/// `end` hooks and their removal), ahead of `matchTick`. The granted buffs' EndPlay with their actor's
/// (SpawnedActorParent @15512).
pub fn end_dead(w: &mut World) {
    // (each list walked by place, live, as a `for of` does: an end that kills a later entry ends it too)
    let mut i = 0;
    while let Some(m) = w.actors.markers.nth(i) {
        i += 1;
        if w.actors.markers.get(m).is_some_and(|m| m.dead && m.elem.is_some()) {
            elem::end_actor_children(w, ActorRef::Marker(m));
        }
    }
    let mut i = 0;
    while let Some(mo) = w.actors.mortars.nth(i) {
        i += 1;
        if w.actors.mortars.get(mo).is_some_and(|m| m.dead && m.elem.is_some()) {
            elem::end_actor_children(w, ActorRef::Mortar(mo));
        }
    }
    w.actors.markers.retain(|_, m| !m.dead);
    // its Comp_EdgeTrace.On Destroyed
    let mut i = 0;
    while let Some(sh) = w.actors.ships.nth(i) {
        i += 1;
        if w.actors.ships.get(sh).is_some_and(|s| s.dead) {
            crate::fighter::traces::spawned_edges_end(w, ActorRef::Ship(sh));
        }
    }
    w.actors.ships.retain(|_, s| !s.dead);
    keep_gone_mortars(w);
    w.actors.mortars.retain(|_, m| !m.dead);
    // A World.actors kind's EndPlay: its buff children (SpawnedActorParent @15512, endActorChildren; an actor a
    // fighter's hit gave OnHitBuffs rows), then the kind's own `end`.
    // Deviation: run here in the list pass, not at the `dead = true` of the actor's kill (the kinds set it in place).
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        let Some(a) = w.actors.actors.get(x).filter(|a| a.dead) else { continue };
        let (cls, has_elem) = (a.cls, a.elem.is_some());
        if has_elem {
            elem::end_actor_children(w, ActorRef::Actor(x));
        }
        if let Some(end) = world_hooks(w, cls).and_then(|k| k.end) {
            end(w, x);
        }
        // OnDestroyed for a run-time binder (as `projectile_end`)
        broadcast_bound(w, DelegateList::Actor(ActorRef::Actor(x), ActorList::Destroyed), &DelegateCall::Destroyed);
        // a SpawnedPlatformParent child's Comp_EdgeTrace.On Destroyed
        crate::fighter::traces::spawned_edges_end(w, ActorRef::Actor(x));
    }
    crate::world::spawned_platform::keep_gone_plats(w);
    keep_gone_actors(w);
    w.actors.actors.retain(|_, x| !x.dead);
    w.actors.aim_punches.retain(|_, a| !a.dead);
    w.actors.accelerators.retain(|_, a| !a.dead);
}

/// `projectileEnd` of the dead projectiles, their removal and the items', and the owners' spawned lists.
pub fn end_projectiles(w: &mut World) {
    let mut i = 0;
    while let Some(pr) = w.actors.projectiles.nth(i) {
        i += 1;
        if pm_ref(w, pr).dead {
            projectile_end(w, pr);
        }
    }
    keep_gone_boxes(w);
    // `Actors::gone_motion`: what a bot's key still holding the projectile reads of it.
    let now = w.flow.tick;
    w.actors.gone_motion.retain(|g| now - g.tick < crate::world::GONE_MOTION_TICKS);
    let dead: Vec<_> = w
        .actors
        .projectiles
        .iter()
        .filter(|(_, p)| p.dead)
        .map(|(id, p)| crate::world::GoneMotion { id, seq: p.seq, tick: now, x: p.x, y: p.y, vx: p.vx, vy: p.vy })
        .collect();
    w.actors.gone_motion.extend(dead);
    w.actors.projectiles.retain(|_, p| !p.dead);
    // OnDestroyed of the dead items for a run-time binder (as `projectile_end`; the Batman tutorial's Retrieve demo
    // binds a batarang's).
    // Deviation: broadcast in the list pass, as a world actor's, not at the item's kill.
    let mut n = 0;
    while let Some(it) = w.actors.items.nth(n) {
        n += 1;
        if w.actors.items.get(it).is_some_and(|i| i.dead) {
            broadcast_bound(w, DelegateList::Actor(ActorRef::Item(it), ActorList::Destroyed), &DelegateCall::Destroyed);
        }
    }
    keep_gone_items(w);
    w.actors.items.retain(|_, it| !it.dead);
    // The delegates of an actor gone from its arena (dropped with the actor); one dead but
    // not yet removed keeps them for its own end.
    let mut d = std::mem::take(&mut w.actor_delegates);
    d.retain(|&a, _| !actor_gone(w, a));
    w.actor_delegates = d;
    // (`this.all`, read once)
    let n = w.pawns.as_ref().unwrap_or(&w.players).len();
    for i in 0..n {
        let f = w.pawns.as_ref().unwrap_or(&w.players)[i];
        let mut l = std::mem::take(&mut w.fighter_mut(f).items.spawned_actors);
        l.retain(|&a| !actor_dead(w, a));
        w.fighter_mut(f).items.spawned_actors = l;
    }
}

// ---- a spawned actor of any family

/// A spawned actor's `dead`: an actor gone from its arena reads as dead.
fn actor_dead(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(i) => w.actors.projectiles.get(i).is_none_or(|x| x.dead),
        ActorRef::Item(i) => w.actors.items.get(i).is_none_or(|x| x.dead),
        ActorRef::Actor(i) => w.actors.actors.get(i).is_none_or(|x| x.dead),
        ActorRef::Marker(i) => w.actors.markers.get(i).is_none_or(|x| x.dead),
        ActorRef::Ship(i) => w.actors.ships.get(i).is_none_or(|x| x.dead),
        ActorRef::Mortar(i) => w.actors.mortars.get(i).is_none_or(|x| x.dead),
        ActorRef::AimPunch(i) => w.actors.aim_punches.get(i).is_none_or(|x| x.dead),
        ActorRef::Accelerator(i) => w.actors.accelerators.get(i).is_none_or(|x| x.dead),
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => false,
    }
}

/// A spawned actor no longer in its arena (a stale id included); a fighter, a stage or a buff never is.
fn actor_gone(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(i) => w.actors.projectiles.get(i).is_none(),
        ActorRef::Item(i) => w.actors.items.get(i).is_none(),
        ActorRef::Actor(i) => w.actors.actors.get(i).is_none(),
        ActorRef::Marker(i) => w.actors.markers.get(i).is_none(),
        ActorRef::Ship(i) => w.actors.ships.get(i).is_none(),
        ActorRef::Mortar(i) => w.actors.mortars.get(i).is_none(),
        ActorRef::AimPunch(i) => w.actors.aim_punches.get(i).is_none(),
        ActorRef::Accelerator(i) => w.actors.accelerators.get(i).is_none(),
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => false,
    }
}

/// A spawned actor's class, `None` for one gone or not a spawned actor.
fn actor_cls(w: &World, a: ActorRef) -> Option<Name> {
    match a {
        ActorRef::Projectile(i) => w.actors.projectiles.get(i).map(|x| x.cls),
        ActorRef::Item(i) => w.actors.items.get(i).map(|x| x.cls),
        ActorRef::Actor(i) => w.actors.actors.get(i).map(|x| x.cls),
        ActorRef::Marker(i) => w.actors.markers.get(i).map(|x| x.cls),
        ActorRef::Ship(i) => w.actors.ships.get(i).map(|x| x.cls),
        ActorRef::Mortar(i) => w.actors.mortars.get(i).map(|x| x.cls),
        ActorRef::AimPunch(i) => w.actors.aim_punches.get(i).map(|x| x.cls),
        ActorRef::Accelerator(i) => w.actors.accelerators.get(i).map(|x| x.cls),
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => None,
    }
}

/// `x.reflect?.unlisted` of any spawned actor (a projectile's or an item's; the other families have no reflect).
fn actor_unlisted(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(i) => w.actors.projectiles.get(i).is_some_and(|x| unlisted(&x.reflect)),
        ActorRef::Item(i) => w.actors.items.get(i).is_some_and(|x| unlisted(&x.reflect)),
        _ => false,
    }
}

// ---- the spawn calls

/// `deathVfx`: SpawnedActorParent EndPlay (ubergraph 17018): DeathVFX at the actor location (Death VFX At Hit Location
/// is off) plus Death VFX Offset, its Y negated when the actor's yaw is below 0 (facing +x: yaw -90).
pub fn death_vfx(w: &World, cls: Name, fighter: Fid, p: &Props, x: Real, y: Real, dir: Real) -> Event {
    let o = obj(p, "Death VFX Offset");
    let oyv = o.and_then(|o| num(o, "Y")).unwrap_or(0.0);
    let oy = if dir > 0.0 { -oyv } else { oyv };
    let r = obj(p, "DeathVFXRotation");
    let rv = |k: &str| r.and_then(|r| num(r, k)).unwrap_or(0.0);
    Event::ProjectileDeath {
        cls,
        fighter,
        vfx: p.get("DeathVFX").and_then(Value::as_str).map(|s| w.lit(s)),
        x: x - oy,
        y: y + o.and_then(|o| num(o, "Z")).unwrap_or(0.0),
        ox: o.and_then(|o| num(o, "X")).unwrap_or(0.0),
        rot: [rv("Pitch"), rv("Yaw"), rv("Roll")],
        scale: obj(p, "DeathVFXScale").and_then(|s| num(s, "X")).unwrap_or(1.0),
        tint: None,
        to: None,
    }
}

/// `isA`: UKismetArrayLibrary::FilterArray's IsA on a spawned actor's class (the entry's registered parents).
pub fn is_a(w: &World, cls: Name, of: Name) -> bool {
    cls == of || entry(w, cls).is_some_and(|e| e.parents.contains(&w.names.str(of)))
}

/// `destroyWithInstigator`: Comp_Spawner DestroyActorsOnEndPlay: K2_DestroyActor on every actor of the fighter's
/// SpawnedActors whose BPI_GetDiesWithInstigator ('Die With Instigator') is true (a respawn's OnBaseFighterReset, a
/// minion's ReceiveEndPlay).
pub fn destroy_with_instigator(w: &mut World, f: FighterId) {
    destroy_spawned_of(w, f, false);
}

/// (the port's own) Every actor fighter `f` spawned destroyed as `destroy_with_instigator` destroys one, whatever its
/// 'Die With Instigator', but an item another fighter holds: a mode pawn that leaves the match for good (a Waves
/// enemy's despawn), whose actors would otherwise go on with no owner in the match.
pub fn destroy_all_spawned(w: &mut World, f: FighterId) {
    destroy_spawned_of(w, f, true);
}

fn destroy_spawned_of(w: &mut World, f: FighterId, every: bool) {
    let dies = |w: &World, a: ActorRef| every || truthy(actor_p(w, a), "Die With Instigator");
    let mut i = 0;
    while let Some(pr) = w.actors.projectiles.nth(i) {
        i += 1;
        let p = pm_ref(w, pr);
        if spawner(p) == Some(f) && !p.dead && !unlisted(&p.reflect) && dies(w, ActorRef::Projectile(pr)) {
            destroy_projectile(w, pr, false);
        }
    }
    let mut i = 0;
    while let Some(it) = w.actors.items.nth(i) {
        i += 1;
        let x = w.actors.items.get(it).expect("a live item id");
        let kept = every && x.held && x.holder != w.fighter(f).id;
        if x.spawner == Some(f) && !x.dead && !kept && !unlisted(&x.reflect) && dies(w, ActorRef::Item(it)) {
            crate::world::items::destroy_item(w, it);
        }
    }
    let mut i = 0;
    while let Some(m) = w.actors.markers.nth(i) {
        i += 1;
        let x = w.actors.markers.get(m).expect("a live marker id");
        if x.owner == f && !x.dead && dies(w, ActorRef::Marker(m)) {
            crate::chars::morty::destroy_marker(w, m);
        }
    }
    let mos: Vec<MortarId> = w
        .actors
        .mortars
        .iter()
        .filter(|&(id, m)| m.owner == f && dies(w, ActorRef::Mortar(id)))
        .map(|(id, _)| id)
        .collect();
    for mo in mos {
        w.actors.mortars.get_mut(mo).expect("listed above").dead = true;
    }
    let xs: Vec<ActorId> = w
        .actors
        .actors
        .iter()
        .filter(|&(id, x)| x.owner == f && dies(w, ActorRef::Actor(id)))
        .map(|(id, _)| id)
        .collect();
    for x in xs {
        w.actors.actors.get_mut(x).expect("listed above").dead = true;
    }
    let aps: Vec<_> = w
        .actors
        .aim_punches
        .iter()
        .filter(|&(id, a)| a.owner == f && dies(w, ActorRef::AimPunch(id)))
        .map(|(id, _)| id)
        .collect();
    for ap in aps {
        w.actors.aim_punches.get_mut(ap).expect("listed above").dead = true;
    }
    let mut i = 0;
    while let Some(ac) = w.actors.accelerators.nth(i) {
        i += 1;
        let x = w.actors.accelerators.get(ac).expect("a live accelerator id");
        if x.owner == f && !x.dead && dies(w, ActorRef::Accelerator(ac)) {
            crate::chars::morty::destroy_accelerator(w, ac);
        }
    }
}

/// `destroyProjectile`: DestroySpawnedActor -> ubergraph 15507: DeathVFX and the 'Ak Death Location' sound at the
/// actor unless DestroyVFXBlocked (@15557 -> @17018-18222), then the SpawnAtDeathChance roll unless
/// DestroySpawnedActorBlocked (@16070-16137). The two are written by BPI_SetDestroyEffectsBlocked /
/// BPI_SetDestroySpawnedActorBlocked (Buff_C015HitProjectile's eat). Inside the Lab reset's pass no roll and no
/// spawn, nested destroys too (destroyAllSpawned). `in_timer_tick`: the death falls in the world's timer tick.
pub fn destroy_projectile(w: &mut World, pr: ProjectileId, in_timer_tick: bool) {
    let Some(p) = w.actors.projectiles.get_mut(pr) else { return };
    p.dead = true;
    let cls = p.cls;
    let kind = proj_hooks(w, cls);
    // a registered class's own ReceiveDestroyed / ReceiveEndPlay
    if let Some(d) = kind.and_then(|k| k.destroyed) {
        d(w, pr);
    }
    // The parent's EndPlay @15512: its buff children end. Not in the Lab reset's pass: the buffs' ends reach old
    // pawns there, and the reset drops the actors with their lists.
    if !w.scratch.reset_pass {
        elem::end_actor_children(w, ActorRef::Projectile(pr));
    }
    let a = ActorRef::Projectile(pr);
    let p = pm_ref(w, pr);
    let (x, y) = (p.x, p.y);
    if !p.destroy_fx_blocked.unwrap_or(false) {
        if let Some(ev) = actor_p(w, a).get("Ak Death Location").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            let s = actor_sound_from(w, a);
            let ev = w.lit(ev);
            let pooled = s.pooled;
            w.events.push(Event::Sound {
                fighter: s.fighter,
                ev,
                x: Some(x),
                y: Some(y),
                hit: None,
                pooled,
                key: None,
            });
        }
        // DeathVFX Team: GetInstigator's colour index (SpawnedActorParent @17642-17922; the sound above: GetOwner
        // @18051); -1: none (an actor the world spawned).
        let p = pm_ref(w, pr);
        let fid = instigator(p).map_or(-1, |f| w.fighter(f).id);
        // Math.sign(pr.vx || pr.facing)
        let dir = math::sign(if p.vx != 0.0 && !p.vx.is_nan() { p.vx } else { p.facing });
        let ev = death_vfx(w, cls, fid, actor_p(w, a), x, y, dir);
        w.events.push(ev);
    }
    if !w.scratch.reset_pass && !pm_ref(w, pr).destroy_spawn_blocked.unwrap_or(false) {
        let spawns = match kind.and_then(|k| k.spawns_at_death) {
            Some(s) => s(w, pr),
            None => truthy(actor_p(w, a), "SpawnActorAtDeath"),
        };
        if spawns && truthy(actor_p(w, a), "ActorClassSpawnedAtDeath") {
            // RandomBoolWithWeight(SpawnAtDeathChance)
            let wt = num(actor_p(w, a), "SpawnAtDeathChance").unwrap_or(0.0);
            if wt > 0.0 && w.rng.next() <= wt {
                spawn_at_death(w, pr, in_timer_tick);
            }
        }
    }
}

/// `projectileEnd`: a registered class's own ReceiveEndPlay, then AActor::Destroyed's OnDestroyed for a run-time
/// binder (after the class's ReceiveEndPlay: EndPlay ahead of OnDestroyed); its boxes of the Platform object type end
/// with it (a fighter whose base one was loses it with no impart: the pending-kill base's SetBase(null)).
pub fn projectile_end(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    if let Some(end) = proj_hooks(w, p.cls).and_then(|h| h.end) {
        end(w, pr);
    }
    broadcast_bound(w, DelegateList::Actor(ActorRef::Projectile(pr), ActorList::Destroyed), &DelegateCall::Destroyed);
    let Some(p) = w.actors.projectiles.get_mut(pr) else { return };
    for s in p.solids.iter_mut().flatten() {
        s.dead = true;
    }
    if p.plasma {
        let (fighter, seq) = (p.owner_id, p.seq);
        w.events.push(Event::SoundStop {
            fighter,
            key: SoundKey { tag: name!("proj"), n: seq },
            fade: Some(0.0),
            ev: None,
            curve: None,
            cut: false,
        });
    }
}

/// `activateActor`: BPI_ActivateObject. A class that is not an attached one (Projectile.activate: the notify's
/// AttachToActor family) answers through its kind alone, or warns.
pub fn activate_actor(w: &mut World, pr: ProjectileId) {
    let cls = pm_ref(w, pr).cls;
    let own = proj_hooks(w, cls).and_then(|k| k.activate);
    if !pm_ref(w, pr).attached {
        match own {
            Some(a) => a(w, pr),
            None => {
                let msg = format!("ActivateObject not implemented ({})", w.names.str(cls));
                warn_text(w, &msg);
            }
        }
        return;
    }
    projectile::activate(pm(w, pr));
    if let Some(a) = own {
        a(w, pr);
    }
}

/// `spawnAtDeath`: the owner's Comp_Spawner.CompSpawnActor(ActorClassSpawnedAtDeath) with the dying actor's transform
/// (SpawnDeathActorWithThisActorsTransform), direction (0, 0, 1), InitialSpeed 200. The item has no projectile
/// movement and simulates physics, so HandleActorPostSpawn gives it that velocity plus half the fighter's. An actor
/// with no owner has no Comp_Spawner to spawn through (SpawnedActorParent @16355: GetOwner's component, Accessed
/// None): nothing (the roll is drawn).
fn spawn_at_death(w: &mut World, pr: ProjectileId, in_timer_tick: bool) {
    if !projectile::is_owned_projectile(w, pr) {
        return;
    }
    let owner = pm_ref(w, pr).owner.expect("an owned projectile");
    let props = actor_p(w, ActorRef::Projectile(pr)).clone();
    let cls = w.lit(props.get("ActorClassSpawnedAtDeath").and_then(Value::as_str).unwrap_or_default());
    let has = actor_def(w, ActorRef::Fighter(owner), cls).is_some();
    if has && cls == name!("C017_CannonExplotion_C") {
        // SpawnDeathActorWithThisActorsTransform: at the shot's place; C017_CannonExplotion ReceiveBeginPlay: the
        // impact sound, its UpwardBurst box tested at its spawn place (CannonBurst).
        crate::chars::iron_giant::cannon_aim::spawn_burst(w, pr, cls);
        return;
    }
    if has && proj_hooks(w, cls).is_some() {
        // A registered projectile class (Firebolt's FireWallHazard_C): @16355-16684, the transform
        // SelectTransform(GetTransform, MakeTransform(location + SpawnedActorAtDeathTransform's location, its
        // rotation, its scale), SpawnDeathActorWithThisActorsTransform), the world transform flag true, the death
        // speeds.
        // Deviation: the transform's rotation and scale are not handed on (the port's Projectile takes its facing
        // from the owner, its scale from the spawn: 1 here); every class that reaches here has the identity
        // transform.
        let t = if truthy(&props, "SpawnDeathActorWithThisActorsTransform") {
            None
        } else {
            obj(&props, "SpawnedActorAtDeathTransform").and_then(|t| obj(t, "Translation"))
        };
        let p = pm_ref(w, pr);
        let x = p.x - t.and_then(|t| num(t, "Y")).unwrap_or(0.0);
        let y = p.y + t.and_then(|t| num(t, "Z")).unwrap_or(0.0);
        let d = obj(&props, "Spawned At DeathInitial Movement Direction");
        let dir = [
            Some(d.and_then(|d| num(d, "X")).unwrap_or(0.0)),
            Some(d.and_then(|d| num(d, "Y")).unwrap_or(0.0)),
            Some(d.and_then(|d| num(d, "Z")).unwrap_or(0.0)),
        ];
        let speed = num(&props, "Spawned At Death Initial Speed").unwrap_or(0.0);
        let max = num(&props, "Spawned At DeathMax Speed").unwrap_or(0.0);
        comp_spawn_projectile(w, owner, cls, x, y, dir, speed, max, None);
        return;
    }
    let it = crate::world::items::spawn_item_at_death(w, pr, cls, in_timer_tick);
    item_spawned(w, it, owner);
}

/// `destroySpawned`: a projectile's or an item's destroy.
pub fn destroy_spawned(w: &mut World, x: ActorRef) {
    match x {
        ActorRef::Item(it) => crate::world::items::destroy_item(w, it),
        ActorRef::Projectile(pr) => destroy_projectile(w, pr, false),
        _ => unreachable!("destroySpawned of a projectile or an item"),
    }
}

/// The World's lists `pickSpawned` walks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ListKind {
    Projectiles,
    Markers,
    Mortars,
    Actors,
    AimPunches,
}

/// One entry of a list: the actor, whose SpawnedActors it is in (`listedBy`: a projectile's `spawner`, every other
/// kind's owner), its class, and whether it has left the list (dead or reflected away).
type ListEntry = (ActorRef, Option<FighterId>, Name, bool);

/// A list's entries in its order.
fn list_entries(w: &World, kind: ListKind) -> Vec<ListEntry> {
    match kind {
        ListKind::Projectiles => w
            .actors
            .projectiles
            .iter()
            .map(|(id, p)| (ActorRef::Projectile(id), spawner(p), p.cls, p.dead || unlisted(&p.reflect)))
            .collect(),
        ListKind::Markers => {
            w.actors.markers.iter().map(|(id, m)| (ActorRef::Marker(id), Some(m.owner), m.cls, m.dead)).collect()
        }
        ListKind::Mortars => {
            w.actors.mortars.iter().map(|(id, m)| (ActorRef::Mortar(id), Some(m.owner), m.cls, m.dead)).collect()
        }
        ListKind::Actors => {
            w.actors.actors.iter().map(|(id, x)| (ActorRef::Actor(id), Some(x.owner), x.cls, x.dead)).collect()
        }
        ListKind::AimPunches => {
            w.actors.aim_punches.iter().map(|(id, a)| (ActorRef::AimPunch(id), Some(a.owner), a.cls, a.dead)).collect()
        }
    }
}

/// The owner's live entries of a list whose class is `cls` or a child of it.
fn owned_of(w: &World, kind: ListKind, f: FighterId, cls: Name) -> Vec<ActorRef> {
    list_entries(w, kind)
        .into_iter()
        .filter(|&(_, by, c, gone)| by == Some(f) && !gone && is_a(w, c, cls))
        .map(|(a, ..)| a)
        .collect()
}

/// `pickSpawned`: Comp_Spawner "Get Spawned Actor" (@95-791) for an ActivateActors call: the spawner's SpawnedActors
/// filtered by the class, then all of them (All), [first, last] (FirstLast: one actor of the class is there twice and
/// is activated twice), the last (Last) or the first. An empty list gives Array_Get's empty item, which the caller's
/// interface cast drops (@4249). SpawnedActors is in spawn order (Array_AddUnique @5513 / @5643 / @8896 at the spawn,
/// Array_RemoveItem @5582 / @6635 when an actor ends). Model: each of the port's lists is appended to at the spawn and
/// filtered in place, and a class lives in one list, so a list's own order stands for it; an actor counts until it
/// is marked dead. An ActivateSpawned without a class (`only`): the actor it picked from SpawnedActors.
/// Unverified: UKismetArrayLibrary::FilterArray's IsA test is the stock 4.26 source, not read in the exe.
fn pick_spawned(w: &World, kind: ListKind, f: FighterId, r: &ActivateRequest) -> Vec<ActorRef> {
    if let Some(only) = r.only {
        // (`x === r.only && !x.dead`)
        return list_entries(w, kind)
            .into_iter()
            .filter(|&(a, ..)| a == only && !actor_dead(w, a))
            .map(|e| e.0)
            .collect();
    }
    let l = owned_of(w, kind, f, r.cls);
    if r.mode == ActivateMode::All || l.is_empty() {
        return l;
    }
    match r.mode {
        ActivateMode::FirstLast => vec![l[0], l[l.len() - 1]],
        ActivateMode::Last => vec![l[l.len() - 1]],
        _ => vec![l[0]],
    }
}

/// `pickSpawned(w, w.projectiles, f, { cls, mode: 'last' })[0]`: the last live projectile of `cls` (or a child class)
/// that `f`'s Comp_Spawner lists.
pub fn last_spawned_projectile(w: &World, f: FighterId, cls: Name) -> Option<ProjectileId> {
    let r = ActivateRequest { cls, mode: ActivateMode::Last, conds: Vec::new(), charge: None, only: None };
    match pick_spawned(w, ListKind::Projectiles, f, &r).first() {
        Some(&ActorRef::Projectile(id)) => Some(id),
        _ => None,
    }
}

/// `despawnOne`: Comp_Spawner DestroySpawnedActors with DestroyLastActor (@844-1196) or DestroyFirstActor (@2109) and
/// not DestroyAllActors: SpawnedActors_ClassChecked's last or first entry destroyed (K2_DestroyActor), nothing when
/// it is not valid. As `pick_spawned`, a class lives in one of the port's lists and that list's order stands for
/// SpawnedActors; an actor marked dead or reflected away has left it. The class test is FilterArray's IsA (`is_a`: a
/// child class passes; Comp_Spawner @166 / @375 / @736).
pub fn despawn_one(w: &mut World, f: FighterId, cls: Name, last: bool) {
    let pick = |l: Vec<ActorRef>| if last { l.last().copied() } else { l.first().copied() };
    if let Some(ActorRef::Projectile(pr)) = pick(owned_of(w, ListKind::Projectiles, f, cls)) {
        destroy_projectile(w, pr, false);
        return;
    }
    if let Some(ActorRef::Mortar(mo)) = pick(owned_of(w, ListKind::Mortars, f, cls)) {
        w.actors.mortars.get_mut(mo).expect("picked above").dead = true;
        return;
    }
    if let Some(ActorRef::Actor(x)) = pick(owned_of(w, ListKind::Actors, f, cls)) {
        w.actors.actors.get_mut(x).expect("picked above").dead = true;
        return;
    }
    if let Some(ActorRef::AimPunch(ap)) = pick(owned_of(w, ListKind::AimPunches, f, cls)) {
        w.actors.aim_punches.get_mut(ap).expect("picked above").dead = true;
        return;
    }
    let its: Vec<ItemId> = w
        .actors
        .items
        .iter()
        .filter(|(_, i)| i.spawner == Some(f) && is_a(w, i.cls, cls) && !i.dead && !unlisted(&i.reflect))
        .map(|(id, _)| id)
        .collect();
    if let Some(&it) = if last { its.last() } else { its.first() } {
        crate::world::items::destroy_item(w, it);
    }
}

/// `projectileMoved`: ActorKind.projectileMoved of each live actor of World.actors, behind the projectile's move.
pub fn projectile_moved(w: &mut World, pr: ProjectileId) {
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        if pm_ref(w, pr).dead {
            return;
        }
        let a = w.actors.actors.get(x).expect("a live actor id");
        if !a.dead
            && let Some(h) = world_hooks(w, a.cls).and_then(|k| k.projectile_moved)
        {
            h(w, x, pr);
        }
    }
}

/// `spawnedMoved`: ActorKind.spawnedMoved of each other live actor of World.actors, behind a World.actors entry's
/// tick or a thrown item's move.
pub fn spawned_moved(w: &mut World, o: ActorRef) {
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        if actor_dead(w, o) {
            return;
        }
        let a = w.actors.actors.get(x).expect("a live actor id");
        if !a.dead
            && ActorRef::Actor(x) != o
            && let Some(h) = world_hooks(w, a.cls).and_then(|k| k.spawned_moved)
        {
            h(w, x, o);
        }
    }
}

/// `addActor`: a World.actors entrant (the caller has put it in the list): its number, the charge, the owner's list,
/// its components' BeginPlay (the passive buffs) and SpawnedActorParent's BeginPlay (GrantBuffsFromInstigator,
/// NumberOfActorsAllowed) for a class of that parent. `spawner`: the actor joins its owner's Comp_Spawner list
/// (SpawnedActors); false for one the spawner did not make (a deferred spawn).
/// Deviation: a kind's `spawn` (the child's own BeginPlay part where it has no beginPlay) and its blast-box loop
/// (where the kind runs one) come ahead of this, the parent's part, which the child's ReceiveBeginPlay calls first.
pub fn add_actor(w: &mut World, a: ActorId, charge: Option<ChargeVals>, spawner: bool) {
    let seq = next_seq(w);
    let x = w.actors.actors.get_mut(a).expect("a listed actor");
    x.seq = seq;
    let (owner, cls) = (x.owner, x.cls);
    elem::receive_charge(w, ActorRef::Actor(a), charge);
    if spawner {
        w.fighter_mut(owner).items.spawned_actors.push(ActorRef::Actor(a));
    }
    // a SpawnedPlatformParent child's Comp_EdgeTrace
    crate::fighter::traces::edge_begin_play(w, ActorRef::Actor(a));
    if actor_is_a(w, ActorRef::Fighter(owner), cls, PARENT) != Some(true) {
        return;
    }
    elem::spawn_passive_buffs(w, ActorRef::Actor(a));
    // SpawnedActorParent BeginPlay's GrantBuffsFromInstigator @9233
    elem::grant_actor_buffs(w, ActorRef::Actor(a));
    actors_allowed(w, ActorRef::Actor(a), owner);
}

/// `afterSpawnedActor`: Comp_Spawner HandleActorPostSpawn's AfterSpawnedActor(SpawnedActor) @12116, broadcast by the
/// Comp_Spawner whose CompSpawnActor made the actor, behind its BeginPlay: Buff_Enlarge's binding (enlargeSpawned),
/// the pawn's own binding (CharHooks.afterSpawnedActor, which takes a projectile or an item: Pawn_C015's burp) and
/// the buffs' (BuffSignal). Every CompSpawnActor route sends it; a deferred spawn sends none. Not for an actor its
/// BeginPlay ended.
pub fn after_spawned_actor(w: &mut World, f: FighterId, a: ActorRef) {
    if actor_dead(w, a) {
        return;
    }
    if !matches!(a, ActorRef::Item(_)) {
        elem::enlarge_spawned(w, f, a);
    }
    if matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_)) {
        let reg: &'static crate::registry::Registry = w.registry;
        if let Some(h) = reg.chars.get(w.names.str(w.fighter(f).ch)).and_then(|c| c.after_spawned_actor) {
            h(w, f, a);
        }
    }
    let arg = BuffSignalArg { actor: Some(a), ..BuffSignalArg::default() };
    crate::combat::buffs::buff_signal(w, f, BuffSignal::AfterSpawnedActor, arg);
    // a run-time binder's
    let call = DelegateCall::AfterSpawnedActor { actor: a };
    broadcast_bound(w, DelegateList::Pawn(f, PawnList::AfterSpawnedActor), &call);
}

/// `compSpawnActor`: Comp_Spawner.CompSpawnActor of a World.actors class from a graph or a notify (the kind's spawn
/// made it, the caller listed it): `add_actor` (its number, the lists, GrantBuffsFromInstigator), the child's
/// BeginPlay behind the parent's (ActorKind.beginPlay), then AfterSpawnedActor. `f`: the spawner. `charge`: the
/// default is `instigator_charge` of the actor's owner.
pub fn comp_spawn_actor(w: &mut World, a: ActorId, charge: Option<ChargeVals>, f: FighterId) {
    add_actor(w, a, charge, true);
    let x = w.actors.actors.get(a).expect("a listed actor");
    if !x.dead
        && let Some(b) = world_hooks(w, x.cls).and_then(|k| k.begin_play)
    {
        b(w, a);
    }
    after_spawned_actor(w, f, ActorRef::Actor(a));
}

/// SpawnActor of a world actor class by a graph that is no Comp_Spawner (a tutorial minigame's own spawn): the kind's
/// `spawn`, the list, its `spawned` hook, the parent's BeginPlay part (`add_actor`, not in the owner's SpawnedActors)
/// and the class's own BeginPlay; no AfterSpawnedActor. `f` stands for the owner the port's actor needs. `None` for
/// a class with no world actor kind (warned).
pub fn spawn_world_actor(w: &mut World, f: FighterId, r: &SpawnRequest) -> Option<ActorId> {
    let e = entry(w, r.cls);
    let (Some(e), Some(h)) = (e, world_hooks(w, r.cls)) else {
        let msg = format!("spawned actor not implemented ({})", w.names.str(r.cls));
        warn_text(w, &msg);
        return None;
    };
    let (ActorClass::World(class), Some(spawn)) = (e.class, h.spawn) else {
        let msg = format!("spawned actor not implemented ({})", w.names.str(r.cls));
        warn_text(w, &msg);
        return None;
    };
    let (state, dead) = spawn(w, f, r);
    let x = new_world_actor(w, r.cls, class, f, dead, state);
    let id = w.actors.actors.insert(x);
    if !dead && let Some(s) = h.spawned {
        s(w, id);
    }
    add_actor(w, id, r.charge, false);
    let x = w.actors.actors.get(id).expect("a listed actor");
    if !x.dead
        && let Some(b) = h.begin_play
    {
        b(w, id);
    }
    Some(id)
}

/// `compSpawnItem`: Comp_Spawner.CompSpawnActor of an item class from an actor's graph (`spawn_item`'s place and
/// launch), with what a notify's spawn gives one besides: GrantBuffsFromInstigator @9233 and AfterSpawnedActor.
#[allow(clippy::too_many_arguments, reason = "the call keeps the build function's parameters")]
pub fn comp_spawn_item(
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
    let it = crate::world::items::spawn_item(w, cls, f, x, y, vx, vy, late, charge);
    item_spawned(w, it, f);
    it
}

/// `itemSpawned`: the grant and the signal for an item a CompSpawnActor made (`comp_spawn_item`, the death spawn, a
/// notify's spawn).
/// Unverified: the grant is taken behind the item's own begin (`add_item`), not ahead of it.
pub fn item_spawned(w: &mut World, it: Option<ItemId>, f: FighterId) {
    let Some(it) = it else { return };
    if w.actors.items.get(it).is_none_or(|i| i.dead) {
        return;
    }
    // (spawned by f: owned)
    elem::grant_spawned_buffs(w, ActorRef::Item(it), f);
    after_spawned_actor(w, f, ActorRef::Item(it));
}

/// `actorsAllowed`: SpawnedActorParent ReceiveBeginPlay @9114 -> @5604-6723, behind GrantBuffsFromInstigator and the
/// blast-box loop (@9203): AddToActorList(self) on the owner's Comp_Spawner @5827, then with NumberOfActorsAllowed
/// above 0 the owner's SpawnedActors filtered by IsA the actor's class (FilterArray @6011: a child class counts
/// towards its parent's limit, not the other way); with more of them than allowed, the first (the oldest) is
/// K2_DestroyActor'd @6515 (`k2_destroy_actor`), or released to the pool @6423 for a class with CanBePooled. One
/// actor a spawn. `f`: whose list the actor joined.
/// Deviation: the port has no pool. A CanBePooled class's oldest is ended as a destroyed one, and an ended actor
/// leaves the count. In the build ReleaseToPool -> AddActorToPool (Comp_Spawner @246) keeps it in SpawnedActors, so a
/// released actor still counts in the FilterArray, and a reused one comes back through InitFromPool (Comp_Spawner
/// @3399) with no BeginPlay, so its spawn runs no limit; here every spawn is limited.
pub fn actors_allowed(w: &mut World, a: ActorRef, f: FighterId) {
    let max = num(actor_p(w, a), "NumberOfActorsAllowed").unwrap_or(0.0);
    if max <= 0.0 {
        return;
    }
    let cls = actor_cls(w, a).expect("a spawned actor");
    if !w.fighter(f).items.spawned_actors.contains(&a) {
        let msg =
            format!("NumberOfActorsAllowed: {} is not in its owner's SpawnedActors (not counted)", w.names.str(cls));
        warn_text(w, &msg);
        return;
    }
    // The actor itself counts while a destroy asked in its BeginPlay waits (the blast-box loop's, the class's own
    // part).
    // (the walk reads alone: the count and the first of the filtered list)
    let (mut same, mut first) = (0u32, None);
    let list = &w.fighter(f).items.spawned_actors;
    for &o in list {
        if o == a
            || (!actor_dead(w, o) && !actor_unlisted(w, o) && actor_cls(w, o).is_some_and(|oc| is_class(w, f, oc, cls)))
        {
            same += 1;
            first.get_or_insert(o);
        }
    }
    if crate::real(same) > max {
        k2_destroy_actor(w, first.expect("one counted"));
    }
}

/// `isClass`: `actorIsA` in the owner's data; a class with no chain there is warned and taken as no.
fn is_class(w: &World, f: FighterId, cls: Name, of: Name) -> bool {
    let is = actor_is_a(w, ActorRef::Fighter(f), cls, of);
    if is.is_none() {
        let msg = format!(
            "NumberOfActorsAllowed: no class chain for {} in the owner's data (taken as no {})",
            w.names.str(cls),
            w.names.str(of)
        );
        warn_text(w, &msg);
    }
    is == Some(true)
}

/// `k2DestroyActor`: K2_DestroyActor on a spawned actor of any of the World's lists: a projectile's or an item's
/// destroy, an accelerator's (its EndPlay ends the loop), a World.actors class's own (ActorKind.destroy), else
/// marked dead (its end in the world's list pass).
pub fn k2_destroy_actor(w: &mut World, x: ActorRef) {
    if actor_dead(w, x) {
        return;
    }
    match x {
        ActorRef::Projectile(_) | ActorRef::Item(_) => destroy_spawned(w, x),
        ActorRef::Accelerator(ac) => crate::chars::morty::destroy_accelerator(w, ac),
        ActorRef::Actor(id) => {
            let cls = w.actors.actors.get(id).expect("a live actor").cls;
            match world_hooks(w, cls).and_then(|k| k.destroy) {
                Some(d) => d(w, id),
                None => w.actors.actors.get_mut(id).expect("a live actor").dead = true,
            }
        }
        ActorRef::Marker(m) => w.actors.markers.get_mut(m).expect("a live marker").dead = true,
        ActorRef::Ship(s) => w.actors.ships.get_mut(s).expect("a live ship").dead = true,
        ActorRef::Mortar(m) => w.actors.mortars.get_mut(m).expect("a live mortar").dead = true,
        ActorRef::AimPunch(a) => w.actors.aim_punches.get_mut(a).expect("a live aim punch").dead = true,
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => {
            unreachable!("K2_DestroyActor of a spawned actor")
        }
    }
}

/// An actor's pending DestroySpawnedActor Delay (`doom`), where its family keeps one.
fn doom_of(w: &World, a: ActorRef) -> Option<i32> {
    match a {
        ActorRef::Projectile(i) => w.actors.projectiles.get(i).and_then(|x| x.doom),
        ActorRef::Item(i) => w.actors.items.get(i).and_then(|x| x.doom),
        ActorRef::Accelerator(i) => w.actors.accelerators.get(i).and_then(|x| x.doom),
        ActorRef::Actor(i) => actor_doom(w, i),
        _ => None,
    }
}

/// The write of an actor's `doom`.
fn set_doom(w: &mut World, a: ActorRef, d: Option<i32>) {
    match a {
        ActorRef::Projectile(i) => pm(w, i).doom = d,
        ActorRef::Item(i) => w.actors.items.get_mut(i).expect("a live item").doom = d,
        ActorRef::Accelerator(i) => w.actors.accelerators.get_mut(i).expect("a live accelerator").doom = d,
        ActorRef::Actor(i) => set_actor_doom(w, i, d),
        _ => unreachable!("a doom of a typed list's actor"),
    }
}

/// `destroySpawnedActor`: SpawnedActorParent DestroySpawnedActor(false) behind a Delay of `frames` (0: at once) in the
/// actor's graph: the parent's OnHitAnything @18379 (DestroyedOnHit, Delay(0.033) -> resume@15) and its Blocked
/// @18642. A Delay asked again while one is pending starts nothing (execDelay 142f9baa0: the action exists). The
/// pending count is the actor's `doom`, taken in the actor's tick (a projectile's behind its move, every one's ahead
/// of the class's own tick). DestroySpawnedActor: DestroyAfterHitPause while the actor's Comp_HitPause receives a
/// pause and one runs (a projectile's `hit_pause`), else ReleaseToPool with CanBePooled (the port has
/// no pool), else K2_DestroyActor (`k2_destroy_actor`).
/// Not ported: a pending Delay on an actor of World's typed lists (markers, ships, aim punches, ...: warned).
pub fn destroy_spawned_actor(w: &mut World, a: ActorRef, frames: Real) {
    if actor_dead(w, a) {
        return;
    }
    if frames <= 0.0 {
        // DestroySpawnedActor(false) @0-220 in the actor's own pause sets DestroyAfterHitPause; BeginPlay binds
        // BPI_HitPauseEnd to the component's OnHitPauseEnded (@11394 / @11417), broadcast as the pause ends (@806),
        // whose @5176 calls DestroySpawnedActor(false) again (`projectiles_pass`).
        if crate::combat::spawned_hits::actor_hit_pause_remaining(w, a) > 0.0
            && crate::combat::spawned_hits::receives_hit_pause(w, a)
        {
            if let ActorRef::Projectile(id) = a
                && let Some(p) = w.actors.projectiles.get_mut(id)
            {
                p.destroy_after_pause = true;
            }
            return;
        }
        k2_destroy_actor(w, a);
        return;
    }
    if !matches!(a, ActorRef::Projectile(_) | ActorRef::Item(_) | ActorRef::Accelerator(_) | ActorRef::Actor(_)) {
        let cls = actor_cls(w, a).map_or("?", |c| w.names.str(c)).to_owned();
        warn_text(w, &format!("destroySpawnedActor: a Delay on {cls} (an actor of a typed list) is not ported"));
        return;
    }
    if doom_of(w, a).is_some_and(|d| d > 0) {
        return;
    }
    // (`latentFrames` gives a whole count of frames)
    set_doom(w, a, Some(math::to_int32(frames)));
}

/// `doomTick`: the actor's tick: a pending `destroy_spawned_actor` Delay counts this frame. True: the actor was
/// destroyed.
pub fn doom_tick(w: &mut World, a: ActorRef) -> bool {
    let Some(d) = doom_of(w, a) else { return false };
    if d <= 0 {
        return false;
    }
    set_doom(w, a, Some(d - 1));
    if d - 1 > 0 {
        return false;
    }
    match a {
        ActorRef::Projectile(pr) => destroy_projectile(w, pr, true),
        _ => k2_destroy_actor(w, a),
    }
    true
}

/// `projectileBounce`: OnProjectileBounce's broadcast inside a projectile's move, for the handlers its class's graph
/// binds to the delegate (ProjectileHooks::bounce_impact). True: K2_DestroyActor.
pub fn projectile_bounce(w: &mut World, pr: ProjectileId, nx: Real, ny: Real, ivx: Real, ivy: Real) -> bool {
    match proj_hooks(w, pm_ref(w, pr).cls).and_then(|k| k.bounce_impact) {
        Some(b) => b(w, pr, nx, ny, ivx, ivy),
        None => false,
    }
}

/// `addProjectile`: a projectile an actor's graph spawns through its owner's Comp_Spawner (the caller has made it and
/// put it in the list), moved out of what its root blocks (`push_out_root`), tested where it spawns
/// (`spawn_overlap`). `begin`: what the class's ReceiveBeginPlay sends (its sounds), run once the actor has its number
/// and before the test, as FinishSpawningActor @9396 comes before BPI_UpdateHitbox @12653.
pub fn add_projectile(w: &mut World, pr: ProjectileId, begin: Option<fn(&mut World, ProjectileId)>) {
    // PostActorConstruction's FindTeleportSpot, ahead of BeginPlay
    push_out_root(w, pr);
    pm(w, pr).seq = next_seq(w);
    let owner = pm_ref(w, pr).owner.expect("a projectile a fighter's Comp_Spawner made");
    // the BeginPlay copy
    let c = elem::instigator_charge(w, owner);
    elem::receive_charge(w, ActorRef::Projectile(pr), c);
    w.fighter_mut(owner).items.spawned_actors.push(ActorRef::Projectile(pr));
    let doomed = parent_begin_play(w, pr);
    if let Some(b) = begin {
        b(w, pr);
    }
    if end_doomed(w, pr, doomed) {
        return;
    }
    spawn_begins(w, pr);
    if pm_ref(w, pr).dead {
        return;
    }
    after_spawned_actor(w, owner, ActorRef::Projectile(pr));
    spawn_overlap(w, pr);
}

/// `compSpawnProjectile`: Comp_Spawner.CompSpawnActor of a registered projectile class with a world transform (the
/// WorldTransforms argument true: the transform is taken as given, SelectTransform @3326 / @10425), from another
/// actor's graph: SpawnedActorParent's death spawn (@16684), Firebolt's own. The owner's ability trigger
/// (actorSpawnAbility), a plain Projectile at (x, y), then `add_projectile`. `dir`: the world direction (UE X, Y,
/// Z), times the speed.
/// Unverified: the direction is mirrored by the owner's facing as a notify's spawn (HandleActorPostSpawn's
/// world-transform branch is not read for it).
/// `attack_id`: the call's AttackID argument when the graph passes one, else (or 0) the owner's count
/// (HandleActorPostSpawn @2444-2747: the spawner's PlayerState BPI_GetPlayerAttackCount).
#[allow(clippy::too_many_arguments, reason = "the call keeps the build function's parameters")]
pub fn comp_spawn_projectile(
    w: &mut World,
    f: FighterId,
    cls: Name,
    x: Real,
    y: Real,
    dir: [Option<Real>; 3],
    speed: Real,
    max: Real,
    attack_id: Option<i32>,
) -> Option<ProjectileId> {
    let attack_id = match attack_id {
        None | Some(0) => w.fighter(f).decay.attack_count,
        Some(a) => a,
    };
    let ap = actor_def(w, ActorRef::Fighter(f), cls).map(|a| a.p.clone());
    let kind = proj_hooks(w, cls);
    let class = entry(w, cls).map(|e| e.class);
    let (Some(ap), Some(kind), Some(ActorClass::Projectile(class))) = (ap, kind, class) else {
        let msg = format!("spawned actor not implemented ({})", w.names.str(cls));
        warn_text(w, &msg);
        return None;
    };
    crate::fighter::control::actor_spawn_ability(w, f, &ap);
    let mut p = Props::new();
    let mut d = Props::new();
    for (k, v) in ["X", "Y", "Z"].into_iter().zip(dir) {
        if let Some(v) = v {
            d.insert(k.into(), Value::Num(f64::from(v)));
        }
    }
    p.insert("InitialMovementDirection".into(), Value::Obj(d));
    p.insert("InitialSpeed".into(), Value::Num(f64::from(speed)));
    p.insert("MaxSpeed".into(), Value::Num(f64::from(max)));
    // The request of this call: the literal `p` (InitialMovementDirection, InitialSpeed, MaxSpeed), the origin, no
    // notify (a request names one: its index is the empty montage's, which a class's make does not read for a literal
    // `p`), no charge.
    // Unverified: a class's `make` and `make_opts` of a CompSpawnActor with a world transform get the request as a
    // notify's spawn does.
    let req = SpawnRequest {
        cls,
        n: crate::events::NotifyRef { montage: Name::NONE, index: 0 },
        attack_id,
        foreign: Foreign::default(),
        copy: None,
        charge: None,
        aim: None,
        props: Some(std::sync::Arc::new(p.clone())),
        instigator: None,
        origin: None,
    };
    let opts = kind.make_opts.map_or_else(MakeOpts::default, |mo| mo(w, f, &req));
    let mut made = make_projectile(w, cls, class, Some(f), attack_id, &p, Some(V2 { x, y }), opts);
    // The rotation is SetTransforms' as given (SelectTransform with WorldTransforms true, Comp_Spawner @3326 /
    // @9323), not the owner's facing: an identity rotation is yaw 0, which K2_GetActorRotation reads back as no
    // ComingFromLeft (sim yawX -1).
    // Unverified: every caller's transform is taken as an identity rotation; the ones read (C018_Platform @14427,
    // Buff_Ice, Pawn_C017, the death spawn's classes) are, the rest of the callers' graphs are not read for it.
    made.spawn_side = Some(-1.0);
    if let Some(make) = kind.make {
        made.ext = make(w, f, &req);
    }
    if let Some(construct) = kind.construct {
        construct(w, &mut made);
    }
    let pr = w.actors.projectiles.insert(made);
    add_projectile(w, pr, kind.begin);
    Some(pr)
}

/// `pushOutRoot`: UWorld::FindTeleportSpot at a Comp_Spawner spawn (BeginDeferredActorSpawnFromClass with collision
/// handling 2, AdjustIfPossibleButAlwaysSpawn, @9054 / @9531), called by every route of a Comp_Spawner projectile.
/// The root Capsule at PostActorConstruction, ahead of BeginPlay: at the spawn transform's scale (`spawn_root_scale`),
/// upright, out of what its profile blocks (`root_blockers`). An attached spawn is not moved: Comp_Spawner attaches
/// it and sets its transform after FinishSpawningActor (@10335-10581, no sweep), undoing the push-out.
/// Unverified: the spawn transform's rotation (a notify's Transforms.Rotation; the capsule's axis) is taken as
/// upright (Projectile's constructor does not read it); a pooled reuse (InitFromPool @3399, no FindTeleportSpot) is
/// not reachable for a ported projectile (no CanBePooled class).
pub fn push_out_root(w: &mut World, pr: ProjectileId) {
    if pm_ref(w, pr).attached {
        return;
    }
    let (cls, x, y) = (pm_ref(w, pr).cls, pm_ref(w, pr).x, pm_ref(w, pr).y);
    // (the push-out runs ahead of the list; a caller that listed the projectile first has its own boxes out of its
    // blockers here)
    let solids = pm(w, pr).solids.take();
    let at = push_out_spot(w, ActorRef::Projectile(pr), cls, projectile::spawn_root_scale(w, pr), x, y);
    pm(w, pr).solids = solids;
    let Some((x, y)) = at else { return };
    let p = pm(w, pr);
    (p.x, p.prev_x, p.y, p.prev_y) = (x, x, y, y);
}

/// `pushOutRoot`'s spot: where FindTeleportSpot moves a root of class `cls` (its data through `own`) at (x, y)
/// with the root scale `scale`; `None` for nothing to test, or clear where it spawned (nothing is written).
fn push_out_spot(w: &mut World, own: ActorRef, cls: Name, scale: Real, x: Real, y: Real) -> Option<(Real, Real)> {
    let cap =
        actor_def(w, own, cls).and_then(|a| obj(&a.components, "Capsule")).map(crate::world::spawn_spot::capsule_c);
    let boxes = root_blockers_of(w, own, cls, name!("spawn push-out"))?;
    let shape = crate::world::spawn_spot::capsule_shape(cap, scale, 0.0, 1.0);
    let at = crate::world::spawn_spot::find_teleport_spot_shape(boxes.view(), x, y, &shape);
    if at.x == x && at.y == y { None } else { Some((at.x, at.y)) }
}

/// `rootBlockers`: the boxes a projectile's root Capsule blocks, for FindTeleportSpot (`push_out_root`, k2TeleportTo):
/// `None` for a class without a Capsule or with no blocking collision (nothing to test).
pub fn root_blockers(w: &mut World, pr: ProjectileId, what: Name) -> Option<StageShapes> {
    let cls = pm_ref(w, pr).cls;
    root_blockers_of(w, ActorRef::Projectile(pr), cls, what)
}

/// [`root_blockers`] of a root of class `cls`, its data through `own`.
fn root_blockers_of(w: &mut World, own: ActorRef, cls: Name, what: Name) -> Option<StageShapes> {
    let data = w.data.clone();
    let cap = crate::fighter::common::actor_data(w, &data, own, cls).and_then(|a| obj(&a.components, "Capsule"))?;
    root_body_blockers(w, obj(cap, "BodyInstance"), cls, what)
}

/// `rootBodyBlockers`: the boxes a root body (a component template's BodyInstance) blocks in FindTeleportSpot's
/// overlap, with the polygon stage solids by the same rules as the solids' boxes: the overlap runs on the root's
/// object channel and keeps what blocks both ways.
/// - The stage block (profile Floor: WorldStatic, blocking the default-Block channels and Item / WorldStaticDetection)
///   where the root blocks WorldStatic (the channel default, Block, unless its responses say otherwise).
/// - The stage platforms and the spawned platform pieces (`World::blockers` of the root's object channel: profile
///   Platform blocks PhysicsBody, Item and WorldStaticDetection) where the root blocks Platform.
///
/// Profiles: ProjectileNavPlatform (PhysicsBody), ProjectileNavNoPlatform (Platform Ignore), Item (Item),
/// PortalProjectile (QueryOnly, WorldDynamic, Platform Overlap), OverlapAllDynamic (QueryOnly, WorldDynamic,
/// WorldStatic Overlap, Platform at its channel default Block: the platforms of `blockers(Dynamic)` alone), Custom
/// (the template's ObjectType, else the shape's WorldDynamic, and its ResponseArray against the default container;
/// none saved: OverlapAllDynamic's, UpdateResponseContainerFromArray 142c82ae0). A template with no BodyInstance
/// values is the shape constructor's profile, OverlapAllDynamic (UShapeComponent ctor 142984280). NoCollision and a
/// CollisionEnabled NoCollision give `None`. Another profile, or a root that blocks Platform on an object channel no
/// BlockKind covers, is warned under `what` (`None`, or the stage block alone).
/// Deviation: only a Block response of the root is counted; FindTeleportSpot's overlap query may count an Overlap
/// response too (ComputeEncroachmentForComponent 142b3cce0, not read). Deviation: the Custom branch reads the root's
/// side only (the stage block's response to the root's object channel is not tested).
pub fn root_body_blockers(w: &mut World, body: Option<&Props>, cls: Name, what: Name) -> Option<StageShapes> {
    let s = |v: Option<&Value>| v.and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
    if s(body.and_then(|b| b.get("CollisionEnabled"))).as_deref() == Some("ECollisionEnabled::NoCollision") {
        return None;
    }
    let prof = match s(body.and_then(|b| b.get("CollisionProfileName"))) {
        Some(p) => p,
        None => {
            if body.is_some_and(|b| truthy(b, "ObjectType") || truthy(b, "CollisionResponses")) {
                let msg = format!(
                    "{} not implemented for a body with responses and no profile ({})",
                    w.names.str(what),
                    w.names.str(cls)
                );
                warn_text(w, &msg);
                return None;
            }
            "OverlapAllDynamic".to_owned()
        }
    };
    if prof == "NoCollision" {
        return None;
    }
    let (ch, ws, plat): (String, bool, bool) = match prof.as_str() {
        "OverlapAllDynamic" => ("ECC_WorldDynamic".into(), false, true),
        "ProjectileNavPlatform" | "ProjectileNavNoPlatform" => {
            ("ECC_PhysicsBody".into(), true, prof == "ProjectileNavPlatform")
        }
        "Item" => ("ECC_GameTraceChannel12".into(), true, true),
        "PortalProjectile" => ("ECC_WorldDynamic".into(), true, false),
        "Custom" => {
            // The root is a shape (the Capsule): an ObjectType it does not save is the shape's WorldDynamic; with no
            // saved responses it keeps OverlapAllDynamic's (WorldStatic Overlap, Platform at its default Block), with
            // saved ones an unlisted channel is at the default container's Block (WorldStatic, and Platform's config
            // default).
            let ch = s(body.and_then(|b| b.get("ObjectType"))).unwrap_or_else(|| "ECC_WorldDynamic".into());
            let list = body
                .and_then(|b| obj(b, "CollisionResponses"))
                .and_then(|c| c.get("ResponseArray"))
                .and_then(Value::as_arr);
            let resp = |c: &str| {
                list.and_then(|l| {
                    l.iter()
                        .filter_map(Value::as_obj)
                        .find(|r| r.get("Channel").and_then(Value::as_str) == Some(c))
                        .and_then(|r| r.get("Response").and_then(Value::as_str))
                })
                .unwrap_or("ECR_Block")
            };
            let ws = list.is_some() && resp("WorldStatic") == "ECR_Block";
            (ch, ws, resp("Platform") == "ECR_Block")
        }
        _ => {
            let msg = format!("{} not implemented for the profile {prof} ({})", w.names.str(what), w.names.str(cls));
            warn_text(w, &msg);
            return None;
        }
    };
    // The stage's solids of object type WorldStatic go with the block (`static_solids`, by their response to the
    // root's object channel), the one-way ones (Platform) with the platforms; `World::blockers` holds both.
    let flag = match ch.as_str() {
        "ECC_PhysicsBody" => SOLID_NAV,
        "ECC_GameTraceChannel12" => SOLID_ITEM,
        "ECC_GameTraceChannel15" => SOLID_DETECT,
        _ => QUERY_UNKNOWN,
    };
    let kind = match ch.as_str() {
        "ECC_PhysicsBody" => Some(BlockKind::Nav),
        "ECC_GameTraceChannel12" => Some(BlockKind::Item),
        "ECC_GameTraceChannel15" => Some(BlockKind::Detect),
        "ECC_WorldDynamic" => Some(BlockKind::Dynamic),
        _ => None,
    };
    let mut out = StageShapes { boxes: if ws { vec![w.stage.ground] } else { Vec::new() }, polys: Vec::new() };
    if plat {
        match kind {
            Some(k) => {
                // (a root that does not block WorldStatic leaves out the stage solids of WorldStatic)
                let all = w.blocker_shapes(k);
                if ws || !w.may_hold_static_solids() {
                    out.boxes.extend(all.boxes);
                    out.polys.extend(all.polys);
                } else {
                    let mut st = Vec::new();
                    crate::fighter::traces::statics_among(w, &all.boxes, &mut st);
                    out.boxes.extend(all.boxes.into_iter().filter(|g| !st.contains(g)));
                    out.polys.extend(all.polys.into_iter().filter(|q| !q.is_static));
                }
            }
            None => {
                let msg = format!(
                    "{}: the platforms of the object channel {ch} not implemented ({})",
                    w.names.str(what),
                    w.names.str(cls)
                );
                warn_text(w, &msg);
            }
        }
    }
    // (the stage solids of object type WorldStatic are in `blockers` of a platform-blocking root already)
    if ws && !(plat && kind.is_some_and(|k| k != BlockKind::Dynamic)) && w.may_hold_static_solids() {
        crate::world::stage_query::static_solid_shapes(w, flag, what, &mut out);
    }
    if out.boxes.is_empty() && out.polys.is_empty() { None } else { Some(out) }
}

/// `beginBlastBox`: SpawnedActorParent's BeginPlay blast-box loop on a new projectile (`blast_box_loop`): its bounds
/// stored, the ForceSpawnPositionInsideBlastBox clamp, or @15066 DestroySpawnedActor(true). True: it is to be
/// destroyed. The loop is the parent's call inside the child's ReceiveBeginPlay, whose script goes on after it: the
/// class's begin runs, then `end_doomed` destroys it.
fn begin_blast_box(w: &mut World, pr: ProjectileId) -> bool {
    let sb = crate::world::blast::spawn_box(w);
    let p = actor_p(w, ActorRef::Projectile(pr));
    let force = truthy(p, "ForceSpawnPositionInsideBlastBox");
    let destroy = truthy(p, "DestroyOnExitedArena?");
    let Projectile { x, y, prev_x, prev_y, blast_bounds, .. } = pm(w, pr);
    crate::world::blast::blast_box_loop(&sb, x, y, Some((prev_x, prev_y)), force, blast_bounds, destroy)
}

/// `parentBeginPlay`: a new projectile's begin up to its class's own part, on every route, behind its number and its
/// lists: its components' BeginPlay (Comp_Pawn_PassiveBuffManager: the passive buffs), then SpawnedActorParent's
/// ReceiveBeginPlay as its ubergraph runs it (@8847: the pushed flows pop @9203 then @9114):
/// GrantBuffsFromInstigator @9233, the blast-box loop, NumberOfActorsAllowed (`actors_allowed`). True: the loop
/// doomed it (`end_doomed`, after the class's begin).
pub fn parent_begin_play(w: &mut World, pr: ProjectileId) -> bool {
    elem::spawn_passive_buffs(w, ActorRef::Projectile(pr));
    // With no owner (an actor the world spawned): no Instigator to grant from, and no owner's Comp_Spawner, so the
    // NumberOfActorsAllowed flow pops before the list (SpawnedActorParent @5646-5746).
    let owner = pm_ref(w, pr).owner;
    if let Some(o) = owner {
        elem::grant_spawned_buffs(w, ActorRef::Projectile(pr), o);
    }
    let doomed = begin_blast_box(w, pr);
    if let Some(o) = owner {
        actors_allowed(w, ActorRef::Projectile(pr), o);
    }
    doomed
}

/// `endDoomed`: the destroy of an actor its BeginPlay loop doomed, after the class's begin. True: destroyed.
/// Unverified: UE defers a destroy asked during BeginPlay to its end (bActorWantsDestroyDuringBeginPlay, stock 4.26
/// source, not read in the exe); Comp_Spawner's AfterSpawnedActor and the spawn-place test are taken as not run.
pub fn end_doomed(w: &mut World, pr: ProjectileId, doomed: bool) -> bool {
    if !doomed {
        return false;
    }
    if !pm_ref(w, pr).dead {
        destroy_projectile(w, pr, false);
    }
    true
}

/// `listProjectile`: a projectile another projectile's or actor's graph spawns in its own tick (a splitter's
/// follow-ups, the mortar's split grenades; both through the instigator's Comp_Spawner.CompSpawnActor), made and put
/// in the list by the caller: numbered, its class's own BeginPlay and the test at its spawn place. It does not join
/// the owner's SpawnedActors.
pub fn list_projectile(w: &mut World, pr: ProjectileId) {
    // PostActorConstruction's FindTeleportSpot, ahead of BeginPlay
    push_out_root(w, pr);
    pm(w, pr).seq = next_seq(w);
    let owner = pm_ref(w, pr).owner.expect("a projectile a fighter's Comp_Spawner made");
    let c = elem::instigator_charge(w, owner);
    elem::receive_charge(w, ActorRef::Projectile(pr), c);
    let doomed = parent_begin_play(w, pr);
    if let Some(b) = proj_hooks(w, pm_ref(w, pr).cls).and_then(|k| k.begin) {
        b(w, pr);
    }
    if end_doomed(w, pr, doomed) {
        return;
    }
    spawn_begins(w, pr);
    if pm_ref(w, pr).dead {
        return;
    }
    after_spawned_actor(w, owner, ActorRef::Projectile(pr));
    spawn_overlap(w, pr);
}

/// `spawnWorldProjectile`: BeginDeferredActorSpawnFromClass + FinishSpawningActor of a projectile class by a world
/// actor with itself as the owner (MapRandomActorSpawnArea SpawnCenterActor @3301 / @3745), put in the list by the
/// caller: no fighter owner and no Instigator, so no charge copy, no owner's SpawnedActors and no
/// NumberOfActorsAllowed, no Comp_Spawner (no AfterSpawnedActor). Collision handling 0 there: AlwaysSpawn with no
/// FindTeleportSpot. The components' and the class's BeginPlay, then the begin overlaps of FinishSpawningActor's
/// UpdateOverlaps. No BPI_UpdateHitbox call follows (DoesImplementInterface(null) @3783 is false), so no spawn-place
/// hit test.
pub fn spawn_world_projectile(w: &mut World, pr: ProjectileId) {
    pm(w, pr).seq = next_seq(w);
    let doomed = parent_begin_play(w, pr);
    if let Some(b) = proj_hooks(w, pm_ref(w, pr).cls).and_then(|k| k.begin) {
        b(w, pr);
    }
    if end_doomed(w, pr, doomed) {
        return;
    }
    spawn_begins(w, pr);
}

/// `spawnBegins`: the begin overlaps on the new actor's HurtBox in FinishSpawningActor's UpdateOverlaps, ahead of
/// AfterSpawnedActor.
pub fn spawn_begins(w: &mut World, pr: ProjectileId) {
    crate::combat::phases::spawn_hurt_begins(w, pr);
}

/// `spawnOverlap`: a spawned actor's hit box is on, and tested, where the actor spawns. Comp_Spawner
/// HandleActorPostSpawn, all of it inside the CompSpawnActor call: FinishSpawningActor @9396 (BeginPlay), the attach
/// and the transform, BPI_SetInitialMovement @8407, AfterSpawnedActor @12116, BPI_UpdateHitbox @12653 ->
/// SpawnedActorParent @4657 -> @18336 -> @15092: with HitBoxOn? @8122 -> Comp_Pawn_HitBoxHandler.HitBoxesOn @8531 ->
/// ActivateHitbox per tagged box: SetCollisionEnabled(QueryAndPhysics) @16817 updates the box's overlaps at once, at
/// the place it has then (142d02cc0 / 142cfe1e0): what it already overlaps is dealt inside the spawner's call, before
/// the actor's first move (HitEachActorOnlyOnce keeps it from hitting again where that move ends).
/// Deviation: the test runs behind every fighter's tick of the frame, with the poses of that point (in the build it
/// runs inside the owner's mesh tick). What else the box begins to overlap there is taken behind the fighters
/// (`spawn_place_hits`).
/// Deviation: a class without HitEachActorOnlyOnce (no ported projectile) would hit the same fighter again where the
/// move ends (the port tests an overlap, not its begin): warned.
pub fn spawn_overlap(w: &mut World, pr: ProjectileId) {
    if hitbox_active(pm_ref(w, pr)) {
        if !pm_ref(w, pr).hit.hit_each_actor_only_once.unwrap_or(false) {
            let cls = w.names.str(pm_ref(w, pr).cls).to_owned();
            let msg = format!("spawn-place hit test: no HitEachActorOnlyOnce, the end place may hit again ({cls})");
            warn_text(w, &msg);
        }
        let mut pending = Vec::new();
        let all = crate::world::pawns::all_list(w);
        for &vic in &all {
            crate::combat::phases::projectile_test(w, pr, vic, &mut pending, false);
        }
        crate::world::pawns::put_fighters(w, all);
        crate::combat::phases::run_pending_hits(w, pending);
    }
    if !pm_ref(w, pr).dead {
        crate::combat::phases::spawn_place_hits(w, pr);
    }
}

/// `SpawnShape`: the shape a spawn spot tests.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SpawnShape {
    Sphere {
        r: crate::Real,
    },
    /// `hh`: the half height, hemispheres included.
    Capsule {
        r: crate::Real,
        hh: crate::Real,
        ux: crate::Real,
        uy: crate::Real,
    },
    /// Half extents (play-plane side, UE local Z).
    Box {
        hx: crate::Real,
        hz: crate::Real,
        ux: crate::Real,
        uy: crate::Real,
    },
}

// ---- the spawn spots (`spawn_spot.rs`)

pub use super::spawn_spot::{
    box_shape, capsule_c, capsule_shape, find_teleport_spot, find_teleport_spot_at, find_teleport_spot_shape,
    spawn_place, spawn_spot,
};

// ---- the call surface's argument and return types

/// What `spawnSpot` takes or gives (a struct literal).
#[derive(Clone, Debug)]
pub struct SpawnSpotR {
    pub cls: Name,
    pub p: Props,
}

/// What `spawnSpot` takes or gives (a struct literal).
#[derive(Clone, Debug)]
pub struct SpawnSpotA {
    pub components: Props,
}

/// What `capsuleShape` takes or gives (a struct literal).
#[derive(Clone, Debug)]
pub struct CapsuleShapeC {
    pub capsule_radius: Option<Real>,
    pub capsule_half_height: Option<Real>,
}

/// What `findTeleportSpotAt` takes or gives (a struct literal).
#[derive(Clone, Debug)]
pub struct FindTeleportSpotAtOut {
    pub x: Real,
    pub y: Real,
    pub ok: bool,
}

/// `findTeleportSpotShape` and `findTeleportSpotAt` over the boxes `root_body_blockers` gives.
pub use super::spawn_spot::{
    find_teleport_spot_at as find_teleport_spot_at_in, find_teleport_spot_shape as find_teleport_spot_shape_in,
};
