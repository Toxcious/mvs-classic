//! Black Adam's electric chains: C021_ElectricChain_C and C021_ElectricChain_Starter_C
//! ([`Chain`], a projectile's `ext`): the throw, the catch of a fighter or a projectile, the chain delay, the throw
//! of the caught actor, the stun; Comp_C021_LightningManager's calls (`state::LightningManager`, cleared at his reset:
//! [`clear_manager`]); the victim buff C021_Buff_ElectricChainVictim on a fighter (`BuffExt::ChainVictim`) and on a
//! spawned actor (`ElemExt::ChainVictim`).
//!
//! `Chain`'s overrides are called from the shared code (`actors::projectile`): `activate` (a no-op:
//! there) and `attachedVelocity` ([`attached_velocity`]); its `setCondition` from the world's activation requests
//! (`world::spawning`, [`set_condition`]). Its `onHitAnything` is the kind's `hit_anything` hook ([`on_hit_anything`],
//! called by `combat::hits::hit_anything`).
//!
//! The flow: the Starter spawns attached by the ProjectileAimer, its montage's activation launches it (cond 0:
//! ThrowLightning(PercentCharged)) and opens the throw (cond 1: ListenForThrow) or shuts it (cond 2). What the
//! chain's hit box or a hit on it catches first (an enemy or a team mate, a fighter or a chainable projectile) is
//! held: the chain stops on it, stuns it (enemies: the Victim buff's grapple to its own root; mates: the electric
//! gauntlets), and after ChainDelay sends a new link to each chainable actor within TetherLength. A link
//! (ThrowAfterHit) and a Starter whose owner chose a direction (BeginThrowDelay) throw the caught enemy after
//! ThrowDelay and go.

use crate::Real;
use crate::State;
use crate::actors::projectile::{
    ProjectileExt, activate_movement, class_def, class_p, max_lifetime_at, set_hitbox, update_max_lifetime, yaw_x,
};
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::buffs::{BuffExt, BuffRef, buff, buff_mut, end_buff, inst_def_of, inst_view, spawn_buff};
use crate::combat::elem::ElemExt;
use crate::combat::elem::{
    actor_buff_signal, actor_inst_def, charge_pct, elem_buff, elem_buff_mut, end_actor_buff, spawn_actor_buff,
};
use crate::combat::hit::knockback::timer_ticks;
use crate::combat::hits::{self, actor_hit_enemy, apply_projectile_hit, dealer, direct_hit};
use crate::combat::holds::{buff_root, grapple_to_root, remove_root_grapple, set_root_mobility};
use crate::combat::{BuffClass, DataEnum, DataVec, HitPacket};
use crate::events::{Event, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::common::{actor_class_def, buff_def, lookup_owner, owner_team, warn_text};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::moves::V2;
use crate::fighter::traces::{TraceHit, sphere_trace_multi_for_objects};
use crate::fighter::{actor_y, facing, in_hitstun, invulnerable, no_hit};
use crate::math::ue::{KINDA_SMALL_NUMBER, normal, size2d};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorBuffHooks, ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks};
use crate::registry::props::{arr, num, obj, truthy};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use mvs_data::value::Props;

const CHAIN: &str = "C021_ElectricChain_C";
const STARTER: &str = "C021_ElectricChain_Starter_C";

