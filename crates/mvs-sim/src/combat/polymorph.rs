//! The Polymorph: Buff_PolymorphParent and its child Buff_C020Polymorph, the buff Rick's
//! Polymorph grenade puts on an enemy, who becomes a small Creature for 6 s. Other children enter their own graph's
//! part with [`polymorph_child`] (Taz's Buff_Chicken, `chars::taz::chicken`).
//!
//! Source: Buff_PolymorphParent (PP@ = its ubergraph), Buff_C020Polymorph (CP@).
//!
//! What runs here and what the fighter does for it:
//! - the buff's mesh is a second montage player (`Polymorph::play`, instance `POLY_INST`), its notifies run through
//!   the owner's handlers with the owner as `f`; its BranchWindows are relayed to the owner's manager by the buff's
//!   own handlers (`foreign_window_begin` / `foreign_window_end` -> `BuffHooks::branch_group_blocked` /
//!   `branch_group_end_blocked`);
//! - the root's non-uniform scale is `Body::actor_scale` (capsule, `scaled` and every attached shape);
//! - the owner's delegates the buff binds come as BuffSignals: EvadeTriggered, DashTriggered, InputPreTriggered,
//!   IsJumping, CrouchChange, HitStunEnded, HitByEnemy;
//! - BlockEvadeMontage is the blocked evade of the fighter's evade code; the stop-anim notifies of the mesh stop
//!   nothing of it.
//!
//! The view reads `BuffShapes::polymorph`, the PolymorphFx events and the `poly` flag of the mesh's particle events.
// Unverified: the mesh's anim instance is ticked right behind the pawn's Mesh (the fighter's mesh tick, after a
// Disguise's copy); the build's order is the level's tick order.
// Deviation: its AnimBP (Creature_AnimBP: locomotion under the montages) is not evaluated: the montage alone, which
// carries the states.

use crate::Real;
use crate::actors::ActorRef;
use crate::combat::buffs::{BuffRef, buff, end_buff, inst_view, set_buff_values, update_states};
use crate::combat::shapes::{ForeignPlay, Polymorph};
use crate::combat::{BuffClass, HitPacket};
use crate::consts;
use crate::events::{Event, NotifyRef, SoundKey, UiStateOp, UiStateParams};
use crate::fighter::anim::branches::{complete_window, file_window, foreign_window_begin, foreign_window_end};
use crate::fighter::anim::grouped::active_instance;
use crate::fighter::anim::montages::{
    KEY_STRIDE, MontageKey, POLY_INST, char_data, drop_state, instance_data, new_instance, notify_ticks, owner_montage,
    running, slot_name, state_mut, state_ref,
};
use crate::fighter::anim::notify_sink::{begin_of, end_of, instant_of};
use crate::fighter::anim::update::inst_pose;
use crate::fighter::anim::visibility::{add_invisibility, remove_invisibility};
use crate::fighter::anim::{MontageSlot, MontageState};
use crate::fighter::body::{FIND_FLOOR_DIST, actor_y, cap_half, cap_radius, capsule_half, set_grounded};
use crate::fighter::common::{latent_frames, peers, timer_frames, warn_text};
use crate::fighter::control::emit;
use crate::fighter::match_side::surface_sfx;
use crate::fighter::moves::{
    dash_dance, disable_attack, enable_attack, is_falling, is_flying, pending_launch_vector, set_crouching,
};
use crate::fighter::tick::FrameMode;
use crate::fighter::traces::capsule_trace_single;
use crate::fighter::{FighterId, ScaledDef, in_hitstun, is_attacking};
use crate::math::min;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::name as data_str;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::flow::items_data_of;
use mvs_anim::montage::{Host, Montage, Play};
use mvs_data::value::{Props, Value};

/// The key of the mesh's notify 0 (`POLY_KEY`).
const POLY_KEY: i32 = POLY_INST * KEY_STRIDE;
/// Trace channel 5 (ProjectileGroundNoPlatform: of the port's geometry the stage block alone).
const TRACE_GROUND_NO_PLATFORM: i32 = 5;
const BRANCH_WINDOW: &str = "AnimNotify_BranchWindow_C";
const UI_ID: Name = name!("Polymorph");

// ---- the children

/// The part of the graph of a child class of Buff_PolymorphParent that the child adds to the parent's
/// (`PolymorphChild`), entered by name with [`polymorph_child`] (the parent's graph is the one buff kind below, the
/// same for every child). No default: a child no one entered is no buff of the registry and warns where it is
/// spawned.
#[derive(Clone, Copy, Debug)]
pub struct PolymorphChild {
    /// `head`: a CreatureHeadScale the child's own BeginPlay writes on the mesh's instance (Buff_C020Polymorph
    /// CP@20-141).
    pub head: Option<Real>,
    /// `prop`: its Polymorph Mesh is a prop of the kit whose data holds the buff, no character's skin
    /// ([`polymorph_owner`]).
    pub prop: bool,
    /// The buff actor's own sounds' key tag (`cls + ':'`; the key's number is the owner's id).
    pub loop_tag: Name,
    /// `begin`: its ReceiveBeginPlay after the parent's (the child's event calls the parent's first).
    pub begin: Option<ChildBegin>,
    /// `damage`: its override of HandleDamageRecieved (bound to the owner's HitByEnemy by the parent, PP@11402), in
    /// place of the parent's: the last argument is the parent's call, which subtracts the hit from CurrentDamage and,
    /// at 0, K2_DestroyActor with the buff's EndPlay (the parent's and the child's `end`) inside that call, then
    /// returns LocalDamageTaken. What the child does after it runs on a destroyed buff (the polymorph gone), as the
    /// build's function runs on. Its return (Damage) is not read: the parent's ubergraph drops it (PP@11402 ->
    /// pop_flow @11434).
    pub damage: Option<ChildDamage>,
    /// `on`: its own binds of the owner's delegates, after the parent's handler of the same signal, on a buff still
    /// live. True: K2_DestroyActor.
    pub on: Option<ChildOn>,
    /// `end`: its ReceiveEndPlay after the parent's (the child's event calls the parent's first).
    pub end: Option<fn(&mut World, FighterId, BuffRef)>,
}

