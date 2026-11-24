//! The lasso's perks: Buff_Perk_WonderWomanLassoGrapple and
//! Buff_Perk_WonderWoman_LassoTipper (marker buffs, entered empty), and the lasso's perk variables and hits
//! ([`LassoPerkState`], made at the lasso's BeginPlay with a lasso perk: [`lasso_perk_begin`], [`lasso_bounce`],
//! [`lasso_perk_tick`], the grapple's server side and its box, the tipper's hits). actors.rs's lasso calls them.
//!
//! The two buff classes do nothing of their own: LassoGrapple has no graph; LassoTipper's ReceiveBeginPlay only calls
//! BuffParentActor's (@15) and its custom event 'Lasso Return' is empty (@10 -> @35). Its HitBoxInfo /
//! ShieldedHitBoxInfo and its own components are read by no graph: the lasso deals its own LassoTipperDamage /
//! ShieldedLassoTipperDamage through its own hitbox handler. So the buffs are markers.
//!
//! What the lasso does with them (the perk state is made at its BeginPlay only with one of the two buffs on its
//! instigator):
//! - BeginPlay @6167-6515: the instigator's BPI_CheckIfHasBuff(LassoGrapple) -> LassoGrappleActive = true,
//!   Capsule.SetCollisionResponseToChannel(20 Platform, 2 Block); @6620-6907: LassoTipperActive.
//! - OnProjectileBounce @10316: a hit that is neither a BP_FighterCharacter nor a SpawnedActorParent ->
//!   Delay(0.0033) (@10802 -> @54): with the grapple buff the return sequence behind StartReturn's do-once, then
//!   LassoGrappleServer.
//! - StartReturn (@10267 -> @351): every return calls LassoGrappleServer after the return sequence.
//! - LassoGrappleServer @22936: LassoGrappleActive = false; with the buff a do-once -> SpawnBuffActor(
//!   Buff_MoveToPosition_LassoGrapple) @1988, MulticastLassoGrappleEffects @2147 and MulticastBeginMove(the lasso's
//!   location, false, false, 0.25) @2189: Wonder Woman is pulled to where the lasso turned back, in 0.25 s.
//! - HitSphere's enemy overlap @16805: with LassoGrapple ActivateGrappleDamageBox @17106 and nothing else; else,
//!   behind LassoOnly1Enemy, also !LassoTipperBuffer @17198 and the target not in TipperSet @17213.
//! - ActivateLassoTipper @22396 (StartReturn's Delay(0.033) resume @858, ahead of ReturnMovementServer).

use super::actors::{lasso, lasso_mut};
use crate::Real;
use crate::State;
use crate::actors::projectile::{CollisionResponse, set_platform_response, start_return};
use crate::actors::{ActorRef, ProjectileId};
use crate::combat::buffs::buff_of;
use crate::combat::hits::{actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::{BuffClass, HitPacket, OnHitBuffRow};
use crate::events::Event;
use crate::fighter::common::latent_frames;
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::end_move_buff;
use crate::fighter::moves::{V2, force_move_to_point};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, invulnerable};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::props::obj;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use mvs_data::value::{Props, Value};

const GRAPPLE: Name = name!("Buff_Perk_WonderWomanLassoGrapple_C");
const TIPPER: Name = name!("Buff_Perk_WonderWoman_LassoTipper_C");
const NULLIFY: Name = name!("Buff_NullifyNextHit_C");
const LIGHTNING_GRAB: Name = name!("PS_WW_LassoPerk_LightningGrab");

