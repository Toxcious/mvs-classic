//! A spawned actor's montages: one montage instance per actor mesh (its `anim`, an
//! [`ActorAnim`]), rollback state as the fighter's, advanced with the fighter's clock (DT). Its notifies go through
//! the notify classes' actor hooks (`registry::notifies`) with the actor as MeshComp's owner; the
//! PlayMontageCallbackProxy's events go to the class's `MeshHooks::montage_event`.
//!
//! The mesh's tick: a spawned actor's SkeletalMeshComponent ticks in TG_PrePhysics. USkeletalMeshComponent::
//! TickComponent: TickPose (the anim instance's update: Montage_Advance, the BranchingPoint notifies inside it), then
//! the queued notifies (TriggerAnimNotifies), then every queued OnMontageBlendingOut, then every OnMontageEnded, in
//! the same frame (inside the anim update the montage events are queued, all blend-outs ahead of all endeds). The
//! world calls [`mesh_tick`] right behind the actor's own tick and ahead of its physics.
//! Unverified: the actor ticks ahead of its mesh (the 4.26 source's reading: the component's tick function has the
//! actor's as a prerequisite only when bTickInEditor / AddPrerequisite says so; nothing in the exe was read for these
//! classes).
//!
//! PlayMontageCallbackProxy: PlayMontage = Montage_Play(rate, StartingPosition, bStopAllMontages true),
//! OnInterrupted at once when it fails; OnMontageBlendingOut: interrupted -> OnInterrupted (and
//! bInterruptedCalledBeforeBlendingOut), else OnBlendOut; OnMontageEnded: not interrupted -> OnCompleted, interrupted
//! -> OnInterrupted unless the flag is set; then the proxy unbinds. Montage_Play outside the anim update (a graph's
//! call) broadcasts the stopped instance's events at once (row 788).
//! The Stop of the old instance (FAnimMontageInstance::Stop 142865260): at full weight it queues the blending-out
//! event (interrupted), broadcast at once: OnInterrupted, and the ended event adds nothing; one already blending out
//! (its OnBlendOut fired) has its blend shortened and bInterrupted set, no event: its ended event at its termination,
//! in the next mesh tick (BlendIn 0: the blend is over in that tick's update), gives OnInterrupted (BlendOut +
//! Interrupted). `ActorAnim::owed` holds those.
//! Deviation: a Montage_Play over a playing instance ends the old one's notify states at once, and the old instance's
//! blend-out under the new one is not kept (every actor montage of the data has BlendIn 0, so it has none to keep: a
//! BlendIn > 0 over a playing instance is warned).
//! The calls that take no montage (Montage_JumpToSection, Montage_GetCurrentSection, Montage_SetPlayRate:
//! FUN_142840820, 142840400, 142841100) act on or read only an instance that is not stopped: none once its blend-out
//! began (its own auto blend-out included) or it was stopped.
//!
//! A placed stage actor with an `anim` (`StageMeshActor`) is the same host; no class of the ported
//! stages holds one, so [`anim_of`] answers none for `ActorRef::Stage` and its hooks come with its port.
//!
//! An instance is found by its number ([`MontageKey`]) on each access, as the fighter's are: a handler may play
//! another montage while the instance runs. One dropped from its place while a player still runs it (the player
//! object outlives its place) is kept in the World's scratch (`actor_limbo`) until that run ends.

use crate::Real;
use crate::State;
use crate::actors::ActorId;
use crate::actors::ActorRef;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::MontageState;
use crate::fighter::anim::MontageKey;
use crate::fighter::common::{lookup_owner, peers, warn_parts};
use crate::names::Name;
use crate::registry::actor_notifies::MeshActorView;
use crate::registry::entries::MeshHooks;
use crate::world::World;
use mvs_anim::montage::{Host, Montage, Play};
use mvs_data::GameData;
use mvs_data::value::{Props, Value};

// ---- the host

/// The PlayMontageCallbackProxy's OnBlendOut / OnCompleted /
/// OnInterrupted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MontageEvent {
    BlendOut,
    Completed,
    Interrupted,
}

/// One skeletal mesh's anim instance as far as its montages go.
#[derive(State, Debug, Default)]
pub struct ActorAnim {
    /// The mesh component's name.
    pub mesh: Name,
    pub m: Option<MontageState>,
    /// A PlayMontageCallbackProxy is bound to the instance.
    pub proxy: bool,
    /// OnMontageEnded broadcast for the instance.
    pub ended: bool,
    /// The montages of earlier instances stopped while blending out with a proxy bound: their OnMontageEnded
    /// (interrupted -> OnInterrupted) is broadcast at their termination, in the next mesh tick.
    pub owed: Vec<Name>,
    /// The instances a Montage_Play with bStopAllMontages false added beside `m` (C018_Platform's HitReactFront /
    /// Back @18981 and BounceMontage @19504), in play order; `None` while there are none (`montagePlaySide`).
    pub side: Option<Vec<MontageState>>,
}

