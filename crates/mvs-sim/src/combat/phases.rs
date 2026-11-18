//! The combat calls of `World.step`, in the step's order, and the hit tests of
//! the overlap tests: a fighter's hit shapes against the fighters, a projectile's hit box against a fighter.

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::ProjectileId;
use crate::combat::HitPacket;
use crate::combat::OnHitBuffRow;
use crate::combat::boxes::{CH_ALL, CH_HURT, CH_PAWN, ExtraHurt, WorldSphere, shape_over_capsule};
use crate::combat::buffs as buff;
use crate::combat::elem;
use crate::combat::hit as fh;
use crate::combat::hits;
use crate::combat::melee_actors;
use crate::combat::packet::DataVec;
use crate::fighter::FighterId;
use crate::fighter::anim::{self, OpenPose};
use crate::fighter::common::{knock_back_shapes, owner_team};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::moves::V2;
use crate::math::{max, min};
use crate::names::Name;
use crate::registry::ActorHooks;
use crate::registry::entries::ProjectileHooks;
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::world::World;
use mvs_data::boxes::BoxKind;
use mvs_data::fighter::NotifyTick;

/// `World.comboResetTick`: the combo reset a hit queued, a frame late.
pub fn combo_reset_tick(w: &mut World) {
    let r = &mut w.combo_reset;
    if r.fighter < 0 {
        return;
    }
    if r.pending {
        r.pending = false;
        return;
    }
    r.left -= 1.0;
    if r.left > 0.0 {
        return;
    }
    let id = r.fighter;
    r.fighter = -1;
    let Some(f) = w.by_id(id) else { return };
    let a = &mut w.fighter_mut(f).attackers;
    a.hitstun_count = 0;
    a.hitstun_damage = 0.0;
}

/// `wallbounceTick`, when the frame's platforms are more than the ships.
pub fn wallbounce_tick(w: &mut World) {
    crate::world::spawned_platform::wallbounce_tick(w);
}

/// `World.frameHits.clear()`.
pub fn clear_frame_hits(w: &mut World) {
    w.frame_hits.clear();
}

/// `dealHoldHits` and `consumeRequests`, in the fighter's request pass.
pub fn hold_hits(_w: &mut World, _f: FighterId) {}

/// `actorBuffTick` of every actor family in its pass: an actor's granted buffs.
pub fn actor_buff_ticks(_w: &mut World) {}

/// `buffMoveTick`, then the fighter's overlaps when it moved: the buff actors' latent
/// MoveComponentTo actions tick after every fighter's CMC, write the root without a sweep and update its overlaps at
/// the written place.
pub fn buff_move_tick(w: &mut World, f: FighterId) {
    if buff::buff_move_tick(w, f) {
        overlaps(w, f, false);
    }
}

/// `SpawnedHits.itemSpawnedHits` and `Items.itemThrowHits`, when the match has items.
pub fn item_spawned_hits(w: &mut World) {
    if !w.actors.items.is_empty() {
        crate::combat::spawned_hits::item_spawned_hits(w);
        crate::world::items::item_throw_hits(w);
    }
}

/// `dodgeTick` of the fighters, then of every actor family in that order: projectiles, items,
/// actors, markers, mortars, aim punches, accelerators.
pub fn dodge_ticks(w: &mut World) {
    use crate::actors::ActorRef;
    use crate::fighter::control::dodge_tick;
    let tick = w.flow.tick;
    let mut events = std::mem::take(&mut w.events);
    let mut i = 0;
    while let Some(&f) = w.scratch.ps.get(i) {
        dodge_tick(w, ActorRef::Fighter(f), tick, &mut events);
        i += 1;
    }
    // Each live actor of a family, by place: a pass makes nothing, so none is added while it runs.
    macro_rules! family {
        ($($list:ident => $kind:ident),+) => {$(
            let mut n = 0;
            while let Some(id) = w.actors.$list.nth(n) {
                if w.actors.$list.get(id).is_some_and(|a| !a.dead) {
                    dodge_tick(w, ActorRef::$kind(id), tick, &mut events);
                }
                n += 1;
            }
        )+};
    }
    family!(
        projectiles => Projectile, items => Item, actors => Actor, markers => Marker, mortars => Mortar,
        aim_punches => AimPunch, accelerators => Accelerator
    );
    w.events = events;
}

/// A key of the overlaps a buff's hitbox sphere holds (`meleeHitboxSphereTest`): the
/// string keys `o<fighter>.h<hit>` (a hit shape of the fighter's) and `o<fighter>.p<component>` (a pawn hit sphere).
#[derive(crate::State, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SphereKey {
    /// A hit shape: the fighter's id and the shape's hit-set key.
    Hit(i32, i32),
    /// A pawn hit sphere: the fighter's id and the sphere's index among `pawnHitSpheres`.
    Pawn(i32, u32),
}

/// The overlaps a buff's hitbox sphere holds (the `o<fighter>.*` keys on the buff's ext), a set of fixed size
/// in the order the keys were added, so that it lives on a `Copy` actor buff extension
/// (`chars::garnet::stop_projectile`'s). The set needs one key per fighter of the match and component met: a hit
/// shape per active Hitframe and the pawn's always-on HitBox spheres (in the roster's data one, Garnet's Sing Sphere);
/// it has no bound of its own. [`SPHERE_KEYS`] holds four fighters with three Hitframes and the sphere each.
#[derive(crate::State, Copy, PartialEq, Debug)]
pub struct SphereKeys {
    keys: [Option<SphereKey>; SPHERE_KEYS],
    len: u8,
}

/// The most keys a [`SphereKeys`] holds.
// Deviation: the build's overlap list has no bound; this set holds 64 (four fighters with a dozen shapes each is far
// past a match's). Past the cap a key is dropped, warned once by the caller: that overlap never begins.
pub const SPHERE_KEYS: usize = 64;

impl Default for SphereKeys {
    fn default() -> Self {
        SphereKeys { keys: [None; SPHERE_KEYS], len: 0 }
    }
}

impl SphereKeys {
    /// The keys in the order they were added.
    pub fn iter(&self) -> impl Iterator<Item = SphereKey> + '_ {
        self.keys[..usize::from(self.len)].iter().filter_map(|k| *k)
    }

    pub fn contains(&self, k: &SphereKey) -> bool {
        self.iter().any(|x| x == *k)
    }

    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// True when [`SPHERE_KEYS`] keys are held: `insert` drops the next.
    pub fn is_full(&self) -> bool {
        usize::from(self.len) >= SPHERE_KEYS
    }

    /// Adds `k` at the end; false when it is held already or the set is full (the key is dropped).
    pub fn insert(&mut self, k: SphereKey) -> bool {
        if self.contains(&k) || self.is_full() {
            return false;
        }
        let n = usize::from(self.len);
        self.keys[n] = Some(k);
        self.len += 1;
        true
    }

    /// Keeps the keys `keep` holds for, in their order.
    pub fn retain(&mut self, mut keep: impl FnMut(&SphereKey) -> bool) {
        let mut out = SphereKeys::default();
        for k in self.iter() {
            if keep(&k) {
                out.keys[usize::from(out.len)] = Some(k);
                out.len += 1;
            }
        }
        *self = out;
    }
}

// ---- the deferred calls

/// A hit call an overlap update defers to its end: each fighter of the update is
/// tested as it stands, then the calls run in the order the tests made them.
#[derive(Clone, Debug)]
pub enum PendingHit {
    /// A Hitframe's hit on an enemy (`meleeTest`'s `deal`).
    Melee {
        att: FighterId,
        vic: FighterId,
        p: Box<HitPacket>,
        charged: Real,
        id: i32,
        name: Name,
        at: V2,
        ignore_stun: bool,
        open: Option<OpenPose>,
    },
    /// A Hitframe over an ally (`meleeTest`'s `deal` with `ally`).
    AllyMelee {
        att: FighterId,
        vic: FighterId,
        p: Box<HitPacket>,
        at: V2,
        ignore_stun: bool,
        open: Option<OpenPose>,
        bound: bool,
    },
    /// pfgDoDamage's entry rows alone: a dodge the DodgingFighters gate lets through, the owner under his own boxes.
    Rows { att: FighterId, rows: Vec<OnHitBuffRow> },
    /// A projectile's hit on an enemy (`projectileBoxTest`), then its DestroyOnHitFighter. `bx`: the extra box
    /// (`ExtraHitBox`) that dealt it, its place in `extra_hit_boxes`; `None` the actor's own.
    Projectile { pr: ProjectileId, vic: FighterId, mid_frame: bool, destroy: bool, bx: Option<usize> },
    /// A projectile's hit box over an ally (`projectileAllyTest`); `bx` as above.
    ProjectileAlly { pr: ProjectileId, vic: FighterId, bx: Option<usize> },
    /// A projectile's hit box over an invulnerable enemy (`projectileBoxTest`'s attackDodged), queued with the hits
    /// so a dealer an earlier hit of the pass destroyed makes no dodge.
    ProjectileDodge { pr: ProjectileId, vic: FighterId, attack_id: i32, bkb: Real, tick: i32, mark: bool },
}

/// The most buffers a pool of the World's scratch keeps.
const POOL: usize = 8;

/// An empty list of deferred hits, from the World's pool.
fn take_pending(w: &mut World) -> Vec<PendingHit> {
    w.scratch.pending_hits.pop().unwrap_or_default()
}