/// The lasso's perk variables (`Lasso.perk`; absent when neither buff was on the instigator at its BeginPlay).
#[derive(State, Debug, Default)]
pub struct LassoPerkState {
    /// LassoTipperActive (BeginPlay @6907).
    pub tipper: bool,
    /// LassoGrappleActive (BeginPlay @6464; LassoGrappleServer @22936 clears it).
    pub grapple: bool,
    /// LassoGrappleServer's do-once (@1875-1926) closed.
    pub grappled: bool,
    /// OnProjectileBounce's Delay(0.0033) @10802: the age it resumes at (-1: none pending).
    pub bounce_at: i32,
    /// LassoTipperBuffer.
    pub buffer: bool,
    /// ActivateLassoTipper's Delay(0.2) @21595 -> @1497 LassoTipperBuffer = false: its age (-1: none).
    pub buffer_end: i32,
    /// TipperSet: the ids of the fighters a tipper hit landed on (never cleared).
    pub tipped: Vec<i32>,
}

/// BPI_CheckIfHasBuff / Comp_State_Manager CheckHasBuff: a live buff of the class.
fn has(w: &World, f: FighterId, cls: Name) -> bool {
    buff_of(w, f, cls).is_some()
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    w.actors.projectiles.get(id).and_then(|p| p.owner).expect("the lasso's owner")
}

/// ReceiveBeginPlay's perk branches (@6167-6515 grapple, @6620-6907 tipper), from the kind's `begin`.
// Deviation: the bounce's resume (@54) asks for the grapple buff again in the build; here a lasso without perk state
// skips the Delay, so a buff that comes during the lasso's flight is not seen there (the perk buffs are given at the
// match's start and never taken away).
pub fn lasso_perk_begin(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let grapple = has(w, f, GRAPPLE);
    let tipper = has(w, f, TIPPER);
    if !grapple && !tipper {
        return;
    }
    lasso_mut(w, id).perk =
        Some(LassoPerkState { tipper, grapple, bounce_at: -1, buffer_end: -1, ..Default::default() });
    if grapple {
        let p = w.actors.projectiles.get_mut(id).expect("the lasso");
        set_platform_response(p, CollisionResponse::Block);
    }
}

/// OnProjectileBounce @10316, inside the move (the kind's `bounce_impact`). The lasso's root Capsule ignores Pawn, so
/// no bounce of the port's is off a fighter.
// Unverified: a bounce off a SpawnedPlatformParent's box (a shield actor's, which the Platform response now blocks) is
// a SpawnedActorParent hit in the build (no Delay); the port's bounce does not say what it met and takes it as the
// stage.
// A second bounce while the Delay is pending does not restart it (a latent Delay node already counting ignores it).
pub fn lasso_bounce(w: &mut World, id: ProjectileId) {
    let age = w.actors.projectiles.get(id).map_or(0, |p| p.age);
    let Some(s) = lasso_mut(w, id).perk.as_mut() else { return };
    if s.bounce_at >= 0 {
        return;
    }
    s.bounce_at = age + to_int32(latent_frames(0.0033));
}

/// The lasso's latent actions and calls of the tick (the kind's `moved`, behind the move and the tether family's
/// tick: the travel Delay's StartReturn, the blast box's, and the return's Delay(0.033)). Runs ahead of the sphere's
/// overlaps of the tick: ActivateLassoTipper runs ahead of ReturnMovementServer (@858).
pub fn lasso_perk_tick(w: &mut World, id: ProjectileId) {
    if lasso(w, id).perk.is_none() {
        return;
    }
    let age = w.actors.projectiles.get(id).map_or(0, |p| p.age);
    let f = owner(w, id);
    let resume = {
        let s = lasso_mut(w, id).perk.as_mut().expect("perk");
        if s.bounce_at >= 0 && age >= s.bounce_at {
            s.bounce_at = -1;
            true
        } else {
            false
        }
    };
    if resume && has(w, f, GRAPPLE) {
        start_return(w.actors.projectiles.get_mut(id).expect("the lasso")); // @54 -> @351 (the do-once inside)
    }
    // StartReturn's second branch (@498): LassoGrappleServer, behind every StartReturn and every grapple bounce.
    // Unverified: one call per tick in which the return has started is taken for each of those calls (the do-once
    // makes the repeats do nothing but LassoGrappleActive = false, which nothing reads after the return).
    let started = w.actors.projectiles.get(id).is_some_and(|p| p.return_started);
    let grappled = lasso(w, id).perk.as_ref().is_some_and(|s| s.grappled);
    if started && !grappled {
        grapple_server(w, id);
    }
    {
        let s = lasso_mut(w, id).perk.as_mut().expect("perk");
        if s.buffer_end >= 0 && age >= s.buffer_end {
            s.buffer_end = -1;
            s.buffer = false;
        }
    }
    if w.actors.projectiles.get(id).is_some_and(|p| p.tipper_due) {
        tipper(w, id);
    }
}