impl ActorAnim {
    pub fn new(mesh: Name) -> Self {
        ActorAnim { mesh, ..ActorAnim::default() }
    }
}

/// The [`ActorAnim`] of an actor whose class holds one (an `AnimHost`): Rick's Meeseeks (`MeeseeksSummon`), the item
/// Meeseeks' runner (`MeeseeksRun`), Neptr (`Nepter`), Jake's house (`JakeHouse`); `None` for any other actor.
/// Meeseeks' runner (`MeeseeksRun`), Neptr (`Nepter`), Jerry (`JerryActor`); `None` for any other actor.
/// Meeseeks' runner (`MeeseeksRun`), Neptr (`Nepter`), Marvin's UFO (`Ufo`); `None` for any other actor.
pub fn anim_of(w: &World, a: ActorRef) -> Option<&ActorAnim> {
    use crate::actors::{ProjectileExt, WorldActorState};
    match a {
        ActorRef::Projectile(id) => match &w.actors.projectiles.get(id)?.ext {
            ProjectileExt::MeeseeksSummon(s) => Some(&s.anim),
            ProjectileExt::MeeseeksRun(s) => Some(&s.anim),
            e @ (ProjectileExt::GizmoCar(_) | ProjectileExt::ItemCar(_) | ProjectileExt::BatMobileCar(_)) => {
                crate::chars::gizmo::car::car_of(e).map(|c| &c.anim)
            }
            _ => None,
        },
        ActorRef::Actor(id) => match &w.actors.actors.get(id)?.state {
            WorldActorState::Nepter(s) => Some(&s.anim),
            WorldActorState::JakeHouse(s) => Some(&s.anim),
            WorldActorState::JerryActor(s) => Some(&s.anim),
            WorldActorState::JenkinsPin(s) => Some(&s.anim),
            WorldActorState::Ufo(s) => Some(&s.anim),
            _ => None,
        },
        _ => None,
    }
}

/// [`anim_of`], to write.
pub fn anim_of_mut(w: &mut World, a: ActorRef) -> Option<&mut ActorAnim> {
    use crate::actors::{ProjectileExt, WorldActorState};
    match a {
        ActorRef::Projectile(id) => match &mut w.actors.projectiles.get_mut(id)?.ext {
            ProjectileExt::MeeseeksSummon(s) => Some(&mut s.anim),
            ProjectileExt::MeeseeksRun(s) => Some(&mut s.anim),
            e @ (ProjectileExt::GizmoCar(_) | ProjectileExt::ItemCar(_) | ProjectileExt::BatMobileCar(_)) => {
                crate::chars::gizmo::car::car_of_mut(e).map(|c| &mut c.anim)
            }
            _ => None,
        },
        ActorRef::Actor(id) => match &mut w.actors.actors.get_mut(id)?.state {
            WorldActorState::Nepter(s) => Some(&mut s.anim),
            WorldActorState::JakeHouse(s) => Some(&mut s.anim),
            WorldActorState::JerryActor(s) => Some(&mut s.anim),
            WorldActorState::JenkinsPin(s) => Some(&mut s.anim),
            WorldActorState::Ufo(s) => Some(&mut s.anim),
            _ => None,
        },
        _ => None,
    }
}

/// `hasAnim` / `stageHasAnim`: the actor holds an [`ActorAnim`] (a montage-playing
/// mesh). The classes that hold one come with their ports, each with its `anim` field and its arm in [`anim_of`].
pub fn has_anim(w: &World, a: ActorRef) -> bool {
    anim_of(w, a).is_some()
}

/// The actor's owner (`a.owner`): its owner, else the fighter its class is looked up through.
fn owner(w: &World, a: ActorRef) -> FighterId {
    lookup_owner(w, a)
}

/// `a.dead`: an actor gone from its arena is dead.
fn dead(w: &World, a: ActorRef) -> bool {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_none_or(|p| p.dead),
        ActorRef::Actor(id) => w.actors.actors.get(id).is_none_or(|p| p.dead),
        _ => false,
    }
}

/// The actor's class (`a.cls`).
fn cls(w: &World, a: ActorRef) -> Name {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map_or(Name::NONE, |p| p.cls),
        ActorRef::Actor(id) => w.actors.actors.get(id).map_or(Name::NONE, |p| p.cls),
        _ => Name::NONE,
    }
}

/// `meshHooks`: the class's mesh hooks (its `ACTORS` entry's).
fn mesh_hooks(w: &World, a: ActorRef) -> Option<MeshHooks> {
    w.registry.actors.get(w.names.str(cls(w, a))).and_then(|e| e.mesh)
}