/// `Chain extends Projectile`: what the class adds.
#[derive(State, Debug, Default)]
pub struct Chain {
    pub starter: bool,
    /// ChainLaunched, SourceLocation (the launch place; the zero vector before), TetherLength.
    pub launched: bool,
    pub src_x: Real,
    pub src_y: Real,
    pub tether_length: Real,
    /// ReceiveTick's DoOnce of the tether stop, AttemptToHitActorWithChain's DoOnce, the Starter tick's DoOnce.
    pub stop_done: bool,
    pub attempt_done: bool,
    pub listen_done: bool,
    /// HitActor, and StunBuff (a fighter's entry or a projectile's serial).
    pub caught: Option<ActorRef>,
    pub stun_f: Option<BuffRef>,
    pub stun_a: Option<u32>,
    /// The ages its timers fire at (-1: not set): AfterChainDelay, ThrowActor, the TailArrivalTimer.
    pub chain_at: Real,
    pub throw_at: Real,
    pub tail_at: Real,
    /// The Starter's ActivationConditions, ListenForThrow, and the cleared HitAnything.
    pub conds: [bool; 3],
    pub listen: bool,
    pub hit_any_off: bool,
    /// The launch velocity an attached Starter's deactivated ProjectileMovement keeps for its Activate.
    pub launch_vx: Real,
    pub launch_vy: Real,
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn chain(w: &World, id: ProjectileId) -> &Chain {
    match &pr(w, id).ext {
        ProjectileExt::Chain(c) => c,
        e => panic!("an electric chain without its state: {e:?}"),
    }
}

fn chain_mut(w: &mut World, id: ProjectileId) -> &mut Chain {
    match &mut pm(w, id).ext {
        ProjectileExt::Chain(c) => c,
        e => panic!("an electric chain without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a chain has an owner")
}

fn me(id: ProjectileId) -> ActorRef {
    ActorRef::Projectile(id)
}

/// `Chain.attachedVelocity`: the launch velocity an attached Starter's deactivated movement keeps.
// Unverified: an attached Starter's world yaw after the hand socket moves later in the frame is not read; the
// look-at's is kept.
pub fn attached_velocity(p: &Projectile) -> Option<(Real, Real)> {
    match &p.ext {
        ProjectileExt::Chain(c) => Some((c.launch_vx, c.launch_vy)),
        _ => p.attached_vel,
    }
}

/// `clearManager` (ResetLightningManager; the joined `reset` and `matchReset`).
pub fn clear_manager(w: &mut World, f: FighterId) {
    if let Some(s) = super::state::black_adam_mut(w, f) {
        s.lightning.targets.clear();
        s.lightning.dirs.clear();
    }
}

/// The constructor's tail: the Starter flag and the launch velocity (InitialMovementDirection x InitialSpeed).
// Unverified: a launch term of the spawn that the throw does not keep (ChargeAffectsInitialSpeed,
// AddMomentumAlpha, MovementSpeedAddsToInitialSpeed); the port does not.
fn construct(w: &World, p: &mut Projectile) {
    let (lvx, lvy) = if p.attached { p.attached_vel.unwrap_or((0.0, 0.0)) } else { (p.vx, p.vy) };
    let starter = w.names.str(p.cls) == STARTER;
    p.ext = ProjectileExt::Chain(Chain {
        starter,
        launch_vx: lvx,
        launch_vy: lvy,
        chain_at: -1.0,
        throw_at: -1.0,
        tail_at: -1.0,
        ..Chain::default()
    });
}

fn team_of(w: &World, a: ActorRef) -> i32 {
    match a {
        ActorRef::Fighter(f) => w.fighter(f).team.team,
        _ => owner_team(w, a),
    }
}

fn loc_of(w: &World, a: ActorRef) -> (Real, Real) {
    match a {
        ActorRef::Fighter(f) => (w.fighter(f).body.x, actor_y(w, f)),
        ActorRef::Projectile(p) => w.actors.projectiles.get(p).map_or((0.0, 0.0), |p| (p.x, p.y)),
        _ => (0.0, 0.0),
    }
}

/// IsProjectile: the class implements BPI_Projectiles (ParentProjectilePhysics and its children) or its
/// Comp_Actor_Destructible says so.
fn is_projectile_actor(w: &World, x: ProjectileId) -> bool {
    let p = pr(w, x);
    let d = actor_class_def(w, lookup_owner(w, me(x)), p.cls);
    let s = w.names.str(p.cls);
    if s == "ParentProjectilePhysics_C" {
        return true;
    }
    if d.and_then(|d| d.supers.as_deref()).is_some_and(|s| format!(" {s} ").contains(" ParentProjectilePhysics_C ")) {
        return true;
    }
    d.and_then(|d| obj(&d.components, "Comp_Actor_Destructible"))
        .is_some_and(|c| c.get("IsProjectile").is_some_and(truthy))
}

/// IsActorChainable: this attack has not chained it yet (AttackID > the manager's latest for it), it is not the
/// owner and not a chain, and it is a fighter or a projectile whose ProjectileAttackType is not in
/// UnchainableProjectileTypes.
fn is_chainable(w: &World, id: ProjectileId, a: ActorRef) -> bool {
    let o = owner(w, id);
    let attack_id = pr(w, id).attack_id;
    let latest = super::state::black_adam_of(w, o).map_or(0, |s| s.lightning.targets.get(&a).copied().unwrap_or(-1));
    if attack_id <= latest {
        return false;
    }
    match a {
        ActorRef::Fighter(f) => f != o,
        ActorRef::Projectile(x) => {
            let Some(t) = w.actors.projectiles.get(x) else { return false };
            if matches!(t.ext, ProjectileExt::Chain(_)) {
                return false;
            }
            let t_type = t.attack_type;
            is_projectile_actor(w, x)
                && !arr(class_p(w, pr(w, id)), "UnchainableProjectileTypes")
                    .iter()
                    .any(|v| v.as_f64() == Some(f64::from(crate::real(t_type))))
        }
        // Unverified: an item (a chain on an item is not ported: here it is false).
        _ => false,
    }
}

/// AttemptToHitActorWithChain @5469: IsActorChainable, a DoOnce, HitActorWithChain.
fn attempt(w: &mut World, id: ProjectileId, a: ActorRef) {
    if pr(w, id).dead || !is_chainable(w, id, a) || chain(w, id).attempt_done {
        return;
    }
    chain_mut(w, id).attempt_done = true;
    hit_actor_with_chain(w, id, a);
}

/// ThrowLightningWithAuthority(PercentCharged) @5325: detach, ProjectileMovement.Activate, ThrowLightningMulticast.
fn throw_lightning(w: &mut World, id: ProjectileId, pc: Real) {
    let (min_l, max_l) = {
        let d = class_p(w, pr(w, id));
        (num(d, "MinLength").unwrap_or(Real::NAN), num(d, "MaxLength").unwrap_or(Real::NAN))
    };
    let p = pm(w, id);
    let (lvx, lvy) = match &p.ext {
        ProjectileExt::Chain(c) => (c.launch_vx, c.launch_vy),
        _ => (0.0, 0.0),
    };
    if p.attached {
        p.attached = false;
        p.vx = lvx;
        p.vy = lvy;
    }
    activate_movement(p);
    let (x, y) = (p.x, p.y);
    let c = chain_mut(w, id);
    c.tether_length = min_l + (max_l - min_l) * pc;
    c.src_x = x;
    c.src_y = y;
    c.launched = true;
}

/// BeginThrowDelay @7777: timer(ThrowActor, ThrowDelay) and StartTailTimerMulticast(ThrowDelay).
fn begin_throw_delay(w: &mut World, id: ProjectileId) {
    let t = timer_ticks(num(class_p(w, pr(w, id)), "ThrowDelay"));
    let at = crate::real(pr(w, id).age) + t;
    let c = chain_mut(w, id);
    c.throw_at = at;
    c.tail_at = at;
}

/// HitActorWithChain @7753 -> ChainActorWithID @7483 -> @2705 (the Starter's override @2421 adds the grab).
fn hit_actor_with_chain(w: &mut World, id: ProjectileId, a: ActorRef) {
    let o = owner(w, id);
    let attack_id = pr(w, id).attack_id;
    chain_mut(w, id).caught = Some(a);
    if let Some(s) = super::state::black_adam_mut(w, o) {
        s.lightning.targets.insert(a, attack_id);
    }
    let enemy = team_of(w, a) != w.fighter(o).team.team;
    // HitActorMulticast @8127-8663: the hit sound at the chain for an enemy (the emitter is the view's).
    if enemy {
        let (fighter, x, y) = (w.fighter(o).id, pr(w, id).x, pr(w, id).y);
        let ev = name!("sfx_cha_C021_chain_lightning_hit");
        w.events.push(Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    }
    let (throw_delay, chain_delay, throw_after) = {
        let d = class_p(w, pr(w, id));
        (num(d, "ThrowDelay").unwrap_or(Real::NAN), num(d, "ChainDelay"), d.get("ThrowAfterHit").is_some_and(truthy))
    };
    let radius = class_def(w, pr(w, id))
        .and_then(|d| obj(&d.components, "HitBox"))
        .and_then(|h| num(h, "SphereRadius"))
        .unwrap_or(0.0);
    let p = pm(w, id);
    p.vx = 0.0;
    p.vy = 0.0;
    if p.attached
        && let ProjectileExt::Chain(c) = &mut p.ext
    {
        c.launch_vx = 0.0;
        c.launch_vy = 0.0;
    }
    set_hitbox(p, false);
    update_max_lifetime(p, throw_delay + 0.5, 1);
    // SetActorLocation(loc + Normal(loc - SourceLocation) x HitBox.SphereRadius).
    // Deviation: an attached Starter (a catch ahead of its launch) is put back on the hand by its follow in the next
    // tick, and its SourceLocation is still the zero vector there.
    let (sx, sy) = match &p.ext {
        ProjectileExt::Chain(c) => (c.src_x, c.src_y),
        _ => (0.0, 0.0),
    };
    let (dx, dy) = (p.x - sx, p.y - sy);
    let [_, nx, ny] = normal([0.0, dx, dy], KINDA_SMALL_NUMBER);
    p.x += nx * radius;
    p.y += ny * radius;
    // @2149: the stun of the caught actor: the Victim for an enemy, the ally electric gauntlets for a mate.
    // Deviation: the fighter's spawnBuff takes BreakIfBlocked as true.
    let cls = if enemy { name!("C021_Buff_ElectricChainVictim_C") } else { name!("Buff_AllyElectricGauntlets_C") };
    match a {
        ActorRef::Fighter(f) => {
            let b = spawn_buff(w, f, cls, 0.0, Some(0), Some(o), true, None);
            chain_mut(w, id).stun_f = b;
        }
        _ => {
            let s = spawn_actor_buff(w, a, cls, 0, 0.0, false, Some(o));
            chain_mut(w, id).stun_a = s;
        }
    }
    let at = crate::real(pr(w, id).age) + timer_ticks(chain_delay);
    chain_mut(w, id).chain_at = at;
    if throw_after {
        begin_throw_delay(w, id);
    }
    if chain(w, id).starter {
        w.fighter_mut(o).holds.holding_enemy = true; // MulticastActorGrabbed -> SetIsHoldingEnemy(true)
    }
}

/// AfterChainDelay @5606: SphereTraceMultiForObjects(loc, loc, TetherLength + ExtraChainDistance, [2, 11, 14, 16, 3],
/// bIgnoreSelf), and for each chainable actor hit a new link from here: CompSpawnActor(C021_ElectricChain_C),
/// direction (0, Normal(target - self) x facing), speed 6000 / 6000, the chain's own AttackID.
/// No dedup (@5864-7473): every hit that passes IsActorChainable gets its link, so an actor met by two of its
/// shapes gets two.
// Unverified: the order of the trace's hits at equal time is the port's.
fn after_chain_delay(w: &mut World, id: ProjectileId) {
    let (x, y, attack_id) = (pr(w, id).x, pr(w, id).y, pr(w, id).attack_id);
    let extra = num(class_p(w, pr(w, id)), "ExtraChainDistance").unwrap_or(0.0);
    let radius = chain(w, id).tether_length + extra;
    let mut out: Vec<TraceHit> = Vec::new();
    sphere_trace_multi_for_objects(w, x, y, x, y, radius, &[2.0, 11.0, 14.0, 16.0, 3.0], &[], Some(me(id)), &mut out);
    let targets: Vec<ActorRef> = out.iter().filter_map(|h| h.actor).filter(|&a| is_chainable(w, id, a)).collect();
    let o = owner(w, id);
    for a in targets {
        let (tx, ty) = loc_of(w, a);
        let [_, ux, uy] = normal([0.0, tx - x, ty - y], KINDA_SMALL_NUMBER);
        let face = facing(w, o);
        crate::world::spawning::comp_spawn_projectile(
            w,
            o,
            name!("C021_ElectricChain_C"),
            x,
            y,
            [Some(0.0), Some(ux * face), Some(uy)],
            6000.0,
            6000.0,
            Some(attack_id),
        );
    }
}

/// ThrowActor @3488: the stun ends, and a caught enemy takes ThrowDamage from the chain's own handler with
/// BaseKnockBack ThrowUp ? 1600 : 1000, KnockBackDirectionVector ((ThrowUp ? Lerp(0.2, 0.7, (Tether - Min) / (Max -
/// Min)) : 0.3) x (owner FacingLeft ? -1 : 1), ThrowUp ? 1 : -1) and HitReaction 0, then a fighter gets
/// Buff_Damage_Electric (1 stack, 0 s, from the owner) whether the hit went through or not.
/// The vector is not IgnoreFacing: pfgDoDamage mirrors its X again by the chain's yaw.
fn throw_actor(w: &mut World, id: ProjectileId) {
    let a = chain(w, id).caught;
    end_stun(w, id);
    let o = owner(w, id);
    let Some(a) = a else { return };
    let gone = match a {
        ActorRef::Projectile(x) => w.actors.projectiles.get(x).is_none_or(|p| p.dead),
        _ => false,
    };
    if gone || team_of(w, a) == w.fighter(o).team.team {
        return;
    }
    let up = super::state::black_adam_of(w, o)
        .is_some_and(|s| s.lightning.dirs.get(&pr(w, id).attack_id).copied().unwrap_or(true));
    let (min_l, max_l) = {
        let d = class_p(w, pr(w, id));
        (num(d, "MinLength").unwrap_or(Real::NAN), num(d, "MaxLength").unwrap_or(Real::NAN))
    };
    let sign = if facing(w, o) < 0.0 { -1.0 } else { 1.0 };
    let h = (if up { 0.2 + 0.5 * ((chain(w, id).tether_length - min_l) / (max_l - min_l)) } else { 0.3 }) * sign;
    let bkb = if up { 1600.0 } else { 1000.0 };
    let v = if up { 1.0 } else { -1.0 };
    let mut hit = obj(class_p(w, pr(w, id)), "ThrowDamage")
        .map_or_else(HitPacket::default, |p| HitPacket::from_props(&w.names, p, None));
    hit.base_knock_back = Some(bkb);
    hit.knock_back_direction_vector = Some(DataVec { x: Some(h), y: Some(v), z: None });
    hit.hit_reaction = Some(DataEnum::Value(0));
    let dir = V2 { x: h * yaw_x(pr(w, id)), y: v }; // the vector's X is UE Y, mirrored by the chain's yaw
    match a {
        ActorRef::Fighter(vic) => {
            if crate::fighter::spared_by(w, o, vic) {
                return; // the port's own spare switch: no hit and no Buff_Damage_Electric_C on an opponent
            }
            if w.fighter(vic).life.dead <= 0.0 && !no_hit(w, vic) {
                if invulnerable(w, vic) {
                    let tick = w.flow.tick;
                    attack_dodged(w, me(id), vic, pr(w, id).attack_id, bkb, tick, None, false);
                } else {
                    dodge_over(w, me(id), vic);
                    let d = dealer(w, me(id));
                    let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
                    if apply_projectile_hit(w, &d, vic, &hit, &[], at, Some(dir), &[], charge_pct(w, me(id))) {
                        actor_hit_enemy(w, &d, vic);
                    }
                }
            }
            // @4902-4953: the chain owner's Comp_State_Manager makes the call, BreakIfBlocked false.
            spawn_buff(w, vic, name!("Buff_Damage_Electric_C"), 0.0, Some(1), Some(o), false, Some(o));
        }
        ActorRef::Projectile(x) => {
            // Unverified: on a projectile only the HitByEnemy broadcast is made (ThrowDamage's Damage is 0, so no
            // ApplyDamage).
            let t = pr(w, id).attack_type;
            hits::hit_by(w, x, false, dir.x, dir.y, bkb, t, Some(me(id)));
        }
        _ => {}
    }
}

fn end_stun(w: &mut World, id: ProjectileId) {
    let (a, bf, ba) = {
        let c = chain(w, id);
        (c.caught, c.stun_f, c.stun_a)
    };
    if let (Some(b), Some(ActorRef::Fighter(f))) = (bf, a)
        && buff(w, b).is_some_and(|x| x.frames > 0.0)
    {
        end_buff(w, f, b);
    }
    if let (Some(s), Some(ActorRef::Projectile(_))) = (ba, a) {
        end_actor_buff(w, a.expect("a caught actor"), s);
    }
    let c = chain_mut(w, id);
    c.stun_f = None;
    c.stun_a = None;
}

fn begin(w: &mut World, id: ProjectileId) {
    // BeginPlay @7613: LaunchChainOnSpawn (the links) with authority: ThrowLightningWithAuthority(0). The cast sound
    // is the branch without authority (Deviation: not played).
    if class_p(w, pr(w, id)).get("LaunchChainOnSpawn").is_some_and(truthy) {
        throw_lightning(w, id, 0.0);
    }
}

/// C021_ElectricChain_Starter ActivateObject @1551: the parent's (empty), then each true condition in order.
fn activate(w: &mut World, id: ProjectileId) {
    if !chain(w, id).starter {
        return;
    }
    let conds = chain(w, id).conds;
    if conds[0] {
        let pc = pr(w, id).charge.as_ref().map_or(0.0, |c| c.pct); // GetChargeValue(4)
        throw_lightning(w, id, pc);
    }
    if conds[1] {
        chain_mut(w, id).listen = true;
    }
    if conds[2] {
        // EX_ClearMulticastDelegate on its own Comp_Pawn_HitBoxHandler.HitAnything.
        // Simplification: the clear empties the whole invocation list (CLEAR ALL); the port stops only the class's
        // own handler and keeps the buff children's listeners. HitNonPlayer is an override, not a binding.
        let c = chain_mut(w, id);
        c.listen = false;
        c.hit_any_off = true;
    }
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    let p = pr(w, id);
    let life = max_lifetime_at(p);
    // Deviation: the attached Starter's MaxLifetimeTimer (the port's attached move has no expiry).
    if p.attached && life > 0 && p.age >= life {
        return true;
    }
    // ReceiveTick @1064-1270: once ChainLaunched and TetherLength away from SourceLocation, a DoOnce stops it.
    let c = chain(w, id);
    if c.launched && !c.stop_done && size2d([p.x - c.src_x, p.y - c.src_y]) >= c.tether_length {
        chain_mut(w, id).stop_done = true;
        let p = pm(w, id);
        p.vx = 0.0;
        p.vy = 0.0;
    }
    // The Starter's tick @2445: with ListenForThrow and the owner holding (BPI_GetIsHoldingEnemy), a DoOnce @1513:
    // ThrowUp from the aim's height (Z < 0 up, Z > 0 down), the stick's MoveY >= 0 at height 0; the montage's
    // section, SetThrowDirectionForID, BeginThrowDelay.
    let o = owner(w, id);
    let c = chain(w, id);
    if c.starter && c.listen && w.fighter(o).holds.holding_enemy && !c.listen_done {
        chain_mut(w, id).listen_done = true;
        let z = w.fighter(o).items.proj_aim.z;
        let up = if z < 0.0 {
            true
        } else if z > 0.0 {
            false
        } else {
            w.fighter(o).controls.inp.y >= 0.0
        };
        if let Some(slot) = crate::fighter::anim::grouped::active_instance(w, o) {
            let section = if up { name!("ThrowUp") } else { name!("ThrowDown") };
            crate::fighter::anim::slot_jump_to_section(w, o, slot, section);
        }
        let attack_id = pr(w, id).attack_id;
        if let Some(s) = super::state::black_adam_mut(w, o) {
            s.lightning.dirs.insert(attack_id, up);
        }
        begin_throw_delay(w, id);
    }
    // Deviation: its timers fire in the actor's tick, not in the world's timer pass.
    let (age, chain_at, throw_at) = (crate::real(pr(w, id).age), chain(w, id).chain_at, chain(w, id).throw_at);
    if chain_at >= 0.0 && age >= chain_at {
        chain_mut(w, id).chain_at = -1.0;
        after_chain_delay(w, id);
    }
    if throw_at >= 0.0 && age >= throw_at {
        chain_mut(w, id).throw_at = -1.0;
        throw_actor(w, id);
        return true;
    }
    false
}

/// HitNonPlayer @5121 (its override, AttemptToHitActorWithChain @5140 with no parent call).
fn hit_non_player(w: &mut World, id: ProjectileId, vic: ActorRef) {
    attempt(w, id, vic);
}

/// OnHitByEnemy @5164 (an Attacker that is a spawned actor).
#[allow(clippy::too_many_arguments, reason = "the hook's parameters")]
fn hit_by(
    w: &mut World,
    id: ProjectileId,
    ally: bool,
    _dx: Real,
    _dy: Real,
    _bkb: Real,
    _t: i32,
    by: Option<ActorRef>,
) {
    if !ally && let Some(b @ (ActorRef::Projectile(_) | ActorRef::Item(_))) = by {
        attempt(w, id, b);
    }
}

/// HitAlly @5273.
fn hit_ally(w: &mut World, id: ProjectileId, vic: FighterId) {
    attempt(w, id, ActorRef::Fighter(vic));
}

/// ReceiveDestroyed @3174: the beam emitters (the view's) and a valid StunBuff.
fn destroyed(w: &mut World, id: ProjectileId) {
    end_stun(w, id);
}

/// The Starter's EndPlay @2119: SetIsHoldingEnemy(false).
fn end(w: &mut World, id: ProjectileId) {
    if chain(w, id).starter {
        let o = owner(w, id);
        w.fighter_mut(o).holds.holding_enemy = false;
    }
}

/// `setCondition` (BPI_SetActivationConditions).
pub fn set_condition(w: &mut World, id: ProjectileId, n: i32, v: bool) {
    if let ProjectileExt::Chain(c) = &mut pm(w, id).ext
        && let Ok(i) = usize::try_from(n)
        && i < c.conds.len()
    {
        c.conds[i] = v;
    }
}

/// OnHitAnything @5069: the parent's (its entry statement, `combat::hits::hit_anything`), then the attempt; activation
/// condition 2 clears the binding (`hit_any_off`).
fn on_hit_anything(w: &mut World, id: ProjectileId, victim: ActorRef) {
    if !chain(w, id).hit_any_off {
        attempt(w, id, victim);
    }
}

// ---- C021_Buff_ElectricChainVictim

fn loop_key(w: &World, f: FighterId) -> SoundKey {
    SoundKey { tag: name!("C021_Buff_ElectricChainVictim_C:"), n: w.fighter(f).id }
}

/// On a fighter (authority): BeginPlay InitiateGrapple(DefaultSceneRoot, None) and a looping timer(DamageTick, Damage
/// Interval); EndPlay RemoveGrapple. Its loop sound is BuffParentActor's (SFX_Loop_Start at BeginPlay, SFX_Loop_Stop
/// at EndPlay).
fn victim_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _: Option<Real>) {
    let (start, iv) = {
        let d = inst_view(w, b);
        (d.get("SFX_Loop_Start").and_then(|v| v.as_str()).map(str::to_owned), d.num("Damage Interval"))
    };
    if let Some(s) = start.filter(|s| !s.is_empty()) {
        let ev = crate::fighter::control::data_name(w, &s);
        let fighter = w.fighter(f).id;
        let key = loop_key(w, f);
        w.fighter_mut(f).events.push(Event::SoundStart { fighter, ev, key });
    }
    let root = buff_root(w, ActorRef::Fighter(f));
    grapple_to_root(w, f, root);
    let at = crate::real(w.fighter(f).clock.tick) + timer_ticks(iv);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::ChainVictim { at, by: from };
    }
}

/// DamageTick @1056: Damage Per Tick's HitStunMult = Interval / 2, then the Instigator's pfgDoDamage(null, owner,
/// Damage Per Tick).
fn victim_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(&BuffExt::ChainVictim { at, by }) = buff(w, b).map(|x| &x.ext) else { return false };
    let tick = crate::real(w.fighter(f).clock.tick);
    if tick < at {
        return false;
    }
    let (iv, dpt) = {
        let d = inst_def_of(w, b);
        let d = d.as_deref();
        (
            d.and_then(|d| num(d, "Damage Interval")),
            d.and_then(|d| obj(d, "Damage Per Tick")).map(|p| HitPacket::from_props(&w.names, p, None)),
        )
    };
    if let Some(BuffExt::ChainVictim { at, .. }) = buff_mut(w, b).map(|x| &mut x.ext) {
        *at += timer_ticks(iv);
    }
    let Some(by) = by else { return false };
    if w.fighter(f).life.dead > 0.0 || crate::combat::phases::no_hit_by(w, by, f) {
        return false;
    }
    let mut dpt = dpt.unwrap_or_default();
    if invulnerable(w, f) {
        let tick = w.flow.tick;
        attack_dodged(w, ActorRef::Fighter(by), f, 0, dpt.base_knock_back.unwrap_or(0.0), tick, Some(by), false);
    } else {
        dodge_over(w, ActorRef::Fighter(by), f);
        dpt.hit_stun_mult = Some((iv.unwrap_or(Real::NAN)) / 2.0);
        dpt.knock_back_direction = dpt.knock_back_direction_vector;
        dpt.on_hit_sound = dpt.on_hit_audio;
        direct_hit(w, by, f, &dpt);
    }
    false
}