/// LassoGrappleServer @22936 -> @1875-2241.
// Deviation: the buff is not put in the buff list (its state group Move and BreakGroups 13, 12 are not applied); an
// older Buff_MoveToPosition of another class is ended first (CanOverride's rule across classes was not read).
// Unverified: the Delay(0.25) counts the tick of the call, as Arya's knife's (the lasso's tick runs behind its
// owner's): the move is removed after 14 of its 15 ticks, so she stops one tick short of the lasso.
fn grapple_server(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    if let Some(s) = lasso_mut(w, id).perk.as_mut() {
        s.grapple = false;
    }
    if !has(w, f, GRAPPLE) {
        return;
    }
    if let Some(s) = lasso_mut(w, id).perk.as_mut() {
        s.grappled = true;
    }
    // SpawnBuffActor's cast fails with no pawn to put it on (waiting for the respawn): no effects, no move.
    if w.fighter(f).life.dead > 0.0 {
        return;
    }
    let (cls, px, py) = {
        let p = w.actors.projectiles.get(id).expect("the lasso");
        (p.cls, p.x, p.y)
    };
    // MulticastLassoGrappleEffects: the emitter at the lasso, rotation (P -10, Y 90, R 0), scale (0.5, 0.9, 0.9).
    let fid = w.fighter(f).id;
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(LIGHTNING_GRAB),
        x: px,
        y: py,
        ox: 0.0,
        rot: [-10.0, 90.0, 0.0],
        scale: 0.9,
        tint: None,
        to: None,
    });
    // MulticastBeginMove(the lasso's location, no ease, 0.25 s): BeginMove's destination with X 0 (the plane), finish
    // SetVelocity with EndingLaunchVector (0, 0, 100).
    let t = 0.25;
    end_move_buff(w, f);
    let mid = force_move_to_point(w, f, px, py, t, 1.0, 0.0, 100.0, 0.0, false);
    w.fighter_mut(f).team.ally.move_id = to_int32(mid);
    w.fighter_mut(f).team.ally.buff = max(latent_frames(t) - 1.0, 1.0);
}

/// SphereOverlapActors(the lasso's location, r, [Pawn], BP_FighterCharacter): the fighters whose root capsule the
/// sphere overlaps.
// Deviation: the capsule is tested in the plane, the results come in the sim's fighter order, and a fighter waiting
// for its respawn or out of the hits (noHit) is left out, and so is an opponent the port's own Dodgeball rule spares
// what `by` deals.
fn sphere_fighters(w: &World, by: FighterId, x: Real, y: Real, r: Real) -> Vec<FighterId> {
    let mut out = Vec::new();
    for vic in match_fighters(w) {
        if w.fighter(vic).life.dead > 0.0 || crate::combat::phases::no_hit_by(w, by, vic) {
            continue;
        }
        let cr = cap_radius(w, vic);
        let h = capsule_half(w, vic) - cr;
        let (vx, vy) = (w.fighter(vic).body.x, actor_y(w, vic));
        let dx = x - vx;
        let dy = y - max(vy - h, min(y, vy + h));
        if (dx * dx + dy * dy).sqrt() <= r + cr {
            out.push(vic);
        }
    }
    out
}