/// [`PolymorphChild::begin`]: the owner, the buff, the Instigator, the duration.
pub type ChildBegin = fn(&mut World, FighterId, BuffRef, Option<FighterId>, Option<Real>);

/// [`PolymorphChild::on`]: the owner, the buff, the signal and its argument.
pub type ChildOn = fn(&mut World, FighterId, BuffRef, BuffSignal, &BuffSignalArg) -> bool;

/// [`PolymorphChild::damage`]: the owner, the buff, the hit, the parent's call.
pub type ChildDamage =
    fn(&mut World, FighterId, BuffRef, Option<&HitPacket>, &mut dyn FnMut(&mut World) -> Real) -> Real;

impl PolymorphChild {
    /// A child with no part of its own beyond its sounds' key tag.
    pub const fn new(loop_tag: Name) -> Self {
        PolymorphChild { head: None, prop: false, loop_tag, begin: None, damage: None, on: None, end: None }
    }
}

/// `polymorphChildren`: enters `cls` as a buff of the parent's kind and its child part.
pub fn polymorph_child(
    r: &mut Registry,
    cls: &'static str,
    class: BuffClass,
    child: PolymorphChild,
) -> Result<(), RegistryError> {
    r.buffs.enter(cls, BuffEntry::new(class, POLYMORPH_KIND))?;
    r.polymorph_children.enter(cls, child)
}

fn child_of(w: &World, cls: Name) -> Option<PolymorphChild> {
    let r: &'static Registry = w.registry;
    r.polymorph_children.get(w.names.str(cls)).copied()
}

// ---- the actor scale

/// `setActorScale`: SetActorScale3D(xy, xy, z) on the owner's root (PP@3776 (1,1,1), PP@4093 the capsule's relative
/// scale, which is the root's: the two make one scale; EndPlay PP@419 OriginalActorScale). No sweep and no movement
/// call: the next move of the movement component meets what the new size overlaps (`cap_resized` -> the resize
/// model of the fighter's physics). `scaled` takes the half heights times Z and the radius and the mesh scale times
/// X.
// Deviation: the build's pop-up of a capsule that grew into its floor (standing: the depth + 2.275, then 2.15 more;
// moving or falling: the MTD's depth + 0.125) is the port's resize model (the walking rest at once). A bottom raised
// past FindFloor's reach (102.4: Iron Giant 450 -> 200) finds no floor and falls, as a base lost under a walker;
// within the reach it is put back at its rest.
// Deviation: the readers of the uniform `size_scale` (root motion, the move command's corner check, the hold traces,
// Batman's glide and Bugs's glide shapes, the Creature pawn's crouch, Wonder Woman's actors, a Disguise's clone
// shapes, the Iron Giant head point) do not take the non-uniform scale; `scaled.mesh_scale` takes X.
fn set_actor_scale(w: &mut World, f: FighterId, xy: Real, z: Real) {
    let k = w.fighter(f).body.size_scale;
    let bottom = w.fighter(f).body.y - cap_half(w, f);
    let a = if xy == k && z == k { None } else { Some([xy, z]) };
    let mut b = ScaledDef::of(crate::combat::hit::def_of(w, w.fighter(f).base_ch));
    if let Some(m) = w.fighter(f).base_mesh_scale {
        b.mesh_scale = m;
    }
    let scaled = match a {
        Some([xy, z]) => ScaledDef {
            cap_half: (b.cap_half * z),
            cap_radius: (b.cap_radius * xy),
            crouched_half: (b.crouched_half * z),
            mesh_scale: (b.mesh_scale * xy),
        },
        None if k == 1.0 => b,
        None => ScaledDef {
            cap_half: (b.cap_half * k),
            cap_radius: (b.cap_radius * k),
            crouched_half: (b.crouched_half * k),
            mesh_scale: (b.mesh_scale * k),
        },
    };
    let fi = w.fighter_mut(f);
    fi.body.actor_scale = a;
    fi.scaled = scaled;
    fi.capsule.cap_resized = true;
    if w.fighter(f).body.on_floor && w.fighter(f).body.y - cap_half(w, f) - bottom > FIND_FLOOR_DIST {
        let fi = w.fighter_mut(f);
        fi.platforms.walk_drop = true;
        fi.platforms.walk_base = fi.platforms.base;
        set_grounded(w, f, false);
    }
}

// ---- the mesh's notify sink

/// `polyRunning`: whether a notify of fighter `f` runs inside its Polymorph's sink
/// (`inSink === f`, set only while `call` runs).
pub fn poly_running(w: &World, f: FighterId) -> bool {
    w.scratch.poly_sink == Some(f)
}

/// `call`: a handler run with the mesh's sink marked as running.
fn in_sink(w: &mut World, f: FighterId, body: impl FnOnce(&mut World)) {
    let was = w.scratch.poly_sink.replace(f);
    body(w);
    w.scratch.poly_sink = was;
}

/// Which of the sinks a [`PolyHost`] is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sink {
    /// `sinkOf`: BranchWindows to the owner's foreign window hooks, every other notify to the owner's handlers.
    Live,
    /// EndPlay's dispatch of the queued states: a BranchWindow's end reaches the still bound handlers.
    EndQueued,
    /// EndPlay's end of the branching-point states: the windows stay listed.
    EndBranching,
}