fn victim_end(w: &mut World, f: FighterId, b: BuffRef) {
    let cls = buff(w, b).map_or(Name::NONE, |x| x.cls);
    let stop = buff_def(w, ActorRef::Fighter(f), cls)
        .and_then(|d| d.get("SFX_Loop_Stop"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    if let Some(s) = stop {
        let ev = crate::fighter::control::data_name(w, &s);
        let fighter = w.fighter(f).id;
        let key = loop_key(w, f);
        w.fighter_mut(f).events.push(Event::SoundStop {
            fighter,
            key,
            fade: None,
            ev: Some(ev),
            curve: None,
            cut: false,
        });
    }
    remove_root_grapple(w, f);
}

fn set_mobility(w: &mut World, a: ActorRef, m: u8) {
    match a {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                set_root_mobility(p, m);
            }
        }
        ActorRef::Item(id) => {
            if let Some(i) = w.actors.items.get_mut(id) {
                set_root_mobility(i, m);
            }
        }
        _ => {}
    }
}

/// On a spawned actor (BPI_SpawnedActor @367): SetMobility(Stationary) at BeginPlay @501 and, with authority, the same
/// looping timer (@573 -> @255).
fn victim_actor_begin(w: &mut World, a: ActorRef, i: u32, from: Option<FighterId>) -> bool {
    set_mobility(w, a, 1);
    if let Some(from) = from {
        let iv = actor_inst_def(w, a, i).and_then(|d| num(&d, "Damage Interval"));
        let at = crate::real(w.flow.tick) + timer_ticks(iv);
        let by = w.fighter(from).id;
        if let Some(b) = elem_buff_mut(w, a, i) {
            b.ext = ElemExt::ChainVictim { at, by };
        }
    }
    false
}