/// One pfgDoDamage(null hitbox, the target, the packet, ...) of the lasso's own handler, with the gates lasso_overlap
/// has: projectile blocks, the invulnerable dodge, the handler's hit-once record (HitEachActorOnlyOnce @5290). A team
/// mate (not the owner) takes NotifyAllyOfHit @21344, which is no Hit; CanBeHitBy refuses the owner. True:
/// HitSuccessful.
fn deal(w: &mut World, id: ProjectileId, vic: FighterId, pk: &HitPacket, rows: &[OnHitBuffRow]) -> bool {
    let f = owner(w, id);
    if vic == f {
        return false;
    }
    let me = ActorRef::Projectile(id);
    let d = dealer(w, me);
    if projectile_blocks(w, &d, vic) {
        return false;
    }
    let ally = w.fighter(vic).team.team == w.fighter(f).team.team;
    if invulnerable(w, vic) {
        if !ally {
            let tick = w.flow.tick;
            let attack_id = w.actors.projectiles.get(id).map_or(0, |p| p.attack_id);
            attack_dodged(w, me, vic, attack_id, pk.base_knock_back.unwrap_or(0.0), tick, None, false);
        }
        return false;
    }
    dodge_over(w, me, vic);
    let vid = w.fighter(vic).id;
    if w.actors.projectiles.get(id).is_some_and(|p| p.hit_set.contains(&vid)) {
        return false;
    }
    if pk.hit_each_actor_only_once.unwrap_or(false) {
        w.actors.projectiles.get_mut(id).expect("the lasso").hit_set.insert(vid);
    }
    if ally {
        ally_hit(w, &d, vic, pk);
        return false;
    }
    let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
    let pct = crate::combat::elem::charge_pct(w, me);
    let ok = apply_projectile_hit(w, &d, vic, pk, rows, at, None, &[], pct);
    if ok {
        actor_hit_enemy(w, &d, vic);
    }
    ok
}

/// ActivateLassoTipper @22396-22876 -> @20858-21649: with LassoTipperActive, both packets' AttackID = the actor's
/// AttackID; the owner's BPI_CheckIfHasBuff(Buff_NullifyNextHit) (a bracer shield up) picks
/// ShieldedLassoTipperDamage (10) over LassoTipperDamage (8); SphereOverlapActors(175) and pfgDoDamage to each
/// result; LassoTipperBuffer |= HitSuccessful, a hit fighter into TipperSet; then Delay(0.2) ->
/// LassoTipperBuffer = false. The packets have HitOnlyOneFighter: the handler's HasHitAFighter refuses every call
/// after the first hit, so the loop's later results take nothing.
fn tipper(w: &mut World, id: ProjectileId) {
    if !lasso(w, id).perk.as_ref().is_some_and(|s| s.tipper) {
        return;
    }
    let f = owner(w, id);
    let key = if has(w, f, NULLIFY) { "ShieldedLassoTipperDamage" } else { "LassoTipperDamage" };
    let (px, py, attack_id, age) = {
        let p = w.actors.projectiles.get(id).expect("the lasso");
        (p.x, p.y, p.attack_id, p.age)
    };
    let mut pk = {
        let p = w.actors.projectiles.get(id).expect("the lasso");
        let base = obj(crate::actors::projectile::class_p(w, p), key).cloned().unwrap_or_default();
        HitPacket::from_props(&w.names, &base, None)
    };
    pk.attack_id = Some(attack_id);
    for vic in sphere_fighters(w, f, px, py, 175.0) {
        if !deal(w, id, vic, &pk, &[]) {
            continue;
        }
        let vid = w.fighter(vic).id;
        let s = lasso_mut(w, id).perk.as_mut().expect("perk");
        s.buffer = true;
        if !s.tipped.contains(&vid) {
            s.tipped.push(vid);
        }
        break;
    }
    let s = lasso_mut(w, id).perk.as_mut().expect("perk");
    s.buffer_end = age + to_int32(latent_frames(0.2));
}

