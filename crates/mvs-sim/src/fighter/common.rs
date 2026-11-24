//! What every fighter module reads.
//!
//! What is computed once from a character's data (the knock-back shapes,
//! the default terminal velocity, an anim set's extras): here the import computed them (`CharData::def_extra`), and
//! the readers read them. The match registry (keyed by the fighters list) is the World
//! the caller passes.

use crate::Real;
use crate::actors::ActorRef;
use crate::consts as C;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::anim::SetRef;
use crate::fighter::control::DodgeBy;
use crate::math;
use crate::names::Name;
use crate::world::World;
use mvs_data::CharData;
use mvs_data::def::{CharDef, CrouchRun, EdgeIdle, SetExtra};
use mvs_data::fighter::ActorDef;
use mvs_data::value::{Props, Value};

/// `KEY_STRIDE`: an attack state's key is the montage instance x `KEY_STRIDE` + the notify's index (instance 0 is
/// the DefaultGroup attack montage; a grouped instance played as an attack from 1), so the instance is
/// `floor(key / KEY_STRIDE)`. No montage has more than 65 notifies.
pub const KEY_STRIDE: Real = 4096.0;

/// `DEG`.
pub const DEG: Real = crate::math::ue::RAD_TO_DEG;

/// `FALL_RATE`: AirState Fall's sequence player PlayRate.
pub const FALL_RATE: Real = 1.5;

/// The fighters a fighter's `peers` list holds: the World's pawns, else its players (`World.all`; every fighter is
/// handed the same list, replaced when a pawn joins or leaves).
pub fn peers(w: &World) -> &[FighterId] {
    w.pawns.as_deref().unwrap_or(&w.players)
}

/// Comp_Actor_Destructible.GetAllies of fighter `f`: its Allies, kept as the component keeps it. BeginPlay fills it
/// with every other player of its team (UpdateAllyListServer over GetAllPlayers @132-558); a knocked-out ally leaves
/// it (OnPlayerKnockedOut -> RemoveItem @2338) and comes back last at its respawn (OnPlayerRespawned -> AddUnique
/// @2269). Players only: a minion is never in it. Here: the live players of its team, by the tick each last joined
/// (`Team::joined`), then in player order.
/// Unverified: GetAllPlayers' order (the first fill) is taken as the player order; it is not decoded. With one ally
/// (the beta's 2v2) the order never shows. The list's ends are [`first_ally`] and [`last_ally`].
pub fn allies(w: &World, f: FighterId) -> impl Iterator<Item = FighterId> + '_ {
    let team = w.fighter(f).team.team;
    peers(w).iter().copied().filter(move |&o| {
        let x = w.fighter(o);
        o != f && x.team.team == team && x.life.dead <= 0.0 && !x.life.is_minion
    })
}

/// The first of fighter `f`'s Allies ([`allies`]): Array_Get(Allies, 0). Of equal ticks, the first in player order.
pub fn first_ally(w: &World, f: FighterId) -> Option<FighterId> {
    allies(w, f).min_by_key(|&o| w.fighter(o).team.joined)
}

/// The last of fighter `f`'s Allies ([`allies`]): the one that joined last; of equal ticks, the last in player order.
pub fn last_ally(w: &World, f: FighterId) -> Option<FighterId> {
    allies(w, f).max_by_key(|&o| w.fighter(o).team.joined)
}

/// The character data of fighter `f` (`f.data`).
pub fn data_of(w: &World, f: FighterId) -> &CharData {
    char_data(w, w.fighter(f).ch)
}

/// The character data of key `ch`.
pub fn char_data(w: &World, ch: Name) -> &CharData {
    let key = w.names.str(ch);
    w.data.chars.get(key).unwrap_or_else(|| panic!("no character data for {key}"))
}

/// A string of the data (a montage, a sub mesh, an anim) as the match's [`Name`] (interned at the bind: the step
/// never interns); `Name::NONE` for one the bind did not intern. A literal of the code is `crate::name!`.
pub fn lit(w: &World, s: &str) -> Name {
    w.names.get(s).unwrap_or(Name::NONE)
}