/// The DamageTick on a spawned actor, pfgDoDamage of the Instigator's (a fighter's) handler with the actor as
/// HitActor: nothing while the Instigator is in hitstun (@5257); the chainable actors all have a
/// Comp_Actor_Destructible with IsProjectile, so @15084 only the actor's Comp_Pawn_DamageManager HitByAlly (the
/// Instigator's TeamIndex, @19370) or HitByEnemy broadcast @19575: the class's own handler (`hits::hit_by`) and its
/// buffs' (Buff_C021_SlowReflectProjectile reflects it for the Instigator). Damage 1 never reaches ApplyDamage.
/// The broadcast's direction: Damage Per Tick's vector (0, 0) with the Instigator's facing.
// Deviation: the timer counts world ticks from the BeginPlay and fires in the actor's buff pass, not in the world's
// timer pass; ApplyBuffDamageMods' dealt mods of the Instigator are not read.
fn victim_actor_tick(w: &mut World, a: ActorRef, i: u32) -> bool {
    let ActorRef::Projectile(pid) = a else { return false };
    let Some(ElemExt::ChainVictim { at, by }) = elem_buff(w, a, i).map(|b| b.ext) else { return false };
    if crate::real(w.flow.tick) < at {
        return false;
    }
    let (iv, kx, ky, bkb) = {
        let d = actor_inst_def(w, a, i);
        let d = d.as_deref().map(|d| d as &Props);
        let dpt = d.and_then(|d| obj(d, "Damage Per Tick"));
        let v = dpt.and_then(|p| obj(p, "KnockBackDirectionVector"));
        (
            d.and_then(|d| num(d, "Damage Interval")),
            v.and_then(|v| num(v, "X")).unwrap_or(0.0),
            v.and_then(|v| num(v, "Y")).unwrap_or(0.0),
            dpt.and_then(|p| num(p, "BaseKnockBack")).unwrap_or(0.0),
        )
    };
    if let Some(b) = elem_buff_mut(w, a, i)
        && let ElemExt::ChainVictim { at, .. } = &mut b.ext
    {
        *at += timer_ticks(iv);
    }
    let Some(by) = match_fighters(w).into_iter().find(|&f| w.fighter(f).id == by) else { return false };
    // actorDamageTick: the Instigator in hitstun or the actor dead, nothing.
    if w.actors.projectiles.get(pid).is_none_or(|p| p.dead) || in_hitstun(w, by) {
        return false;
    }
    let cls = pr(w, pid).cls;
    let destructible = actor_class_def(w, lookup_owner(w, a), cls)
        .and_then(|d| obj(&d.components, "Comp_Actor_Destructible"))
        .is_some_and(|c| c.get("IsProjectile").is_some_and(truthy));
    if !destructible {
        let msg = format!(
            "C021_Buff_ElectricChainVictim_C: DamageTick on {} (no IsProjectile: DamageAndLaunch @15630) is not ported",
            w.names.str(cls)
        );
        warn_text(w, &msg);
        return false;
    }
    let (dx, dy) = (kx * facing(w, by), ky);
    let ally = w.fighter(by).team.team == owner_team(w, a);
    hits::hit_by(w, pid, ally, dx, dy, bkb, -1, None);
    if !ally && w.actors.projectiles.get(pid).is_some_and(|p| !p.dead && p.elem.is_some()) {
        actor_buff_signal(w, a, ActorBuffSignal::HitByEnemy, Some(&ActorBuffArg { by, kx: -dx, ky: dy }));
    }
    false
}