/// HitSphere's enemy overlap behind LassoOnly1Enemy (@17198-17287): no hit while LassoTipperBuffer and none on a
/// fighter in TipperSet.
pub fn lasso_tipper_spares(w: &World, id: ProjectileId, o: FighterId) -> bool {
    let oid = w.fighter(o).id;
    lasso(w, id).perk.as_ref().is_some_and(|s| s.buffer || s.tipped.contains(&oid))
}

/// The enemy overlap's grapple branch (@16904-17120): the instigator has LassoGrapple.
pub fn lasso_grapple_on(w: &World, id: ProjectileId) -> bool {
    has(w, owner(w, id), GRAPPLE)
}

fn n(v: Real) -> Value {
    Value::Num(f64::from(v))
}

fn xy(x: Real, y: Real) -> Value {
    Value::Obj([("X".to_owned(), n(x)), ("Y".to_owned(), n(y))].into_iter().collect())
}

/// The packet ActivateGrappleDamageBox builds (@2242-3850).
fn grapple_hit(w: &World) -> HitPacket {
    let mut h = Props::new();
    let mut put = |k: &str, v: Value| {
        h.insert(k.to_owned(), v);
    };
    let b = Value::Bool;
    let s = |v: &str| Value::Str(v.to_owned());
    put("Damage", n(3.0));
    put("BaseKnockBack", n(600.0));
    put("KnockBackScaling", n(3.0));
    put("KnockBackDirectionVector", xy(0.25, 0.5));
    put("AttackTag", s("None"));
    put("HitReaction", n(4.0));
    put("HitPauseTime", n(0.0));
    put("HitStunMult", n(1.0));
    put("OnHitAudio", Value::Null);
    put("OnHitEffectOverride", Value::Null);
    put("DirectionalKnockback", b(false));
    put("VelocityInsteadOfFacing", b(false));
    put("KnockBackMirror", b(false));
    put("IgnoreFacing", b(false));
    put("AttackAnimation", Value::Null);
    put("AttackID", n(0.0));
    put("DoLOSCheck", b(false));
    put("ForceKnockback", b(false));
    put("QueueHitPauseUntilPoses", b(false));
    put("HitOnlyOneFighter", b(false));
    put("ArmorPierce", b(false));
    put("IgnoreActor", Value::Null);
    put("DestroyProjectiles", b(false));
    put("ReflectProjectiles", b(false));
    put("VacuumKnockback", b(false));
    put("DirectKnockbackTowardsSocket", b(false));
    put("KnockbackTargetSocket", s("None"));
    put("KnockbackSocketOffset", xy(0.0, 0.0));
    put("ControllerRumble", s("MediumForceFeedbackRumbleEffect"));
    put("PlayOnHitEffects", b(true));
    put("ApplyHitPauseToDamageDealer", b(true));
    put("AttackDecayType", n(0.0));
    put("AttackDecayName", s("None"));
    put("AdditiveKBMode", s("AdditiveKBMode::NewEnumerator0"));
    put("KBScalingStartingDmg", n(0.0));
    put("HitEachActorOnlyOnce", b(true));
    put("AccountForVelocityWithVacuumKnockback", b(true));
    put("IgnoreKBI", b(false));
    put("KBI_Multiplier", n(1.0));
    let mut vi = Props::new();
    for (k, v) in [
        ("UseVacuumKnockBack", b(false)),
        ("VacuumStrengthAlpha", n(0.0)),
        ("VacuumTowardsInstigatingFighter", b(false)),
        ("VacuumTargetSocket", s("None")),
        ("VacuumSocketOffset", xy(0.0, 0.0)),
        ("VacuumDuration", n(0.0)),
        ("VacuumPause", n(0.0)),
        ("ForceSmallHitEffects", b(false)),
        ("AccountForAttackerVelocity", b(true)),
    ] {
        vi.insert(k.to_owned(), v);
    }
    h.insert("VacuumInfo".to_owned(), Value::Obj(vi));
    HitPacket::from_props(&w.names, &h, None)
}