/// The mesh's play as the player sees it: its montage (from the data the polymorph names), its state found by number
/// on each access, and one of the [`Sink`]s.
struct PolyHost<'w, 'm> {
    w: &'w mut World,
    f: FighterId,
    key: MontageKey,
    m: &'m Montage,
    spare: MontageState,
    sink: Sink,
}

impl<'m> Host<'m, MontageKey> for PolyHost<'_, 'm> {
    fn m(&self) -> &'m Montage {
        self.m
    }
    fn s(&mut self) -> &mut MontageState {
        if let Some(s) = state_mut(self.w, self.f, self.key.uid) {
            return s;
        }
        &mut self.spare
    }
    fn begin(&mut self, idx: usize) {
        if self.sink != Sink::Live {
            return;
        }
        let (f, n, r) = (self.f, &self.m.notifies[idx], self.r(idx));
        if n.cls == BRANCH_WINDOW {
            foreign_window_begin(self.w, f, r, self.key.name, crate::math::narrow(n.dur));
        } else {
            in_sink(self.w, f, |w| begin_of(w, f, n, r, POLY_KEY + ix(idx)));
        }
    }
    fn end(&mut self, idx: usize) {
        let (f, n, r) = (self.f, &self.m.notifies[idx], self.r(idx));
        let window = n.cls == BRANCH_WINDOW;
        match (self.sink, window) {
            (Sink::Live, true) => foreign_window_end(self.w, f, r),
            (Sink::EndQueued, true) => {
                if let Some(h) = self.w.fighter(f).hooks.char.branch_group_end_blocked {
                    h(self.w, f, r);
                }
                window_end(self.w, f, r);
            }
            (Sink::EndBranching, true) => {}
            (_, false) => in_sink(self.w, f, |w| end_of(w, f, n, r, POLY_KEY + ix(idx))),
        }
    }
    fn instant(&mut self, idx: usize) {
        if self.sink != Sink::Live {
            return;
        }
        let (f, n, r) = (self.f, &self.m.notifies[idx], self.r(idx));
        in_sink(self.w, f, |w| instant_of(w, f, n, r));
    }
}

impl PolyHost<'_, '_> {
    fn r(&self, idx: usize) -> NotifyRef {
        NotifyRef { montage: self.key.name, index: u32::try_from(idx).expect("a notify index") }
    }
}

fn ix(i: usize) -> i32 {
    i32::try_from(i).expect("a notify index")
}

fn poly(w: &World, f: FighterId) -> Option<&Polymorph> {
    w.fighter(f).shapes.polymorph.as_ref()
}

fn poly_mut(w: &mut World, f: FighterId) -> Option<&mut Polymorph> {
    w.fighter_mut(f).shapes.polymorph.as_mut()
}

fn play_of(w: &World, f: FighterId) -> Option<&ForeignPlay> {
    poly(w, f)?.play.as_ref()
}

fn play_uid(w: &World, f: FighterId) -> Option<u32> {
    play_of(w, f).map(|p| p.m.montage.uid)
}

/// Runs `body` over the player of the mesh's instance `uid` with sink `sink` (kept in limbo if a handler drops it
/// while it runs).
fn with_poly<R>(
    w: &mut World,
    f: FighterId,
    uid: u32,
    sink: Sink,
    body: impl FnOnce(&mut PolyHost<'_, '_>) -> R,
) -> Option<R> {
    let data = w.data.clone();
    let m = instance_data(&data, w, f, uid)?;
    let key = state_ref(w, f, uid)?.montage;
    Some(running(w, f, uid, |w| body(&mut PolyHost { w, f, key, m, spare: MontageState::new(key, false), sink })))
}

/// `setPrev`.
fn set_prev(w: &mut World, f: FighterId) {
    let Some(uid) = play_uid(w, f) else { return };
    let p = inst_pose(&w.data, w, f, uid, None);
    if let Some(pp) = poly_mut(w, f).and_then(|p| p.play.as_mut()) {
        pp.prev = p;
    }
}

/// `dropPlay`.
fn drop_play(w: &mut World, f: FighterId, uid: u32) {
    if state_ref(w, f, uid).is_some_and(|s| !s.active.is_empty()) {
        with_poly(w, f, uid, Sink::Live, |h| {
            h.end_branching();
            h.end_queued();
        });
    }
    clear_charge_notify(w, f);
    if play_uid(w, f) == Some(uid) {
        let pp = poly_mut(w, f).and_then(|p| p.play.take()).expect("the polymorph's play");
        drop_state(w, f, pp.m);
    }
}

/// A ChargeAttack notify of the mesh's instance held as the fighter's charge notify is let go.
fn clear_charge_notify(w: &mut World, f: FighterId) {
    let c = &mut w.fighter_mut(f).charge;
    if c.charge_notify >= POLY_KEY && c.charge_notify < POLY_KEY + KEY_STRIDE {
        c.charge_notify = -1;
    }
}