/// `montageOf`: the actor's montage by name, from the class's character data (the owner's, else a peer's: actorDef's
/// rule), else, in a match with items, the items part's (a random item's actor with no character of its class in
/// the match: the Meeseek runner, Neptr; ItemsData.actorMontages).
pub fn montage_of<'d>(d: &'d GameData, w: &World, a: ActorRef, name: &str) -> Option<&'d Montage> {
    let of = |f: FighterId| d.chars.get(w.names.str(w.fighter(f).ch))?.fighter.actor_montages.as_ref()?.get(name);
    if let Some(m) = of(owner(w, a)) {
        return Some(m);
    }
    for &o in peers(w) {
        if let Some(m) = of(o) {
            return Some(m);
        }
    }
    if crate::setup::items_on(&w.setup) {
        return d.items.as_ref()?.actor_montages.get(name);
    }
    None
}

/// Where an instance of the actor's mesh is.
#[derive(Clone, Copy)]
enum Loc {
    M,
    Side(usize),
    Limbo(usize),
    Spare,
}

fn loc(w: &World, a: ActorRef, uid: u32) -> Loc {
    if let Some(an) = anim_of(w, a) {
        if an.m.as_ref().is_some_and(|s| s.montage.uid == uid) {
            return Loc::M;
        }
        if let Some(k) = an.side.as_ref().and_then(|l| l.iter().position(|s| s.montage.uid == uid)) {
            return Loc::Side(k);
        }
    }
    match w.scratch.actor_limbo.iter().position(|(x, s)| *x == a && s.montage.uid == uid) {
        Some(k) => Loc::Limbo(k),
        None => Loc::Spare,
    }
}

/// The state of instance `uid`, where it is (`None`: nowhere).
fn state(w: &World, a: ActorRef, uid: u32) -> Option<&MontageState> {
    match loc(w, a, uid) {
        Loc::M => anim_of(w, a)?.m.as_ref(),
        Loc::Side(k) => anim_of(w, a)?.side.as_ref()?.get(k),
        Loc::Limbo(k) => Some(&w.scratch.actor_limbo[k].1),
        Loc::Spare => None,
    }
}

fn state_mut(w: &mut World, a: ActorRef, uid: u32) -> Option<&mut MontageState> {
    match loc(w, a, uid) {
        Loc::M => anim_of_mut(w, a)?.m.as_mut(),
        Loc::Side(k) => anim_of_mut(w, a)?.side.as_mut()?.get_mut(k),
        Loc::Limbo(k) => Some(&mut w.scratch.actor_limbo[k].1),
        Loc::Spare => None,
    }
}

/// A state dropped from its place: kept in limbo while a player still runs it.
fn drop_state(w: &mut World, a: ActorRef, s: MontageState) {
    if w.scratch.actor_running.contains(&(a, s.montage.uid)) {
        w.scratch.actor_limbo.push((a, s));
    }
}

/// Marks the instances as run by a player (their states kept if a handler drops them).
fn run_begin(w: &mut World, a: ActorRef, uids: &[u32]) {
    w.scratch.actor_running.extend(uids.iter().map(|&u| (a, u)));
}

/// [`run_begin`]'s end: the limbo keeps only the states still run.
fn run_end(w: &mut World, a: ActorRef, uids: &[u32]) {
    let sc = &mut w.scratch;
    for &u in uids.iter().rev() {
        if let Some(k) = sc.actor_running.iter().rposition(|&r| r == (a, u)) {
            sc.actor_running.remove(k);
        }
    }
    let run = std::mem::take(&mut sc.actor_running);
    sc.actor_limbo.retain(|(x, s)| run.contains(&(*x, s.montage.uid)));
    sc.actor_running = run;
}

/// One instance as the player sees it: its montage, its state found by number on each access, and the actor's sink.
struct AHost<'w, 'm> {
    w: &'w mut World,
    a: ActorRef,
    key: MontageKey,
    m: &'m Montage,
    spare: MontageState,
}