/// ActivateGrappleDamageBox @23390 -> @3855-3901 -> @2242-3850: SphereOverlapActors(the lasso's location, 150) and
/// for each result pfgDoDamage(null, it, the packet built in the graph, null, no elementals, the class's 'On Hit
/// Buffs Param' (Buff_Low_Damage_Electric on a hit enemy, 0 stacks, 5 s)) @3436, then PF_Post_Event_At_Location(
/// sfx_cha_wonderwoman_hit_bracer_01, the lasso's location) @3568 for every result, hit or not (the owner and her
/// team too: their call fails CanBeHitBy). No team, invulnerability or authority filter of its own.
pub fn lasso_grapple_box(w: &mut World, id: ProjectileId) {
    let (px, py) = {
        let p = w.actors.projectiles.get(id).expect("the lasso");
        (p.x, p.y)
    };
    let rows = {
        let p = w.actors.projectiles.get(id).expect("the lasso");
        let v = obj(crate::actors::projectile::class_p(w, p), "On Hit Buffs Param").and_then(|o| o.get("OnHitBuffs"));
        OnHitBuffRow::rows(&w.names, v)
    };
    let pk = grapple_hit(w);
    let f = owner(w, id);
    let fid = w.fighter(f).id;
    for vic in sphere_fighters(w, f, px, py, 150.0) {
        deal(w, id, vic, &pk, &rows);
        w.events.push(Event::Sound {
            fighter: fid,
            ev: name!("sfx_cha_wonderwoman_hit_bracer_01"),
            x: Some(px),
            y: Some(py),
            hit: None,
            pooled: false,
            key: None,
        });
    }
}

/// The view's question: LassoTipperActive, read for BeginPlay's SetTemplate.
pub fn lasso_tipper_active(w: &World, id: ProjectileId) -> bool {
    lasso(w, id).perk.as_ref().is_some_and(|s| s.tipper)
}

