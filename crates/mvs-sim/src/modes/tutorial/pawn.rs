//! The fighter writes and calls the tutorial's demos and minigames make on a pawn,
//! gathered so that no drill module reaches into the fighter's fields itself, and what every tutorial class shares
//! around them: the tagged actors' places, the pooled sound at a place, the bags' spawn and the input calls on a
//! bot-held pawn. Re-exports are called at their Rust homes, never copied here.
//!
//! The stage's actors found by Actor Tag (which only the tutorial reads) are here too.

use crate::Real;
use crate::ai::bt::BotController;
use crate::ai::pad::{Btn, Pad};
use crate::consts::MAX_EVADE_COUNT;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::input::Dir;
use crate::math::narrow;
use crate::names::Name;
use crate::world::World;
use crate::world::pawns::PawnCtl;
use mvs_data::stage::{StageData, StageStart, StageTagged};

// ---- the stage actors found by tag
// Both lookups walk the class hash, not the level (GetAllActorsWithTag 142a7dc30 over GetObjectsOfClass 1415d78d0):
// the actors of one class come in their load order, which the port takes as the levels' actor order (the data's).

/// `startsWithTag`: every PlayerStart with the Actor Tag `tag`, all matches (the tutorial's loops have no break).
/// Unverified: spawns before respawns. The build's order among PlayerStarts is their load order, which the data's
/// two lists do not keep across each other; a tag on starts of both lists (FreeForAll, the match maps') would
/// interleave them. The tutorial's tags are on spawns alone (Map_TrophyRoom_Tutorial has no Respawn start).
pub fn starts_with_tag<'a>(st: &'a StageData, tag: &str) -> Vec<&'a StageStart> {
    st.spawns.iter().chain(&st.respawns).filter(|s| s.tags.iter().any(|t| t == tag)).collect()
}

/// `actorsWithTag`: every tagged plain Actor with the Actor Tag `tag`, in the levels' actor order.
/// Unverified: the levels' export order stands for the class bucket's load order; a minigame that indexes the
/// AerialCombat or FireWall points reads this order.
pub fn actors_with_tag<'a>(st: &'a StageData, tag: &str) -> Vec<&'a StageTagged> {
    st.tagged.iter().flatten().filter(|a| a.tags.iter().any(|t| t == tag)).collect()
}

// ---- the fighter's writes

/// EvadeInvulnerabilityRechargeTime written (the dodge meter's refill period, `Folds::evade_recharge`).
/// Unverified: the regen reads it at every tick, so the write takes effect from the next one; the build's
/// BPI_ResetEvadeRegenTimer is not called by the write, and a later ApplyEvadeBuffs (a buff gained or lost) writes
/// the base x mult + flat over it, as in the build.
pub fn set_evade_recharge_time(w: &mut World, f: FighterId, v: Real) {
    w.fighter_mut(f).moves.folds.get_or_insert_with(Default::default).evade_recharge = Some(v);
}

/// BaseEvadeInvulnerability written (`Folds::evade_invuln`: the next evade's invulnerable time).
pub fn set_base_evade_invulnerability(w: &mut World, f: FighterId, v: Real) {
    w.fighter_mut(f).moves.folds.get_or_insert_with(Default::default).evade_invuln = Some(v);
}

/// MaxEvadeCount (the port's one value for every pawn).
pub fn max_evade_count(_w: &World, _f: FighterId) -> Real {
    MAX_EVADE_COUNT
}

/// EvadeCount written (the dodge meter's spent amount).
pub fn set_evade_count(w: &mut World, f: FighterId, v: Real) {
    w.fighter_mut(f).evade.evade_count = v;
}

/// CurrentDamage written.
pub fn set_current_damage(w: &mut World, f: FighterId, v: Real) {
    w.fighter_mut(f).stats.damage = v;
}

/// Comp_Pawn_AbilityManager DisableCD written.
pub fn set_disable_cd(w: &mut World, f: FighterId, on: bool) {
    w.fighter_mut(f).abilities.disable_cd = on;
}

/// SpawnInvulnerabilityTime written: read by SpawnInvulnerability alone (BaseFighter @136702-136791), which no
/// tutorial spawn runs (the tutorial's pawns get no spawn invulnerability), so the write changes nothing the port runs.
pub fn set_spawn_invulnerability_time(_w: &mut World, _f: FighterId, _v: Real) {}

/// `placeOfLoc`: a UE location (X, Y, Z) on the sim plane: x = -Y (0 - Y keeps +0), y = Z.
pub fn place_of_loc(loc: &[f64]) -> (Real, Real) {
    (narrow(0.0 - loc[1]), narrow(loc[2]))
}

/// `Tagged`: a tagged actor's transform (UE cm, rotation pitch / yaw / roll).
#[derive(Clone, PartialEq, Debug)]
pub struct Tagged {
    pub loc: Vec<f64>,
    pub rot: Vec<f64>,
}

/// `tagged`: GetAllActorsWithTag(tag)[i]: the tagged plain actors in the levels' order, then the PlayerStarts with
/// the tag (their rotation the yaw alone). `None` (warned) past the end, where the build reads a None actor.
pub fn tagged(w: &World, tag: &str, i: usize) -> Option<Tagged> {
    let st = &w.data.stage;
    let a = actors_with_tag(st, tag);
    let t = match a.get(i) {
        Some(a) => Some(Tagged { loc: a.loc.clone(), rot: a.rot.clone() }),
        None => {
            starts_with_tag(st, tag).get(i - a.len()).map(|s| Tagged { loc: s.loc.clone(), rot: vec![0.0, s.yaw, 0.0] })
        }
    };
    if t.is_none() {
        warn_text(w, &format!("tutorial: no actor tagged {tag}"));
    }
    t
}