impl<'m> Host<'m, MontageKey> for AHost<'_, 'm> {
    fn m(&self) -> &'m Montage {
        self.m
    }
    fn s(&mut self) -> &mut MontageState {
        let (w, a, uid) = (&mut *self.w, self.a, self.key.uid);
        match loc(w, a, uid) {
            Loc::M => anim_of_mut(w, a).and_then(|an| an.m.as_mut()).expect("the instance found"),
            Loc::Side(k) => {
                anim_of_mut(w, a).and_then(|an| an.side.as_mut()).map(|l| &mut l[k]).expect("the instance found")
            }
            Loc::Limbo(k) => &mut w.scratch.actor_limbo[k].1,
            Loc::Spare => &mut self.spare,
        }
    }
    fn begin(&mut self, idx: usize) {
        sink(self.w, self.a, self.m, self.key.name, idx, Call::Begin);
    }
    fn end(&mut self, idx: usize) {
        sink(self.w, self.a, self.m, self.key.name, idx, Call::End);
    }
    fn instant(&mut self, idx: usize) {
        sink(self.w, self.a, self.m, self.key.name, idx, Call::Instant);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Call {
    Begin,
    End,
    Instant,
}

fn u32x(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// `SINK`: the notify classes' actor hooks, the actor as the owner. A class with no actor hook is reported at its
/// begin or instant (no default).
fn sink(w: &mut World, a: ActorRef, m: &Montage, name: Name, idx: usize, c: Call) {
    let n = &m.notifies[idx];
    let r = NotifyRef { montage: name, index: u32x(idx) };
    let h = w.registry.notifies.get(&n.cls).copied();
    let h = h.filter(|h| h.actor_begin.is_some() || h.actor_end.is_some() || h.actor_instant.is_some());
    let Some(h) = h else {
        if c != Call::End {
            let ac = w.names.str(cls(w, a)).to_owned();
            warn_parts(w, &["actor notify class not handled: ", &n.cls, " (", &ac, ")"]);
        }
        return;
    };
    match c {
        Call::Begin => {
            if let Some(f) = h.actor_begin {
                f(w, a, r, u32x(idx));
            }
        }
        Call::End => {
            if let Some(f) = h.actor_end {
                f(w, a, r, u32x(idx));
            }
        }
        Call::Instant => {
            if let Some(f) = h.actor_instant {
                f(w, a, r);
            }
        }
    }
}

/// `withSink`: runs `body` over instance `key` of the actor's mesh with the actor's sink (`curA`, `curP` set and
/// restored), the instance marked as run.
fn with_sink<R>(w: &mut World, a: ActorRef, key: MontageKey, m: &Montage, body: impl FnOnce(&mut AHost) -> R) -> R {
    let prev = w.scratch.actor_sink.replace((a, key.uid));
    run_begin(w, a, &[key.uid]);
    let r = body(&mut AHost { w, a, key, m, spare: MontageState::new(key, false) });
    run_end(w, a, &[key.uid]);
    w.scratch.actor_sink = prev;
    r
}

// ---- the call surface of the actor montages

/// AnimNotify_PlayMontageNotify / AnimNotify_PlayMontageNotifyWindow on
/// an actor's mesh (the classes' entries in `registry::notifies` call this from the sink, so `begin` comes inside
/// the montage advance at the marker: both classes are branching points by their CDO). The broadcast
/// BranchingPointNotify 142459ad0 / NotifyBegin 142459b30 / NotifyEnd 142459b90 makes OnPlayMontageNotifyBegin /
/// End(NotifyName, payload) of the mesh's anim instance. The PlayMontageCallbackProxy's OnNotifyBeginReceived
/// 142486780 / OnNotifyEndReceived 1424867b0 forward the name to OnNotifyBegin / End only when the payload's
/// MontageInstanceID is the one its PlayMontage stored (142486da0): the proxy bound to the instance the notify comes
/// from, until its OnMontageEnded unbinds it (14248f590). An ActorAnim holds one proxy, bound to its current instance
/// (`proxy`, `ended`): a notify of an instance played without one, or of an earlier instance, reaches nothing.
/// A window open when its instance is stopped by a Montage_Play over it: the build ends it at the instance's
/// Terminate, after the Stop's OnInterrupted; the port's stop ends it at once ([`montage_play`]), so its OnNotifyEnd
/// is held until that OnInterrupted has been sent (`deferred`). A point notify gives a begin only.
pub fn proxy_notify(w: &mut World, a: ActorRef, n: NotifyRef, begin: bool) {
    let Some(an) = anim_of(w, a) else { return };
    let Some(m) = an.m.as_ref() else { return };
    let cur = w.scratch.actor_sink.filter(|s| s.0 == a).map(|s| s.1);
    if !an.proxy || an.ended || dead(w, a) || cur != Some(m.montage.uid) {
        return;
    }
    let montage = m.montage.name;
    let data = w.data.clone();
    let name = crate::registry::props::text(&actor_notify_data(w, &data, a, n).p, "NotifyName")
        .map_or(crate::name!("None"), |s| crate::registry::props::name(w, s));
    if !begin && let Some(d) = w.scratch.actor_deferred.as_mut() {
        d.push((a, name, montage));
        return;
    }
    let h = mesh_hooks(w, a);
    let f = if begin { h.and_then(|h| h.notify_begin) } else { h.and_then(|h| h.notify_end) };
    if let Some(f) = f {
        f(w, a, name, montage);
    } else if begin {
        let (ac, nm) = (w.names.str(cls(w, a)).to_owned(), w.names.str(name).to_owned());
        warn_parts(w, &["PlayMontage proxy notify with no handler: ", &ac, " ", &nm]);
    }
}

fn event(w: &mut World, a: ActorRef, ev: MontageEvent, montage: Name) {
    match mesh_hooks(w, a).and_then(|h| h.montage_event) {
        Some(h) => h(w, a, ev, montage),
        None => {
            let ac = w.names.str(cls(w, a)).to_owned();
            let e = match ev {
                MontageEvent::BlendOut => "blendOut",
                MontageEvent::Completed => "completed",
                MontageEvent::Interrupted => "interrupted",
            };
            warn_parts(w, &["montage event with no handler: ", &ac, " ", e]);
        }
    }
}

/// `stopInstance`: the Stop of the actor's current instance (see the head comment), by a Montage_Play over it or a
/// Montage_Stop(0) ([`montage_stop`]): at
/// full weight OnMontageBlendingOut(interrupted) -> OnInterrupted now; already blending out, OnMontageEnded
/// (interrupted) -> OnInterrupted at its termination.
fn stop_instance(w: &mut World, a: ActorRef, data: &GameData, key: MontageKey) {
    let Some(old) = state(w, a, key.uid) else { return };
    let live = !old.ended && !old.terminated;
    let was_fired = old.blend_out_fired;
    let an = anim_of(w, a).expect("an actor with an anim");
    let bound = live && an.proxy && !an.ended;
    let hold = bound && !old.blend_out_fired; // its named windows' OnNotifyEnd after OnInterrupted (proxy_notify)
    let pd = if hold { w.scratch.actor_deferred.replace(Vec::new()) } else { None };
    let fired = match montage_of(data, w, a, w.names.str(key.name)) {
        Some(md) => with_sink(w, a, key, md, |h| {
            h.stop();
            h.s().blend_out_fired
        }),
        None => was_fired,
    };
    let held = if hold { std::mem::replace(&mut w.scratch.actor_deferred, pd) } else { None };
    if let Some(an) = anim_of_mut(w, a)
        && let Some(s) = an.m.take()
    {
        drop_state(w, a, s);
    }
    if bound && fired {
        if let Some(an) = anim_of_mut(w, a) {
            an.owed.push(key.name);
        }
    } else if bound {
        event(w, a, MontageEvent::Interrupted, key.name);
    }
    if let Some(held) = held {
        let h = mesh_hooks(w, a).and_then(|h| h.notify_end);
        for (ea, name, montage) in held {
            if let Some(h) = h
                && !dead(w, a)
            {
                h(w, ea, name, montage);
            }
        }
    }
}

/// Montage_Play of `name` on the actor's anim at `rate` from `start`
/// (bStopAllMontages true), `proxy` a PlayMontageCallbackProxy bound; false where the montage does not play (a
/// missing one is warned).
pub fn montage_play(w: &mut World, a: ActorRef, name: Name, rate: Real, start: Real, proxy: bool) -> bool {
    if anim_of(w, a).is_none() {
        return false;
    }
    let data = w.data.clone();
    let m = montage_of(&data, w, a, w.names.str(name));
    // the side instances of the new montage's group, last first (Montage_PlayInternal walks MontageInstances
    // backwards; they have no proxy, so their stop sends no event)
    if let Some(m) = m {
        let mut i = anim_of(w, a).and_then(|an| an.side.as_ref()).map_or(0, Vec::len);
        while i > 0 {
            i -= 1;
            let Some(side) = anim_of(w, a).and_then(|an| an.side.as_ref()) else { break };
            let Some(s) = side.get(i) else { continue };
            let key = s.montage;
            let sm = montage_of(&data, w, a, w.names.str(key.name));
            if sm.is_none_or(|sm| sm.group != m.group) {
                continue;
            }
            let s = anim_of_mut(w, a).and_then(|an| an.side.as_mut()).expect("the side list").remove(i);
            run_begin(w, a, &[key.uid]);
            w.scratch.actor_limbo.push((a, s));
            if let Some(sm) = sm {
                with_sink(w, a, key, sm, |h| h.stop());
            }
            run_end(w, a, &[key.uid]);
        }
        if let Some(an) = anim_of_mut(w, a)
            && an.side.as_ref().is_some_and(Vec::is_empty)
        {
            an.side = None;
        }
    }
    if let Some(old) = anim_of(w, a).and_then(|an| an.m.as_ref()) {
        let key = old.montage;
        if !old.ended && !old.terminated && m.is_some_and(|m| m.blend_in > 0.0) {
            let (n, o) = (w.names.str(name).to_owned(), w.names.str(key.name).to_owned());
            warn_parts(w, &["actor montage ", &n, " blends in over ", &o, ": the old one is ended at once"]);
        }
        stop_instance(w, a, &data, key);
    }
    let Some(an) = anim_of_mut(w, a) else { return false };
    an.proxy = false;
    an.ended = false;
    if m.is_none() {
        let (n, ac) = (w.names.str(name).to_owned(), w.names.str(cls(w, a)).to_owned());
        warn_parts(w, &["actor montage missing: ", &n, " (", &ac, ")"]);
        if proxy {
            event(w, a, MontageEvent::Interrupted, name); // PlayMontage's failed Montage_Play
        }
        return false;
    }
    let mut p = crate::fighter::anim::montages::new_instance(w, owner(w, a), name, false);
    p.play_rate = f64::from(rate);
    if start > 0.0 {
        p.pos = f64::from(start);
    }
    let Some(an) = anim_of_mut(w, a) else { return false };
    if let Some(s) = an.m.replace(p) {
        drop_state(w, a, s);
    }
    if let Some(an) = anim_of_mut(w, a) {
        an.proxy = proxy;
    }
    true
}

/// `notStopped`: the instance the montage-less calls reach (see the head comment).
fn not_stopped(an: &ActorAnim) -> Option<&MontageState> {
    an.m.as_ref().filter(|p| !p.ended && !p.blending_out())
}

/// Montage_JumpToSection(Section, the current montage): false
/// when no montage plays or it has no such section.
pub fn montage_jump_to_section(w: &mut World, a: ActorRef, section: Name) -> bool {
    let Some(p) = anim_of(w, a).and_then(not_stopped) else { return false };
    let key = p.montage;
    let data = w.data.clone();
    let Some(md) = montage_of(&data, w, a, w.names.str(key.name)) else { return false };
    let sec = w.names.str(section).to_owned();
    let ok = anim_of_mut(w, a).and_then(|an| an.m.as_mut()).is_some_and(|s| s.jump_to_section(md, &sec));
    if !ok {
        let n = w.names.str(key.name).to_owned();
        warn_parts(w, &["actor montage ", &n, " has no section ", &sec]);
    }
    ok
}

/// Montage_GetCurrentSection, the section at the position of
/// the active instance; `Name::NONE` for none.
pub fn montage_current_section(w: &World, a: ActorRef) -> Name {
    let Some(p) = anim_of(w, a).and_then(not_stopped) else { return Name::NONE };
    let Some(md) = montage_of(&w.data, w, a, w.names.str(p.montage.name)) else { return Name::NONE };
    let i = p.section_index_at(md, crate::math::dbl::max(p.pos, 0.0));
    md.sections.get(i).map_or(Name::NONE, |s| crate::registry::props::name(w, &s.name))
}

/// Montage_SetPlayRate(the active montage, rate).
pub fn montage_set_play_rate(w: &mut World, a: ActorRef, rate: Real) {
    let Some(an) = anim_of_mut(w, a) else { return };
    if let Some(p) = an.m.as_mut().filter(|p| !p.ended && !p.blending_out()) {
        p.play_rate = f64::from(rate);
    }
}

/// Montage_Play(montage, rate, 0, start, bStopAllMontages false) on the
/// actor's mesh, with no proxy: Montage_PlayInternal stops no instance with the flag clear, not even one of its own
/// group (none of these montages has root motion, which would stop the root-motion instance), and appends the new
/// one. A proxy bound to `m` stays bound, so its OnCompleted still comes (FireRocketMontage's reload under a HitReact
/// or a Bounce on the AdditiveSlot). False where the montage does not play (a missing one is warned).
// Unverified: the montage-less calls (montage_active, montage_jump_to_section, ...) keep reaching `m` while side
// instances play; the build's GetActiveMontageInstance may answer the newest (no class that plays side instances
// calls them).
// Unverified: the side instances advance after `m` in the mesh tick (play order when `m` is the older one).
pub fn montage_play_side(w: &mut World, a: ActorRef, name: Name, rate: Real, start: Real) -> bool {
    if anim_of(w, a).is_none() {
        return false;
    }
    let data = w.data.clone();
    if montage_of(&data, w, a, w.names.str(name)).is_none() {
        let (n, ac) = (w.names.str(name).to_owned(), w.names.str(cls(w, a)).to_owned());
        warn_parts(w, &["actor montage missing: ", &n, " (", &ac, ")"]);
        return false;
    }
    let mut p = crate::fighter::anim::montages::new_instance(w, owner(w, a), name, false);
    p.play_rate = f64::from(rate);
    if start > 0.0 {
        p.pos = f64::from(start);
    }
    let Some(an) = anim_of_mut(w, a) else { return false };
    an.side.get_or_insert_with(Vec::new).push(p);
    true
}

/// Every instance of the mesh that is not ended, `m` first, then the
/// side instances in play order (the drawing's slots: the newest instance with a track in a slot is the one it shows
/// there). Empty for an actor without an [`ActorAnim`].
pub fn mesh_instances(w: &World, a: ActorRef) -> Vec<&MontageState> {
    let Some(an) = anim_of(w, a) else { return Vec::new() };
    let mut out: Vec<&MontageState> = an.m.iter().filter(|p| !p.ended).collect();
    if let Some(side) = &an.side {
        out.extend(side.iter().filter(|s| !s.ended));
    }
    out
}

/// Montage_Stop(0, Montage) on the actor's mesh (Jerry_SpawnedActor
/// HitVisuals @22431 with no montage: any; StopMontageMulticast @29648 with one): the instance that is not stopped,
/// when it plays that montage (`name`; `None`: any), stopped as a Montage_Play over it stops it (`stop_instance`).
/// The proxy stays unbound after it, as after that Montage_Play.
// Unverified: Montage_Stop's BlendOut 0 is taken as the Stop of the head comment; an instance already blending out is
// not reached (the calls that take no montage skip it: `not_stopped`).
pub fn montage_stop(w: &mut World, a: ActorRef, name: Option<Name>) {
    let Some(p) = anim_of(w, a).and_then(not_stopped) else { return };
    let key = p.montage;
    if name.is_some_and(|n| n != key.name) {
        return;
    }
    let data = w.data.clone();
    stop_instance(w, a, &data, key);
    if let Some(an) = anim_of_mut(w, a) {
        an.proxy = false;
        an.ended = false;
    }
}

/// GetCurrentActiveMontage / Montage_IsActive, the montage of the
/// instance that is not stopped; `None` without one.
pub fn montage_active(w: &World, a: ActorRef) -> Option<Name> {
    anim_of(w, a).and_then(not_stopped).map(|p| p.montage.name)
}

/// Montage_IsPlaying(montage) of the actor's mesh, the instance
/// that is not stopped (GetCurrentActiveMontage FUN_14283c270, Montage_IsPlaying FUN_142840760; one blending out is
/// stopped).
pub fn montage_is_playing(w: &World, a: ActorRef, name: Name) -> bool {
    montage_active(w, a) == Some(name)
}

/// Montage_Advance with its BranchingPoint notifies, the queued notifies,
/// then the proxy's blend-out events, then the ended events: the stopped instances' (older, so first in the queue),
/// then the current one's.
pub fn mesh_tick(w: &mut World, a: ActorRef) {
    let Some(an) = anim_of_mut(w, a) else { return };
    let owed = if an.owed.is_empty() { None } else { Some(std::mem::take(&mut an.owed)) };
    let p = an.m.as_ref().map(|s| (s.montage, s.terminated));
    // Every instance a player object is held of in this tick is kept while a handler drops it.
    let mut run: Vec<u32> = p.iter().map(|(k, _)| k.uid).collect();
    run_begin(w, a, &run);
    let data = w.data.clone();
    let md = |w: &World, k: MontageKey| montage_of(&data, w, a, w.names.str(k.name));
    let term_before = p.is_some_and(|(_, t)| t);
    let alive = match p {
        Some((k, _)) => md(w, k).is_some_and(|m| with_sink(w, a, k, m, |h| h.advance(crate::consts::DT, false, false))),
        None => false,
    };
    let cur = |w: &World| anim_of(w, a).and_then(|an| an.m.as_ref()).map(|s| s.montage.uid);
    let own = p.is_some_and(|(k, _)| cur(w) == Some(k.uid)); // (not when a handler played another montage)
    // The side instances (montage_play_side), behind `m`: Montage_Advance returns right after an Advance that
    // terminated its instance, so the ones behind it skip that frame's Advance (the player's `skip`; their weights
    // still update), a side instance's ending included.
    // Deviation: each instance's queued notifies are dispatched right behind its own Advance, not after every
    // instance's.
    if let Some(side) = anim_of(w, a).and_then(|an| an.side.as_ref()) {
        let sides: Vec<MontageKey> = side.iter().map(|s| s.montage).collect();
        let uids: Vec<u32> = sides.iter().map(|k| k.uid).collect();
        run_begin(w, a, &uids);
        run.extend(uids);
        let p_term = p.is_some_and(|(k, _)| state(w, a, k.uid).is_some_and(|s| s.terminated));
        let mut skip = p.is_some() && !term_before && p_term;
        for &k in &sides {
            if dead(w, a) {
                break;
            }
            let Some(m) = md(w, k) else { continue };
            let was_term = state(w, a, k.uid).is_some_and(|s| s.terminated);
            let (live, term) = with_sink(w, a, k, m, |h| (h.advance(crate::consts::DT, skip, false), h.s().terminated));
            if !skip && !was_term && term {
                skip = true;
            }
            if !live
                && let Some(side) = anim_of_mut(w, a).and_then(|an| an.side.as_mut())
                && let Some(i) = side.iter().position(|s| s.montage.uid == k.uid)
            {
                let s = side.remove(i);
                drop_state(w, a, s);
            }
        }
        if let Some(an) = anim_of_mut(w, a)
            && an.side.as_ref().is_some_and(Vec::is_empty)
        {
            an.side = None;
        }
    }
    // The states the dispatch began after a handler stopped the instance (a Montage_Play over it: stop_instance):
    // the stop dropped it, so they end now, as the stop ended the others.
    // Deviation: the build ends them at the next dispatch.
    if let Some((k, _)) = p
        && !own
        && state(w, a, k.uid).is_some_and(|s| s.ended && !s.active.is_empty())
        && let Some(m) = md(w, k)
    {
        with_sink(w, a, k, m, |h| h.end_queued());
    }
    let proxy = |w: &World| anim_of(w, a).is_some_and(|an| an.proxy);
    if let Some((k, _)) = p
        && own
        && proxy(w)
        && !dead(w, a)
        && state(w, a, k.uid).is_some_and(|s| s.blending_out() && !s.blend_out_fired)
    {
        if let Some(s) = state_mut(w, a, k.uid) {
            s.blend_out_fired = true;
        }
        event(w, a, MontageEvent::BlendOut, k.name); // (an auto blend-out is never interrupted)
    }
    for name in owed.into_iter().flatten() {
        if !dead(w, a) {
            event(w, a, MontageEvent::Interrupted, name);
        }
    }
    if let Some((k, _)) = p
        && own
        && proxy(w)
        && !dead(w, a)
        && cur(w) == Some(k.uid)
        && state(w, a, k.uid).is_some_and(|s| s.terminated)
        && anim_of(w, a).is_some_and(|an| !an.ended)
    {
        if let Some(an) = anim_of_mut(w, a) {
            an.ended = true;
        }
        event(w, a, MontageEvent::Completed, k.name);
    }
    if let Some((k, _)) = p
        && !alive
        && cur(w) == Some(k.uid)
        && let Some(an) = anim_of_mut(w, a)
        && let Some(s) = an.m.take()
    {
        drop_state(w, a, s);
    }
    run_end(w, a, &run);
}

// ---- what the notifies and the render read of a mesh actor

/// What the notifies' actor branches read of a `MeshActor`: its owner, id, class, place,
/// scale and the montage its mesh plays; `None` for an actor with no mesh.
pub fn mesh_actor(w: &World, a: ActorRef) -> Option<MeshActorView> {
    let an = anim_of(w, a)?;
    let montage = an.m.as_ref().map(|s| s.montage.name);
    let o = owner(w, a);
    match a {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id)?;
            let (seq, cls, x, y, scale) = (p.seq, p.cls, p.x, p.y, Some(p.scale));
            Some(MeshActorView { owner: o, owner_id: p.owner_id, seq, cls, x, y, scale, montage })
        }
        ActorRef::Actor(id) => {
            let p = w.actors.actors.get(id)?;
            let (x, y, _) = crate::actors::world_actor::actor_place(w, a)?;
            let (owner_id, seq, cls) = (p.owner_id, p.seq, p.cls);
            Some(MeshActorView { owner: o, owner_id, seq, cls, x, y, scale: None, montage })
        }
        _ => None,
    }
}