/// `s.toLowerCase()` handed to `f`, on the stack for an ASCII string of up to 64 bytes (every tag and name of the
/// data); a longer or non-ASCII one takes the heap.
pub fn with_lower<R>(s: &str, f: impl FnOnce(&str) -> R) -> R {
    let mut buf = [0u8; 64];
    if s.is_ascii() && s.len() <= buf.len() {
        let b = &mut buf[..s.len()];
        b.copy_from_slice(s.as_bytes());
        b.make_ascii_lowercase();
        f(std::str::from_utf8(b).expect("ASCII stays UTF-8"))
    } else {
        f(&s.to_lowercase())
    }
}

// ---- the call surface of common

/// What `capsuleOut` gives: the move's length (-1: no overlap) and its direction (`OUT_N`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CapsuleOut {
    pub d: Real,
    pub nx: Real,
    pub ny: Real,
}

/// `capsuleOut`: the least move that takes an upright capsule (centre (x, y), radius r,
/// the axis reaching hs up and down) out of a box. With the axis outside the box it is along the line of the two
/// nearest points, by the radius less their distance; with the axis through the box the least of the four ways
/// along the box's axes, plus the radius. The direction is unset (0, 0) without an overlap (`OUT_N` is left
/// as it was, which no caller reads then).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn capsule_out(
    left: Real,
    right: Real,
    bottom: Real,
    top: Real,
    x: Real,
    y: Real,
    r: Real,
    hs: Real,
) -> CapsuleOut {
    let y0 = y - hs;
    let y1 = y + hs;
    if x >= left && x <= right && y1 >= bottom && y0 <= top {
        let (mut pen, mut nx, mut ny) = (right - x, 1.0, 0.0);
        if x - left < pen {
            pen = x - left;
            nx = -1.0;
        }
        if top - y0 < pen {
            pen = top - y0;
            nx = 0.0;
            ny = 1.0;
        }
        if y1 - bottom < pen {
            pen = y1 - bottom;
            nx = 0.0;
            ny = -1.0;
        }
        return CapsuleOut { d: pen + r, nx, ny };
    }
    let cx = math::max(left, math::min(x, right));
    let py = math::max(y0, math::min(y1, math::max(bottom, math::min(y, top))));
    let cy = math::max(bottom, math::min(py, top));
    let vx = x - cx;
    let vy = py - cy;
    let d = (vx * vx + vy * vy).sqrt();
    if d >= r {
        return CapsuleOut { d: -1.0, nx: 0.0, ny: 0.0 };
    }
    CapsuleOut { d: r - d, nx: if d > 0.0 { vx / d } else { 0.0 }, ny: if d > 0.0 { vy / d } else { 1.0 } }
}

/// `latentFrames`: the frames a latent Delay(s) counts down (TimeRemaining, a float,
/// less DeltaSeconds each frame, fired once <= 0, at least one frame).
pub fn latent_frames(s: Real) -> Real {
    let mut tr = s;
    let mut n = 0.0;
    loop {
        tr -= C::DELTA_SECONDS;
        n += 1.0;
        if tr <= 0.0 || tr.is_nan() {
            break;
        }
    }
    n
}

/// `timerFrames`: a world timer of rate T set outside the timer tick is pending until
/// that frame's timer tick, which starts it without counting the frame: one more tick than `timerTicks`.
pub fn timer_frames(t: Option<Real>) -> Real {
    crate::combat::hit::timer_ticks(t) + 1.0
}

/// `warnOnce` of a message the code builds: written to stderr once per World
/// (`World::warned`).
pub fn warn_text(w: &World, msg: &str) {
    let mut seen = w.warned.borrow_mut();
    if !seen.contains(msg) {
        seen.insert(Box::from(msg));
        eprintln!("[sim] {msg}");
    }
}