/// This file's registry entries (the two perk buffs: markers).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    for c in [BuffClass::BuffPerkWonderWomanLassoGrapple, BuffClass::BuffPerkWonderWomanLassoTipper] {
        let n = c.registered_name().expect("a registered class");
        r.buffs.enter(n, BuffEntry::new(c, BuffHooks::default()))?;
    }
    r.buff_graphs_ported.enter("Buff_MoveToPosition_LassoGrapple_C", ())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::projectile::ProjectileExt;
    use crate::actors::{Projectile, ProjectileClass};
    use crate::chars::wonder_woman::actors::Lasso;
    use crate::combat::buffs::spawn_buff;
    use crate::setup::{MatchPlayer, MatchSetup};
    use std::sync::Arc;

    /// A Wonder Woman (player 0) and a Shaggy (player 1) with a lasso of hers in the list at the enemy's place, or
    /// `None` without a dev store (the tests say so and pass).
    fn world() -> Option<(World, FighterId, FighterId, ProjectileId)> {
        let dir = mvs_store::dev_store_dir();
        let Ok(store) = mvs_store::Store::open(&dir) else {
            eprintln!("no store at {}: skipped", dir.display());
            return None;
        };
        let data = Arc::new(
            mvs_data::GameData::load(&store, &["wonderwoman", "shaggy"], "trainingroomsmall")
                .expect("the gamedata loads"),
        );
        let p = |c: &str| MatchPlayer { character: Some(c.into()), ..MatchPlayer::default() };
        let setup = MatchSetup { players: vec![p("wonderwoman"), p("shaggy")], ..MatchSetup::default() };
        let mut w = World::new(data, 21, &setup).expect("binds");
        let (f, vic) = (w.players[0], w.players[1]);
        let mut lasso = Projectile::new(
            name!("LassoProjectileFastV3_C"),
            ProjectileClass::LassoProjectileFastV3,
            Some(f),
            w.fighter(f).id,
            5,
        );
        lasso.ext = ProjectileExt::Lasso(Lasso::default());
        lasso.x = w.fighter(vic).body.x;
        lasso.y = actor_y(&w, vic);
        let id = w.actors.projectiles.insert(lasso);
        w.fighter_mut(f).items.spawned_actors.push(ActorRef::Projectile(id));
        Some((w, f, vic, id))
    }

    #[test]
    fn a_grapple_bounce_returns_the_lasso_and_pulls_her_to_it() {
        let Some((mut w, f, _, id)) = world() else { return };
        assert!(spawn_buff(&mut w, f, GRAPPLE, 1000.0, Some(0), Some(f), true, None).is_some());
        lasso_perk_begin(&mut w, id);
        assert!(lasso(&w, id).perk.as_ref().is_some_and(|s| s.grapple && !s.tipper));
        assert_eq!(w.actors.projectiles.get(id).unwrap().plat_response, CollisionResponse::Block);
        lasso_bounce(&mut w, id);
        let at = lasso(&w, id).perk.as_ref().unwrap().bounce_at;
        assert!(at > 0, "the Delay(0.0033) is pending: {at}");
        lasso_bounce(&mut w, id);
        assert_eq!(lasso(&w, id).perk.as_ref().unwrap().bounce_at, at, "a second bounce does not restart it");
        w.actors.projectiles.get_mut(id).unwrap().age = at;
        lasso_perk_tick(&mut w, id);
        let p = w.actors.projectiles.get(id).unwrap();
        assert!(p.return_started && !p.can_control, "StartReturn's sequence ran");
        assert!(lasso(&w, id).perk.as_ref().unwrap().grappled, "LassoGrappleServer's do-once closed");
        assert_ne!(w.fighter(f).team.ally.move_id, 0, "the move to the lasso began");
        assert!(
            w.events.iter().any(|e| matches!(e, Event::ProjectileDeath { vfx: Some(v), .. } if *v == LIGHTNING_GRAB))
        );
    }

    #[test]
    fn a_lasso_without_a_perk_buff_takes_none_of_the_branches() {
        let Some((mut w, f, _, id)) = world() else { return };
        lasso_perk_begin(&mut w, id);
        assert!(lasso(&w, id).perk.is_none());
        lasso_bounce(&mut w, id);
        lasso_perk_tick(&mut w, id);
        assert!(!lasso_grapple_on(&w, id));
        assert_eq!(w.fighter(f).team.ally.move_id, 0);
    }

    #[test]
    fn the_tipper_hits_once_and_spares_the_fighter_it_hit() {
        let Some((mut w, f, vic, id)) = world() else { return };
        assert!(spawn_buff(&mut w, f, TIPPER, 1000.0, Some(0), Some(f), true, None).is_some());
        lasso_perk_begin(&mut w, id);
        w.actors.projectiles.get_mut(id).unwrap().tipper_due = true;
        let before = w.fighter(vic).stats.damage;
        lasso_perk_tick(&mut w, id);
        let s = lasso(&w, id).perk.as_ref().unwrap();
        assert!(s.buffer && s.tipped == [w.fighter(vic).id], "the hit landed");
        assert!(s.buffer_end > 0);
        assert!(w.fighter(vic).stats.damage > before, "the packet dealt");
        assert!(lasso_tipper_spares(&w, id, vic));
    }

    #[test]
    fn the_grapple_box_hits_what_is_inside_150() {
        let Some((mut w, f, vic, id)) = world() else { return };
        assert!(spawn_buff(&mut w, f, GRAPPLE, 1000.0, Some(0), Some(f), true, None).is_some());
        lasso_perk_begin(&mut w, id);
        let before = w.fighter(vic).stats.damage;
        lasso_grapple_box(&mut w, id);
        assert!(w.fighter(vic).stats.damage > before, "the grapple packet dealt");
        assert!(
            w.events
                .iter()
                .any(|e| matches!(e, Event::Sound { ev, .. } if *ev == name!("sfx_cha_wonderwoman_hit_bracer_01")))
        );
    }
}