fn run_pending(w: &mut World, mut pending: Vec<PendingHit>) {
    for h in pending.drain(..) {
        run_one(w, h);
    }
    if w.scratch.pending_hits.len() < POOL {
        w.scratch.pending_hits.push(pending);
    }
}

/// A copy of `p` in a packet of the World's pool (the pool's lists keep their buffers).
fn pooled_packet(w: &mut World, p: &HitPacket) -> Box<HitPacket> {
    match w.scratch.hit_packets.pop() {
        Some(mut b) => {
            HitPacket::clone_from(&mut b, p);
            b
        }
        None => Box::new(p.clone()),
    }
}

/// A deferred hit's packet, back to the pool.
fn recycle_packet(w: &mut World, p: Box<HitPacket>) {
    if w.scratch.hit_packets.len() < POOL * 4 {
        w.scratch.hit_packets.push(p);
    }
}

fn run_one(w: &mut World, h: PendingHit) {
    match h {
        PendingHit::Melee { att, vic, p, charged, id, name, at, ignore_stun, open } => {
            run_melee(w, att, vic, &p, charged, id, name, at, ignore_stun, open);
            recycle_packet(w, p);
        }
        PendingHit::AllyMelee { att, vic, p, at, ignore_stun, open, bound } => {
            run_ally_melee(w, att, vic, &p, at, ignore_stun, open, bound);
            recycle_packet(w, p);
        }
        PendingHit::Rows { att, rows } => hits::hit_buffs_call(w, att, &rows),
        PendingHit::Projectile { pr, vic, mid_frame, destroy, bx } => {
            run_projectile(w, pr, vic, mid_frame, destroy, bx)
        }
        PendingHit::ProjectileAlly { pr, vic, bx } => run_projectile_ally(w, pr, vic, bx),
        PendingHit::ProjectileDodge { pr, vic, attack_id, bkb, tick, mark } => {
            // As `run_projectile`: a dealer an earlier hit of this pass destroyed begins no more overlaps.
            if !projectile_dead(w, pr) {
                attack_dodged(w, ActorRef::Projectile(pr), vic, attack_id, bkb, tick, None, mark);
            }
        }
    }
}

#[allow(clippy::too_many_arguments, reason = "the pending hit's members")]
fn run_melee(
    w: &mut World,
    att: FighterId,
    vic: FighterId,
    p: &HitPacket,
    charged: Real,
    id: i32,
    name: Name,
    at: V2,
    ignore_stun: bool,
    open: Option<OpenPose>,
) {
    // pfgDoDamage's entry rows (@842-2821) come ahead of every gate below.
    if !p.on_hit_buffs.is_empty() {
        hits::hit_buffs_call(w, att, &p.on_hit_buffs);
    }
    // HitBoxHandler @4922-5257: the event deals nothing when its owner is in hitstun (CheckHitstunActive),
    // unless the hit box has the IgnoreHitStun tag (@4848). A hit dealt earlier in this update has already
    // added the hitstun source, so of two attacks that meet in one update only the first to report lands.
    if hits::in_hitstun(w.fighter(att)) && !ignore_stun {
        return;
    }
    // HitOnlyOneFighter (pfgDoDamage @3236-3288): refused once a hit with the option landed on a fighter;
    // the victim stays in the Hitframe's record (@2906 comes first).
    // Unverified: DamageAndLaunch's Hit is taken as true for every hit that reaches applyHit.
    let once = p.hit_only_one_fighter.unwrap_or(false);
    if once && w.fighter(att).decay.has_hit_a_fighter {
        return;
    }
    w.fighter_mut(att).frame.hit_this_frame = true;
    w.fighter_mut(vic).frame.hit_this_frame = true;
    hits::apply_hit_behind_entry(w, att, vic, p, charged, id, name, at, true, open.as_ref());
    if once {
        w.fighter_mut(att).decay.has_hit_a_fighter = true; // @16069-16144
    }
}

#[allow(clippy::too_many_arguments, reason = "the pending hit's members")]
fn run_ally_melee(
    w: &mut World,
    att: FighterId,
    vic: FighterId,
    p: &HitPacket,
    at: V2,
    ignore_stun: bool,
    open: Option<OpenPose>,
    bound: bool,
) {
    hits::hit_buffs_call(w, att, &p.on_hit_buffs); // the entry @842-2821, ahead of the gates below
    if hits::in_hitstun(w.fighter(att)) && !ignore_stun {
        return; // @5181-5257, IgnoreHitStun @4848
    }
    if p.hit_only_one_fighter.unwrap_or(false) && w.fighter(att).decay.has_hit_a_fighter {
        return;
    }
    // @5617-5904: the dealer's dealt pass with the ally as Victim (no team gate ahead of it), behind the
    // entry rows and ahead of the ally rows @5931 and the vacuum; NotifyAllyOfHit hands the ally's
    // HitByAlly @584 the modded hit.
    let q = buff::apply_dealt_damage_mods(w, att, ActorRef::Fighter(vic), p, true);
    let mult = w.fighter(att).charge.charge_buff_mult;
    hits::hit_buffs_ally(w, att, vic, &p.on_hit_buffs, mult);
    // DamageAndLaunch @15630, refused by CanBeHitBy: a ForceKnockback Hitframe launches the ally.
    hits::refused_damage_and_launch(w, att, vic, &q, at, true, open.as_ref(), false, ActorRef::Fighter(att));
    // NotifyAllyOfHit @433, ahead of the victim's HitByAlly @584.
    w.fighter_mut(att).decay.recent_hit_actor = Some(ActorRef::Fighter(vic));
    if bound {
        // The ally's HitByAlly with the hit (behind the legacy vacuum: BaseKnockBack and KnockBackScaling 1
        // of a VacuumKnockback Hitframe) and the dealer.
        let mut hit = q.into_owned();
        if hit.vacuum_knockback.unwrap_or(false) {
            hit.base_knock_back = Some(hits::legacy_vacuum(w, att, vic, &hit, open.as_ref()).bkb);
            hit.knock_back_scaling = Some(1.0);
        }
        let arg = BuffSignalArg { hit: Some(Box::new(hit)), dealer: Some(att), ..Default::default() };
        buff::buff_signal(w, vic, BuffSignal::HitByAlly, arg);
    }
    // The HitAlly broadcast @886, behind HitByAlly @584.
    crate::fighter::match_side::set_help_vo_tags(w, att, vic);
    if let Some(h) = w.fighter(att).hooks.char.hit_ally {
        h(w, att, vic);
    }
}

fn run_projectile(w: &mut World, pr: ProjectileId, vic: FighterId, mid_frame: bool, destroy: bool, bx: Option<usize>) {
    // A dealer an earlier hit of this pass destroyed begins no more overlaps: its HitBox's
    // OnComponentDestroyed clears bGenerateOverlapEvents and its overlaps (UActorComponent::DestroyComponent;
    // 4.26 BeginComponentOverlap returns on a pending-kill component).
    // Deviation: HitEachActorOnlyOnce's mark of a later fighter is made at the test, ahead of the queue (the
    // dodge is queued: `PendingHit::ProjectileDodge`).
    if projectile_dead(w, pr) {
        return;
    }
    if mid_frame {
        w.fighter_mut(vic).frame.hit_this_frame = true;
    }
    let a = ActorRef::Projectile(pr);
    let d = hits::dealer(w, a);
    let p = box_packet(w, pr, bx).clone(); // the packet as it stands
    let rows = d.buffs(w);
    let at = V2 { x: d.x, y: d.y };
    let pct = elem::charge_pct(w, a);
    let hit = hits::apply_projectile_hit(w, &d, vic, &p, &rows, at, None, &[], pct);
    if hit {
        projectile_hit_enemy(w, pr, vic, mid_frame);
    }
    if destroy && hit {
        crate::world::spawning::destroy_projectile(w, pr, false);
    }
}

fn run_projectile_ally(w: &mut World, pr: ProjectileId, vic: FighterId, bx: Option<usize>) {
    if projectile_dead(w, pr) {
        return; // a dealer an earlier hit of the pass destroyed (as for an enemy)
    }
    let a = ActorRef::Projectile(pr);
    let d = hits::dealer(w, a);
    let p = box_packet(w, pr, bx).clone();
    // pfgDoDamage's entry rows (@842-2821: the Instigator's, his team's), then the rows for the hit ally
    // (@5931-6835, Round(StackCount x GetChargeBuffStackMultiplier)) and NotifyAllyOfHit @21344
    // (`actor_ally_hit`), as the enemy's hit makes its entry rows behind the same gates (`hit_buffs_call`'s
    // note).
    // The rows are applied here for every dealer, Velma's book and sass bubble included (behind the ally's HitByAlly:
    // projectileAllyTest's note).
    let rows = d.buffs(w);
    if let Some(att) = d.owner {
        hits::hit_buffs_call(w, att, &rows);
    }
    let mult = elem::charge_buff(w, a);
    hits::actor_ally_hit(w, &d, vic, &p, &rows, mult);
}

