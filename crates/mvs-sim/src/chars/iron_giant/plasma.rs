//! Iron Giant's plasma shot (firePlasma (`cannon_aim.rs`), the
//! C017_Plasma_Shot_C projectile kind, Buff_HeldByPlasmaBall and its BallHold): the weapon mode side special's shot
//! and the hold on the fighter it hits.

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, ProjectileId};
use crate::combat::BuffRef;
use crate::combat::buff_class::BuffClass;
use crate::combat::buffs::{BuffExt, buff, buff_mut, buff_of, end_buff, spawn_buff};
use crate::combat::holds::apply_stun_buffs;
use crate::events::{Event, SoundKey};
use crate::fighter::common::latent_frames;
use crate::fighter::moves::{
    clear_gravity_override, disable_movement, enable_falling, override_gravity, recall_move, settle,
};
use crate::fighter::{FighterId, grapple_off, grapple_on, hold_def};
use crate::math::max;
use crate::name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// Buff_HeldByPlasmaBall's own variables (`BallHold`, the buff entry's extension).
#[derive(State, Debug, Default)]
pub struct BallHold {
    /// InstigatorRef, set by the shot after SpawnBuffActor (C017_Plasma_Shot @564): the projectile.
    pub shot: Option<ActorRef>,
    /// 0: polling the BuffstunTimer (@1395), 1: its Delay(0.033) @1532 runs, 2: attached (@2071-).
    pub stage: i32,
    /// The latent ticks left of the running Delay(0.033).
    pub wait: Real,
    /// Temp_int_Variable of the retry gate (@1903-1989).
    pub tries: i32,
    /// The latent ticks left of the Delay(0.06) (@2674) before the second K2_SetActorLocation, 0 none.
    pub snap: Real,
    /// ReceiveEndPlay has run.
    pub ended: bool,
}

const HELD: crate::names::Name = name!("Buff_HeldByPlasmaBall_C");

fn ball(w: &mut World, b: BuffRef) -> Option<&mut BallHold> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::HeldByPlasmaBall(e) => Some(e),
        _ => None,
    }
}

fn ball_of(w: &World, b: BuffRef) -> Option<&BallHold> {
    match &buff(w, b)?.ext {
        BuffExt::HeldByPlasmaBall(e) => Some(e),
        _ => None,
    }
}

fn loop_key(w: &World, pr: ProjectileId) -> SoundKey {
    SoundKey { tag: name!("proj"), n: w.actors.projectiles.get(pr).map_or(-1, |p| p.seq) }
}

/// C017_Plasma_Shot ReceiveBeginPlay @812: the loop @896, ahead of the test at the spawn place (whose hit stops it,
/// [`hit_enemy`]). `begin` of `add_projectile`.
pub fn plasma_begin(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let fighter = p.owner_id;
    let key = loop_key(w, pr);
    w.events.push(Event::SoundStart {
        fighter,
        ev: name!("sfx_cha_C017_weaponmode_plasmaattack_normal_missle_loop"),
        key,
    });
}

/// K2_SetActorLocation(owner, InstigatorRef.K2_GetActorLocation(), no sweep, teleport) (@2612, @2952).
fn ball_snap(w: &mut World, vic: FighterId, b: BuffRef) {
    let Some(ActorRef::Projectile(pr)) = ball_of(w, b).and_then(|e| e.shot) else { return };
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    if p.dead {
        return;
    }
    let (x, y) = (p.x, p.y);
    recall_move(w, vic, x, y);
}