/// `play`: Montage_Play(montage, rate 1, start 0, stop all) on the mesh's anim instance (PP@10059 / @10341 / @10891,
/// the taunt's PlayMontage proxy @12160). The return: the montage's plain length, 0 with no montage.
// Deviation (as the Disguise's copy): the instance the play stops is over at once, its states ended now; the build
// blends it out and ends its queued states at the next dispatch.
fn play(w: &mut World, f: FighterId, name: Option<Name>) -> Real {
    let Some(name) = name.filter(|n| !n.is_none()) else { return 0.0 };
    let Some(p) = poly(w, f) else { return 0.0 };
    let len = p.from.and_then(|ch| owner_montage(&w.data, &w.names, ch, w.names.str(name))).map(|m| m.length);
    let Some(len) = len else {
        let msg =
            format!("Polymorph: montage {} not in the data of {}'s character", w.names.str(name), w.names.str(p.mesh));
        warn_text(w, &msg);
        return 0.0;
    };
    if let Some(uid) = play_uid(w, f) {
        drop_play(w, f, uid);
    }
    let ground = w.fighter(f).body.on_floor;
    let m = new_instance(w, f, name, ground);
    if let Some(p) = poly_mut(w, f) {
        p.play = Some(ForeignPlay { m, stopped: false, blend_out: false, prev: None, neutral: None });
    }
    set_prev(w, f);
    crate::math::narrow(len)
}

/// `stopPlay`: Montage_Stop(blend, the mesh's current montage): its states end at the mesh's next update
/// ([`poly_tick`]).
// Deviation: the blend-out is not played out (the copy's rule).
fn stop_play(w: &mut World, f: FighterId) {
    let Some(pp) = poly_mut(w, f).and_then(|p| p.play.as_mut()) else { return };
    if pp.stopped {
        return;
    }
    pp.stopped = true;
    pp.m.ended = true; // halt
}

/// `polyTick`: the mesh's anim instance update, behind the pawn's Mesh (and a Disguise's
/// copy) in the fighter's mesh tick.
pub fn poly_tick(w: &mut World, f: FighterId, mode: FrameMode) {
    let Some(pp) = play_of(w, f) else { return };
    let (uid, stopped, any) = (pp.m.montage.uid, pp.stopped, !pp.m.active.is_empty());
    if stopped {
        if mode == FrameMode::Paused {
            if any {
                with_poly(w, f, uid, Sink::Live, |h| h.end_queued());
            }
            return;
        }
        drop_play(w, f, uid);
        return;
    }
    if mode == FrameMode::Dead {
        return;
    }
    // Under a hit pause the mesh ticks dilated (0.005), as the pawn's Mesh: the montage moves by that share of the
    // frame with its notifies and NotifyTicks.
    let dil = if mode == FrameMode::Paused { consts::HITPAUSE_DILATION } else { 1.0 };
    set_prev(w, f);
    let traces = w.fighter(f).frame.surface_traces.len();
    let alive = with_poly(w, f, uid, Sink::Live, |h| h.advance(consts::DT * dil, false, dil != 1.0))
        .expect("the polymorph's montage in its data");
    // Ak_SurfaceEffect of its montages (Creature_Run_Turn, Creature_Taunt): the port's fallback point, the capsule's
    // bottom. Not ported: the socket of the creature mesh is not sampled.
    let mut k = traces;
    while let Some(ev) = w.fighter(f).frame.surface_traces.get(k).map(|t| t.ev) {
        k += 1;
        let oy = w.fighter(f).body.y - cap_half(w, f);
        surface_sfx(w, f, ev, oy, false, None, None);
    }
    w.fighter_mut(f).frame.surface_traces.truncate(traces);
    if play_uid(w, f) != Some(uid) || play_of(w, f).is_some_and(|p| p.stopped) {
        return;
    }
    if !alive {
        drop_play(w, f, uid);
        return;
    }
    notify_ticks(w, f, MontageSlot::Poly, uid, POLY_INST);
    // AnimNotify_AerialAttack NotifyTick with MeshComp the mesh (its GetCurrentActiveMontage is this one): grounded,
    // no pending upward launch -> Montage_Stop(0.1) of the mesh's montage, then the dash out as on the pawn: the
    // pawn's dash, with isEvading still set by the blocked evade.
    if w.fighter(f).body.on_floor
        && pending_launch_vector(w, f).map_or(0.0, |v| v.1) <= 0.0
        && has_state(w, f, "AnimNotify_AerialAttack_C")
    {
        let evading = w.fighter(f).evade.evading;
        stop_play(w, f);
        if w.fighter(f).controls.inp.x.abs() >= 0.3 && !in_hitstun(w, f) && !evading {
            dash_dance(w, f, true);
        }
    }
}

/// `hasState`: a state of class `cls` is active on the mesh's play.
fn has_state(w: &World, f: FighterId, cls: &str) -> bool {
    let Some(pp) = play_of(w, f) else { return false };
    let Some(m) = instance_data(&w.data, w, f, pp.m.montage.uid) else { return false };
    pp.m.active.iter().any(|&i| m.notifies[i].cls == cls)
}

// ---- the buff

/// The key of the items part as a polymorph's data owner (`Polymorph::from`): no character is named so.
pub const ITEMS_OWNER: Name = name!("items");