fn victim_actor_end(w: &mut World, a: ActorRef, _i: u32) {
    set_mobility(w, a, 2);
}

fn kind() -> ProjectileHooks {
    ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        hit_non_player: Some(hit_non_player),
        hit_anything: Some(on_hit_anything),
        hit_by: Some(hit_by),
        hit_ally: Some(hit_ally),
        activate: Some(activate),
        destroyed: Some(destroyed),
        end: Some(end),
        ..ProjectileHooks::default()
    }
}

/// This file's registry entries (the two chain classes, the victim buff).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        CHAIN,
        ActorEntry {
            calls_parent_hit_anything: true,
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::C021ElectricChain),
                ActorHooks::Projectile(kind()),
            )
        },
    )?;
    r.actors.enter(
        STARTER,
        ActorEntry {
            parents: &[CHAIN],
            calls_parent_hit_anything: true,
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::C021ElectricChainStarter),
                ActorHooks::Projectile(kind()),
            )
        },
    )?;
    let victim = BuffEntry {
        actor: Some(ActorBuffHooks {
            begin: Some(victim_actor_begin),
            tick: Some(victim_actor_tick),
            end: Some(victim_actor_end),
            ..ActorBuffHooks::default()
        }),
        ..BuffEntry::new(
            BuffClass::C021BuffElectricChainVictim,
            BuffHooks {
                begin: Some(victim_begin),
                timer: Some(victim_timer),
                end: Some(victim_end),
                ..BuffHooks::default()
            },
        )
    };
    let n = victim.class.registered_name().expect("a registered class");
    r.buffs.enter(n, victim)
}