/// Buff_HeldByPlasmaBall: a child of BuffParentActor (not of Buff_HeldByEnemy), the hold on the fighter a weapon mode
/// plasma shot hits. Its tick is off (PrimaryActorTick.bStartWithTickEnabled false); the graph is latent actions and
/// delegates.
// Unverified: the natives, as for Buff_HeldByEnemy (`combat::holds`): from InitiateGrapple on the held fighter's own
// movement does not run and it is put at the attach point, here the shot's location after the shot's move of every
// frame (K2_AttachToActor @2481, SnapToTarget location); it is released with no velocity.
// Unverified: with the actor's tick off its Delays would be run by the world's pass over the remaining latent
// actions (stock UE 4.26, after the timer manager; not read in the exe); taken with the owner's buff tick, the first
// Delay counting the spawn frame, as Buff_HeldByEnemy's.
///
/// BuffParentActor ReceiveBeginPlay: SFX_Loop_Start on the buff actor (@5258). ReceiveBeginPlay @3015: the owner's
/// HitstunHandler valid -> @1395: BuffstunTimer active -> Delay(0.033) @1532; else the retry gate @1803-2016: at most
/// 5 times Delay(0.033) and @1395 again. SpawnBuffActor starts the buff's own stun after BeginPlay
/// (UpdatePlayerStatus @1382), so the first test sees only an older buff's timer.
fn begin(w: &mut World, vic: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    let start = hold_def(w, vic, HELD).get("SFX_Loop_Start").and_then(|v| v.as_str()).map(str::to_owned);
    if let Some(s) = start.filter(|s| !s.is_empty()) {
        let ev = crate::fighter::control::data_name(w, &s);
        let fighter = w.fighter(vic).id;
        w.fighter_mut(vic).events.push(Event::Sound {
            fighter,
            ev,
            x: None,
            y: None,
            hit: None,
            pooled: false,
            key: None,
        });
    }
    let wait = max(latent_frames(0.033) - 1.0, 1.0);
    let stunned = w.fighter(vic).buffs.buff_stunned;
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::HeldByPlasmaBall(BallHold {
            shot: None,
            stage: i32::from(stunned),
            wait,
            tries: i32::from(!stunned),
            snap: 0.0,
            ended: false,
        });
    }
}

fn tick(w: &mut World, vic: FighterId, b: BuffRef) -> bool {
    let stunned = w.fighter(vic).buffs.buff_stunned;
    let Some(e) = ball(w, b) else { return false };
    if e.stage == 2 {
        // @2729: not being destroyed and the owner valid -> the same K2_SetActorLocation @2952.
        if e.snap > 0.0 {
            e.snap -= 1.0;
            if e.snap == 0.0 {
                ball_snap(w, vic, b);
            }
        }
        return false;
    }
    if e.wait <= 0.0 {
        return false;
    }
    e.wait -= 1.0;
    if e.wait > 0.0 {
        return false;
    }
    if e.stage == 0 {
        if stunned {
            e.stage = 1;
        } else if e.tries < 5 {
            e.tries += 1;
        } else {
            return false; // the gate is used up: the buff stays, never attached
        }
        e.wait = latent_frames(0.033);
        return false;
    }
    // @1587 (authority, not being destroyed) -> @2071: DisableMovement @2142, FMC.OverrideGravity(0.0) @2324,
    // InitiateGrapple(InstigatorRef's root, None) @2403, the owner's K2_AttachToActor(InstigatorRef, None,
    // SnapToTarget / KeepRelative / KeepWorld, weld) @2481, K2_SetActorLocation @2612, Delay(0.06) @2674. Then
    // BindEscape (@1788 -> 3164) and both actors' OnEndPlay -> DestroyWithOwner (@1662-1746).
    e.stage = 2;
    override_gravity(w, vic); // @2324
    grapple_on(w, vic);
    disable_movement(w, vic); // MOVE_None from @2142 to EndPlay's Falling
    ball_snap(w, vic, b);
    if let Some(e) = ball(w, b) {
        e.snap = latent_frames(0.06);
    }
    false
}