/// `polymorphOwner`: the data that holds the mesh and its montages, by character key, in
/// the list's order (the match's players, then the characters the match loaded beside them), then the items part:
/// - the first character whose skin's mesh is the buff's Polymorph Mesh (Buff_C020Polymorph: Creature_Skelmesh_New,
///   the Creature's data, which holds the Creature_* montages the buff names);
/// - else, for a child entered with `prop`, the first whose class defaults of the buff name the mesh: the kit's own
///   prop (Buff_Chicken: Taz's Chicken_SkelMesh, whose montages and clips his data holds), else, in a match with
///   items, the items part when its class defaults of the buff name the mesh (the drumstick's Buff_Chicken with no
///   Taz in the match).
///
/// (port's own: the build loads the assets the buff's class defaults name; the port keeps a montage in the data of
/// one character or of the items part, so this finds that one.)
// Deviation: after the players' characters the list is every character of the World's data in key order (the default
// character's and the needed ones). A mesh that a skin of the default character and of a needed one share would
// differ from the needed ones alone: none does.
// The items part is the owner [`ITEMS_OWNER`] (no character has that key): `montages::owner_montage` reads its
// montages.
pub fn polymorph_owner(w: &World, cls: Name, mesh: &str) -> Option<Name> {
    let mut list: Vec<Name> = Vec::new();
    for &o in peers(w) {
        list.push(w.fighter(o).ch);
    }
    for k in w.data.chars.keys() {
        if let Some(n) = w.names.get(k)
            && !list.contains(&n)
        {
            list.push(n);
        }
    }
    let data = |ch: Name| char_data(&w.data, &w.names, ch).map(|d| &d.fighter);
    if let Some(&ch) = list.iter().find(|&&ch| data(ch).is_some_and(|d| d.skin.mesh == mesh)) {
        return Some(ch);
    }
    if !child_of(w, cls).is_some_and(|c| c.prop) {
        return None; // a skin mesh whose character is not loaded: the caller warns
    }
    let cs = w.names.str(cls);
    let names_mesh = |p: Option<&Props>| p.and_then(|p| p.get("Polymorph Mesh")).and_then(Value::as_str) == Some(mesh);
    if let Some(&ch) =
        list.iter().find(|&&ch| names_mesh(data(ch).and_then(|d| d.buffs.as_ref()).and_then(|b| b.get(cs))))
    {
        return Some(ch);
    }
    if names_mesh(items_data_of(w).and_then(|i| i.buffs.get(cs))) {
        return Some(ITEMS_OWNER);
    }
    None
}

/// ResetEvade (PP@10668): owner.isEvading = false, EvadingThroughPlatform = false.
fn reset_evade(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    fi.evade.evading = false;
    fi.platforms.evading_through_platform = false;
}

/// The buff actor's own sounds by key.
fn loop_key(w: &World, f: FighterId, child: Option<&PolymorphChild>) -> SoundKey {
    SoundKey { tag: child.map_or(Name::NONE, |c| c.loop_tag), n: w.fighter(f).id }
}

/// A string of the buff's values as a name (`d.k`, none when unset or empty).
fn dname(w: &World, b: BuffRef, k: &str) -> Option<Name> {
    inst_view(w, b).get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| data_str(w, s))
}

/// `d.k ?? ''` as a name.
fn dname_or_none(w: &World, b: BuffRef, k: &str) -> Name {
    inst_view(w, b).get(k).and_then(Value::as_str).map_or(Name::NONE, |s| data_str(w, s))
}

/// The polymorph of buff `b` (`f.polymorph` with `p.cls === b.cls`).
fn own(w: &World, f: FighterId, b: BuffRef) -> bool {
    let cls = buff(w, b).map(|x| x.cls);
    poly(w, f).is_some_and(|p| Some(p.cls) == cls)
}

/// The windows of the mesh's montages, relayed (`windowBegin`).
fn window_begin(w: &mut World, f: FighterId, n: NotifyRef, duration: Real) {
    let montage = active_instance(w, f).map(|s| slot_name(w, f, s));
    file_window(w, f, n, montage, duration);
    if let Some(p) = poly_mut(w, f)
        && !p.windows.contains(&n)
    {
        p.windows.push(n);
    }
}

/// `windowEnd`.
fn window_end(w: &mut World, f: FighterId, n: NotifyRef) {
    if !poly(w, f).is_some_and(|p| p.windows.contains(&n)) {
        return;
    }
    complete_window(w, f, n);
    if let Some(p) = poly_mut(w, f)
        && let Some(k) = p.windows.iter().position(|&x| x == n)
    {
        p.windows.remove(k);
    }
}

/// `endBurst` (PP@2473-3412): PolymorphChangeVFX on the owner's Mesh (VFX_Root + (0,0,80)), PolymorphEndSFX (null for
/// C020: nothing), RTPC mix_duck_off_polymorph = 0 on the buff's root and on the owner, RemoveStateForPlayer.
fn end_burst(w: &mut World, f: FighterId, vfx: Name, end_sfx: Option<Name>) {
    emit(w, f, |fighter| Event::PolymorphFx { fighter, begin: false, vfx });
    if let Some(ev) = end_sfx {
        let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
        emit(w, f, |fighter| Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    }
    emit(w, f, |fighter| Event::Rtpc { fighter, name: name!("mix_duck_off_polymorph"), value: 0.0, interp_ms: 0.0 });
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(UI_ID));
}