/// `warn_text` of the message the parts make, joined only when it is new: a warning on a per-tick path allocates
/// nothing once its text has been seen.
pub fn warn_parts(w: &World, parts: &[&str]) {
    let seen = w.warned.borrow().iter().any(|m| {
        let mut rest: &str = m;
        parts.iter().all(|p| match rest.strip_prefix(p) {
            Some(r) => {
                rest = r;
                true
            }
            None => false,
        }) && rest.is_empty()
    });
    if !seen {
        warn_text(w, &parts.concat());
    }
}

/// True the first time a check named `check` asks for the class `cls` (a per-class set of a check that
/// runs once per class, such as the barrier's `UNMET_CHECKED`).
pub fn first_check(w: &World, check: &'static str, cls: Name) -> bool {
    w.checked.borrow_mut().insert((check, cls))
}

/// `warnOnce` of an interned message.
pub fn warn_once(w: &World, msg: Name) {
    warn_text(w, w.names.str(msg));
}

/// `stripEnum`: the enum value's name past its `Type::` prefix.
pub fn strip_enum(s: &str) -> &str {
    match s.find("::") {
        Some(i) => &s[i + 2..],
        None => s,
    }
}

/// `defIsProjectile`: the class implements BPI_Projectiles (its own or a parent's
/// interfaces) or its Comp_Actor_Destructible says IsProjectile.
pub fn def_is_projectile(a: &ActorDef) -> bool {
    let ifaces = a.ifaces.as_deref().unwrap_or("");
    ifaces.split(' ').any(|s| s == "BPI_Projectiles_C") || destructible_is_projectile(a)
}

fn destructible_is_projectile(a: &ActorDef) -> bool {
    a.components
        .get("Comp_Actor_Destructible")
        .and_then(Value::as_obj)
        .and_then(|c| c.get("IsProjectile"))
        .is_some_and(truthy)
}

/// A JavaScript truthiness test of a data value.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Num(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        _ => true,
    }
}

/// `lookupOwner`: the fighter an actor's class is looked up through (its owner, else the
/// fighter it was spawned with for the lookup). A fighter is its own.
pub fn lookup_owner(w: &World, a: ActorRef) -> FighterId {
    let (owner, lookup) = match a {
        ActorRef::Fighter(f) => (Some(f), None),
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or((None, None), |p| (p.owner, p.lookup_fighter)),
        ActorRef::Item(id) => w.actors.items.get(id).map_or((None, None), |p| (p.owner, p.lookup_fighter)),
        ActorRef::Actor(id) => (w.actors.actors.get(id).map(|p| p.owner), None),
        ActorRef::Marker(id) => (w.actors.markers.get(id).map(|p| p.owner), None),
        ActorRef::Ship(id) => (w.actors.ships.get(id).map(|p| p.owner), None),
        ActorRef::Mortar(id) => (w.actors.mortars.get(id).map(|p| p.owner), None),
        ActorRef::AimPunch(id) => (w.actors.aim_punches.get(id).map(|p| p.owner), None),
        ActorRef::Accelerator(id) => (w.actors.accelerators.get(id).map(|p| p.owner), None),
        ActorRef::Stage(_) => (None, None),
        ActorRef::Buff(b) => (crate::combat::buffs::buff(w, b).and_then(|x| x.ext.actor_owner()), None),
    };
    owner.or(lookup).expect("an actor with no owner and no fighter to look its class up through")
}

/// An actor's owner and class (`a.owner`, `a.cls`); a fighter has neither.
fn owner_and_class(w: &World, a: ActorRef) -> (Option<FighterId>, Option<Name>) {
    match a {
        ActorRef::Fighter(_) | ActorRef::Stage(_) => (None, None),
        ActorRef::Buff(b) => {
            crate::combat::buffs::buff(w, b).map_or((None, None), |x| (x.ext.actor_owner(), Some(x.cls)))
        }
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or((None, None), |p| (p.owner, Some(p.cls))),
        ActorRef::Item(id) => w.actors.items.get(id).map_or((None, None), |p| (p.owner, Some(p.cls))),
        ActorRef::Actor(id) => w.actors.actors.get(id).map_or((None, None), |p| (Some(p.owner), Some(p.cls))),
        ActorRef::Marker(id) => w.actors.markers.get(id).map_or((None, None), |p| (Some(p.owner), Some(p.cls))),
        ActorRef::Ship(id) => w.actors.ships.get(id).map_or((None, None), |p| (Some(p.owner), Some(p.cls))),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).map_or((None, None), |p| (Some(p.owner), Some(p.cls))),
        ActorRef::AimPunch(id) => w.actors.aim_punches.get(id).map_or((None, None), |p| (Some(p.owner), Some(p.cls))),
        ActorRef::Accelerator(id) => {
            w.actors.accelerators.get(id).map_or((None, None), |p| (Some(p.owner), Some(p.cls)))
        }
    }
}