/// BindEscape (@3164, authority): the owner's HitstunHandler.HitStunEnded with CanGetKnockedOut (@3194-3302 -> 3775)
/// and BuffStunEnded (@3437 -> Escaped 3770), its DamageManager.HitByAlly (@3572 -> 4307) and KnockedBack (@3707 ->
/// 4297 -> 4115: K2_DetachFromActor, Falling, RemoveHitstunFromKnockBack with HitstunBuffActive): each ends in
/// K2_DestroyActor (@3755). The detach and Falling of KnockedBack are EndPlay's as well.
/// HitbyAlly (@54 -> 4307 -> 3755) reads nothing of the hit: any team mate's hit the DamageManager refuses ends the
/// hold (BuffSignal `DamageHitByAlly`). The class is entered as bound for its whole life; before BindEscape the
/// signal is dropped by the stage test, as the build has no binding there yet.
fn on(w: &mut World, vic: FighterId, b: BuffRef, sig: BuffSignal, _a: &BuffSignalArg) -> bool {
    let Some(e) = ball_of(w, b) else { return false };
    if e.stage != 2 {
        return false;
    }
    match sig {
        BuffSignal::BuffStunEnded | BuffSignal::KnockedBack | BuffSignal::DamageHitByAlly => true,
        BuffSignal::HitStunEnded => {
            let cls = buff(w, b).map_or(HELD, |x| x.cls);
            crate::registry::props::flag(hold_def(w, vic, cls), "CanGetKnockedOut")
        }
        _ => false,
    }
}

/// BuffParentActor ReceiveEndPlay: StopLoopSound (@4301 -> 4957: SFX_Loop_Stop on the buff actor), the stun's end
/// (UpdateCurrentStates @4611). ReceiveEndPlay @3140 -> 15: Unbinding, the owner's K2_DetachFromActor (KeepWorld)
/// @501 and zero rotation @559, SetMovementMode(Falling) @686 / @726, FMC.RemoveGrapple @888, ClearGravityOverride
/// @1069, Delay(0.2) @1101 -> RemoveHitstunFromKnockBack with HitstunBuffActive (@1184-1358). (@49-360 moves the buff
/// actor itself to X 0, not the owner.)
// Unverified: the Delay(0.2) is started by an actor in EndPlay; a destroyed actor's latent actions are dropped by the
// stock latent action manager (its object key no longer resolves), not read in the exe: taken as never resuming. It
// would only clear a knockback hitstun under another buff's stun.
fn end(w: &mut World, vic: FighterId, b: BuffRef) {
    let Some(e) = ball(w, b) else { return };
    if e.ended {
        return;
    }
    e.ended = true;
    let (stage, shot) = (e.stage, e.shot);
    let cls = buff(w, b).map_or(HELD, |x| x.cls);
    let stop = hold_def(w, vic, cls).get("SFX_Loop_Stop").and_then(|v| v.as_str()).map(str::to_owned);
    if let Some(s) = stop.filter(|s| !s.is_empty()) {
        let ev = crate::fighter::control::data_name(w, &s);
        let fighter = w.fighter(vic).id;
        w.fighter_mut(vic).events.push(Event::Sound {
            fighter,
            ev,
            x: None,
            y: None,
            hit: None,
            pooled: false,
            key: None,
        });
    }
    apply_stun_buffs(w, vic, None, true); // UpdateCurrentStates: the stun ends, or another live buff's stun starts
    // No release check in this class (EndPlay @15-1101): the capsule stays at the ball's centre in Falling, and a
    // capsule taller than the ball's radius can be in the floor. The movement component's next move starts in
    // penetration and moves it out (142977610 / 142977160): `settle`, as `holdEnd`.
    if stage == 2 {
        enable_falling(w, vic); // SetMovementMode(Falling) @686
        grapple_off(w, vic);
        settle(w, vic);
    }
    clear_gravity_override(w, vic); // @1069, attached or not
    if let Some(ActorRef::Projectile(pr)) = shot
        && let Some(p) = w.actors.projectiles.get_mut(pr)
        && p.hold_buff.is_some_and(|(_, hb)| hb == b)
    {
        p.hold_buff = None;
    }
}

// ------------------------------------------------------------------ the shot