/// BeginPlay, in order.
fn poly_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, duration: Option<Real>) {
    let Some(cls) = buff(w, b).map(|x| x.cls) else { return };
    let child = child_of(w, cls);
    // 1. BuffParentActor's: StartLoopSoundMulticast, SFX_Loop_Start on the buff actor (BPA@5258), at the owner; posted
    // on the buff's key, which its end stops (the actor's destroy, StopWhenOwnerDestroyed).
    if let Some(ev) = dname(w, b, "SFX_Loop_Start") {
        let key = loop_key(w, f, child.as_ref());
        emit(w, f, |fighter| Event::SoundStart { fighter, ev, key });
    }
    let d = inst_view(w, b);
    let mesh_s = d.get("Polymorph Mesh").and_then(Value::as_str).unwrap_or("").to_owned();
    let owner = polymorph_owner(w, cls, &mesh_s);
    if owner.is_none() {
        warn_text(w, &format!("Polymorph: no character of the match has the mesh {mesh_s} (MATCH_NEEDS)"));
    }
    let d = inst_view(w, b);
    let scale = d.obj("RelativeScale").and_then(|s| s.get("X")).and_then(Value::as_f64).unwrap_or(1.0);
    let damage = d.num("MaxDamageTaken").unwrap_or(30.0);
    let (radius, half) = (d.num("TargetRadius").unwrap_or(100.0), d.num("TargetHalfHeight").unwrap_or(200.0));
    let walk = d.num("TargetWalkSpeed").unwrap_or(1550.0);
    let timer = duration.or_else(|| d.num("BuffDuration")).unwrap_or(-1.0);
    let anim = dname_or_none(w, b, "PolymorphAnimClass");
    let icon = dname_or_none(w, b, "PolymorphIcon");
    let vfx = dname_or_none(w, b, "PolymorphChangeVFX");
    let start_sfx = dname(w, b, "PolymorphStartSFX");
    let mesh = data_str(w, &mesh_s);
    // 5-6. OriginalActorScale; the scale (TargetRadius / R0, TargetHalfHeight / HH0), R0 / HH0 the pawn's unscaled
    // capsule (BPI_GetOriginalCapsuleHalfHeightAndRadius: the template, BaseFighter @124896 / @124945).
    let fi = w.fighter(f);
    let orig = fi.body.actor_scale.map_or((fi.body.size_scale, fi.body.size_scale), |a| (a[0], a[1]));
    let crouched = fi.capsule.cmc_crouched;
    let show_in = crate::math::to_int32(latent_frames(0.02));
    w.fighter_mut(f).shapes.polymorph = Some(Polymorph {
        cls,
        from: owner,
        mesh,
        anim,
        scale: crate::math::narrow(scale),
        // the child's own BeginPlay (PolymorphChild::head)
        head: child.and_then(|c| c.head).unwrap_or(1.0),
        play: None,
        damage,
        evade_timer: -1,
        show_in,
        active: false,
        crouched,
        hitstun_bound: true,
        windows: Vec::new(),
        orig,
    });
    w.fighter_mut(f).shapes.is_polymorphed = true; // 4. BPI_SetPolymorph(true)
    let base = crate::combat::hit::def_of(w, w.fighter(f).base_ch);
    let (r0, hh0, max_walk) = (base.cap_radius, base.cap_half, base.max_walk);
    set_actor_scale(w, f, radius / crate::math::narrow(r0), half / crate::math::narrow(hh0));
    // 8. BPI_DisableAttack (counter); BlockEvadeMontage @4536.
    disable_attack(w, f);
    w.fighter_mut(f).evade.block_evade_montage = true;
    // 20. MulticastUI(BuffDuration) @12855: AddStateForPlayer.
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: UI_ID,
        state_type: 1,
        display_type: 0,
        current_value: -1.0,
        max_value: -1.0,
        timer,
        icon_material: icon,
        active: true,
        pop_text: Name::NONE,
        auto_remove_time: -1.0,
        subicon: None,
    }));
    // 21. the start burst and RTPC mix_duck_off_polymorph = 1.
    emit(w, f, |fighter| Event::PolymorphFx { fighter, begin: true, vfx });
    emit(w, f, |fighter| Event::Rtpc { fighter, name: name!("mix_duck_off_polymorph"), value: 1.0, interp_ms: 0.0 });
    // 23. PF_Post_Event(PolymorphStartSFX): null for C020.
    if let Some(ev) = start_sfx {
        emit(w, f, |fighter| Event::Sound { fighter, ev, x: None, y: None, hit: None, pooled: false, key: None });
    }
    // 24. Authority: SpeedModType Override, SpeedMod = TargetWalkSpeed / CharacterBaseSpeed @8296,
    // UpdatePlayerStatus.
    // Unverified: CharacterBaseSpeed is taken as the pawn's template walk speed (`max_walk`).
    let mut v = Props::new();
    v.insert("SpeedModType".to_owned(), Value::Str("BuffModType::NewEnumerator0".to_owned()));
    v.insert("SpeedMod".to_owned(), Value::Num(f64::from(walk / crate::math::narrow(max_walk))));
    set_buff_values(w, b, &v);
    update_states(w, f);
    if let Some(begin) = child.and_then(|c| c.begin) {
        begin(w, f, b, from, duration);
    }
}

/// 22. The Delay(0.02) @8241 -> PP@15: AddInvisibility (the pawn's mesh hidden) and SetActiveMesh(Polymorph).
fn poly_buff_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if !own(w, f, b) {
        return false;
    }
    let p = poly_mut(w, f).expect("the polymorph");
    if p.show_in <= 0 {
        return false;
    }
    p.show_in -= 1;
    if p.show_in > 0 {
        return false;
    }
    add_invisibility(w, f);
    poly_mut(w, f).expect("the polymorph").active = true;
    false
}

/// EvadeTimer (K2_SetTimerDelegate from EvadeResetTimer @10540): ResetEvade.
fn poly_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if !own(w, f, b) {
        return false;
    }
    let p = poly_mut(w, f).expect("the polymorph");
    if p.evade_timer <= 0 {
        return false;
    }
    p.evade_timer -= 1;
    if p.evade_timer == 0 {
        p.evade_timer = -1;
        reset_evade(w, f);
    }
    false
}

fn poly_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let child = buff(w, b).and_then(|x| child_of(w, x.cls));
    let r = own(w, f, b) && parent_on(w, f, b, child.as_ref(), s, a);
    if r {
        return true;
    }
    let Some(on) = child.and_then(|c| c.on) else { return false };
    if !buff(w, b).is_some_and(|x| x.frames > 0.0) {
        return false;
    }
    on(w, f, b, s, a)
}

/// OnBranchGroupBlocked (PP@11435), behind the pawn's.
fn poly_branch_blocked(w: &mut World, f: FighterId, b: BuffRef, n: NotifyRef, _montage: Name, duration: Real) {
    if own(w, f, b) {
        window_begin(w, f, n, duration);
    }
}