/// What a projectile's hit on a fighter sets off past the hit (`projectileBoxTest`'s pending call).
fn projectile_hit_enemy(w: &mut World, pr: ProjectileId, vic: FighterId, mid_frame: bool) {
    let a = ActorRef::Projectile(pr);
    // The HitEnemyPlayer of the handler: every Buff_ElementalParent on the actor has it bound (@890).
    // Unverified: ahead of the class's own event; the order of the delegate's bindings is not read.
    if has_elem(w, pr) {
        elem::elem_hit_enemy(w, a, vic);
    }
    // The granted buffs bound in the parent's BeginPlay, ahead of the class's own part.
    // Unverified: behind the passive elementals; the delegate's order is not read.
    let arg = ActorBuffArg { by: vic, kx: 0.0, ky: 0.0 };
    if has_elem(w, pr) {
        elem::actor_buff_signal(w, a, ActorBuffSignal::HitEnemyPlayerFirst, Some(&arg));
    }
    // The class's own HitEnemyPlayer, with overlapMidFrame as for its overlap; skipped for an actor a binding ahead
    // of it destroyed (ProcessMulticastDelegate 14079aa20 skips a PendingKill object).
    w.overlap_mid_frame = mid_frame;
    if !projectile_dead(w, pr)
        && let Some(h) = projectile_hooks(w, pr).and_then(|k| k.hit_enemy)
    {
        h(w, pr, vic);
    }
    w.overlap_mid_frame = false;
    // A buff on the actor that binds the handler's HitEnemyPlayer in its own BeginPlay, later than the class's.
    // Unverified: behind the class's handler, as bound later; the order against the elemental's is not read.
    if has_elem(w, pr) {
        elem::actor_buff_signal(w, a, ActorBuffSignal::HitEnemyPlayer, Some(&arg));
    }
}

fn projectile_dead(w: &World, pr: ProjectileId) -> bool {
    w.actors.projectiles.get(pr).is_none_or(|p| p.dead)
}

fn has_elem(w: &World, pr: ProjectileId) -> bool {
    w.actors.projectiles.get(pr).is_some_and(|p| p.elem.is_some())
}

/// The class's projectile hooks (`ACTORS.get(pr.cls)?.projectile`).
fn projectile_hooks(w: &World, pr: ProjectileId) -> Option<&'static ProjectileHooks> {
    let cls = w.actors.projectiles.get(pr)?.cls;
    match &w.registry.actors.get(w.names.str(cls))?.hooks {
        ActorHooks::Projectile(h) => Some(h),
        _ => None,
    }
}

// ---- small reads

/// `!f.dead`: a number, so NaN is alive too.
pub(crate) fn alive(w: &World, f: FighterId) -> bool {
    let d = w.fighter(f).life.dead;
    d == 0.0 || d.is_nan()
}

/// `noHit`: damage turned off or the actor's collision off.
pub(crate) fn no_hit(w: &World, f: FighterId) -> bool {
    w.fighter(f).invuln.damage_disabled || actor_collision_off(w, f)
}

/// `actorCollisionOff`: the pull's move, or a live buff that switched the collision off.
pub(crate) fn actor_collision_off(w: &World, f: FighterId) -> bool {
    w.fighter(f).moves.pull.is_some() || buff::buff_collision_off(w, f)
}

/// A `World.frameHits` key: an attacker / victim pair by fighter id (a player's 0..7, a minion's below 1000).
fn hit_key(att: i32, vic: i32) -> i64 {
    let (a, v) = (i64::from(att), i64::from(vic));
    if att < 8 && vic < 8 { a * 8 + v } else { 65536 + a * 1024 + v }
}

/// `hitAllyRows`: a Hitframe's OnHitBuffs has a row pfgDoDamage applies with an ally under the hit box (one for the
/// hit ally @5931, or one of the entry's), or the Hitframe has ForceKnockback (DamageAndLaunch @8093).
fn hit_ally_rows(p: &HitPacket) -> bool {
    p.force_knockback.unwrap_or(false)
        || p.on_hit_buffs.iter().any(|e| {
            e.apply_buff_ally || (e.apply_buff_self && !(e.apply_buff_hit && e.require_hit_to_apply_buff_to_self))
        })
}

/// `entryRows`: a row pfgDoDamage's entry applies (`hitBuffsCall`): one for the owner or one for his team.
fn entry_rows(rows: &[OnHitBuffRow]) -> bool {
    rows.iter().any(entry_row)
}

fn entry_row(e: &OnHitBuffRow) -> bool {
    (e.apply_buff_self && !(e.apply_buff_hit && e.require_hit_to_apply_buff_to_self))
        || (e.apply_buff_ally && !e.apply_buff_hit)
}

/// A spawned actor's On Hit Buffs has a row its pfgDoDamage applies with an ally under the hit box: one for the hit
/// ally (ApplyBuffHit? and ApplyBuffAlly?, @5931), or one of the entry's (`entry_rows`).
fn ally_call_row(e: &OnHitBuffRow) -> bool {
    (e.apply_buff_hit && e.apply_buff_ally) || entry_row(e)
}

/// One of the fighter's active Hitframes has a row for an ally (`allyRows`).
fn ally_rows(w: &World, f: FighterId) -> bool {
    w.fighter(f).montages.active_hits.values().any(|h| hit_ally_rows(&hits::hitframe_packet(w, f, h.n)))
}

/// A Hitframe of the fighter, as the tests read it.
#[derive(Clone, Debug, Default)]
pub struct Active {
    pub(crate) key: i32,
    pub(crate) p: HitPacket,
    /// The notify is a queued one (its activation's scale ends at 1).
    queued: bool,
    open: Option<OpenPose>,
    copy: Option<OpenPose>,
    pub(crate) charged: Real,
    id: i32,
    name: Name,
}

/// A fighter's active Hitframes ([`take_actives`]): the first `n` of the list, the rest kept for their packets'
/// buffers.
#[derive(Clone, Debug, Default)]
pub struct Actives {
    v: Vec<Active>,
    n: usize,
}

impl std::ops::Deref for Actives {
    type Target = [Active];

    fn deref(&self) -> &[Active] {
        &self.v[..self.n]
    }
}

/// `actives`: the fighter's active Hitframes as the tests read them, in a list of the World's pool; [`give_actives`]
/// hands it back (one not given back is made anew at the next call).
pub(crate) fn take_actives(w: &mut World, f: FighterId) -> Actives {
    let mut a = w.scratch.hit_actives.pop().unwrap_or_default();
    a.n = 0;
    for (&key, h) in &w.fighter(f).montages.active_hits {
        if a.n == a.v.len() {
            a.v.push(Active::default());
        }
        let slot = &mut a.v[a.n];
        a.n += 1;
        slot.key = key;
        match &h.p {
            Some(p) => slot.p.read_props(&w.names, p, Some(h.n)),
            None => hits::hitframe_packet_into(w, f, h.n, &mut slot.p),
        }
        slot.queued = hits::notify_of(w, f, h.n).is_some_and(|n| n.tick == NotifyTick::Queued);
        slot.open = h.open_pose.map(|(anim, time)| OpenPose { anim, time });
        slot.copy = h.copy_pose.map(|(anim, time)| OpenPose { anim, time });
        slot.charged = h.charged_time;
        slot.id = h.attack_id;
        slot.name = h.attack_name;
    }
    a
}

pub(crate) fn give_actives(w: &mut World, a: Actives) {
    if w.scratch.hit_actives.len() < POOL {
        w.scratch.hit_actives.push(a);
    }
}

/// An empty list for [`hit_spheres_into`], from the World's pool; [`give_spheres`] hands it back.
pub(crate) fn take_spheres(w: &mut World) -> Vec<WorldSphere> {
    let mut v = w.scratch.hit_spheres.pop().unwrap_or_default();
    v.clear();
    v
}

pub(crate) fn give_spheres(w: &mut World, v: Vec<WorldSphere>) {
    if w.scratch.hit_spheres.len() < POOL {
        w.scratch.hit_spheres.push(v);
    }
}

/// The first of the Hitframe's shapes ([`hit_spheres`]) that `pred` takes, in the order of the list.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub(crate) fn hit_sphere_find(
    w: &mut World,
    att: FighterId,
    p: &HitPacket,
    pose: &[f32],
    chan: u8,
    pred: impl Fn(&WorldSphere) -> bool,
) -> Option<WorldSphere> {
    let mut v = take_spheres(w);
    hit_spheres_into(w, att, p, pose, chan, 0.0, Real::INFINITY, None, &mut v);
    let found = v.iter().find(|s| pred(s)).cloned();
    give_spheres(w, v);
    found
}

pub(crate) fn hit_set_has(w: &World, f: FighterId, key: i32, id: i32) -> bool {
    w.fighter(f).montages.active_hits.get(&key).is_some_and(|h| h.hit_set.contains(&id))
}

pub(crate) fn hit_set_add(w: &mut World, f: FighterId, key: i32, id: i32) {
    w.fighter_mut(f).montages.active_hit_write(key, |h| {
        h.hit_set.insert(id);
    });
    // A sweet spot shares its hitframe's set (hit::share_hit_set).
    let pk = crate::registry::notifies::sweet_partner(key);
    if w.fighter(f).montages.active_hits.contains_key(&pk) {
        w.fighter_mut(f).montages.active_hit_write(pk, |h| {
            h.hit_set.insert(id);
        });
    }
}

/// `att.sampleBoxes()`: the attacker's pose, in a buffer of the World's pool (`anim::put_pose` hands it back).
pub(crate) fn sample(w: &World, f: FighterId) -> Option<Vec<f32>> {
    anim::sampled_boxes(w, f)
}

/// `vic.sampleBoxes(w.vbuf)`: the victim's pose in the World's victim buffer, lent out; [`give_vbuf`] puts it back
/// (a buffer not given back is made anew at the next lend).
pub(crate) fn victim_pose(w: &mut World, f: FighterId) -> Option<Vec<f32>> {
    lend_vbuf(w, f, anim::sample_boxes)
}