/// The component-bound event of ProjectileMovement.OnProjectileBounce (@962): the bounce sound at the Capsule's
/// location @1012, inside the move, where the shot is at the hit.
fn bounce_impact(w: &mut World, pr: ProjectileId, _nx: Real, _ny: Real, _ovx: Real, _ovy: Real) -> bool {
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    let (fighter, x, y) = (p.owner_id, p.x, p.y);
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_C017_weaponmode_plasmaattack_normal_missle_bounce"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    false
}

/// The actor's tick: a fighter attached to the shot (Buff_HeldByPlasmaBall @2481) has moved with it.
fn step(w: &mut World, pr: ProjectileId) -> bool {
    for vic in crate::world::pawns::match_fighters(w) {
        let Some(b) = buff_of(w, vic, HELD) else { continue };
        let Some(e) = ball_of(w, b) else { continue };
        if e.stage == 2 && e.shot == Some(ActorRef::Projectile(pr)) && w.fighter(vic).life.dead <= 0.0 {
            ball_snap(w, vic, b);
        }
    }
    false
}

/// HitEnemyPlayer (no parent call; ubergraph 343-811): the victim's Comp_State_Manager.SpawnBuffActor(
/// Buff_HeldByPlasmaBall, victim, self, stacks 0, duration 0, BreakIfBlocked) @394 (its Instigator becomes the shot's
/// owner, SpawnBuffActor @2217), HoldBuff = the new buff @470; a spawn that failed ends here (@554). Then
/// InstigatorRef = self @564, the hit sound @647 and the loop's stop event @725.
// Deviation: the hit sound is posted at the shot (the build: at the emitter's scale vector) and the loop is stopped
// by its key.
fn hit_enemy(w: &mut World, pr: ProjectileId, vic: FighterId) {
    let Some(p) = w.actors.projectiles.get(pr) else { return };
    let (owner, fighter, x, y) = (p.owner, p.owner_id, p.x, p.y);
    let b = spawn_buff(w, vic, HELD, 0.0, Some(0), owner, true, None);
    if let Some(p) = w.actors.projectiles.get_mut(pr) {
        p.hold_buff = b.map(|b| (vic, b));
    }
    let Some(b) = b else { return };
    if let Some(e) = ball(w, b) {
        e.shot = Some(ActorRef::Projectile(pr));
    }
    w.events.push(Event::Sound {
        fighter,
        ev: name!("sfx_cha_C017_weaponmode_plasmaattack_normal_missle_hit"),
        x: Some(x),
        y: Some(y),
        hit: None,
        pooled: false,
        key: None,
    });
    let key = loop_key(w, pr);
    w.events.push(Event::SoundStop { fighter, key, fade: Some(0.0), ev: None, curve: None, cut: false });
}

/// ReceiveEndPlay @1091: HoldBuff valid and not being destroyed -> its K2_DestroyActor @1206 (not in the Lab reset's
/// pass: an old pawn there).
fn shot_end(w: &mut World, pr: ProjectileId) {
    let Some(p) = w.actors.projectiles.get_mut(pr) else { return };
    let h = p.hold_buff.take();
    if let Some((vic, b)) = h
        && !crate::world::flow::reset_pass(w)
    {
        end_buff(w, vic, b);
    }
}

/// This file's registry entries (C017_Plasma_Shot_C's projectile kind, Buff_HeldByPlasmaBall_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        bounce_impact: Some(bounce_impact),
        step: Some(step),
        hit_enemy: Some(hit_enemy),
        end: Some(shot_end),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "C017_Plasma_Shot_C",
        ActorEntry::new(
            crate::actors::ActorClass::Projectile(crate::actors::ProjectileClass::C017PlasmaShot),
            ActorHooks::Projectile(kind),
        ),
    )?;
    let mut held = BuffEntry::new(
        BuffClass::BuffHeldByPlasmaBall,
        BuffHooks { begin: Some(begin), tick: Some(tick), on: Some(on), end: Some(end), ..BuffHooks::default() },
    );
    held.damage_hit_by_ally = true;
    r.buffs.enter(HELD_NAME, held)
}

const HELD_NAME: &str = "Buff_HeldByPlasmaBall_C";