/// OnBranchGroupEndBlocked (PP@11777), behind the pawn's.
fn poly_branch_end_blocked(w: &mut World, f: FighterId, b: BuffRef, n: NotifyRef) {
    if own(w, f, b) {
        window_end(w, f, n);
    }
}

/// EndPlay, behind BuffParentActor's; then the child's own (`PolymorphChild::end`).
fn poly_end(w: &mut World, f: FighterId, b: BuffRef) {
    parent_end(w, f, b);
    if let Some(end) = buff(w, b).and_then(|x| child_of(w, x.cls)).and_then(|c| c.end) {
        end(w, f, b);
    }
}

/// The parent's graph: one kind for every child.
const POLYMORPH_KIND: BuffHooks = BuffHooks {
    begin: Some(poly_begin),
    tick: Some(poly_buff_tick),
    timer: Some(poly_timer),
    on: Some(poly_on),
    branch_group_blocked: Some(poly_branch_blocked),
    branch_group_end_blocked: Some(poly_branch_end_blocked),
    end: Some(poly_end),
    take_any_damage: None,
    actor_collision_off: None,
    max_stacks: None,
    stacks: None,
    cleanse: None,
    status: None,
    dealt_cond: None,
    taken_cond: None,
    attach_parent: None,
};

/// HandleDamageRecieved (PP@11402 -> the function @0-267): LocalDamageTaken = FMin(Hit.Damage, CurrentDamage),
/// CurrentDamage -= it; at 0 K2_DestroyActor @226. Hit.Damage as the DamageManager broadcasts it (the signal's
/// `hit`). `now`: the destroy runs here (`end_buff`: the EndPlay inside the call, a child's override going on after
/// it); else the caller's true destroys it once the handler returns (the parent's own handler: nothing follows the
/// call in it).
fn handle_damage(w: &mut World, f: FighterId, b: BuffRef, dmg: Option<Real>, now: bool) -> Real {
    if dmg.is_none() {
        warn_text(w, "Polymorph: a HitByEnemy without its hit: nothing counted");
    }
    let Some(p) = poly_mut(w, f) else { return 0.0 };
    let taken = min(dmg.unwrap_or(0.0), p.damage);
    p.damage -= taken;
    let gone = p.damage <= 0.0;
    if now && gone {
        end_buff(w, f, b);
    }
    taken
}

fn parent_on(
    w: &mut World,
    f: FighterId,
    b: BuffRef,
    child: Option<&PolymorphChild>,
    s: BuffSignal,
    a: &BuffSignalArg,
) -> bool {
    match s {
        BuffSignal::EvadeTriggered => {
            // PP@9068 (4.1): in the air (IsFalling, IsFlying or the knockback mode, @9209-9414) the first Evades row
            // of the direction, RequiredButtonInput and Conditions not read; on the ground the spot dodge for 4, the
            // forward one else.
            let air = is_falling(w, f) || is_flying(w, f) || w.fighter(f).hit.kb_mode;
            let name = if air {
                let d = inst_view(w, b);
                let rows = d.get("Evades").and_then(Value::as_arr);
                let row = rows.and_then(|r| {
                    r.iter()
                        .filter_map(Value::as_obj)
                        .find(|e| e.get("RequiredDirectionInput").and_then(Value::as_f64) == a.dir.map(f64::from))
                });
                row.and_then(|e| e.get("AttackMontage")).and_then(Value::as_str).map(|s| data_str(w, s))
            } else if a.dir == Some(4.0) {
                dname(w, b, "GroundSpotEvade")
            } else {
                dname(w, b, "GroundForwardEvade")
            };
            let len = play(w, f, name);
            // EvadeResetTimer(Montage_Play's return): a time of 0 clears the timer and sets none.
            let t = if len > 0.0 { crate::math::to_int32(timer_frames(Some(len))) } else { -1 };
            if let Some(p) = poly_mut(w, f) {
                p.evade_timer = t;
            }
            false
        }
        BuffSignal::DashTriggered => {
            // OnDash PP@10830: DashMontage for IsDashAnim, else DashTurnMontage.
            let name = dname(w, b, if a.value == Some(true) { "DashMontage" } else { "DashTurnMontage" });
            play(w, f, name);
            false
        }
        BuffSignal::InputPreTriggered => {
            // PP@12609: Taunt (4) while not attacking: the Taunt montage (a PlayMontage proxy whose events do
            // nothing).
            if a.input == Some(4) && !is_attacking(w, f) {
                let name = dname(w, b, "Taunt");
                play(w, f, name);
            }
            false
        }
        BuffSignal::IsJumping => {
            // PP@13519: a jump while evading stops the mesh's montage (Montage_Stop(0)) and resets the evade.
            if w.fighter(f).evade.evading && a.value == Some(true) {
                stop_play(w, f);
                reset_evade(w, f);
            }
            false
        }
        BuffSignal::CrouchChange => {
            let c = a.crouched == Some(true);
            poly_mut(w, f).expect("the polymorph").crouched = c; // Handle Crouch (PP@13949): the mesh's Z
            false
        }
        BuffSignal::HitStunEnded => {
            // PP@11124: SetCrouching(!isCrouching), then the handler unbinds itself.
            let p = poly_mut(w, f).expect("the polymorph");
            if !p.hitstun_bound {
                return false;
            }
            p.hitstun_bound = false;
            let c = !w.fighter(f).crouch.crouching;
            set_crouching(w, f, c);
            false
        }
        BuffSignal::HitByEnemy => {
            // PP@11402 HandleDamageRecieved: the child's override (PolymorphChild::damage) or the parent's function.
            let dmg = a.hit.as_ref().and_then(|h| h.damage);
            if let Some(damage) = child.and_then(|c| c.damage) {
                damage(w, f, b, a.hit.as_deref(), &mut |w: &mut World| handle_damage(w, f, b, dmg, true));
                return false;
            }
            handle_damage(w, f, b, dmg, false);
            poly(w, f).is_some_and(|p| p.damage <= 0.0)
        }
        _ => false,
    }
}