/// Lib_Audio PF_Post_Event_At_Location(ev, location, no Instigator): the pooled post at that place.
pub fn sound_at(w: &mut World, ev: Name, x: Real, y: Real) {
    w.events.push(Event::Sound { fighter: -1, ev, x: Some(x), y: Some(y), hit: None, pooled: true, key: None });
}

/// `BagSpawn`: the bag a tutorial class spawns (the TutorialActor's demos, TutorialController_* SpawnPunchingBag(
/// SpawnTag, PlayerIndex, PawnClass)): a deferred pawn of the class at the first actor of GetAllActorsWithTag(tag),
/// FacingLeft true, the SpawnInvulnerabilityTime write where the class's graph makes one, PlayerDataReadyServer(slot),
/// Team = team (the player data's), CurrentDamage (OnRep_CurrentDamage), an Idle_AIController possessing it.
#[derive(Clone, Copy, Debug)]
pub struct BagSpawn<'a> {
    pub tag: &'a str,
    /// The fighter's data key.
    pub character: Name,
    pub slot: i32,
    pub team: i32,
    pub damage: Real,
    /// `None`: the graph writes none (TutorialController_Combat's).
    pub spawn_invulnerability_time: Option<Real>,
}

/// `spawnPunchingBag`.
pub fn spawn_punching_bag(w: &mut World, b: BagSpawn<'_>) -> Option<FighterId> {
    let t = tagged(w, b.tag, 0)?;
    let (x, y) = place_of_loc(&t.loc);
    let f = super::run::spawn_fighter(w, b.character, x, y, true, b.slot, b.team);
    if let Some(v) = b.spawn_invulnerability_time {
        set_spawn_invulnerability_time(w, f, v);
    }
    set_current_damage(w, f, b.damage);
    super::run::possess_bot(w, f, BotController::Idle);
    Some(f)
}

/// BPI_SetFacingLogic(FacingLeft) and the controller's SetRotation(FacingLeft): both reach SetRotationLogic
/// (BaseFighter @61066-61181, @105727): the pawn faces that side.
pub fn set_facing_logic(w: &mut World, f: FighterId, left: bool) {
    crate::fighter::set_facing(w, f, if left { -1.0 } else { 1.0 });
}

/// The controller's SetRotation(FacingLeft) (`setRotation`).
pub fn set_rotation(w: &mut World, f: FighterId, left: bool) {
    set_facing_logic(w, f, left);
}

/// K2_SetActorRotation((0, yaw, 0)): facing left for a yaw between 0 and 180 (yaw 90 left, -90 right, as the
/// actors' facing yaw).
/// Unverified: BPI_IsFacingLeft is taken to read the new yaw at once; a yaw of 0 or 180 leaves the facing.
pub fn set_actor_rotation(w: &mut World, f: FighterId, yaw: f64) {
    let a = ((yaw % 360.0) + 360.0) % 360.0;
    if a > 0.0 && a < 180.0 {
        crate::fighter::set_facing(w, f, -1.0);
    } else if a > 180.0 {
        crate::fighter::set_facing(w, f, 1.0);
    }
}

// ---- the input calls a tutorial class makes on a bot-held pawn (a demo or a minigame driving its bag, its Wonder
// Woman or the TutorialActor's): the bot's pad holds them to the bot's next think, the pawn's next frame. A pawn
// with no bot controller: nothing.
// Unverified: a call is taken to reach the pawn in its next frame, as the bot's own calls do; a MoveX of a task in
// that think comes after it and wins.

fn bot_pad(w: &mut World, f: FighterId) -> Option<&mut Pad> {
    match w.pawn_ctl.ctl.get_mut(&f) {
        Some(PawnCtl::Bot(b)) => Some(&mut b.bot.pad),
        _ => None,
    }
}

/// BPI_MoveX(v).
pub fn bot_move_x(w: &mut World, f: FighterId, v: Real) {
    if let Some(p) = bot_pad(w, f) {
        p.set_x(v);
    }
}

/// BPI_NormalAttackPressed(dir) / BPI_SpecialAttackPressed(dir), JumpPressed (local: BPI_JumpPressedLocal), the
/// dodge press.
pub fn bot_press(w: &mut World, f: FighterId, b: Btn, dir: Option<Dir>, local: bool) {
    if let Some(p) = bot_pad(w, f) {
        p.press(b, dir, local);
    }
}

/// BPI_NormalAttackPressed / BPI_SpecialAttackPressed with ForceAttackDirection / ForceSpecialDirection true: the
/// press forced to `dir`'s byte (EActionDirection: 2 Forward, 3 Back, passed as the call passes them).
pub fn bot_press_forced(w: &mut World, f: FighterId, b: Btn, dir: Dir) {
    if let Some(p) = bot_pad(w, f) {
        p.press_forced(b, dir as u8);
    }
}

/// BPI_NormalAttackReleased / BPI_SpecialAttackReleased and the other releases.
pub fn bot_release(w: &mut World, f: FighterId, b: Btn) {
    if let Some(p) = bot_pad(w, f) {
        p.release(b);
    }
}

/// BPI_DownPressed / BPI_DownReleased.
pub fn bot_down(w: &mut World, f: FighterId, on: bool) {
    if let Some(p) = bot_pad(w, f) {
        p.down = on;
    }
}