/// The notify a [`NotifyRef`] names in a mesh actor's montages (its owner's `actorMontages`, `montageOf`'s rule),
/// borrowed from `data` (a clone of `w.data`'s `Arc`).
pub fn actor_notify_data<'a>(
    w: &World,
    data: &'a GameData,
    a: ActorRef,
    n: NotifyRef,
) -> &'a mvs_data::fighter::Notify {
    montage_of(data, w, a, w.names.str(n.montage))
        .and_then(|m| m.notifies.get(n.index as usize))
        .unwrap_or_else(|| panic!("no notify {} of actor montage {}", n.index, w.names.str(n.montage)))
}

/// The mesh component of a world actor: the class data of the mesh component its anim
/// plays on.
pub fn mesh_component(w: &World, a: ActorId) -> Option<&Props> {
    mesh_component_of(w, ActorRef::Actor(a))
}

/// The mesh component of any mesh actor, a projectile's too: the class data of the mesh
/// component its anim plays on (the Ak_SurfaceEffect trace's origin).
pub fn mesh_component_of(w: &World, a: ActorRef) -> Option<&Props> {
    let an = anim_of(w, a)?;
    let def = crate::fighter::common::actor_def(w, ActorRef::Fighter(owner(w, a)), cls(w, a))?;
    def.components.get(w.names.str(an.mesh)).and_then(Value::as_obj)
}