/// The fighter an `own` argument stands for (the owning fighter is passed).
fn own_fighter(w: &World, own: ActorRef) -> FighterId {
    match own {
        ActorRef::Fighter(f) => f,
        a => lookup_owner(w, a),
    }
}

/// `ownerTeam`: Comp_Actor_Destructible.TeamIndex of a spawned actor, its owner's, the
/// class default -1 with no owner.
pub fn owner_team(w: &World, a: ActorRef) -> i32 {
    owner_and_class(w, a).0.map_or(-1, |o| w.fighter(o).team.team)
}

/// `actorSoundFrom`: the fields of a 'sound' event a spawned actor posts on itself.
// Unverified: the build posts on the actor's own AkComponent; with no owner, cha_locally_controlled and the skin's
// event name have no fighter to come from, taken as the pooled post's (1, the plain name).
pub fn actor_sound_from(w: &World, a: ActorRef) -> ActorSoundFromOut {
    match owner_and_class(w, a).0 {
        Some(o) => ActorSoundFromOut { fighter: w.fighter(o).id, pooled: false },
        None => ActorSoundFromOut { fighter: -1, pooled: true },
    }
}

/// `soundAtFor`: Lib_Audio PF_Post_Event_At_Location(ev, location, Instigator); with no
/// Instigator the pooled post's.
pub fn sound_at_for(w: &mut World, inst: Option<FighterId>, ev: Name, x: Real, y: Real) {
    let (fighter, pooled) = match inst {
        Some(f) => (w.fighter(f).id, false),
        None => (-1, true),
    };
    w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled, key: None });
}

/// `instigatorOf`: GetInstigator of a spawned actor that keeps its instigator's id
/// (`inst`): `None` once that pawn is gone (a mode's pawn despawned while the actor lives); the caller takes the
/// failed cast's or IsValid's path.
// Unverified: a destroyed pawn is taken as None at once (the build's Instigator pointer reads the pending-kill pawn
// until the next garbage collection, and the casts pass on it).
pub fn instigator_of(w: &World, inst: i32) -> Option<FighterId> {
    w.by_id(inst)
}

/// `actorDef`: a spawned actor class's defaults for an owner: its own character's data,
/// else the first fighter of the match that has the class, else the stage's, else the items part's (a match with
/// items), else a perk part's (`perkPartActor`).
pub fn actor_def(w: &World, own: ActorRef, cls: Name) -> Option<&ActorDef> {
    actor_data(w, &w.data, own, cls)
}

/// [`actor_def`] borrowed from `data` (the World's `data`, a clone of its `Arc`) rather than from the World, so a
/// hook can hold the class's defaults and components across its writes to the World.
pub fn actor_data<'a>(w: &World, data: &'a mvs_data::GameData, own: ActorRef, cls: Name) -> Option<&'a ActorDef> {
    let key = w.names.str(cls);
    let char_actors = |f: FighterId| {
        let ch = w.names.str(w.fighter(f).ch);
        data.chars.get(ch).unwrap_or_else(|| panic!("no character data for {ch}")).fighter.actors.as_ref()
    };
    let f = own_fighter(w, own);
    if let Some(a) = char_actors(f).and_then(|m| m.get(key)) {
        return Some(a);
    }
    for &o in peers(w) {
        if let Some(a) = char_actors(o).and_then(|m| m.get(key)) {
            return Some(a);
        }
    }
    if let Some(a) = data.stage.actors.as_ref().and_then(|m| m.get(key)) {
        return Some(a);
    }
    if crate::setup::items_on(&w.setup)
        && let Some(a) = data.items.as_ref().and_then(|i| i.actors.get(key))
    {
        return Some(a);
    }
    // the last step: an actor a character perk's buff spawns that no character's data holds (the owner's perk
    // part's or a peer's, `perkPartActor`)
    let part = |f: FighterId| data.chars.get(w.names.str(w.fighter(f).ch))?.perks.as_ref()?.actors.as_ref()?.get(key);
    if let Some(a) = part(f).or_else(|| peers(w).iter().find_map(|&o| part(o))) {
        return Some(a);
    }
    // A tutorial graph's own spawn of another character's class with no pawn of it in the World (the Batman
    // tutorial's HarleyJackInTheBox targets): any character's data that holds it.
    if crate::modes::tutorial::running(w) {
        return data.chars.values().find_map(|c| c.fighter.actors.as_ref()?.get(key));
    }
    None
}

