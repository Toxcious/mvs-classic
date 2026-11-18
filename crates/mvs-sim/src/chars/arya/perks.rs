//! Arya's signature perks: Buff_Arya_EnrageAllies and Buff_Arya_Wounded (markers the knife
//! reads), Buff_Arya_Trophy (a disguise at the ring-out she caused).
//!
//! Buff_Arya_EnrageAllies: no graph of its own (a marker). AryaKnifeProjectile reads it: HitAlly gives the ally a
//! Buff_ScoobySnack (EnragePerkAllyStacks, 14 s) and refunds a quarter of the cooldown instead of half; the dash's
//! end to a knife on a team mate gives Arya one (EnragePerkSelfStacks). Both branches need a team mate: 2v2 only.
//! SpawnOnAllies false.
//! Buff_Arya_Wounded: no graph of its own (a marker; SpawnOnAllies true). The knife's attach to an enemy gives him a
//! Buff_FearStack (5.0 s, 1 stack), its end with a dash removes it. OnAttachedParentKnockedBack (Weakened at
//! WoundedHitsForStack) has no binder in this build: not ported.
//! Its DealtDamageMods / TakenDamageMods are DoNothing in the data, so the markers take no part in the passes.

use crate::Real;
use crate::combat::buffs::{BuffExt, buff, buff_mut};
use crate::combat::disguise::disguise_as;
use crate::combat::{BuffClass, BuffRef};
use crate::fighter::FighterId;
use crate::fighter::common::{latent_frames, peers};
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

// ---- Buff_Arya_Trophy (slug perk_arya_copycat)
// ReceiveBeginPlay @626 -> @541 (authority): Delay(1.0) @571, resuming at @15: GetPlayerCount > 1 @106 and the game
// mode a ParentGameMode @171 -> PlayerKnockedOut bound to OnFighterRungOut @246-269; Team Index = the owner's
// Comp_Actor_Destructible TeamIndex @491 (class default -1). It never unbinds: the buff goes at its owner's defeat
// (DestroyBuffWhenOwnerDefeated) with its binding, and the next life's buff waits its own second.
// OnFighterRungOut @641: the loser's player data @750, its pawn @965, the pawn's DamageManager MostRecentAttacker
// @1253 and that controller's pawn @1583 valid; the loser's team != Team Index and that pawn == the owner @2025 ->
// the owner's StateManager SpawnBuffActor(Buff_Disguise_C, owner, owner, 0, 0.0, true, false) @2274, the cast @2358,
// InitDisguiseServer(the loser's pawn) @2587 (`disguise_as` with override: no "already disguised" test,
// Buff_Disguise's CanOverride ends the live one).
// The pawn's own PlayerKnockedOut (Pawn_Arya, bound at its BeginPlay's Delay, ahead of this buff's) runs first:
// the World's ring-out sends the pawn hook ahead of the buff signal.
// Unverified: GetPlayerCount is taken as the match's fighters; the game mode is taken as a ParentGameMode child
// (every mode of the port). The Delay is counted from the buff's first tick, as the port's other buff BeginPlay
// Delays.

fn trophy_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _dur: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::AryaTrophy { delay: latent_frames(1.0), bound: false, team: -1 };
    }
}

fn trophy_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let many = peers(w).len() > 1;
    let team = w.fighter(f).team.team;
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::AryaTrophy { delay, bound, team: t } = &mut x.ext else { return false };
    if *delay <= 0.0 {
        return false;
    }
    *delay -= 1.0;
    if *delay > 0.0 || !many {
        return false;
    }
    *bound = true;
    *t = team;
    false
}

fn trophy_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let (BuffSignal::PlayerKnockedOut, Some(loser)) = (s, a.victim) else { return false };
    let Some(BuffExt::AryaTrophy { bound, team, .. }) = buff(w, b).map(|x| &x.ext) else { return false };
    let (bound, team) = (*bound, *team);
    let att = w.fighter(loser).attackers.most_recent_attacker;
    if !bound || att.is_none() {
        return false;
    }
    if w.fighter(loser).team.team == team || att != Some(f) {
        return false;
    }
    disguise_as(w, f, loser, true);
    false
}

/// This file's registry entries (the three perk buffs).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let trophy =
        BuffHooks { begin: Some(trophy_begin), tick: Some(trophy_tick), on: Some(trophy_on), ..BuffHooks::default() };
    let list = [
        BuffEntry::new(BuffClass::BuffAryaEnrageAllies, BuffHooks::default()),
        BuffEntry::new(BuffClass::BuffAryaWounded, BuffHooks::default()),
        BuffEntry::new(BuffClass::BuffAryaTrophy, trophy),
    ];
    for e in list {
        let n = e.class.registered_name().expect("a registered class");
        r.buffs.enter(n, e)?;
    }
    Ok(())
}