/// EndPlay, behind BuffParentActor's.
fn parent_end(w: &mut World, f: FighterId, b: BuffRef) {
    if !own(w, f, b) {
        return;
    }
    let child = buff(w, b).and_then(|x| child_of(w, x.cls));
    let key = loop_key(w, f, child.as_ref());
    // BPA@4282: StopLoopSound: SFX_Loop_Stop on the buff actor @4957, SFX_Loop_End at the owner's location @5132.
    if let Some(ev) = dname(w, b, "SFX_Loop_Stop") {
        emit(w, f, |fighter| Event::SoundStop { fighter, key, fade: None, ev: Some(ev), curve: None, cut: false });
    }
    if let Some(ev) = dname(w, b, "SFX_Loop_End") {
        let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
        emit(w, f, |fighter| Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    }
    // The buff actor's destroy stops what it posted (StopWhenOwnerDestroyed: StopAll, taken as no fade).
    emit(w, f, |fighter| Event::SoundStop { fighter, key, fade: Some(0.0), ev: None, curve: None, cut: false });
    // 2. Clear EvadeTimer; ResetEvade. 3. BPI_SetPolymorph(false).
    poly_mut(w, f).expect("the polymorph").evade_timer = -1;
    reset_evade(w, f);
    w.fighter_mut(f).shapes.is_polymorphed = false;
    // 4. SetActorScale3D(OriginalActorScale) @419: the scale saved at BeginPlay, stale after a size change of the
    // Size Manager since.
    let orig = poly(w, f).expect("the polymorph").orig;
    set_actor_scale(w, f, orig.0, orig.1);
    // 6. The floor snap: CapsuleTraceSingle @770 from the actor's location + (0, 0, the scaled half height) to the
    // location - (0, 0, 100), the restored capsule, trace channel 5, ignoring self; a blocking hit:
    // K2_SetActorLocation(its Location, no sweep, teleport) @1264.
    let (hh, ay, x, r) = (capsule_half(w, f), actor_y(w, f), w.fighter(f).body.x, cap_radius(w, f));
    let own_ref = Some(ActorRef::Fighter(f));
    let h = capsule_trace_single(w, x, ay + hh, x, ay - 100.0, r, hh, TRACE_GROUND_NO_PLATFORM, &[], own_ref);
    if let Some(h) = h.filter(|h| h.blocking) {
        let off = cap_half(w, f) - capsule_half(w, f);
        let fi = w.fighter_mut(f);
        fi.body.x = h.x;
        fi.body.y = h.y + off;
    }
    // 7. BPI_EnableAttack @1326; BlockEvadeMontage false.
    enable_attack(w, f);
    w.fighter_mut(f).evade.block_evade_montage = false;
    // 8. ResetActiveMesh, RemoveMesh, the mesh destroyed @1529: a Queued state of its montage gets its NotifyEnd
    // there (a BranchWindow's reaches the still bound handler), a BranchingPoint one none.
    // Deviation: the BranchingPoint states other than the windows are ended here too (the port keeps no state of a
    // destroyed mesh); the windows stay listed for the loop below. RemoveInvisibility @1574.
    if let Some(uid) = play_uid(w, f) {
        if state_ref(w, f, uid).is_some_and(|s| !s.active.is_empty()) {
            with_poly(w, f, uid, Sink::EndQueued, |h| h.end_queued());
            with_poly(w, f, uid, Sink::EndBranching, |h| h.end_branching());
        }
        clear_charge_notify(w, f);
        if let Some(pp) = poly_mut(w, f).and_then(|p| p.play.take()) {
            drop_state(w, f, pp.m);
        }
    } else {
        clear_charge_notify(w, f);
    }
    // @1574: RemoveInvisibility with no test (before the Delay(0.02)'s Add too: it lowers a count above 0 that
    // another source holds, as the build's would).
    remove_invisibility(w, f);
    // 9-10. The unbinds, then the loop over BranchWindows @2201-2449 with the end burst in its body: the handler
    // removes the window from the list the loop walks and the index still goes up, so every second window is left
    // filed; the burst runs once per window handled and once at the loop's exit.
    let vfx = dname_or_none(w, b, "PolymorphChangeVFX");
    let end_sfx = dname(w, b, "PolymorphEndSFX");
    let mut i = 0;
    while let Some(n) = poly(w, f).and_then(|p| p.windows.get(i).copied()) {
        window_end(w, f, n);
        end_burst(w, f, vfx, end_sfx);
        i += 1;
    }
    end_burst(w, f, vfx, end_sfx);
    w.fighter_mut(f).shapes.polymorph = None;
}

/// This file's registry entries: Buff_C020Polymorph, the parent's graph and CP's BeginPlay (CP@20-141:
/// Creature_AnimBP CreatureHeadScale (2,2,2)). Every child of Buff_PolymorphParent is entered by name: Buff_Chicken
/// by Taz's module (Buff_Tasty @2081).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    polymorph_child(
        r,
        "Buff_C020Polymorph_C",
        BuffClass::BuffC020Polymorph,
        PolymorphChild { head: Some(2.0), ..PolymorphChild::new(name!("Buff_C020Polymorph_C:")) },
    )
}