/// `actorDefOwner`: the fighter whose character data `actorDef` takes the class from.
pub fn actor_def_owner(w: &World, own: FighterId, cls: Name) -> FighterId {
    let key = w.names.str(cls);
    let has = |f: FighterId| data_of(w, f).fighter.actors.as_ref().is_some_and(|m| m.contains_key(key));
    if has(own) {
        return own;
    }
    peers(w).iter().copied().find(|&o| has(o)).unwrap_or(own)
}

/// `actorIsA`: a graph's cast of a spawned actor to a Blueprint class: the class itself or
/// one of its parents in the data (`supers`). `None`: no chain for the class; the caller warns.
pub fn actor_is_a(w: &World, own: ActorRef, cls: Name, parent: Name) -> Option<bool> {
    if cls == parent {
        return Some(true);
    }
    let a = actor_def(w, own, cls)?;
    let supers = a.supers.as_deref()?;
    let p = w.names.str(parent);
    Some(supers.split(' ').any(|s| s == p))
}

/// `actorIsProjectile`.
pub fn actor_is_projectile(w: &World, own: ActorRef, cls: Name) -> bool {
    actor_def(w, own, cls).is_some_and(def_is_projectile)
}

/// `actorBy`: the attacking actor of a dodged hit or a KnockedBack's DamageDealer as a
/// buff's graph reads it. An item: ItemActorParent implements BPI_Projectiles, its Destructible's IsProjectile is
/// the item's current one, its attack type its own. A fighter: no BPI_SpawnedActor (attack type 0), IsProjectile
/// false.
pub fn actor_by(w: &World, a: ActorRef, fighter: Option<FighterId>) -> DodgeBy {
    if let (Some(fid), ActorRef::Fighter(af)) = (fighter, a)
        && fid == af
    {
        let team = w.fighter(fid).team.team;
        return DodgeBy { actor: a, fighter: Some(fid), team, projectile: false, is_projectile: false, attack_type: 0 };
    }
    let (owner, cls) = owner_and_class(w, a);
    let team = owner.map_or(-1, |o| w.fighter(o).team.team);
    if let ActorRef::Item(it) = a {
        let is_projectile = crate::actors::item::item_is_projectile(w, it);
        let attack_type = crate::actors::item::item_attack_type(w, it);
        return DodgeBy { actor: a, fighter: None, team, projectile: true, is_projectile, attack_type };
    }
    let attack_type = match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(0, |p| p.attack_type),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).map_or(0, |p| p.attack_type),
        // The instance's own first (Velma's beam resets its own to 0).
        ActorRef::Actor(id) => {
            crate::actors::world_actor::instance_attack_type(w, id).unwrap_or_else(|| class_attack_type(w, owner, cls))
        }
        _ => class_attack_type(w, owner, cls),
    };
    let (projectile, is_projectile) = match (owner, cls) {
        (Some(o), Some(c)) => {
            let own = ActorRef::Fighter(o);
            (actor_is_projectile(w, own, c), actor_def(w, own, c).is_some_and(destructible_is_projectile))
        }
        _ => (false, false),
    };
    DodgeBy { actor: a, fighter: None, team, projectile, is_projectile, attack_type }
}