/// [`victim_pose`] with the hurt shapes at the step's start pose ([`anim::struck_boxes`]), lent the same way: only
/// LeBron's ball's overlap takes it.
pub(crate) fn struck_pose(w: &mut World, f: FighterId) -> Option<Vec<f32>> {
    lend_vbuf(w, f, anim::struck_boxes)
}

fn lend_vbuf(w: &mut World, f: FighterId, sample: fn(&World, FighterId, &mut Vec<f32>) -> bool) -> Option<Vec<f32>> {
    let mut v = std::mem::take(&mut w.scratch.vbuf);
    if sample(w, f, &mut v) {
        Some(v)
    } else {
        w.scratch.vbuf = v;
        None
    }
}

/// The victim buffer [`victim_pose`] lent, back to the World.
pub(crate) fn give_vbuf(w: &mut World, v: Vec<f32>) {
    w.scratch.vbuf = v;
}

fn pose_at(w: &World, f: FighterId, o: OpenPose) -> Option<Vec<f32>> {
    anim::posed_boxes(w, f, o.anim, o.time)
}

// ---- the fighter's overlaps

/// `rootMoved`: a fighter's root written outside its movement (the weld to an item):
/// its overlaps at the written place.
pub fn root_moved(w: &mut World, f: FighterId) {
    overlaps(w, f, false);
}

/// `overlaps`. Offline only begin-overlap events deal damage
/// (OnComponentBeginOverlap_Event; CheckOverlaps runs only in the rollback driver, which is off). Overlaps update when
/// a component moves: the CMC's capsule move, ActivateHitbox in the montage advance (the previous frame's bones) and
/// the mesh's finalize (bUpdateOverlapsOnAnimationFinalize, `mesh` true; the new pose). Each update is tested both
/// ways, each fighter as it stands at that point of the frame; a hit is dealt at the update's end, in test order.
pub fn overlaps(w: &mut World, f: FighterId, mesh: bool) {
    let mut pending = take_pending(w);
    if alive(w, f) {
        melee_self_test(w, f, mesh, &mut pending);
        let mut nf = 0;
        while let Some(o) = hits::fighter_at(w, nf) {
            nf += 1;
            if !alive(w, o) || no_hit(w, o) {
                continue;
            }
            if w.fighter(o).team.team == w.fighter(f).team.team {
                // An ally under a hit box: no hit (CanBeHitBy: another team), but pfgDoDamage runs for it and its
                // OnHitBuffs rows for an ally apply, and NotifyAllyOfHit broadcasts the ally's HitByAlly.
                if o != f {
                    if ally_rows(w, o) || buff::hit_by_ally_bound(w, f) || hits::ally_observed(w, o) {
                        melee_test(w, o, f, false, true, &mut pending, true);
                    }
                    if ally_rows(w, f) || buff::hit_by_ally_bound(w, o) || hits::ally_observed(w, f) {
                        melee_test(w, f, o, mesh, false, &mut pending, true);
                    }
                }
                continue;
            }
            // The update visits the hurtboxes before the fighter's own hitboxes (attach order).
            melee_test(w, o, f, false, true, &mut pending, false);
            melee_test(w, f, o, mesh, false, &mut pending, false);
        }
        // A fighter moving into a projectile's HitBox2D (the projectile itself moves after both fighters).
        // the list by place, read anew each turn (a destroyed one stays listed until the sweep; a spawn is met)
        let mut k = 0;
        while let Some(pr) = w.actors.projectiles.nth(k) {
            k += 1;
            projectile_test(w, pr, f, &mut pending, true);
        }
        melee_actor_tests(w, f);
    }
    if mesh {
        let m = &mut w.fighter_mut(f).montages;
        for h in m.active_hits.values_mut() {
            h.open_pose = None;
        }
        for c in m.fading.iter_mut().flat_map(|fd| fd.hits.iter_mut().flatten()) {
            // (the fade's copy of a hit still active: the one object)
            if m.active_hits.get(&c.idx).is_some_and(|h| h.n == c.n) {
                c.open_pose = None;
            }
        }
    }
    run_pending(w, pending);
}

/// The tests of a fighter's hit shapes against the spawned actors, the items and the stage's actors, in their
/// order (`combat/melee_actors.rs`).
fn melee_actor_tests(w: &mut World, att: FighterId) {
    melee_actors::melee_projectile_test(w, att);
    melee_actors::melee_item_test(w, att);
    melee_actors::melee_hitbox_sphere_test(w, att);
    melee_actors::melee_damage_box_test(w, att);
    melee_actors::melee_actor_test(w, att);
    melee_actors::melee_stage_actor_test(w, att);
}

/// `meleeTest`: one hit per attacker and victim per frame, the first hitbox whose
/// overlap event fires. An opening Hitframe tests its boxes as it activates them (notify order, then HitBoxTags
/// order: ActivateHitbox per tag, HitBoxHandler ubergraph 8910-9429). Otherwise the overlaps come from an update
/// of the moving side, which visits the Mesh's attach children in array order (UPrimitiveComponent::
/// UpdateOverlapsImpl 14297ec80): when the attacker updates, the first overlapping hitbox in attach order, whichever
/// Hitframe it belongs to; when the victim updates (`victim_update`), the hitbox over its first overlapped hurtbox.
/// Unverified: the live AttachChildren array was not read; several hitboxes newly over the same hurtbox in a
/// victim's update come in the physics query's order, taken as attach order.
/// `ally`: `vic` is of the attacker's team, and only the Hitframes with a row for an ally are tested, unless the
/// ally's buffs bound HitByAlly or the attacker observes its ally hits (`allyObserved`).
fn melee_test(
    w: &mut World,
    att: FighterId,
    vic: FighterId,
    activation: bool,
    victim_update: bool,
    pending: &mut Vec<PendingHit>,
    ally: bool,
) {
    let (aid, vid) = (w.fighter(att).id, w.fighter(vic).id);
    if w.fighter(att).montages.active_hits.is_empty() || w.frame_hits.contains(&hit_key(aid, vid)) {
        return;
    }
    let Some(vp) = victim_pose(w, vic) else { return };
    let pose = sample(w, att);
    let hs = take_actives(w, att);
    let mut sp = take_spheres(w);
    melee_test_poses(w, att, vic, activation, victim_update, pending, ally, &vp, pose.as_deref(), &hs, &mut sp);
    give_spheres(w, sp);
    give_actives(w, hs);
    if let Some(p) = pose {
        anim::put_pose(w, p);
    }
    give_vbuf(w, vp);
}

/// [`melee_test`] with the victim's pose `vp`, the attacker's `pose`, its Hitframes `hs` and a list `sp` for their
/// shapes, lent by it.
#[allow(clippy::too_many_arguments, reason = "the function's locals")]
fn melee_test_poses(
    w: &mut World,
    att: FighterId,
    vic: FighterId,
    activation: bool,
    victim_update: bool,
    pending: &mut Vec<PendingHit>,
    ally: bool,
    vp: &[f32],
    pose: Option<&[f32]>,
    hs: &[Active],
    sp: &mut Vec<WorldSphere>,
) {
    let vid = w.fighter(vic).id;
    let bound = ally && buff::hit_by_ally_bound(w, vic);
    let observed = ally && hits::ally_observed(w, att);
    let root_hits = w.fixed.samplers[&w.fighter(att).ch].root_hits; // a hit shape that meets the victim's root capsule
    let skip = |h: &Active| ally && !bound && !observed && !hit_ally_rows(&h.p);
    // Activation: SetCollisionEnabled(QueryAndPhysics) at scale 1, then SetRelativeScale3D(HitBoxScale); both update
    // overlaps at once with the previous frame's bones. A queued Hitframe begins inside FinalizeBoneTransform, after
    // the bone buffers flipped (FinalizeAnimationUpdate 142ca33e0): SetCollisionEnabled tests the box where it
    // still is (previous pose, scale 1), SetRelativeScale3D recomputes it from the new bones (the test below).
    if activation {
        for h in hs {
            let Some(o) = h.open else { continue };
            if hit_set_has(w, att, h.key, vid) || skip(h) {
                continue;
            }
            let op = pose_at(w, att, o);
            let top = if h.queued { 1.0 } else { Real::INFINITY };
            sp.clear();
            if let Some(op) = op.as_ref() {
                hit_spheres_into(w, att, &h.p, op, CH_HURT, 1.0, top, h.copy, sp);
            }
            let hit = if sp.is_empty() { None } else { fh::hurt_hit(w, vic, sp, vp) };
            let hit = match hit {
                Some(s) => Some(s),
                None if root_hits => op.as_ref().and_then(|op| root_capsule_hit(w, att, &h.p, op, vic, 1.0, top)),
                None => None,
            };
            if let Some(op) = op {
                anim::put_pose(w, op);
            }
            if let Some(s) = hit {
                return deal(w, att, vic, h, &s, Some(o), ally, bound, pending);
            }
        }
    }
    let Some(pose) = pose else { return };
    let mut best: Option<(usize, WorldSphere)> = None;
    let mut best_key = Real::INFINITY;
    for (k, h) in hs.iter().enumerate() {
        if hit_set_has(w, att, h.key, vid) || skip(h) {
            continue;
        }
        // The fighter's own shapes (points with its hitboxes off), then the clone hurt shapes of a Disguise.
        sp.clear();
        hit_spheres_into(w, att, &h.p, pose, CH_HURT, 0.0, Real::INFINITY, None, sp);
        for s in sp.iter() {
            let r = fh::hurt_rank(w, vic, s, vp);
            if r < 0.0 {
                continue;
            }
            let key = (if victim_update { r * 4096.0 } else { 0.0 }) + order_of(s);
            if key < best_key {
                best_key = key;
                best = Some((k, s.clone()));
            }
        }
        if !root_hits {
            continue;
        }
        // The victim's root capsule, after its hurt shapes and the floor bounce one in the victim's update.
        // Unverified: the order of the overlap events of one update across the victim's components is the order the
        // port gives its hurt shapes (attach order, the extra last); the root capsule's place among them is not read.
        let Some(s) = root_capsule_hit(w, att, &h.p, pose, vic, 0.0, Real::INFINITY) else { continue };
        let hurt = w.fixed.samplers[&w.fighter(vic).ch].hurt.len();
        let key = (if victim_update { (count(hurt) + 1.0) * 4096.0 } else { 0.0 }) + order_of(&s);
        if key < best_key {
            best_key = key;
            best = Some((k, s));
        }
    }
    if let Some((k, s)) = best {
        deal(w, att, vic, &hs[k], &s, None, ally, bound, pending);
    }
}