/// `x.p?.ProjectileAttackType`: the class defaults' (a world actor's `p`), 0 without one.
fn class_attack_type(w: &World, owner: Option<FighterId>, cls: Option<Name>) -> i32 {
    match (owner, cls) {
        (Some(o), Some(c)) => actor_def(w, ActorRef::Fighter(o), c)
            .and_then(|d| d.p.get("ProjectileAttackType"))
            .and_then(Value::as_f64)
            .map_or(0, math::dbl::to_int32),
        _ => 0,
    }
}

/// `buffDef`: a buff class's defaults: the owner's own character data, else the first
/// fighter of the match that has the class, else the stage data's, else the arcade buff classes' (a match with an
/// arcade round), else the mutators' (a match with mutators), else the items part's (a match with items), else the
/// perk tables'. A class none has warns and gives `None`.
// A test's buff defs (`testBuffDefs`) are not kept: no match has
// them.
pub fn buff_def(w: &World, own: ActorRef, cls: Name) -> Option<&Props> {
    let d = find_buff_def(w, own, cls);
    if d.is_none() {
        warn_text(
            w,
            &format!(
                "buff {}: neither the match's fighters' data nor its perk tables have its class defaults",
                w.names.str(cls)
            ),
        );
    }
    d
}

fn find_buff_def(w: &World, own: ActorRef, cls: Name) -> Option<&Props> {
    let key = w.names.str(cls);
    let f = own_fighter(w, own);
    if let Some(d) = data_of(w, f).fighter.buffs.as_ref().and_then(|m| m.get(key)) {
        return Some(d);
    }
    for &o in peers(w) {
        if let Some(d) = data_of(w, o).fighter.buffs.as_ref().and_then(|m| m.get(key)) {
            return Some(d);
        }
    }
    if let Some(d) = w.data.stage.buffs.as_ref().and_then(|m| m.get(key)) {
        return Some(d);
    }
    if w.setup.arcade.is_some()
        && let Some(d) = w.data.arcade.get(key)
    {
        return Some(d);
    }
    // (a Waves run gives its Giants the Giant mutator: `modes::waves`)
    let waves = matches!(w.setup.mode, Some(crate::setup::MatchMode::Waves { .. }));
    if (crate::setup::reads_mutators(&w.setup) || waves)
        && let Some(d) = w.data.mutators.buffs.get(key)
    {
        return Some(d);
    }
    if crate::setup::items_on(&w.setup)
        && let Some(d) = w.data.items.as_ref().and_then(|i| i.buffs.get(key))
    {
        return Some(d);
    }
    // the general table's, else the owner's part, else the first of the peers' parts.
    let d = crate::combat::perks::perk_buff_def(w, f, Some(peers(w)), key);
    if d.is_none() && waves {
        // (a Waves enemy's actors outlive its despawn: their buffs' classes are in the data of a character no
        // longer among the peers; the run's cast is all in the match's data)
        return w.data.chars.values().find_map(|c| c.fighter.buffs.as_ref().and_then(|m| m.get(key)));
    }
    d
}

// ---- the call surface of character

/// `charDef`: computed at the import.
pub fn char_def(d: &CharData) -> &CharDef {
    &d.def
}

/// The extras of anim set `set` of a character (per-set extras); `None` for a set the import has none
/// for (a polymorph prop's set: it has none either).
pub fn set_extra<'a>(w: &World, d: &'a CharData, set: SetRef) -> Option<&'a SetExtra> {
    match set {
        SetRef::Base => Some(&d.def_extra.anim),
        SetRef::Named(n) => d.def_extra.anim_sets.as_ref().and_then(|m| m.get(w.names.str(n))),
    }
}

/// `knockBackShapes`: the names of the pawn's hit shapes on the collision profile
/// KnockBack, which never meet a hurt box, a capsule, a projectile or an item.
pub fn knock_back_shapes(d: &CharData) -> &[String] {
    &d.def_extra.knock_back_shapes
}