fn count(n: usize) -> Real {
    crate::real(u32::try_from(n).expect("a shape count"))
}

fn order_of(s: &WorldSphere) -> Real {
    crate::real(s.order.expect("a hit shape's order"))
}

/// `meleeTest`'s `deal`: the hit `s` of Hitframe `h` on `vic`; `open`: it came through the activation test, with
/// the previous frame's bones (`vacuumSocket`).
#[allow(clippy::too_many_arguments, reason = "the function's captures")]
fn deal(
    w: &mut World,
    att: FighterId,
    vic: FighterId,
    h: &Active,
    s: &WorldSphere,
    open: Option<OpenPose>,
    ally: bool,
    bound: bool,
    pending: &mut Vec<PendingHit>,
) {
    let (aid, vid) = (w.fighter(att).id, w.fighter(vic).id);
    let once = h.p.hit_only_one_fighter.unwrap_or(false);
    let at = V2 { x: s.x, y: s.y };
    if ally {
        // HitOnlyOneFighter (pfgDoDamage @3236-3288, ahead of the ally hand-off @8400 -> @21344): refused, ally or
        // enemy, once a hit with the option landed on a fighter; an ally hit never sets HasHitAFighter. Refused here
        // the ally does not enter the Hitframe's record. An enemy hit dealt earlier in the same update sets the flag
        // in its pending call: the test is made again there.
        if once && w.fighter(att).decay.has_hit_a_fighter {
            return;
        }
        if hits::invulnerable(w.fighter(vic)) {
            return;
        }
        hit_set_add(w, att, h.key, vid);
        w.frame_hits.insert(hit_key(aid, vid));
        let p = pooled_packet(w, &h.p);
        pending.push(PendingHit::AllyMelee { att, vic, p, at, ignore_stun: s.ignore_stun, open, bound });
        return;
    }
    if hits::invulnerable(w.fighter(vic)) {
        // pfgDoDamage @4608 -> @7381, behind HitOnlyOneFighter @3236-3288: an enemy hit box over an invulnerable
        // fighter it could hit is dodged once per AttackID and overlap (BPI_SetDodgedSuccessfully and the pawn's
        // EvadedAttack). The call's entry rows (@842-2821) come ahead of HitOnlyOneFighter and the dodge, behind
        // the DodgingFighters gate @410-772 (dodgeCall).
        if once && w.fighter(att).decay.has_hit_a_fighter {
            return;
        }
        if dodge_call(w, att, vic, h.id, w.flow.tick) && !h.p.on_hit_buffs.is_empty() {
            // in the update's call order, behind the owner's own
            pending.push(PendingHit::Rows { att, rows: h.p.on_hit_buffs.clone() });
        }
        let bkb = h.p.base_knock_back.unwrap_or(0.0);
        let tick = w.flow.tick;
        attack_dodged(w, ActorRef::Fighter(att), vic, h.id, bkb, tick, Some(att), false);
        return;
    }
    dodge_over(w, ActorRef::Fighter(att), vic); // @782
    hit_set_add(w, att, h.key, vid);
    w.frame_hits.insert(hit_key(aid, vid));
    let p = pooled_packet(w, &h.p);
    pending.push(PendingHit::Melee {
        att,
        vic,
        p,
        charged: h.charged,
        id: h.id,
        name: h.name,
        at,
        ignore_stun: s.ignore_stun,
        open,
    });
}

/// `meleeSelfTest`: the owner's own hurt boxes under his hit boxes. The overlap event
/// has no owner filter (UpdateOverlapsImpl 14297ec80; BeginComponentOverlap 14295a850), so the handler's event calls
/// pfgDoDamage (@7466-7502) with the owner as HitActor; ahead of IgnoreActor @2826 stand only the dead test @281 and
/// DodgingFighters @410-772: the call runs the entry rows and stops. Tested for a Hitframe with such rows alone,
/// once per Hitframe, in the fighter's own update, at the opening's pose and at the current one.
/// Unverified: the order of the owner's call among the update's others: taken first.
/// Unverified: AnimNotify_Hitframe_Conditional's true branch passes IgnoreActor none (@1114), so that call goes on
/// past @2826; only the entry rows are ported.
fn melee_self_test(w: &mut World, att: FighterId, activation: bool, pending: &mut Vec<PendingHit>) {
    if w.fighter(att).montages.active_hits.is_empty() || actor_collision_off(w, att) {
        return;
    }
    let mut pose: Option<Option<Vec<f32>>> = None;
    let hs = take_actives(w, att);
    let mut sp = take_spheres(w);
    melee_self_test_poses(w, att, activation, pending, &mut pose, &hs, &mut sp);
    give_spheres(w, sp);
    give_actives(w, hs);
    if let Some(Some(p)) = pose {
        anim::put_pose(w, p);
    }
}

/// [`melee_self_test`] with the current pose `pose`, sampled at its first use (`Some(None)`: none).
fn melee_self_test_poses(
    w: &mut World,
    att: FighterId,
    activation: bool,
    pending: &mut Vec<PendingHit>,
    pose: &mut Option<Option<Vec<f32>>>,
    hs: &[Active],
    sp: &mut Vec<WorldSphere>,
) {
    let aid = w.fighter(att).id;
    for h in hs {
        if h.p.on_hit_buffs.is_empty() || hit_set_has(w, att, h.key, aid) || !entry_rows(&h.p.on_hit_buffs) {
            continue;
        }
        let mut met = false;
        if activation && let Some(o) = h.open {
            let top = if h.queued { 1.0 } else { Real::INFINITY };
            if let Some(op) = pose_at(w, att, o) {
                sp.clear();
                hit_spheres_into(w, att, &h.p, &op, CH_HURT, 1.0, top, h.copy, sp);
                met = !sp.is_empty() && fh::hurt_hit(w, att, sp, &op).is_some();
                anim::put_pose(w, op);
            }
        }
        if !met {
            if pose.is_none() {
                *pose = Some(sample(w, att));
            }
            let Some(Some(p)) = pose.as_ref() else { return };
            sp.clear();
            hit_spheres_into(w, att, &h.p, p, CH_HURT, 0.0, Real::INFINITY, None, sp);
            met = !sp.is_empty() && fh::hurt_hit(w, att, sp, p).is_some();
        }
        if !met {
            continue;
        }
        hit_set_add(w, att, h.key, aid);
        pending.push(PendingHit::Rows { att, rows: h.p.on_hit_buffs.clone() });
    }
}

/// `dodgeCall`: attackDodged's call reaches pfgDoDamage's entry rows: a begin-overlap
/// as attackDodged counts one that the DodgingFighters gate @410-772 lets through. Read ahead of attackDodged.
fn dodge_call(w: &World, host: FighterId, vic: FighterId, attack_id: i32, tick: i32) -> bool {
    let vid = w.fighter(vic).id;
    let Some(e) = w.fighter(host).evade.dodge.as_ref().and_then(|d| d.fighters.get(&vid)) else { return true };
    let mut over = false;
    for (&id, &t) in &e.seen {
        if tick - t > 1 {
            continue;
        }
        if id == attack_id {
            return false;
        }
        over = true;
    }
    !(over && e.id == Some(attack_id))
}

/// `rootCapsuleHit`: a fighter's hit shape that overlaps Pawn and is no HitBox object
/// (an OverlapAllDynamic shape, object type WorldDynamic) over the victim's root capsule: the Pawn profile Blocks
/// WorldDynamic, so the two overlap, and the handler's OnComponentBeginOverlap_Event (Comp_Pawn_HitBoxHandler @7466
/// -> pfgDoDamage @7502) takes the overlapped component's actor whatever the component is. The capsule: the actor
/// location, the current half height and radius, upright on the play plane.
fn root_capsule_hit(
    w: &mut World,
    att: FighterId,
    p: &HitPacket,
    pose: &[f32],
    vic: FighterId,
    min_scale: Real,
    max_scale: Real,
) -> Option<WorldSphere> {
    let r = crate::fighter::cap_radius(w, vic);
    let h = max(crate::fighter::capsule_half(w, vic) - r, 0.0);
    let y = crate::fighter::actor_y(w, vic);
    let body = ExtraHurt { x: w.fighter(vic).body.x, y0: y - h, y1: y + h, r };
    let mut best: Option<WorldSphere> = None;
    let mut sp = take_spheres(w);
    hit_spheres_into(w, att, p, pose, CH_PAWN, min_scale, max_scale, None, &mut sp);
    for s in sp.iter() {
        if s.ch.is_none_or(|c| c & i32::from(CH_PAWN) == 0) || !shape_over_capsule(s, &body) {
            continue;
        }
        if best.as_ref().is_none_or(|b| s.order.expect("an order") < b.order.expect("an order")) {
            best = Some(s.clone());
        }
    }
    give_spheres(w, sp);
    best
}

/// `losBlocked`: pfgDoDamage DoLOSCheck (3298-3778, 6852-7067): LineTraceSingle on
/// trace type 4 (ProjectileExplode) from the hitbox owner's actor location to the victim's. Only the Floor collision
/// profile blocks that channel, so the hit is dropped exactly when the stage floor box cuts the segment or holds its
/// start, or a stage solid whose collision blocks the channel does (a polygon solid: by its own line trace).
pub fn los_blocked(w: &mut World, x0: Real, y0: Real, x1: Real, y1: Real) -> bool {
    let why = crate::name!("the DoLOSCheck trace");
    let gs = crate::world::stage_query::static_shapes(w, crate::consts::SOLID_EXPLODE, why);
    gs.polys.iter().any(|q| q.ray(x0, y0, x1, y1).is_some())
        || gs.boxes.iter().any(|g| {
            segment_in_box(
                crate::math::narrow(g.left),
                crate::math::narrow(g.right),
                crate::math::narrow(g.bottom),
                crate::math::narrow(g.top),
                x0,
                y0,
                x1,
                y1,
            )
        })
}

/// `segmentInBox`: the segment (x0, y0)-(x1, y1) against the box by its slabs.
#[allow(clippy::too_many_arguments, reason = "the box and the segment")]
fn segment_in_box(left: Real, right: Real, bottom: Real, top: Real, x0: Real, y0: Real, x1: Real, y1: Real) -> bool {
    let mut t0: Real = 0.0;
    let mut t1: Real = 1.0;
    let mut slab = |p: Real, d: Real, lo: Real, hi: Real| -> bool {
        if d == 0.0 {
            return p >= lo && p <= hi;
        }
        let a = (lo - p) / d;
        let b = (hi - p) / d;
        t0 = max(t0, min(a, b));
        t1 = min(t1, max(a, b));
        t0 <= t1
    };
    slab(x0, x1 - x0, left, right) && slab(y0, y1 - y0, bottom, top)
}

// ---- a projectile against a fighter

/// `ownerTeam(pr)`: the owner's team, -1 for none.
fn projectile_team(w: &World, pr: ProjectileId) -> i32 {
    owner_team(w, ActorRef::Projectile(pr))
}

/// `hitboxActive`: its hit box on, the actor alive and its collision on (a class's override included:
/// `projectile::hitbox_active`).
pub(crate) fn hitbox_active(w: &World, pr: ProjectileId) -> bool {
    w.actors.projectiles.get(pr).is_some_and(crate::actors::projectile::hitbox_active)
}

/// `hitShapes` (`projectile::hit_shapes`, a class's override included): the sphere on the play plane (`sphere_3d`)
/// or HitBox2D's circle, at hitX / hitY; C015StewSpitAir's adds its CapsuleHitBox.
fn hit_shapes(w: &World, pr: ProjectileId) -> Vec<WorldSphere> {
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    let mut out = Vec::new();
    crate::actors::projectile::hit_shapes(w, p, &mut out);
    out
}

/// `projectileTest`: the projectile's HitBox2D (a circle), or its HitBox sphere on the
/// play plane, against a fighter's hurtboxes; then each of its extra dealing boxes (`extraHitBoxes`: Gizmo's popcorn
/// has them) the same way.
/// Unverified: the order of the boxes' begin overlaps with one fighter is not read: the actor's own box first.
pub fn projectile_test(
    w: &mut World,
    pr: ProjectileId,
    vic: FighterId,
    pending: &mut Vec<PendingHit>,
    mid_frame: bool,
) {
    // The class's own fighter-overlap graph beside the parent's hit.
    if let Some(own) = projectile_hooks(w, pr).and_then(|k| k.overlap) {
        w.overlap_mid_frame = mid_frame;
        own(w, pr, vic);
        w.overlap_mid_frame = false;
    }
    projectile_box_test(w, pr, vic, pending, mid_frame, None);
    let extra = crate::actors::projectile::extra_hit_boxes(w, pr);
    for i in 0..extra {
        projectile_box_test(w, pr, vic, pending, mid_frame, Some(i));
    }
}

/// The packet of the dealing box `bx` (`box ? box.hit : pr.hit`).
fn box_packet(w: &World, pr: ProjectileId, bx: Option<usize>) -> &crate::combat::HitPacket {
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    match bx {
        None => &p.hit,
        Some(i) => crate::chars::gizmo::popcorn::box_hit(p, i),
    }
}

/// The hit list of the dealing box `bx` has the fighter (`set.has`).
fn box_has_hit(w: &World, pr: ProjectileId, bx: Option<usize>, vid: i32) -> bool {
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    match bx {
        None => p.hit_set.contains(&vid),
        Some(i) => crate::chars::gizmo::popcorn::box_hit_set(p, i).contains(&vid),
    }
}

/// `set.add` on the hit list of the dealing box `bx`.
fn box_add_hit(w: &mut World, pr: ProjectileId, bx: Option<usize>, vid: i32) {
    let p = w.actors.projectiles.get_mut(pr).expect("a live projectile");
    match bx {
        None => p.hit_set.insert(vid),
        Some(i) => crate::chars::gizmo::popcorn::box_hit_set_mut(p, i).insert(vid),
    };
}

/// `box ? box.shapes() : pr.hitShapes()`.
fn box_shapes(w: &World, pr: ProjectileId, bx: Option<usize>) -> Vec<WorldSphere> {
    match bx {
        None => hit_shapes(w, pr),
        Some(i) => {
            let mut out = Vec::new();
            crate::chars::gizmo::popcorn::box_shapes(
                w.actors.projectiles.get(pr).expect("a live projectile"),
                i,
                &mut out,
            );
            out
        }
    }
}

/// `box.meetsFighter(vic)`: `None` for a box without one (its shapes meet the fighter's hurt boxes).
fn box_meets(w: &World, pr: ProjectileId, bx: Option<usize>, vic: FighterId) -> Option<bool> {
    bx.and_then(|i| crate::chars::gizmo::popcorn::box_meets_fighter(w, pr, i, vic))
}

/// Whether the dealing box `bx` meets the fighter now: its own test, or its shapes against his hurt boxes.
fn box_met(w: &mut World, pr: ProjectileId, bx: Option<usize>, vic: FighterId) -> bool {
    if let Some(met) = box_meets(w, pr, bx, vic) {
        return met;
    }
    let Some(vp) = victim_pose(w, vic) else { return false };
    let shapes = box_shapes(w, pr, bx);
    let met = fh::hurt_hit(w, vic, &shapes, &vp).is_some();
    give_vbuf(w, vp);
    met
}

/// `projectileBoxTest` of one dealing box of the projectile: `bx` `None` is its own
/// (`hitShapes`, `hit`, `hitSet`), else an extra one.
fn projectile_box_test(
    w: &mut World,
    pr: ProjectileId,
    vic: FighterId,
    pending: &mut Vec<PendingHit>,
    mid_frame: bool,
    bx: Option<usize>,
) {
    let vid = w.fighter(vic).id;
    let team = projectile_team(w, pr);
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    let (dead, owner) = (p.dead, p.owner);
    let in_set = box_has_hit(w, pr, bx, vid);
    let same = w.fighter(vic).team.team == team;
    if !hitbox_active(w, pr) || dead || same || !alive(w, vic) || no_hit(w, vic) || in_set {
        if same && owner != Some(vic) {
            projectile_ally_test(w, pr, vic, pending, bx);
        }
        return;
    }
    if !box_met(w, pr, bx, vic) {
        return;
    }
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    let (x, y) = (p.x, p.y);
    let h = box_packet(w, pr, bx);
    let (los, bkb, each_once, once) = (
        h.do_los_check.unwrap_or(false),
        h.base_knock_back.unwrap_or(0.0),
        h.hit_each_actor_only_once.unwrap_or(false),
        h.hit_only_one_fighter.unwrap_or(false),
    );
    let (vx, vy) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
    if los && los_blocked(w, x, y, vx, vy) {
        return;
    }
    let d = hits::dealer(w, ActorRef::Projectile(pr));
    if hits::projectile_blocks(w, &d, vic) {
        return;
    }
    if hits::invulnerable(w.fighter(vic)) {
        // pfgDoDamage @7381, the projectile's handler; @7538-7637: its owner's (the actor's) BPI_CanMarkFighter.
        let mark = projectile_hooks(w, pr).and_then(|k| k.can_mark).is_some_and(|m| m(w, pr));
        let tick = w.flow.tick;
        pending.push(PendingHit::ProjectileDodge { pr, vic, attack_id: d.attack_id, bkb, tick, mark });
        return;
    }
    dodge_over(w, ActorRef::Projectile(pr), vic);
    if each_once {
        box_add_hit(w, pr, bx, vid);
    }
    // HasHitAFighter (HitBoxHandler @16069-16144) is set only behind LocalHasHit (@15812): not on a hit Light
    // Projectile Armor fails. The hit itself is applied in the pending pass, so its answer is read ahead here to keep
    // the boxes off for the next fighter tested in this step, as the build's overlaps run one after the other.
    if once && !hits::light_armor_fails(w, &d, vic, &[]) {
        w.actors.projectiles.get_mut(pr).expect("a live projectile").hit_on = false;
    }
    let destroy = d.flag(w, "DestroyOnHitFighter")
        || w.actors.projectiles.get(pr).expect("a live projectile").destroy_on_enemy_hit;
    pending.push(PendingHit::Projectile { pr, vic, mid_frame, destroy, bx });
}