/// `defaultTerminal`: DefaultTerminalVelocity, the pawn's.
pub fn default_terminal(d: &CharData) -> Real {
    crate::math::narrow(d.def_extra.default_terminal)
}

/// `altIdleDelay`: AlternateIdle_TimeDelay of a set (Infinity for a set without one).
pub fn alt_idle_delay(w: &World, d: &CharData, set: SetRef) -> Real {
    set_extra(w, d, set).map_or(Real::INFINITY, |e| crate::math::narrow(e.alt_idle_delay))
}

/// `crouchRunOf`: UseCrouchRun's clips and speed, on a set with it.
pub fn crouch_run_of<'a>(w: &World, d: &'a CharData, set: SetRef) -> Option<&'a CrouchRun> {
    set_extra(w, d, set).and_then(|e| e.crouch_run.as_ref())
}

/// `edgeIdleOf`: UseEdgeIdle's clips, on a set with it.
pub fn edge_idle_of<'a>(w: &World, d: &'a CharData, set: SetRef) -> Option<&'a EdgeIdle> {
    set_extra(w, d, set).and_then(|e| e.edge_idle.as_ref())
}

/// `runRateLandRun`: RunRateEffectsLandRun.
pub fn run_rate_land_run(w: &World, d: &CharData, set: SetRef) -> bool {
    set_extra(w, d, set).is_some_and(|e| e.run_rate_land)
}

// ---- the call surface's argument and return types

/// What `actorSoundFrom` gives.
#[derive(Clone, Debug)]
pub struct ActorSoundFromOut {
    pub fighter: i32,
    pub pooled: bool,
}

/// `capsuleOut` with its `OUT_N`: the length and the direction (x, y).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn capsule_out_n(
    left: Real,
    right: Real,
    bottom: Real,
    top: Real,
    x: Real,
    y: Real,
    r: Real,
    hs: Real,
) -> (Real, Real, Real) {
    let o = capsule_out(left, right, bottom, top, x, y, r, hs);
    (o.d, o.nx, o.ny)
}

/// `actorDef(own, cls)?.components`: the class's component templates by name.
// Unverified: an actor a perk's buff spawns (`perkPartActor`, the last step of `actorDef`) gives none: the port's
// perk part actor holds its class defaults alone.
pub fn actor_components(w: &World, own: ActorRef, cls: Name) -> Option<&Props> {
    actor_def(w, own, cls).map(|d| &d.components)
}

/// `actorDef` whole: the class's `ActorDef` (`p`, `components`, `supers`) in the owner's
/// data, its peers', the stage's.
pub fn actor_class_def(w: &World, own: FighterId, cls: Name) -> Option<&ActorDef> {
    actor_def(w, ActorRef::Fighter(own), cls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latent_frames_counts_float_seconds() {
        // Delay(0): one frame; Delay(0.1): 0.1 - 6 x 0.0166668 (floats) is below 0, the 6th fires.
        assert_eq!(latent_frames(0.0), 1.0);
        assert_eq!(latent_frames(0.1), 6.0);
        assert_eq!(latent_frames(0.5), 30.0);
        assert_eq!(latent_frames(1.0), 60.0);
    }

    #[test]
    fn strip_enum_drops_the_type() {
        assert_eq!(strip_enum("EActionButton::AB_BasicAttack"), "AB_BasicAttack");
        assert_eq!(strip_enum("AD_Up"), "AD_Up");
    }

    #[test]
    fn capsule_out_ways() {
        let o = capsule_out(0.0, 100.0, 0.0, 100.0, 90.0, 50.0, 10.0, 20.0);
        assert_eq!(o, CapsuleOut { d: 20.0, nx: 1.0, ny: 0.0 });
        let o = capsule_out(0.0, 100.0, 0.0, 100.0, 105.0, 50.0, 10.0, 20.0);
        assert_eq!(o, CapsuleOut { d: 5.0, nx: 1.0, ny: 0.0 });
        assert_eq!(capsule_out(0.0, 100.0, 0.0, 100.0, 125.0, 50.0, 10.0, 20.0).d, -1.0);
    }
}