/// `projectileAllyTest`: a projectile's hit box over a fighter of its owner's team
/// (not the owner: NotifyAllyOfHit needs Victim != Instigator). pfgDoDamage runs for the ally as for anyone at the
/// begin of the overlap, and behind its gates reaches NotifyAllyOfHit @21344 (`allyHit`); HitEachActorOnlyOnce tracks
/// the ally too. Tested only while something listens: a buff of the ally bound to HitByAlly, a buff child on the
/// projectile, the class's own HitAlly, or an On Hit Buffs row the call applies with an ally under the box
/// (`ally_call_row`, read without a list: this runs every tick per pair).
/// Deviation: the begin of the overlap is kept per projectile (`ally_over`) only while the pair listens: an ally
/// already inside the hit box when it freezes gets the call at once (the build: at the next begin overlap).
/// `bx`: one of its extra dealing boxes (`projectile_box_test`), `None` its own.
fn projectile_ally_test(
    w: &mut World,
    pr: ProjectileId,
    vic: FighterId,
    pending: &mut Vec<PendingHit>,
    bx: Option<usize>,
) {
    let vid = w.fighter(vic).id;
    let listens =
        w.actors.projectiles.get(pr).expect("a live projectile").elem.as_ref().is_some_and(|e| !e.buffs.is_empty())
            || buff::hit_by_ally_bound(w, vic)
            || projectile_hooks(w, pr).is_some_and(|k| k.hit_ally.is_some())
            || hits::dealer(w, ActorRef::Projectile(pr)).any_buff_row(w, ally_call_row);
    if !listens {
        over_remove(w, pr, bx, vid);
        return;
    }
    let p = w.actors.projectiles.get(pr).expect("a live projectile");
    if !hitbox_active(w, pr) || p.dead || !alive(w, vic) || no_hit(w, vic) || box_has_hit(w, pr, bx, vid) {
        return;
    }
    if !box_met(w, pr, bx, vic) {
        over_remove(w, pr, bx, vid);
        return;
    }
    if over_has(w, pr, bx, vid) {
        return;
    }
    over_add(w, pr, bx, vid);
    let pm = w.actors.projectiles.get(pr).expect("a live projectile");
    let (x, y) = (pm.x, pm.y);
    let h = box_packet(w, pr, bx);
    let (los, each_once) = (h.do_los_check.unwrap_or(false), h.hit_each_actor_only_once.unwrap_or(false));
    let (vx, vy) = (w.fighter(vic).body.x, w.fighter(vic).body.y);
    if los && los_blocked(w, x, y, vx, vy) {
        return;
    }
    let d = hits::dealer(w, ActorRef::Projectile(pr));
    if hits::projectile_blocks(w, &d, vic) || hits::invulnerable(w.fighter(vic)) {
        return;
    }
    if each_once {
        box_add_hit(w, pr, bx, vid);
    }
    pending.push(PendingHit::ProjectileAlly { pr, vic, bx });
}

/// `over.delete(id)` of `allyOver` (the actor's own is lazy: `over !== undefined`).
fn over_remove(w: &mut World, pr: ProjectileId, bx: Option<usize>, vid: i32) {
    let p = w.actors.projectiles.get_mut(pr).expect("a live projectile");
    match bx {
        None => {
            if let Some(o) = p.ally_over.as_mut() {
                o.shift_remove(&vid);
            }
        }
        Some(i) => {
            crate::chars::gizmo::popcorn::box_ally_over_mut(p, i).shift_remove(&vid);
        }
    }
}

fn over_has(w: &mut World, pr: ProjectileId, bx: Option<usize>, vid: i32) -> bool {
    let p = w.actors.projectiles.get_mut(pr).expect("a live projectile");
    match bx {
        None => p.ally_over.as_ref().is_some_and(|o| o.contains(&vid)),
        Some(i) => crate::chars::gizmo::popcorn::box_ally_over_mut(p, i).contains(&vid),
    }
}

fn over_add(w: &mut World, pr: ProjectileId, bx: Option<usize>, vid: i32) {
    let p = w.actors.projectiles.get_mut(pr).expect("a live projectile");
    match bx {
        None => {
            p.ally_over.get_or_insert_with(Default::default).insert(vid);
        }
        Some(i) => {
            crate::chars::gizmo::popcorn::box_ally_over_mut(p, i).insert(vid);
        }
    }
}

/// `detectProjectileHits`. The projectile's movement component ticks in
/// TG_PrePhysics after both fighters' meshes, so it moves and overlaps after both fighters, tested at the end
/// position (HitBox2D is a child of the swept root Capsule).
pub fn detect_projectile_hits(w: &mut World) {
    let mut pending = Vec::new();
    // the lists by place, read anew each turn (a destroyed one stays listed until the sweep; a spawn is met)
    let mut k = 0;
    while let Some(pr) = w.actors.projectiles.nth(k) {
        k += 1;
        let mut nf = 0;
        while let Some(vic) = hits::fighter_at(w, nf) {
            nf += 1;
            projectile_test(w, pr, vic, &mut pending, false);
        }
    }
    run_pending(w, pending);
    let mut k = 0;
    while let Some(x) = w.actors.actors.nth(k) {
        k += 1;
        let b = w.actors.actors.get(x).expect("a listed actor");
        if b.dead {
            continue;
        }
        let hook = w.registry.actors.get(w.names.str(b.cls)).and_then(|e| match &e.hooks {
            ActorHooks::World(k) => k.hits,
            _ => None,
        });
        if let Some(h) = hook {
            h(w, x);
        }
    }
    let fs = crate::world::pawns::live_list(w);
    for &f in &fs {
        melee_actor_tests(w, f);
    }
    crate::world::pawns::put_fighters(w, fs);
    // A projectile's hit box against another projectile's HurtBox (the pair of one with itself too), a World.actors
    // pawn's root capsule, a placed stage actor's hurt shape, and the mortars' overlaps.
    crate::combat::spawned_hits::projectile_spawned_hits(w);
}

// ---- the fighter's hit shapes

/// `findScale`: Map_Find on a TMap<FName, FVector>: an FName key matches without case.
fn find_scale<'a>(w: &World, map: &'a [(Name, DataVec)], tag: &str) -> Option<&'a DataVec> {
    map.iter().find(|(k, _)| w.names.str(*k).eq_ignore_ascii_case(tag)).map(|(_, v)| v)
}

/// Hit-frame tags no component of the pawn carries, read in the data: GetComponentsByTag (@646) finds nothing and
/// the build has no fallback (@758 / @1895), so the warning is skipped for them. creature:HitCollision:
/// Creature_Attack_F_Montage's hit frame; Reindog is not 2D and his HitCollision sphere is tagged TailMid.
fn dead_hit_tag(ch: &str, tag: &str) -> bool {
    ch == "creature" && tag == "HitCollision"
}

/// `hitSpheres`: the spheres (and boxes, capsules) of the Hitframe's tags at the
/// pose. `chan`: the object types of the body the caller tests (CH_*): a shape whose own body overlaps none of them
/// gives no sphere. The KnockBack shapes stay out of every test. `min_scale` 1 covers the activation's two overlap
/// updates (scale 1, then the tag's scale) in one test; `max_scale` 1 leaves the first of them only. `copy_at`: for an
/// attacker with a Disguise, the pose its second mesh's clone shapes are taken at.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn hit_spheres(
    w: &mut World,
    att: FighterId,
    p: &HitPacket,
    pose: &[f32],
    chan: u8,
    min_scale: Real,
    max_scale: Real,
    copy_at: Option<OpenPose>,
) -> Vec<WorldSphere> {
    let mut out = Vec::new();
    hit_spheres_into(w, att, p, pose, chan, min_scale, max_scale, copy_at, &mut out);
    out
}

/// [`hit_spheres`] into `out` (emptied first), a list of the World's pool ([`take_spheres`]) in the hit tests.
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
pub fn hit_spheres_into(
    w: &mut World,
    att: FighterId,
    p: &HitPacket,
    pose: &[f32],
    chan: u8,
    min_scale: Real,
    max_scale: Real,
    copy_at: Option<OpenPose>,
    out: &mut Vec<WorldSphere>,
) {
    out.clear();
    // The pawn has put its Mesh's HitBox spheres at nothing (CharHooks.normalHitboxesOff): a tagged sphere is found
    // and activated as ever, with the 0.1 the native leaves for its radius. FullBody is no HitBox object.
    let off = w.fighter(att).hooks.char.normal_hitboxes_off.is_some_and(|h| h(w, att));
    let disguised = w.fighter(att).shapes.disguise.is_some();
    let mut missing: Vec<Name> = Vec::new();
    {
        let f = w.fighter(att);
        let d = hits::char_data(w, f);
        let bx = &w.fixed.samplers[&f.ch];
        let shapes = &d.boxes.shapes;
        let kb = knock_back_shapes(d);
        let is_kb = |name: &str| kb.iter().any(|k| k == name);
        let (facing, mwx, mwy) = (f.body.facing_dir, hits::mesh_wx(f), hits::mesh_wy(f));
        let lim = |v: Real| min(max(v, min_scale), max_scale);
        let at = |o: usize| crate::real(pose[o]);
        let ch = w.names.str(f.ch);
        let flags = |i: usize, s: &mut WorldSphere| {
            if bx.shape_ch[i] != CH_ALL {
                s.ch = Some(i32::from(bx.shape_ch[i]));
            }
            s.ignore_stun = bx.shape_ignore_stun[i] != 0; // IgnoreHitStun (HitBoxHandler @4848)
        };
        let order = |i: usize| shapes[i].order.unwrap_or_else(|| i32::try_from(i).expect("a shape index"));
        // A BoxComponent (Pawn_C017 SpinHit on the spine, SpinHit2D): the oriented box of the pose, each half
        // extent vector x HitBoxScale's part on that axis (ActivateHitbox SetRelativeScale3D(HitBoxScale) @16855).
        // Deviation: the activation's first overlap update (SetCollisionEnabled @16817, before the scale is set) is
        // tested at scale 1 like the spheres (min_scale), where a box never turned off before still has its own
        // RelativeScale3D (SpinHit: 1.0989).
        let boxed = |i: usize, hs: &DataVec, out: &mut Vec<WorldSphere>| {
            let o = bx.offsets[i] as usize;
            let mut v = [0.0; 9];
            for k in 0..3 {
                let part = match k {
                    0 => hs.x,
                    1 => hs.y,
                    _ => hs.z,
                };
                let s = lim(part.unwrap_or(1.0));
                v[k * 3] = at(o + 3 + k * 3) * facing * s;
                v[k * 3 + 1] = at(o + 4 + k * 3) * s;
                v[k * 3 + 2] = at(o + 5 + k * 3) * s;
            }
            let mut s = WorldSphere {
                x: mwx + at(o) * facing,
                y: mwy + at(o + 1),
                z: Some(at(o + 2)),
                r: 0.0,
                order: Some(order(i)),
                bx: Some(v),
                ..WorldSphere::default()
            };
            flags(i, &mut s);
            out.push(s);
        };
        let none = DataVec::default();
        for &tag in &p.hit_box_tags {
            let ts = w.names.str(tag);
            // HitBoxesOnWithMultipleScales (Comp_Pawn_HitBoxHandler ubergraph @9246-9429): each component of the tag
            // is activated at Map_Find(Specific Scales Per HitBox, tag) when found, else at HitBoxScale.
            // Deviation: a component two tags of one notify find is activated twice in the build and keeps the
            // second tag's scale; here each tag's shape keeps its own.
            let hs = (if p.specific_scales.is_empty() { None } else { find_scale(w, &p.specific_scales, ts) })
                .or(p.hit_box_scale.as_ref())
                .unwrap_or(&none);
            let scale = lim(hs.x.unwrap_or(1.0));
            // GetHitBoxes: with BPI_Use2DHitboxes and a tag other than FullBody, the components tagged tag + "2D":
            // shapes in the play plane whose radius takes HitBoxScale only. A tag without such a component gives no
            // hitbox.
            if d.def.use_2d && ts != "FullBody" {
                for &i in bx.tagged_suffixed(ts, "2D") {
                    let i = i as usize;
                    let sh = &shapes[i];
                    if sh.kind != BoxKind::Hit || is_kb(&sh.name) || bx.shape_ch[i] & chan == 0 {
                        continue;
                    }
                    if sh.is_box == Some(true) {
                        boxed(i, hs, out);
                        continue;
                    }
                    let o = bx.offsets[i] as usize;
                    let mut s = WorldSphere {
                        x: mwx + at(o) * facing,
                        y: mwy + at(o + 1),
                        r: crate::math::narrow(sh.radius * f64::from(scale) * f64::from(at(o + 3))),
                        order: Some(order(i)),
                        ..WorldSphere::default()
                    };
                    flags(i, &mut s);
                    out.push(s);
                }
                if disguised {
                    crate::combat::disguise::clone_hit_spheres(w, att, ts, true, hs, &lim, copy_at, out);
                }
                continue;
            }
            // A None entry in the notify's HitBoxTags goes to GetComponentsByTag like any other (@646) and finds
            // nothing. Unverified: UActorComponent::ComponentHasTag is not read in the exe.
            if ts == "None" {
                continue;
            }
            let mut any = false;
            for &i in bx.tagged(ts) {
                let i = i as usize;
                let sh = &shapes[i];
                if sh.kind != BoxKind::Hit {
                    continue;
                }
                any = true;
                if is_kb(&sh.name) || bx.shape_ch[i] & chan == 0 {
                    continue;
                }
                if sh.is_box == Some(true) {
                    boxed(i, hs, out);
                    continue;
                }
                let o = bx.offsets[i] as usize;
                if sh.capsule {
                    // A capsule hit shape (OverlapAllDynamic): its two ends and its radius x HitBoxScale.X.
                    // Deviation: SetRelativeScale3D(HitBoxScale) scales a capsule's radius by min(X, Y) and its half
                    // height by Z; every Hitframe that reaches one has HitBoxScale 1, so the ends are left unscaled.
                    out.push(WorldSphere {
                        x: mwx + at(o) * facing,
                        y: mwy + at(o + 1),
                        z: Some(at(o + 2)),
                        r: crate::math::narrow(
                            (if off { 0.1 } else { sh.radius }) * f64::from(scale) * f64::from(at(o + 6)),
                        ),
                        order: Some(order(i)),
                        ch: Some(i32::from(bx.shape_ch[i])),
                        seg: Some([mwx + at(o + 3) * facing, mwy + at(o + 4), at(o + 5)]),
                        ignore_stun: bx.shape_ignore_stun[i] != 0,
                        ..WorldSphere::default()
                    });
                    continue;
                }
                let mut s = WorldSphere {
                    x: mwx + at(o) * facing,
                    y: mwy + at(o + 1),
                    z: Some(at(o + 2)),
                    r: crate::math::narrow(
                        (if off && sh.name != "FullBody" { 0.1 } else { sh.radius })
                            * f64::from(scale)
                            * f64::from(at(o + 3)),
                    ),
                    order: Some(order(i)),
                    ..WorldSphere::default()
                };
                flags(i, &mut s);
                out.push(s);
            }
            // A Disguise: every tag is looked up on the owner's components, the second mesh's clones among them.
            if disguised && crate::combat::disguise::clone_hit_spheres(w, att, ts, false, hs, &lim, copy_at, out) {
                any = true;
            }
            if !any && !dead_hit_tag(ch, ts) {
                missing.push(tag);
            }
        }
    }
    for tag in missing {
        crate::fighter::common::warn_text(w, &format!("hit shape missing for tag {}", w.names.str(tag)));
    }
}

// ---- the call surface of the spawned actors' hits

pub use crate::combat::spawned_hits::{
    damage_actor, damage_spawned_actor, dealer_hit_non_player, item_hits, spawn_hurt_begins, spawn_place_hits,
    spawned_hit,
};

// ---- the rest of the call surface of the overlap tests

pub use crate::combat::melee_actors::{
    fighter_hits_actor, fighter_hits_item, fighter_hits_projectile, hitbox_sphere_begin,
};

/// The run of the deferred hits behind a caller's own overlap calls (`spawnOverlap`):
/// the deferred hits run in their order.
pub fn run_pending_hits(w: &mut World, pending: Vec<PendingHit>) {
    run_pending(w, pending);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_keys_as_the_ts() {
        assert_eq!(hit_key(1, 0), 8);
        assert_eq!(hit_key(0, 7), 7);
        assert_eq!(hit_key(8, 1), 65536 + 8 * 1024 + 1);
        assert_eq!(hit_key(1, 900), 65536 + 1024 + 900);
    }

    #[test]
    fn segments_against_a_box() {
        // A floor box from x -10..10, y -5..0.
        let b = (-10.0, 10.0, -5.0, 0.0);
        let hit = |x0, y0, x1, y1| segment_in_box(b.0, b.1, b.2, b.3, x0, y0, x1, y1);
        assert!(hit(-20.0, -2.0, 20.0, -2.0)); // across
        assert!(!hit(-20.0, 5.0, 20.0, 5.0)); // above
        assert!(hit(0.0, -1.0, 0.0, -1.0)); // a point inside
        assert!(!hit(0.0, 1.0, 0.0, 10.0)); // vertical, above
        assert!(hit(0.0, 10.0, 0.0, -10.0)); // vertical, through
        assert!(!hit(-20.0, 10.0, -11.0, -10.0)); // beside
    }

    #[test]
    fn ally_and_entry_rows() {
        let row = |s, h, a, r| OnHitBuffRow {
            applied_buffs: Name::NONE,
            apply_buff_self: s,
            apply_buff_hit: h,
            apply_buff_ally: a,
            apply_buff_enemy: false,
            require_hit_to_apply_buff_to_self: r,
            stack_count: None,
            buff_duration: None,
        };
        assert!(entry_rows(&[row(true, false, false, false)]));
        assert!(!entry_rows(&[row(true, true, false, true)]));
        assert!(entry_rows(&[row(false, false, true, false)]));
        assert!(!entry_rows(&[row(false, true, true, false)]));
        let p = HitPacket { on_hit_buffs: vec![row(false, true, true, false)], ..HitPacket::default() };
        assert!(hit_ally_rows(&p));
        let q = HitPacket { force_knockback: Some(true), ..HitPacket::default() };
        assert!(hit_ally_rows(&q));
        assert!(!hit_ally_rows(&HitPacket::default()));
    }
}
