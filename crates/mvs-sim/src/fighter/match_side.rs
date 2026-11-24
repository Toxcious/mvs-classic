//! The match's side of a fighter: its score and stats, its life (defeat, respawn, the minion and pawn marks), its
//! team, its VO, its items and spawner requests, its clock and its tutorial marks.
//!
//! The fields that point at the World's shared objects (`peers`, `stagePlatforms`, `comboReset`, `voShared`) are
//! the World's own fields here, read through it.

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, ItemId};
use crate::combat::ChargeVals;
use crate::combat::boxes::BoxSampler;
use crate::combat::buffs::BuffRef;
use crate::combat::{buffs, defense, disguise, elem, hit, hits, holds, perks};
use crate::consts as C;
use crate::events::NotifyRef;
use crate::events::{Event, Fid, MoveFxKind, SoundKey};
use crate::fighter::FighterId;
use crate::fighter::anim::{self, GroundState};
use crate::fighter::moves::V2;
use crate::fighter::traces;
use crate::fighter::{self, body, common, control, moves};
use crate::input::Btn;
use crate::input::Dir;
use crate::math;
use crate::math::ue;
use crate::names::Name;
use crate::world::World;
use crate::world::{flow, items};
use mvs_data::fighter::VoRow;
use mvs_data::tables::SkinAudio;
use mvs_data::value::{Props, Value};
use mvs_data::{CharData, GameData};
use std::cmp::Ordering;
use std::sync::{Arc, LazyLock};

/// The damage, the starting damage and the PFGPlayerState stats.
#[derive(State, Debug, Default)]
pub struct Stats {
    /// Comp_Actor_Damage's CurrentDamage (percent, float32). Every write is combat's (`combat::hit`, `combat::buffs`):
    /// the world's spawn and reset call `combat::hit::reset_damage`, a character's heal (Shaggy's) `applyHealing`.
    pub damage: Real,
    /// Comp_Actor_Damage.StartingDamage: the damage of a new pawn and of a respawn.
    pub starting_damage: Real,
    pub kos: i32,
    /// RingOuts (scored as the attacker).
    pub ring_outs: i32,
    /// DamageDealt (`recordDamage`).
    pub damage_dealt: Real,
    /// Assists (ring-outs scored as the assist attacker).
    pub assists: i32,
    /// OffPlatformRingouts.
    pub off_platform_ring_outs: i32,
    /// LastRingOutTime.
    pub last_ring_out_time: Real,
    /// TotalAssists.
    pub ring_outs_assisted: i32,
    /// TotalDoubleRingouts.
    pub double_ring_outs: i32,
    /// TotalAttacksDodged.
    pub attacks_dodged: i32,
    /// `tempHealth`.
    pub temp_health: Vec<TempHealth>,
}

/// A `tempHealth` entry.
#[derive(State, Debug, Default)]
pub struct TempHealth {
    pub kind: Name,
    pub amount: Real,
    pub time: Real,
    pub added: Real,
    pub fire: Real,
}

/// What `Fighter.onFfaDamage` holds (set once per fighter): the call a damage taken makes.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum FfaDamage {
    #[default]
    None,
    /// `(d) => MatchFlow.updateFFATeamDamage(w, f, d)` (`World.step`).
    UpdateTeamDamage,
}

/// Defeat, respawn and the pawn marks.
#[derive(State, Debug, Default)]
pub struct Life {
    /// `dead`: frames until respawn; `f64::INFINITY` once the fighter is out for good (no stock left, the match
    /// over). 0 while alive.
    pub dead: Real,
    pub abducted: bool,
    pub cam_focus: bool,
    /// `koHitstun`: HitstunTimer frames left at a ring-out.
    pub ko_hitstun: Real,
    pub cant_die: bool,
    pub blast_in: i32,
    pub blast_listed: bool,
    pub pre_spawn_delay: i32,
    pub outline_count: i32,
    pub on_ffa_damage: FfaDamage,
    pub is_minion: bool,
    pub not_player_character: bool,
    pub minion_creator: Option<FighterId>,
    /// `slot`: a mode pawn's bound player slot.
    pub slot: Option<i32>,
    /// `uncontrolled`.
    pub uncontrolled: bool,
}

/// `ally`: the team-up state.
#[derive(State, Debug, Default)]
pub struct Ally {
    pub no_ally: bool,
    pub reached: bool,
    pub cast: Real,
    pub shield: Real,
    pub launch: Real,
    pub reach: Real,
    pub vfx: Real,
    pub buff: Real,
    pub move_id: i32,
    /// `vfxBorn`: the tick DashedVFX was spawned in, -1 for none.
    pub vfx_born: Real,
    /// `safe`: Buff_SafeMoveToPosition's Destination and MovementSpeed while that child holds the move.
    pub safe: Option<AllySafe>,
}

/// `ally.safe`.
#[derive(State, Debug, Default)]
pub struct AllySafe {
    pub x: Real,
    pub y: Real,
    pub speed: Real,
}

/// The team.
#[derive(State, Debug, Default)]
pub struct Team {
    pub team: i32,
    pub bot_controlled: bool,
    pub ally: Ally,
    /// The world tick this fighter last joined its teammates' Comp_Actor_Destructible.Allies (`common::allies`): 0
    /// at the match's spawn, its respawn's tick after a knock-out.
    pub joined: i32,
}

/// A `voLines` entry.
#[derive(State, Debug, Default)]
pub struct VoLine {
    /// The line's key, `{id}:vo{n}`: its number `n` (`voSeq` at the post).
    pub key: i32,
    /// The world time (seconds) the line ends at.
    pub until: Real,
}

/// The fighter's Comp_Actor_VO.
#[derive(State, Debug, Default)]
pub struct Vo {
    pub vo_recent: Real,
    pub vo_speaking: bool,
    pub vo_lines: Vec<VoLine>,
    /// `voCurrent`: the key number of the last line posted.
    pub vo_current: Option<i32>,
    pub vo_seq: i32,
    /// VO_TagContainer: the tags SetAttackVOTags / SetHelpVOTags add (AddVOTag: Array_AddUnique, which compares
    /// without the case of ASCII letters), in their order.
    pub vo_tags: Vec<String>,
    /// SetAttackVOTags' RetriggerableDelay(1.0) @2739 -> @15: its TimeRemaining (float seconds), 0 or less none.
    pub vo_enemy_delay: Real,
    /// SetHelpVOTags' RetriggerableDelay(1.0) @2347 -> @186: its TimeRemaining (float seconds), 0 or less none.
    pub vo_friend_delay: Real,
}

/// `projAim`.
#[derive(State, Debug, Default)]
pub struct ProjAim {
    pub y: Real,
    pub z: Real,
    pub reset: Real,
    pub t: crate::Clock,
    pub at: crate::Clock,
}

/// `Aimer`: a Spawner notify's aim the throw reads.
#[derive(State, Debug)]
pub struct Aimer {
    /// `p`: the notify whose properties it aims with.
    pub n: crate::events::NotifyRef,
    pub attack_id: i32,
    pub sy: Real,
    pub sz: Real,
    pub ay: Real,
    pub az: Real,
    pub throw_offset: Option<crate::combat::DataVec>,
    pub fl: Option<bool>,
    pub pm: Option<Real>,
    /// AnimNotify_AimItemThrow's aimer: the AimInfo its begin builds (`{ AimInfo }` as the aimer's `p`) in place of
    /// the notify's own `AimInfo` member; `None` for a Spawner notify's aimer.
    pub info: Option<ThrowAimInfo>,
}

/// The AimInfo AnimNotify_AimItemThrow's begin gives its aimer: 'Aim Angle Max'
/// (45), 'Aim Angle Min' (-45), 'Aim Speed' (1000), 'Input Rotation Offset' (0); the rest of AimInfo absent.
#[derive(State, Copy, PartialEq, Debug)]
pub struct ThrowAimInfo {
    pub max: Real,
    pub min: Real,
    pub speed: Real,
    pub input_rotation: Real,
}

/// An `itemReleases` entry.
#[derive(State, Debug)]
pub struct ItemRelease {
    pub it: ItemId,
    pub dx: Real,
    pub dy: Real,
    pub x: Real,
    pub y: Real,
    pub vx: Real,
    pub vy: Real,
    pub attack_id: i32,
}

/// `itemReach`: a mode pawn's own Comp_ItemManager reach (a player's is the World's `reach`).
#[derive(State, Debug, Default)]
pub struct ItemReach {
    pub items: Vec<ActorRef>,
    pub closest: Option<ActorRef>,
}

/// The fighter's Comp_ItemManager and Comp_Spawner.
#[derive(State, Debug, Default)]
pub struct Items {
    pub aimer: Option<Aimer>,
    pub aimer_under: Option<Aimer>,
    pub proj_aim: ProjAim,
    pub item_reachable: bool,
    pub block_pick_up: bool,
    pub reach_recheck: bool,
    pub held_item: Option<ItemId>,
    pub item_holstered: bool,
    pub item_releases: Vec<ItemRelease>,
    pub pickup_request: bool,
    pub item_reach: Option<ItemReach>,
    /// `spawnedActors`: what the fighter's Comp_Spawner spawned and still lists.
    pub spawned_actors: Vec<ActorRef>,
}

/// A `spawnRequests` entry (`SpawnRequest`).
#[derive(State, Debug)]
pub struct SpawnRequest {
    pub cls: Name,
    /// The notify whose properties it spawns with.
    pub n: crate::events::NotifyRef,
    pub attack_id: i32,
    pub foreign: Foreign,
    /// `copy`: the spawn point read on a disguise's second mesh (`Some(None)`: no socket).
    pub copy: Option<Option<V2>>,
    pub charge: Option<ChargeVals>,
    /// What a ProjectileAimer's ActivateObject writes over the notify's properties (`activateAimer`'s `p`): `None`
    /// for a request that spawns with the notify's own.
    pub aim: Option<AimedSpawn>,
    /// `p` where the request carries literal properties (a buff's spawn, not a notify's): in place of the notify's.
    pub props: Option<Arc<Props>>,
    /// `instigator`: CompSpawnActor's AssignInstigator where it is not the owner.
    pub instigator: Option<FighterId>,
    /// `origin`: the place a request made outside a notify spawns at (a class's make hands it on as the constructor's
    /// origin); `None` for a notify's request.
    pub origin: Option<V2>,
}

/// ProjectileAimer ActivateObject's values over the stored notify's: AimOn false,
/// InitialMovementDirection (0, `y`, `z`), InitialSpeed and MaxSpeed.
#[derive(State, Copy, PartialEq, Debug)]
pub struct AimedSpawn {
    pub y: Real,
    pub z: Real,
    pub initial_speed: Real,
    pub max_speed: Real,
}

/// `SpawnRequest.foreign`.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum Foreign {
    #[default]
    None,
    Disguise,
    Burp,
}

/// `ActivateMode`.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum ActivateMode {
    #[default]
    All,
    FirstLast,
    Last,
    First,
}

/// An `activateRequests` entry.
#[derive(State, Debug)]
pub struct ActivateRequest {
    pub cls: Name,
    pub mode: ActivateMode,
    /// `conds`: ConditionNumber, ConditionValue.
    pub conds: Vec<(i32, bool)>,
    pub charge: Option<ChargeVals>,
    /// `only`: the one actor it activates.
    pub only: Option<ActorRef>,
}

/// A `despawnRequests` entry: a class's every actor, or its last and / or first.
#[derive(State, Debug)]
pub enum DespawnRequest {
    All(Name),
    Ends { cls: Name, last: bool, first: bool },
}

/// The spawner and buff requests the World serves after the fighters' ticks.
#[derive(State, Debug, Default)]
pub struct Requests {
    pub spawn_requests: Vec<SpawnRequest>,
    pub activate_requests: Vec<ActivateRequest>,
    pub despawn_requests: Vec<DespawnRequest>,
    /// `nearbyBuffRequests`: the ApplyBuffNearbyPlayer notifies.
    pub nearby_buff_requests: Vec<crate::events::NotifyRef>,
    /// `allyBuffRequests`.
    pub ally_buff_requests: Vec<crate::events::NotifyRef>,
}

/// The fighter's clock.
#[derive(State, Debug, Default)]
pub struct Clock {
    /// The frames this fighter has stepped.
    pub tick: i32,
    /// UWorld TimeSeconds (GetTimeSeconds): a float32 sum of DeltaSeconds from the level start; a Lab reset keeps
    /// it.
    pub time: Real,
    /// `prevTime`: the previous frame's world clock.
    pub prev_time: Real,
    pub creation_time: Real,
}

/// `TutorialEvent`: TutorialEventTypes by byte, the values of the enum asset's Names
/// map (not the NewEnumerator order). NeutralsAerialSpecialHit is the asset's own spelling.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum TutorialEvent {
    #[default]
    None = 0,
    NeutralNormalHit = 1,
    DownNormalHit = 2,
    UpNormalHit = 3,
    ForwardNormalHit = 4,
    NeutralSpecialHit = 5,
    DownSpecialHit = 6,
    UpSpecialHit = 7,
    ForwardSpecialHit = 8,
    NeutralAerialNormalHit = 9,
    DownAerialNormalHit = 10,
    UpAerialNormalHit = 11,
    ForwardAerialNormalHit = 12,
    NeutralsAerialSpecialHit = 13,
    DownAerialSpecialHit = 14,
    UpAerialSpecialHit = 15,
    ForwardAerialSpecialHit = 16,
    Jump = 17,
    HoldJump = 18,
    DoubleJump = 19,
    WallJump = 20,
    LedgeSlide = 21,
    RingOut = 22,
    FastFall = 23,
    ChargeUp = 24,
    ComboChain = 25,
    Equip = 26,
    Unequip = 27,
    CustomActorEvent = 28,
    NeutralNormalUse = 29,
    DownNormalUse = 30,
    UpNormalUse = 31,
    ForwardNormalUse = 32,
    NeutralSpecialUse = 33,
    DownSpecialUse = 34,
    UpSpecialUse = 35,
    ForwardSpecialUse = 36,
    NeutralAerialNormalUse = 37,
    DownAerialNormalUse = 38,
    UpAerialNormalUse = 39,
    ForwardAerialNormalUse = 40,
    NeutralAerialSpecialUse = 41,
    DownAerialSpecialUse = 42,
    UpAerialSpecialUse = 43,
    ForwardAerialSpecialUse = 44,
    MoveRight = 45,
    MoveLeft = 46,
    Crouch = 47,
    Up = 48,
    DropThroughPlatform = 49,
    EvadeThroughPlatform = 50,
    Evade = 51,
    EvadeGroundNeutral = 52,
    EvadeGroundRight = 53,
    EvadeGroundLeft = 54,
    EvadeAirNeutral = 55,
    EvadeAirUp = 56,
    EvadeAirUpRight = 57,
    EvadeAirRight = 58,
    EvadeAirDownRight = 59,
    EvadeAirDown = 60,
    EvadeAirDownLeft = 61,
    EvadeAirLeft = 62,
    EvadeAirUpLeft = 63,
    EvadeGroundToAirUp = 64,
    EvadeGroundToAirUpRight = 65,
    EvadeGroundToAirUpLeft = 66,
    EvadeAir = 67,
    ArmorBreak = 68,
    FastFallJump = 69,
    EvadeJump = 70,
    UsedUpAllAirMoves = 71,
    UniqueCharacterMechanic0 = 72,
    UniqueCharacterMechanic1 = 73,
    UniqueCharacterMechanic2 = 74,
    UniqueCharacterMechanic3 = 75,
    UniqueCharacterMechanic4 = 76,
    UniqueCharacterMechanic5 = 77,
    DodgeJumpCancel = 78,
    FastfallDuringAttack = 79,
    MaximumAttackDecay = 80,
    NoAttackDecay = 81,
    AnyAttackDecay = 82,
    DodgeSuccess = 83,
    DodgeMeterEmpty = 84,
    DodgeMeterFull = 85,
    KBILeft = 86,
    KBIRight = 87,
    KBIUp = 88,
    ElementalFireBuff = 89,
}

/// The tutorial's marks on a fighter.
#[derive(State, Debug, Default)]
pub struct TutorialFlags {
    pub last_attack_tutorial_event: Option<TutorialEvent>,
    pub active_in_tutorial_intro: bool,
    pub drop_tutorial_at: Option<i32>,
    /// DisabledInputTypes (BaseFighter): by an Enum_InputButton byte, the mask of its disabled
    /// Enum_InputDirection8Way bits; 0 is no entry (BPI_EnableInputTypeDirection removes an entry its last bit
    /// leaves). Only the Batman tutorial's minigames write it.
    pub disabled_input_types: [i32; 6],
    /// MoveDisabledCounter (BPI_DisableMovement).
    pub move_disabled: i32,
}

// ---- what the module's code shares

/// SpawnInvulnerabilityVisuals' sound.
const SFX_INVULNERABILITY: Name = crate::name!("sfx_shared_combat_invulnerability_alt");

/// A fighter's own sound keys, after its id (`{id}:wallslide`, `{id}:wind`, `{id}:vo{n}`): the trace writes a key
/// whose tag starts with `:` as the number and the tag.
const TAG_WALL_SLIDE: Name = crate::name!(":wallslide");
const TAG_WIND: Name = crate::name!(":wind");
/// A VO line's key: `{id}:vo{n}`, here the tag and the line's number `n` (the fighter is the event's).
const TAG_VO: Name = crate::name!(":vo");

static NO_PROPS: LazyLock<Props> = LazyLock::new(Props::default);

fn fid(w: &World, f: FighterId) -> Fid {
    w.fighter(f).id
}

fn push(w: &mut World, f: FighterId, e: Event) {
    w.fighter_mut(f).events.push(e);
}

/// A `{ type: 'sound', fighter, ev }` event (with `x`, `y` where it has them).
fn sound(w: &mut World, f: FighterId, ev: Name, at: Option<(Real, Real)>) {
    let fighter = fid(w, f);
    push(
        w,
        f,
        Event::Sound { fighter, ev, x: at.map(|a| a.0), y: at.map(|a| a.1), hit: None, pooled: false, key: None },
    );
}

fn sound_stop(w: &mut World, f: FighterId, key: SoundKey, fade: Real) {
    let fighter = fid(w, f);
    push(w, f, Event::SoundStop { fighter, key, fade: Some(fade), ev: None, curve: None, cut: false });
}

fn rtpc(w: &mut World, f: FighterId, name: Name, value: Real) {
    let fighter = fid(w, f);
    push(w, f, Event::Rtpc { fighter, name, value, interp_ms: 0.0 });
}

/// The fighter's own key `{id}{tag}`.
fn own_key(w: &World, f: FighterId, tag: Name) -> SoundKey {
    SoundKey { tag, n: fid(w, f) }
}

/// `p[k]` as a number.
fn num(p: &Props, k: &str) -> Option<Real> {
    p.get(k).and_then(Value::as_f64).map(crate::math::narrow)
}

/// JavaScript truthiness of `p[k]`.
fn flag(p: &Props, k: &str) -> bool {
    match p.get(k) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// A component's properties (`components.pawn[name]?.p ?? {}`).
fn pawn_comp<'a>(cd: &'a CharData, name: &str) -> Option<&'a Props> {
    cd.fighter.components.pawn.get(name).and_then(|c| c.p.as_obj())
}

/// The properties of notify `n` of the fighter's montages (an actor montage's when the fighter's own list has none
/// of that name); empty for a notify the data does not hold.
pub fn notify_props(w: &World, f: FighterId, n: NotifyRef) -> &Props {
    let d = &common::data_of(w, f).fighter;
    let m = w.names.str(n.montage);
    let notify = d
        .montages
        .get(m)
        .or_else(|| d.actor_montages.as_ref().and_then(|a| a.get(m)))
        .and_then(|m| m.notifies.get(n.index as usize))
        .or_else(|| crate::fighter::anim::montages::any_notify_in(&w.data, w, f, n));
    notify.map_or(&NO_PROPS, |x| &x.p)
}

/// JavaScript `a.toLowerCase() === b.toLowerCase()` without the copies.
fn lower_eq(a: &str, b: &str) -> bool {
    a.chars().flat_map(char::to_lowercase).eq(b.chars().flat_map(char::to_lowercase))
}

/// A JavaScript array index: `r` when it is a whole number from 0 (a lookup by any other number gives `undefined`).
fn index(r: Real) -> Option<usize> {
    if r.fract() != 0.0 || !(0.0..4294967295.0).contains(&r) {
        return None;
    }
    usize::try_from(math::to_int32(r).cast_unsigned()).ok()
}

/// The fighter of the peers whose id is `id`.
fn peer_by_id(w: &World, id: i32) -> Option<FighterId> {
    common::peers(w).iter().copied().find(|&o| w.fighter(o).id == id)
}

// ---- life

/// `pawnGone`: true inside `spawn`'s buff ends, the ends of a pawn that is gone.
pub fn pawn_gone(w: &World) -> bool {
    w.scratch.pawn_gone
}

/// `pawnDeath`: the pawn's own part of a death (`World.koFighter`), after BaseFighter's.
pub fn pawn_death(w: &mut World, f: FighterId) {
    if let Some(h) = w.fighter(f).hooks.char.death {
        h(w, f);
    }
}

/// The base definition's HoldingItemOverride pack (`f.nav.hold` of the class defaults' navigation set).
fn base_hold_pack(w: &World, f: FighterId) -> Vec<anim::HoldOverride> {
    let def = &common::char_data(w, w.fighter(f).base_ch).def;
    def.nav
        .hold
        .iter()
        .map(|h| anim::HoldOverride {
            air: h.air,
            dir: h.dir,
            montage: h.montage.as_deref().map_or(Name::NONE, |m| common::lit(w, m)),
        })
        .collect()
}

/// `spawn`.
pub fn spawn(w: &mut World, f: FighterId, x: Real, y: Real, facing: Real) {
    anim::stop_montage(w, f, None, None);
    // The deferred ends of the stopped montage are not dropped: a sound or VFX would be left on, a move command would
    // run on (its NotifyEnd's CompleteMoveCommand, ahead of the others as the dispatch has it: terminateFading).
    let move_ends = std::mem::take(&mut w.fighter_mut(f).branching.pending_move_ends);
    for idx in move_ends {
        moves::complete_remove_move(w, f, idx);
    }
    let late = std::mem::take(&mut w.fighter_mut(f).branching.pending_ends);
    for (n, idx) in late {
        anim::end(w, f, n, idx);
    }
    // The despawns those ends asked for are served here, before the requests are emptied below.
    // The build's AnimNotify_Spawner end destroys its actors in the call; the port queues them for the
    // World's request pass, which this fighter's requests would no longer reach. A request queued earlier in the frame
    // goes with them.
    crate::world::spawning::serve_despawns(w, f);
    {
        let fm = w.fighter_mut(f);
        fm.controls.recent_attacks.clear();
        fm.controls.attack_spam_times.clear();
        fm.controls.spam_delayed = None;
        fm.controls.rot_lock_until = -1.0;
        fm.montages.dash_montage = None;
    }
    anim::clear_fading(w, f);
    anim::clear_grouped(w, f);
    {
        let fm = w.fighter_mut(f);
        fm.body.x = x;
        fm.body.prev_x = x;
        fm.body.ak_x = x;
        fm.body.y = y;
        fm.body.prev_y = y;
        fm.body.ak_y = y;
        fm.body.vx = 0.0;
        fm.body.vy = 0.0;
    }
    fighter::set_facing(w, f, facing);
    let has_aimer = {
        let fm = w.fighter_mut(f);
        fm.requests = Requests::default();
        fm.moves.pull = None;
        fm.moves.force_move = None;
        fm.moves.force_finish = None;
        let a = &mut fm.team.ally;
        a.reached = false;
        a.shield = 0.0;
        a.launch = 0.0;
        a.reach = 0.0;
        a.vfx = 0.0;
        a.buff = 0.0;
        a.move_id = 0;
        a.vfx_born = -1.0;
        fm.items.aimer.is_some() || fm.items.aimer_under.is_some()
    };
    // The aimers go with the spawned actors (Comp_Spawner OnBaseFighterReset -> DestroyActorsOnEndPlay @9135); each
    // one's EndPlay calls ResetAim. ProjectileAimVector itself is left to the decay.
    if has_aimer {
        reset_aim(w, f);
    }
    {
        let fm = w.fighter_mut(f);
        fm.items.aimer = None;
        fm.items.aimer_under = None;
        fm.items.held_item = None;
        fm.items.item_holstered = false;
    }
    defense::reset_rage(w, f);
    // No stun pick at a respawn: ResetFighterLogic (BaseFighter @773 RemoveHitstunFromKnockBack, @809
    // RemoveHitStunFromBuff) ends the stun and reads no buff.
    // Deviation: the buffs still there (DestroyBuffWhenOwnerDefeated false, or put on after the defeat) end
    // here together without the pick an EndPlay makes among the others; the build's graph ends none at a respawn.
    w.scratch.pawn_gone = true;
    let mut i = 0;
    while let Some(b) = w.fighter(f).buffs.buffs.get(i) {
        i += 1;
        let (cls, serial) = (b.cls, b.serial);
        let r = BuffRef { fighter: f, serial };
        buffs::rescue_end(w, f, cls);
        buffs::fire_end(w, f, r);
        let live = w.fighter(f).buffs.buffs.iter().find(|b| b.serial == serial).is_some_and(|b| b.frames > 0.0);
        if live && let Some(end) = w.registry.buffs.get(w.names.str(cls)).and_then(|e| e.hooks.end) {
            end(w, f, r);
        }
    }
    // Buff_HeldByEnemy: the hold this fighter is in ends with its buffs; the holds it is the instigator of end at
    // its OnExitedArena (Ringed Out @7586), bound once their Delay has run. Unverified: OnExitedArena is taken
    // as the ring-out. The release check is left out for a pawn that is gone (the respawn places it).
    holds::hold_end(w, f, false);
    w.scratch.pawn_gone = false;
    let victims = w.fighter(f).holds.hold_victims.clone();
    for v in victims {
        if w.fighter(v).holds.hold.as_ref().is_some_and(|h| h.bound) {
            holds::end_hold_buff(w, v);
        }
    }
    {
        let fm = w.fighter_mut(f);
        fm.holds.hold_hits.clear();
        fm.buffs.buff_stunned = false;
        fm.buffs.grappled = false;
        // Dead is MOVE_None, and the respawn sets Falling (ResetFighterLogic @392).
        fm.moves.move_none = false;
        fm.buffs.buff_stun = 0.0;
        fm.buffs.struggle = None;
        // DestroyBuffWhenOwnerDefeated.
        fm.buffs.buffs.clear();
        fm.buffs.speed_mod = 1.0;
        fm.moves.speed_override = None;
    }
    buffs::set_size_scale(w, f, 1);
    w.fighter_mut(f).abilities.cooldown_multiplier = 1.0;
    // The packs leave through their owners' ends, each a Comp_Armor RemoveArmor -> HandleRemoveArmorPack ->
    // RemoveArmorVisibility (@1607): AnimNotify_Armor's NotifyEnd at the stopMontage above, Buff_ArmorPack's and the
    // other buffs' EndPlay in the loop above (DestroyBuffWhenOwnerDefeated), so ArmorVisibilityCount is back at 0
    // here as in the build. Comp_Armor has no reset of its own (its remove-all loop U@574-897 is
    // ArmorBreakMulticast's). Deviation: a pack whose owner the port has not ended by now (an actor's, a class not
    // ported) is taken out here through the same HandleRemoveArmorPack, so the count follows the packs; the build's
    // would stay.
    while let Some(kind) = w.fighter(f).armor.armor_packs.first().map(|p| p.kind) {
        defense::remove_armor(w, f, kind);
    }
    // RespawnCharacter sets the location with no floor read; the CMC then finds the floor under him: the highest
    // walking box under the start (Trophy 2's Player2 is over Floor2, a stage solid), World.ground when none of the
    // solids is.
    let under = body::floor_under_base(w, f, x, y);
    w.fighter_mut(f).platforms.base = under;
    let grounded = y <= fighter::stand_y(w, f) + 1.0;
    fighter::set_grounded(w, f, grounded);
    if grounded {
        let sy = fighter::stand_y(w, f);
        let b = &mut w.fighter_mut(f).body;
        b.y = sy;
        b.prev_y = sy;
        b.ak_y = sy;
    }
    let stunned = {
        let fm = w.fighter_mut(f);
        let p = &mut fm.platforms;
        p.dropping_down = false;
        p.evading_through_platform = false;
        p.down_press_count = 0;
        p.down_press_active = false;
        p.down_window_tick = -1;
        // Until the next pawn tick: a spawn point is in the air over the stage.
        p.plat_ignored = -1;
        std::mem::replace(&mut fm.attackers.stun_source, false)
    };
    // Deviation: the hitstun source of a fighter that dies in hitstun is taken to end here (the build's
    // HitstunTimer outlives the ring-out and ends the source when it fires).
    if stunned {
        hit::stun_changed(w, f, false);
    }
    // ResetFighterLogic @773 RemoveHitstunFromKnockBack reaches the hitstun end with no source left (HitstunHandler
    // @7427-7563 -> @1132), whose broadcast runs a knockback hit's HitStunEnded binding: unbound (@12013), its
    // StopAnimMontage(null) (@12054) taken by the stopMontage above.
    w.fighter_mut(f).attackers.hit_stun_ended_bound = false;
    // The same broadcast's pawn bindings (CharHooks.hitStunEnded).
    if let Some(h) = w.fighter(f).hooks.char.hit_stun_ended {
        h(w, f);
    }
    {
        let fm = w.fighter_mut(f);
        let h = &mut fm.hit;
        h.hitpause = 0.0;
        h.hitstun = 0.0;
        h.true_hitstun = 0.0;
        h.hitstun_paused = false;
        h.hitstun_pending = false;
        h.hitstun_rate = 0.0;
        h.hitstun_counted = 0.0;
        h.kb_mode = false;
        h.can_bounce_no_hitstun = false;
        h.free_launch = false;
        // The mesh at (0, 0, MeshStartZLocation). At a respawn it is the build's too, a timer tick later:
        // ResetFighterLogic @732 Comp_HitPause.SetHitPauseTime(0.0) is a hit pause of 0.01 s (Comp_HitPause ubergraph
        // @1283-1368: SelectFloat(Time, 0.01, Time > 0) into SetHitPauseTimeMulticast), whose timer ends in
        // OnHitPauseEnd @514 -> BPI_HitPauseEnd -> BFHitPauseEnd @143454, the write of X 0 (not when bIsCrouched). A
        // new pawn (resetMatchState) has its CDO's X instead. Deviation: that 0.01 s pause itself is not played at a
        // respawn.
        h.mesh_x = 0.0;
        h.mesh_z = 0.0;
        h.bounce_z_offset = 0.0;
        h.bouncing = false;
        // That pause's end broadcasts OnIsBouncing(0).
        h.anim_bounce = None;
        h.floor_bounce_hurt = false;
        h.launch = None;
        fm.react.bounce_pitch = 0.0;
        // Comp_Actor_MoveCommandManager is not reset (ResetFighterLogic calls nothing of it and it binds no reset): it
        // ticked through the death (duringPhysicsTick), and the end of a command the stop above completed (MoveEnd)
        // runs at its next tick (tickMoveCmds: the terminal velocity, the stop impulse, NetFriction, MaxAcceleration
        // 4096). SetMovementMode(Falling) @392 ends a flight.
        let m = &mut fm.moves;
        m.flying = false;
        m.fly_input = None;
        // The CMC's pending launch went with MOVE_None (PerformMovement's early exit, ClearAccumulatedForces); the
        // FMC's stored NetImpulse (+0xb64) and NetFriction stay for the first move after the respawn (nothing resets
        // them).
        m.impulse = None;
        m.hit_launch = false;
        m.ground_jump = None;
        let a = &mut fm.air;
        a.dash_tick = -1e9;
        a.last_direction = 0.0;
        a.move_in = 0.0;
        a.last_move_in = 0.0;
    }
    moves::reset_air(w, f, false, true);
    {
        let a = &mut w.fighter_mut(f).air;
        a.fall_lat_friction = 0.0;
        a.air_control_gate = true;
        a.dash_fall_gate = false;
        a.time_in_air = 0.0;
    }
    if grounded_now(w, f) {
        let idle = fighter::anim_set_def(w, f).idle.as_deref().map_or(Name::NONE, |s| common::lit(w, s));
        anim::set_loco(w, f, idle, 1.0, None, None);
    } else {
        let a = anim::fall_anim(w, f);
        let r = anim::fall_rate(w, f);
        anim::set_loco(w, f, a, r, None, None);
    }
    let g = grounded_now(w, f);
    let sm = &mut w.fighter_mut(f).sm;
    sm.gs = if g { GroundState::Idle } else { GroundState::Fall };
    sm.gs_time = 0.0;
    sm.sm_active.clear();
    sm.sm_times.clear();
    sm.gs_rel = 1.0;
}

fn grounded_now(w: &World, f: FighterId) -> bool {
    fighter::grounded(w, f)
}

/// `resetMatchState`: a fresh fighter (`World.reset` stands in for a new match).
pub fn reset_match_state(w: &mut World, f: FighterId) {
    // ReadyForIntro's AddInvisibility, which World.reset has written before this call.
    let intro = w.fighter(f).vis.invisible;
    // ... and its mesh at the CDO's RelativeLocation. Unverified: `ch.meshX` of the fighter's
    // size-scaled definition copy may be the source; the character's own definition is read (the two agree at size
    // scale 1).
    let mesh_x = common::data_of(w, f).def.mesh_x;
    let (cap_half, cap_radius) = {
        let d = &common::char_data(w, w.fighter(f).base_ch).def;
        (d.cap_half, d.cap_radius)
    };
    let hold_pack = base_hold_pack(w, f);
    {
        let fm = w.fighter_mut(f);
        fm.hit.in_hit_pause_pose = false;
        fm.hit.queued_hit_pause = 0.0;
        fm.air.air_attack_available = true;
        fm.air.air_attack_delay = 0.0;
        let e = &mut fm.evade;
        e.evade_count = 0.0;
        e.evade_invuln_time = C::EVADE_INVULN;
        e.evade_invuln = 0.0;
        e.last_evade_spent_meter = false;
        e.evade_regen_start = -1.0;
        e.evade_regen_once = false;
        e.evade_crouch_rate = false;
        // A reset stands for new pawns; the blast boxes' list held the old one ...
        fm.life.cant_die = false;
        // ... and a new pawn's capsule has its collision ...
        fm.capsule.capsule_off = false;
        // ... and CanWallBounce? at its class default (true) ...
        fm.hit.no_wall_bounce = false;
        // ... and the CMC's bCanWalkOffLedges at its template's (true) ...
        fm.platforms.no_walk_off_ledges = false;
        // ... and AirSpecialMax at its class default ...
        fm.air.air_special_max_set = None;
        fm.hit.mesh_x = crate::math::narrow(mesh_x);
        // ... and its ArmorShake timeline stopped ...
        fm.hit.armor_shake = None;
        // ... with the class defaults' navigation set (a character that swaps it sets its own in `matchReset`) and a
        // BranchManager that counts no active attack.
        fm.anim.nav = anim::SetRef::Base;
        fm.anim.hold_pack = hold_pack;
        fm.charge.attack_is_active = 0;
        fm.charge.anim_attack_off = 0;
        // ... and isCrouching / isCrouchingFX at their class defaults (false), no crouch owed to a dispatch.
        fm.crouch.crouching = false;
        fm.crouch.crouch_fx = false;
        fm.crouch.crouch_owed = false;
        // ... whose SameAttackBlocked is the class default, and an empty Comp_AbilityOverrideManager.OverridePacks.
        let b = &mut fm.branching;
        b.same_attack_blocked = false;
        b.override_packs = None;
        b.foreign_wins = None;
        // ... and no CurrentMontage.
        b.branch_montage = None;
        // ... with the CollisionCylinder template's size and BlockLandReset at its class default (a respawn writes
        // neither: the pawn is kept, and only AnimNotify_SupermanFlight's End puts them back).
        fm.capsule.cap_half_u = crate::math::narrow(cap_half);
        fm.capsule.cap_radius_u = crate::math::narrow(cap_radius);
        fm.capsule.cap_resized = false;
        fm.crouch.block_land_reset = false;
        // ... with an empty Comp_Spawner.IsEquippedComponents and no actor bound to its JumpPressedDispatch.
        // (CrouchLocked: `spawn` stops the montage and runs its notify ends, AnimNotify_Crouch's among them.)
        fm.pose.equipped.clear();
        fm.frame.jump_listeners.clear();
    }
    // ... and an anim instance with no active sequence state.
    anim::seq_reset(w, f);
    // A new pawn's Comp_FighterVisibilityManager, after the old anim instance's teardown (seqReset: its active states'
    // ends, a ReverseAtNotifyEnd sub-mesh end among them, belong to the old pawn):
    // Comp_Pawn_SkinManager.ApplySkinToPawn's SetupInitialVisibility (the skin's initial sub-mesh set,
    // InvisibilityCount 0) and an empty BlendOutDependentNotifies, then ReadyForIntro's AddInvisibility.
    // PandaGameMode SpawnCharacter's SetRotationLogic (@1812) comes after: the character's `matchReset`.
    {
        // A new pawn's ArmorVisibilityCount (class default 0).
        let a = &mut w.fighter_mut(f).armor;
        if a.armor_vis.is_some() {
            a.armor_vis = Some(0.0);
        }
    }
    anim::setup_initial_visibility(w, f);
    if intro {
        anim::add_invisibility(w, f);
    }
    w.fighter_mut(f).pose.blend_out_subs.clear();
    reset_combat_state(w, f);
    let fm = w.fighter_mut(f);
    // A new pawn's CDO value (the respawn leaves it).
    fm.attackers.zero_out_vel_for_kb = false;
    // A fresh fighter has not run UpdateAirControl: the stock AirControlBoostMultiplier 2 stays until the first
    // ResetAirControl (hitstun end, respawn). Read from game memory after a Lab reset.
    fm.air.air_control = C::AIR_CONTROL;
    fm.air.air_boost_mult = C::AIR_CONTROL_BOOST_MULT;
    fm.life.ko_hitstun = 0.0;
    // A fresh pawn's world-clock stamps are the class default 0 (none set in the class defaults).
    fm.hit.hitstun_end_time = 0.0;
    fm.hit.last_hitstun_time = 0.0;
    fm.evade.last_evade_time = 0.0;
    fm.jump.last_jump_time = 0.0;
    fm.air.dash_game_time = 0.0;
    fm.charge.charge_start_time = 0.0;
    fm.hit.prev_bounce_time = 0.0;
    fm.charge.set_charge_ui((0.0, 0.0));
    // Class default.
    fm.jump.jump_stop_gate = false;
    fm.jump.end_jump_latent = false;
    fm.moves.net_gravity = None;
    // A new movement component (resetForRespawn keeps it).
    fm.moves.gravity_override = false;
    // Class default.
    fm.moves.gravity_mod_count = 0;
    fm.moves.gravity_scale = C::GRAVITY_SCALE;
    // A new pawn's layer instance: the class default.
    fm.react.fall_lean = 0.0;
}

/// SetInvulnerableCount(0), then SpawnInvulnerability: AddInvulnerability and the 0.03 s Delay, and
/// SpawnInvulnerabilityVisuals: PS_Invulnerability on the active mesh (CancelInvuln's DestroySpawnVFX ends it) and
/// sfx_shared_combat_invulnerability_alt on the fighter's Ak component. A respawn's, and a Waves enemy's at its spawn
/// (`modes::waves`).
pub fn spawn_invulnerability(w: &mut World, f: FighterId) {
    control::clear_invuln(w, f);
    control::enable_invulnerability_raw(w, f);
    w.fighter_mut(f).invuln.invuln = math::round((C::SPAWN_INVULN_DELAY + C::SPAWN_INVULN) * 60.0);
    let fighter = fid(w, f);
    push(w, f, Event::SpawnFx { fighter, on: true });
    let ev = SFX_INVULNERABILITY;
    sound(w, f, ev, None);
}

/// `resetForRespawn`: after `spawn` at the Respawn point, ResetFighterLogic, a fast
/// downward launch and spawn invulnerability (SpawnInvulnerability after a 0.03 s delay).
pub fn reset_for_respawn(w: &mut World, f: FighterId) {
    // Read by PandaGameMode before RespawnCharacter.
    let killer = w.fighter(f).attackers.most_recent_attacker;
    hit::reset_damage(w.fighter_mut(f));
    // ResetFighterLogic @305 Comp_FighterVisibilityManager.Reset: SetupInitialVisibility again, the pawn's
    // non-primary sub-meshes hidden and InvisibilityCount 0, ahead of the pawn's own reset (OnFighterReset @1106,
    // `hooks.reset` below). RespawnCharacter's K2_SetActorTransform comes after BPI_ResetFighter (@432 / @1056) and
    // calls no SetRotationLogic: a sub-mesh a turn sets (Wonder Woman's LeftHair / RightHair, both primary) is shown
    // until the next turn.
    anim::setup_initial_visibility(w, f);
    // ResetFighterLogic: Comp_Armor.ClearTempHealth (Map_Clear @76), then @117.
    w.fighter_mut(f).stats.temp_health.clear();
    defense::temp_health_visuals(w, f);
    // Velocity set directly; the first NewFallVelocity clamps it to the terminal.
    w.fighter_mut(f).body.vy = C::RESPAWN_VELOCITY_Z;
    // RemoveHitstunFromKnockBack runs the HitstunHandler end sequence: BPI_SetTerminalVelocity(true, 0, false).
    // ResetFighterLogic -> ResetWallFatigue.
    if w.fighter(f).wall.wall_sliding {
        moves::wall_slide_off(w, f);
    }
    moves::set_terminal(w, f, true, 0.0, false);
    {
        // ResetFighterLogic 1047-1080.
        let fm = w.fighter_mut(f);
        fm.fast_fall.ff_mult = false;
        fm.jump.crouch_jump = false;
    }
    // IsFastFalling = false @1069 (the FastFallLanded broadcast's).
    moves::fast_fall_end(w, f);
    // ResetFighterLogic @447.
    control::reset_abilities(w, f);
    // SetInvulnerableCount(0) at the KO, then SpawnInvulnerability (@845).
    spawn_invulnerability(w, f);
    // @859 Comp_Pawn_PerkManager.ResetFighter -> Add: the perk buffs again, after SpawnInvulnerability and before the
    // DamageManager reset (@923, resetCombatState below), ResetWallFatigue (@1092) and OnFighterReset (@1106, the
    // character's reset hook). Deviation: the passives' reset (TeamIndexSetOnSpawn @633, before these) runs after
    // this call in the port (World.respawn).
    perks::add_perks(w, f, false);
    // @1092.
    moves::reset_wall_fatigue(w, f);
    // OnFighterReset (@1106): Pawn_C017 ResetFighterMulticast @21829 and the component's OnOwnerReset (fuel full,
    // ResetRage(true)); Comp_HoldCharge's OnOwnerReset (ClearCharges), bound at its BeginPlay ahead of the pawn's.
    hits::clear_charges(w, f);
    if let Some(h) = w.fighter(f).hooks.char.reset {
        h(w, f);
    }
    // The other pawns' bindings of this OnFighterReset (Comp_StevenShield OnAllyRespawn).
    let mut i = 0;
    while let Some(&o) = common::peers(w).get(i) {
        i += 1;
        if o != f
            && let Some(h) = w.fighter(o).hooks.char.other_reset
        {
            h(w, o, f);
        }
    }
    {
        // ResetFighterLogic @378 BFReset Gravity: NetGravity(BFDefaultGravityScale), GravityModifierCounter = 0. A
        // gravity override that stands (gravityOverride, kept) gives this NetGravity one move, then 1.0 again.
        let fm = w.fighter_mut(f);
        fm.moves.net_gravity = Some(C::GRAVITY_SCALE);
        fm.moves.gravity_mod_count = 0;
        fm.wall.no_wall_slide_until = -1;
    }
    // RemoveHitstunFromKnockBack runs the hitstun end path even with no hitstun left: BPI_ResetAirControl.
    moves::update_air_control(w, f, C::AIR_CONTROL);
    {
        let fm = w.fighter_mut(f);
        fm.evade.evade_count = 0.0;
        fm.evade.evade_invuln = 0.0;
        let b = &mut fm.branching;
        b.queued = None;
        b.late_branch = None;
        b.late_auto = None;
        // The maps' windows of another anim instance go with them (sameAttackBlocked stays).
        b.foreign_wins = None;
        fm.montages.dash_montage = None;
    }
    anim::clear_fading(w, f);
    anim::clear_grouped(w, f);
    {
        let fm = w.fighter_mut(f);
        fm.hit.in_hit_pause_pose = false;
        fm.hit.queued_hit_pause = 0.0;
    }
    reset_combat_state(w, f);
    // PandaGameMode SpawnAndInitPlayer after RespawnCharacter: pfgPlayVO(respawn, the killer's GetVOTagForEnemies
    // (@2299, LocalKiller.Comp_Actor_VO) or NoAttacker, AlwaysPlayVO true; @2366).
    let data = Arc::clone(&w.data);
    let tag = match killer {
        Some(k) => vo_tag_in(&data, w, k, false),
        None => "NoAttacker",
    };
    play_vo(w, f, crate::name!("respawn"), &[tag], true);
}

/// `resetCombatState`.
pub fn reset_combat_state(w: &mut World, f: FighterId) {
    {
        let fm = w.fighter_mut(f);
        fm.evade.dodged_successfully = false;
        let d = &mut fm.decay;
        d.attack_count = 0;
        d.landed_attack_ids.clear();
        d.has_hit_a_fighter = false;
        // BPI_ResetAttackData (ResetFighterLogic).
        d.attack_decay.clear();
        d.current_decay_sequence = 0;
        d.landed_decay_sequences.clear();
    }
    // The respawn's RemoveHitstunFromKnockBack runs the hitstun end sequence, which clears the combo resistance map.
    hit::reset_combo_res(w, f);
    let fm = w.fighter_mut(f);
    let a = &mut fm.attackers;
    a.di_combo = 0.0;
    a.combo_reset_tick = -1;
    a.most_recent_attacker = None;
    a.assist_attacker = None;
    // DamageManager ResetFighter @33.
    a.most_recent_friendly_fire = None;
    // ResetFighterLogic @773: RemoveHitstunFromKnockBack with no source left clears PreviousAttacker (HitstunHandler
    // ubergraph 7427-7530) and runs the hitstun end (1821-1908: HitByMultipleAttackers, ForceAdditiveKnockback);
    // DamageManager ResetFighter @44-55 clears HitStunnedBeforeHit and HitByMultipleAttackers. Neither writes
    // ZeroOutVelocityForKnockback.
    a.previous_attacker = None;
    a.hit_by_multiple_attackers = false;
    a.force_additive_kb = false;
    a.hit_stunned_before_hit = false;
    let h = &mut fm.hit;
    h.prev_bounce_time = 0.0;
    h.rapid_bounce_count = 0;
    h.base_vx = 0.0;
    h.base_vy = 0.0;
    h.add_kb_vx = 0.0;
    h.add_kb_vy = 0.0;
    h.bounce_fx_until = -1e9;
}

// ---- items

/// `holdMontage`: Comp_AbilityOverrideManager.IsOverrideValid for the hold pack, which is on
/// while an item is held (a fighter that is not a Shaggy; ConstructHoldOverridePack): HoldingItemOverride's
/// entries, and for an item with CanUseItem false (the sandwich) each Forward entry preceded by a Neutral copy and
/// the Neutral (use) entries left out. An entry answers the Item button or the normal attack with exactly its
/// direction (diagonals match none); the last matching entry of the fighter's ground / air state wins, and the hold
/// pack (Priority 0) is taken ahead of the pickup pack (1), whose own rules all fail with a held item except
/// Item + Neutral, which the Neutral copies shadow.
/// The entries are HoldingItemOverride's as it was at the pick-up (`Anim::hold_pack`: the pack is built once, in
/// EquipOnSpawn), so a pawn that writes HoldingItemOverride while holding an item (Pawn_C016 SetHasBall) changes
/// the pack of its next pick-up only. The pick-up pack is no part of the navigation set.
// Unverified: the two priorities are the class defaults (the template does not serialize them); an item with
// CanUseItem and a character without a Neutral-capable entry are not met here.
pub fn hold_montage(w: &World, f: FighterId, b: Btn, d: Dir) -> Option<Name> {
    let it = w.fighter(f).items.held_item?;
    if b != Btn::Item && b != Btn::Attack {
        return None;
    }
    // BranchManagerGetAttack @192: LocalIsInAir = BPI_GetIsInAir.
    let air = fighter::in_air(w, f);
    let can_use = match items::item_def(w, it).and_then(|p| p.get("CanUseItem")) {
        None | Some(Value::Null) => true,
        Some(_) => items::item_def(w, it).is_some_and(|p| flag(p, "CanUseItem")),
    };
    let (neutral, fwd, d) = (Dir::Neutral as i32, Dir::Fwd as i32, d as i32);
    let mut out = None;
    for e in &w.fighter(f).anim.hold_pack {
        if e.air != air || (!can_use && e.dir == neutral) {
            continue;
        }
        if e.dir == d || (!can_use && e.dir == fwd && d == neutral) {
            out = (!e.montage.is_none()).then_some(e.montage);
        }
    }
    out
}

/// `itemOverride`: CheckForOverride, the override montage for a press. `raw`: the press's
/// unmodified direction (`rawPressDir`, or the stored one of a buffered press resolved at its window's end).
pub fn item_override(w: &mut World, f: FighterId, b: Btn, raw: Dir) -> Option<Name> {
    if w.fighter(f).items.held_item.is_some() {
        return hold_montage(w, f, b, raw);
    }
    pickup_override(w, f, b, raw).then(|| fighter::pickup_montage(w, f))
}

/// `pickupOverride`: IsOverrideValid @980 compares the unmodified direction with Neutral (the
/// folded one never differs there: the fold only turns diagonals into Up / Down).
pub fn pickup_override(w: &World, f: FighterId, b: Btn, raw: Dir) -> bool {
    let fm = w.fighter(f);
    if !fm.items.item_reachable {
        return false;
    }
    b == Btn::Item || (b == Btn::Attack && raw == Dir::Neutral && fm.controls.prefs.attack_picks_up_items)
}

/// `aimerTick`: the older aimer, then the newer one.
pub fn aimer_tick(w: &mut World, f: FighterId) {
    if w.fighter(f).items.aimer_under.is_some() {
        tick_aimer(w, f, true);
    }
    if w.fighter(f).items.aimer.is_some() {
        tick_aimer(w, f, false);
    }
}

/// The aimer's AimInfo, read from its notify.
struct AimInfo {
    aim_type: Real,
    max: Real,
    min: Real,
    only_y: bool,
    /// AimInfo.UseRightStickToAim: StickAttackX / Y first (ReceiveTick @722).
    right_stick: bool,
    input_rotation: Real,
    speed: Real,
    power_speed: Real,
    power_min: Real,
    power_max: Real,
}

fn aim_info(w: &World, f: FighterId, n: NotifyRef, over: Option<ThrowAimInfo>) -> AimInfo {
    if let Some(t) = over {
        // The throw's AimInfo is built from the notify (Received_NotifyBegin @481-954), whose class defaults the
        // importer merged under each instance.
        let p = notify_props(w, f, n);
        return AimInfo {
            aim_type: 0.0,
            max: t.max,
            min: t.min,
            only_y: flag(p, "Only Use Inpute Y"),
            right_stick: flag(p, "Use Right Stick to Aim"),
            input_rotation: t.input_rotation,
            speed: t.speed,
            power_speed: 0.0,
            power_min: 1.0,
            power_max: 1.0,
        };
    }
    let p = notify_props(w, f, n);
    let i = p.get("AimInfo").and_then(Value::as_obj).unwrap_or(&NO_PROPS);
    AimInfo {
        aim_type: num(i, "AimType").unwrap_or(0.0),
        max: num(i, "AimAngleMax").unwrap_or(0.0),
        min: num(i, "AimAngleMin").unwrap_or(0.0),
        only_y: flag(i, "OnlyUseInputeY"),
        right_stick: flag(i, "UseRightStickToAim"),
        input_rotation: num(i, "InputRotationOffset").unwrap_or(0.0),
        speed: num(i, "AimSpeed").unwrap_or(0.0),
        power_speed: num(i, "LaunchPowerMultSpeed").unwrap_or(0.0),
        power_min: num(i, "LaunchPowerMinMultiplier").unwrap_or(1.0),
        power_max: num(i, "LaunchPowerMaxMultiplier").unwrap_or(1.0),
    }
}

/// DegAtan2 (142f68770): FMath::Atan2, then one multiply.
fn deg(y: Real, z: Real) -> Real {
    ue::deg_atan2(y, z)
}

/// RotateAngleAxis of (0, Y, Z) by `d` degrees about the axis (`ax`, 0, 0) (FMath::SinCos, 14136dfb0).
fn rot(y: Real, z: Real, d: Real, ax: Real) -> (Real, Real) {
    let r = ue::rotate_angle_axis([0.0, y, z], d, [ax, 0.0, 0.0]);
    (r[1], r[2])
}

/// Normal((0, Y, Z), 0.0001) (142f7adf0).
fn unit(y: Real, z: Real) -> (Real, Real) {
    let n = ue::normal([0.0, y, z], 1e-4);
    (n[1], n[2])
}

/// FindNearAngleDirection: the difference wrapped to the nearest turn (ReturnMult, its sign, the rounded output).
struct Near {
    mult: Real,
    pos: bool,
    diff: Real,
}

fn near(a1: Real, a2: Real) -> Near {
    let d0 = a1 + 180.0 - (a2 + 180.0);
    let s = if d0.abs() < (d0 - 360.0).abs() { d0 } else { d0 - 360.0 };
    let r = if s.abs() < (d0 + 360.0).abs() { s } else { d0 + 360.0 };
    // Round (142f817a0) then Conv_IntToFloat.
    Near { mult: if r < 0.0 { 1.0 } else { -1.0 }, pos: r < 0.0, diff: math::real(ue::round_to_int(r)) }
}

/// The input vector's Z: the stick's sign turned (1, 0, -1), a positive zero at rest (`-stick_sign` would give -0,
/// and DegAtan2(-0, z) differs from DegAtan2(0, z) for z < 0).
fn stick_sign_down(v: Real) -> Real {
    if v == 0.0 {
        0.0
    } else if v > 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// The stick's sign (-1, 0, 1).
fn stick_sign(v: Real) -> Real {
    if v == 0.0 {
        0.0
    } else if v > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// `tickAimer`: ProjectileAimer ReceiveTick (ubergraph 604, AimType 0, the right stick first when
/// AimInfo.UseRightStickToAim, else the left): the arrow
/// turns toward the stick's 8-way direction at AimSpeed deg/s, held within AimAngleMin..Max of the start vector. The
/// graph works in angles DegAtan2(Y, Z): limits from the start vector rotated about X by (FacingLeft ? Min : -Max)
/// and (FacingLeft ? Max : -Min); the input vector is (0, sign(MoveX), -sign(MoveY)), i.e. the direction opposite
/// the stick in the world (+MoveX is -Y), and the arrow is turned away from it: FindNearAngleDirection(arrow, input)
/// gives the wrapped difference, ReturnMult +1 when it is negative, and RotateAngleAxis about +X lowers
/// DegAtan2(Y, Z). No turn without input, with |difference| >= 179 (the arrow is on the stick's direction), or
/// within 1 degree of the limit on that side; the step is clamped to the (rounded) distance to that limit. The
/// arrow is normalised every tick. FacingLeft is read every tick unless AimInfo.LockAimDirection (@1442 skips the
/// read @1708-1833: the value SetAimInfoMulticast stored at the spawn, `Aimer::fl`). With AimInfo.OnlyUseInputeY the
/// X input is 0 (SelectFloat(0, MoveX, OnlyUseInputeY) @1328, @861). The launch power multiplier (Power Mult)
/// follows the stick at the tick's end. BPI_SetAimDirection on the AnimBP is the pose's `aim_direction`.
/// AimType (the aimer's own copy of AimInfo.AimType, @15420): the tick's switch @1465-1586 sends 0 and 3 to the stick
/// rule @1873, 1 to the skill shot path @5527 (refused at the spawn: spawnerBegin) and anything else (2) out with
/// nothing done. 3 differs from 0 only in the setup (@14813-15060): ArrowAimVector = StartAimVector either way, then
/// 0 calls TurnOnArrowUI (Arrow.SetVisibility(false) @15065: the arrow component is bVisible false in the class
/// anyway) and 3 sets AimTypeNone, which nothing reads. So 3 is 0 in the sim and the view.
/// Each tick on that path ends in SetArrowRotationServer(rotation, ArrowAimVector) @4483 -> SetArrowRotationMulticast
/// @12996-13023 -> @13463: the owner's Comp_Spawner.ProjectileAimVector = ArrowAimVector (`Items::proj_aim`), turned
/// or not; of two aimers the newer one's write is the later, as for BPI_SetAimDirection.
// Unverified: the aimer is taken to tick every frame from its spawn frame on, after the owner's notifies of that
// frame (its class defaults are not dumped: tick interval / group unknown). With two aimers the newer one's
// BPI_SetAimDirection is taken as the later (the order of two actors' ticks is not decoded;
// C017_ThrowScrap_Montage's two have the same AimInfo and so the same arrow).
pub fn tick_aimer(w: &mut World, f: FighterId, under: bool) {
    let it = &w.fighter(f).items;
    let Some(a) = (if under { it.aimer_under.as_ref() } else { it.aimer.as_ref() }) else { return };
    let (n, a_fl, sy, sz, ay, az, pm, over) = (a.n, a.fl, a.sy, a.sz, a.ay, a.az, a.pm, a.info);
    let info = aim_info(w, f, n, over);
    // The switch's default @1586-1600.
    if info.aim_type == 2.0 {
        return;
    }
    let fm = w.fighter(f);
    let fl = a_fl.unwrap_or(fm.body.facing_dir < 0.0);
    let (max, min) = (info.max, info.min);
    let cur = deg(ay, az);
    let la = rot(sy, sz, if fl { min } else { -max }, 1.0);
    let lb = rot(sy, sz, if fl { max } else { -min }, 1.0);
    let da = near(cur, deg(la.0, la.1));
    let db = near(cur, deg(lb.0, lb.1));
    // The input pair (@722-1415): with UseRightStickForAiming StickAttackX (0 under OnlyUseInputeY) and StickAttackY,
    // unless their float32 sum is exactly 0 (@1100-1180), else the move axes (X 0 under OnlyUseInputeY).
    let inp = &fm.controls.inp;
    let stick = (if info.only_y { 0.0 } else { inp.rsx.unwrap_or(0.0) }, inp.rsy.unwrap_or(0.0));
    let (mx, my) = if info.right_stick && stick.0 + stick.1 != 0.0 {
        stick
    } else {
        (if info.only_y { 0.0 } else { inp.x }, inp.y)
    };
    // The input vector turned by InputRotationOffset about (FacingLeft ? +X : -X) (2793-3543; 0 on the grenade's
    // notifies, 11 on the forward item throws), then Normal(.., 0.0001) @3409.
    let iv = rot(stick_sign(mx), stick_sign_down(my), info.input_rotation, if fl { 1.0 } else { -1.0 });
    let iv = unit(iv.0, iv.1);
    let dt = near(cur, deg(iv.0, iv.1));
    let sel = (if dt.pos { db.diff } else { da.diff }).abs();
    let hold = dt.diff.abs() >= 179.0 || (max != min && sel <= 1.0) || (mx == 0.0 && my == 0.0);
    let (mut y, mut z) = (ay, az);
    if !hold {
        let step = info.speed * C::DELTA_SECONDS;
        (y, z) = rot(y, z, (if max == min { step } else { math::min(math::max(step, 0.0), sel) }) * dt.mult, 1.0);
    }
    // Normal(MakeVector(0, Y, Z), 0.0001) @4276.
    let (ay, az) = unit(y, z);
    // SetArrowRotationMulticast (ubergraph 13065-14705): BPI_SetAimDirection(NormalizeToRange(angle, start + Min,
    // start + Max) x 2 - 1) on the owner's AnimBP, the angles DegAtan2(Z, FacingLeft ? Y : -Y) of the arrow and of
    // StartAimVector.
    let el = |vy: Real, vz: Real| deg(vz, if fl { vy } else { -vy });
    let lo = el(sy, sz) + min;
    let hi = el(sy, sz) + max;
    let ang = el(ay, az);
    let aim_direction =
        (if lo == hi { if ang >= hi { 1.0 } else { 0.0 } } else { (ang - math::min(lo, hi)) / (hi - lo).abs() }) * 2.0
            - 1.0;
    // Power Mult (@4515-5495, behind SetArrowRotationServer, on the same path as the turn): with stick input and
    // |FindNearAngleDirection(DegAtan2 of the arrow just turned, DegAtan2 of (sign(MoveX), -sign(MoveY)))| >= 90 it
    // grows by LaunchPowerMultSpeed x DT, else it shrinks by that, clamped to [LaunchPowerMinMultiplier,
    // LaunchPowerMaxMultiplier]. That input vector is the turn's (without InputRotationOffset), the direction the
    // arrow turns away from, so the power grows while the arrow is within 90 degrees of the stick's direction (it
    // has turned onto it, |difference| >= 179, or is on its way). Only the fishing montages' notifies have a speed
    // (Tom_and_Jerry_Fishing_Ground 1.5, _Air 1.25, max 2); every other aimer's min and max are 1, so it stays 1.
    // @5522 goto 15: SetPowerMultServer -> SetPowerMultMulticast @15100 writes the same value back.
    let pm0 = pm.unwrap_or(1.0);
    let pstep = info.power_speed * C::DELTA_SECONDS;
    let pin = near(deg(ay, az), deg(stick_sign(mx), stick_sign_down(my)));
    let pv = if pin.diff.abs() >= 90.0 && !(mx == 0.0 && my == 0.0) { pstep + pm0 } else { pm0 - pstep };
    // FClamp.
    let pm = if pv < info.power_min {
        info.power_min
    } else if pv < info.power_max {
        pv
    } else {
        info.power_max
    };
    let fm = w.fighter_mut(f);
    fm.pose.aim_direction = aim_direction;
    // @13463.
    fm.items.proj_aim.y = ay;
    fm.items.proj_aim.z = az;
    let a = if under { fm.items.aimer_under.as_mut() } else { fm.items.aimer.as_mut() };
    let a = a.expect("the aimer read above");
    a.ay = ay;
    a.az = az;
    if pm != pm0 {
        a.pm = Some(pm);
    }
}

/// `resetAim`: Comp_Spawner.ResetAim (ubergraph 8965 -> 7920-7996), called from
/// ProjectileAimer's ReceiveEndPlay (@12745-12959) whenever an aimer goes (activated @11051, destroyed by its
/// spawner's end, its AimItemThrow's end or the owner's reset): K2_SetTimerDelegate(OnResetAimTimer, 0.1, looping)
/// into ResetAimTimer. Set in the tick groups it is pending until the frame's timer pass; set again while it runs
/// it starts over.
// Unverified: that a second K2_SetTimerDelegate of the same delegate replaces the running timer
// (K2_FindDynamicTimerHandle, then SetTimer on that handle) is the 4.26 source, not read in the exe (as the evade's
// DoOnce timer takes it).
pub fn reset_aim(w: &mut World, f: FighterId) {
    w.fighter_mut(f).items.proj_aim.reset = 1.0;
}

/// ResetAimTimer's rate.
fn reset_aim_rate() -> Real {
    0.1
}

/// `resetAimTimer`: ResetAimTimer in the world's timer pass (FTimerManager::Tick: active at
/// the pass after the set with ExpireTime = InternalTime + Rate, fired with InternalTime > ExpireTime, CallCount
/// calls in a row, re-armed by ExpireTime += CallCount x Rate). OnResetAimTimer (@8970-9107): with
/// ProjectileAimVector equal to zero within 0.0001 (each axis, X being 0) the timer is cleared, else
/// ProjectileAimVector = VLerp(it, 0, 0.1): a tenth of the way to zero, every 6th frame at 60 Hz. The timer's clock is
/// FTimerManager's double (142e45520): CallCount = (int)((Internal - Expire) / (double)Rate) + 1, the re-arm
/// Expire += (double)((float)CallCount x Rate).
pub fn reset_aim_timer(w: &mut World, f: FighterId) {
    let rate = reset_aim_rate();
    let s = &mut w.fighter_mut(f).items.proj_aim;
    if s.reset == 0.0 {
        return;
    }
    if s.reset == 1.0 {
        s.reset = 2.0;
        s.t = 0.0;
        s.at = f64::from(rate);
        return;
    }
    s.t += f64::from(C::DELTA_SECONDS);
    if s.t.partial_cmp(&s.at) != Some(Ordering::Greater) {
        return;
    }
    let calls = ((s.t - s.at) / f64::from(rate)).trunc() + 1.0;
    let mut i = 0.0;
    while i < calls && s.reset == 2.0 {
        if s.y.abs() <= 1e-4 && s.z.abs() <= 1e-4 {
            // K2_ClearAndInvalidateTimerHandle @9031.
            s.reset = 0.0;
        } else {
            s.y += 0.1 * (0.0 - s.y);
            s.z += 0.1 * (0.0 - s.z);
        }
        i += 1.0;
    }
    if s.reset == 2.0 {
        s.at += f64::from(math::real(calls) * rate);
    }
}

/// `throwItem`: Comp_ItemManager.ThrowItem(ThrowDirection, DisconnectWithOffset, Offset) up
/// to the release: HeldItem is cleared and the hold pack removed in the call; the World moves the item
/// (`releaseItems`). The release location: with an offset (fighter.Y - Offset.X, fighter.Z + Offset.Y) (@558-647:
/// SelectFloat(X, -X, false), the facing is not read, so the point is on the -Y side for both facings), else the
/// item's own location: where it hangs, the holder's hold socket (`hold_point`; Bugs's Item_Drop releases at VB
/// PropPickup, his R_PropOffset).
// Unverified: the offset's side for a left-facing thrower is the literal bytecode, not captured; a holstered item
// (the side specials) is dropped from the hold socket too.
pub fn throw_item(w: &mut World, f: FighterId, dx: Real, dy: Real, offset: Option<V2>) {
    let Some(it) = w.fighter(f).items.held_item else { return };
    {
        let fm = w.fighter_mut(f);
        fm.items.held_item = None;
        fm.items.item_holstered = false;
    }
    let (x, y) = match offset {
        Some(o) => (w.fighter(f).body.x + o.x, fighter::actor_y(w, f) + o.y),
        None => hold_point(w, f),
    };
    let fm = w.fighter_mut(f);
    let (vx, vy, attack_id) = (fm.body.vx, fm.body.vy, fm.decay.attack_count);
    fm.items.item_releases.push(ItemRelease { it, dx, dy, x, y, vx, vy, attack_id });
}

/// `holdSlots`' sockets: facing right, facing left, and the socket of the right-facing pose that is the left one's
/// place (`hold_point`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoldSlots {
    pub right: String,
    pub left: String,
    pub place: String,
}

/// `holdSlots`: where a held item is attached (Comp_ItemManager.EquipOnSpawn @1262..1482,
/// any ItemActorParent, also one used on its pickup): BlockMirroring ? HoldSlotName : FindMirrorSocket(HoldSlotName,
/// the holder, FacingLeft). FindMirrorSocket (@0..941; Replace and Contains ignore case): cand = FacingLeft ? (the
/// name has "L_" ? "L_" -> "R_" : "R_" -> "L_") : the name; None when neither cand nor the name is a socket, cand when
/// the name has "L_" or "R_" and cand exists, else the name. Shaggy's, Morty's and most pawns' R_Hand (L_Hand
/// facing left); LeBron's L_Hand; Wonder Woman's VB R_HoldPoint; Bugs's VB PropPickup at both facings; Pawn_C017's
/// R_HoldPoint with BlockMirroring.
/// DoesSocketExist: a socket of the skeleton, a bone of the mirror table or a virtual bone (a virtual bone is a bone
/// of the mesh's reference skeleton, stock 4.26 USkeleton::HandleVirtualBoneChanges).
// Deviation: a mesh bone outside the mirror table is not known here (no hold socket of the port's pawns is one).
// The answer is a few string reads, made per call.
pub fn hold_slots(cd: &CharData) -> HoldSlots {
    let comp = |base: bool| {
        let m = if base { &cd.fighter.components.base } else { &cd.fighter.components.pawn };
        m.get("Comp_ItemManager_GEN_VARIABLE").and_then(|c| c.p.as_obj()).unwrap_or(&*NO_PROPS)
    };
    let (p, b) = (comp(false), comp(true));
    let name = p
        .get("HoldSlotName")
        .filter(|v| !v.is_null())
        .or_else(|| b.get("HoldSlotName").filter(|v| !v.is_null()))
        .and_then(Value::as_str)
        .unwrap_or("None");
    let sk = &cd.fighter.skeleton;
    let has = |s: &str| {
        sk.sockets.contains_key(s)
            || sk.mirror.contains_key(s)
            || sk.virtual_bones.as_ref().is_some_and(|v| v.contains_key(s))
    };
    let lower = name.to_lowercase();
    let (has_l, has_r) = (lower.contains("l_"), lower.contains("r_"));
    let block = match p.get("BlockMirroring") {
        None | Some(Value::Null) => flag(b, "BlockMirroring"),
        Some(_) => flag(p, "BlockMirroring"),
    };
    let left = if block {
        name.to_owned()
    } else {
        let cand = if has_l { replace_ignore_case(name, "L_", "R_") } else { replace_ignore_case(name, "R_", "L_") };
        let cand_ok = has(&cand);
        if !cand_ok && !has(name) {
            "None".to_owned()
        } else if (has_l || has_r) && cand_ok {
            cand
        } else {
            name.to_owned()
        }
    };
    let right = if block || has(name) { name.to_owned() } else { "None".to_owned() };
    // The socket of the right-facing pose that is the left one's place: FindMirrorSocket's swapped name (L_ <-> R_)
    // is the mirror twin of the slot name, so the slot name itself.
    let place = if left != name && left != "None" { name.to_owned() } else { left.clone() };
    HoldSlots { right, left, place }
}

/// `holdSocket`.
pub fn hold_socket(w: &World, f: FighterId) -> String {
    let n = hold_slots(common::data_of(w, f));
    if w.fighter(f).body.facing_dir < 0.0 { n.left } else { n.right }
}

/// `holdPoint`: the hold socket's location in the current pose (a held item's place: the
/// release, the carried fighters). The sampled pose is the right-facing one P, reflected with the facing (as the
/// view draws a left facing): facing left the mirror node's pose has a socket where P has its mirror twin, reflected
/// (the view's boneFollow rule), so FindMirrorSocket's L_Hand of a left facing is P's R_Hand reflected. A virtual
/// bone is sampled at its target, or at the bone of the AnimBP's Constraint node that holds it (Bugs's VB PropPickup
/// at fml_un_R_PropOffset: BugsBunny_AnimBP Constraint_1, its right-facing node; the left-facing node's
/// L_PropOffset on the mirrored pose is its reflection; the box import). None: the mesh component's location
/// (K2_AttachToComponent with no socket).
// A name kept at both facings on a side bone (Black Adam's ItemHold on fml_un_R_wrist) is P's socket reflected: his
// item hangs on HeadMesh (Comp_ItemManager OverrideAttachParentTag), which copies the main mesh's pose
// (C021_Skin00_AnimBP: AnimNode_CopyPoseFromMesh, bUseAttachedParent) under its VisMesh scale (1, -1, 1) facing left
// (C021_AnimBP On Facing Changed @466; no mirror node).
pub fn hold_point(w: &World, f: FighterId) -> (Real, Real) {
    let n = hold_slots(common::data_of(w, f));
    let facing = w.fighter(f).body.facing_dir;
    if (if facing < 0.0 { &n.left } else { &n.right }) == "None" {
        return (fighter::mesh_wx(w, f), fighter::mesh_wy(w, f));
    }
    let name = if facing < 0.0 { &n.place } else { &n.right };
    let sampler = &w.fixed.samplers[&w.fighter(f).ch];
    let i = w.names.get(&format!("socket:{name}")).and_then(|k| sampler.by_name.get(&k)).copied();
    let pose = i.and_then(|_| anim::sampled_boxes(w, f));
    let (Some(i), Some(pose)) = (i, pose) else {
        common::warn_text(w, &format!("held item: hold socket {name} not sampled"));
        return (w.fighter(f).body.x, fighter::actor_y(w, f));
    };
    let o = sampler.offsets[i as usize] as usize;
    let (px, py) = (crate::real(pose[o]), crate::real(pose[o + 1]));
    anim::put_pose(w, pose);
    (fighter::mesh_wx(w, f) + px * facing, fighter::mesh_wy(w, f) + py)
}

/// `activateAimer`: ProjectileAimer ActivateObject (ubergraph 9447; the ActivateSpawned
/// notify with ActivateLastObject): nothing while the owner's Comp_Pawn_HitstunHandler.CheckHitstunActive; else
/// Comp_Spawner.CompSpawnActor with the stored notify values, the direction (FacingLeft ? ArrowY : -ArrowY, ArrowZ)
/// in the spawner's facing frame, InitialSpeed x Power Mult (@10234) and MaxSpeed = max(MaxSpeed, that speed); then
/// the aimer destroys itself (its EndPlay: ResetAim). The spawn is for AimType 0 and 3 alone (@9654-9744): any other
/// type returns there and the aimer lives on.
// Unverified: CheckHitstunActive (hitstun or buff-stun sources) is taken as inHitstun.
pub fn activate_aimer(w: &mut World, f: FighterId) {
    if w.fighter(f).items.aimer.is_none() || fighter::in_hitstun(w, f) {
        return;
    }
    let (n, over) = w.fighter(f).items.aimer.as_ref().map(|a| (a.n, a.info)).expect("an aimer");
    let aim_type = aim_info(w, f, n, over).aim_type;
    if aim_type != 0.0 && aim_type != 3.0 {
        return;
    }
    let a = {
        let it = &mut w.fighter_mut(f).items;
        let a = it.aimer.take().expect("an aimer");
        it.aimer = it.aimer_under.take();
        a
    };
    reset_aim(w, f);
    let p = notify_props(w, f, n);
    let speed = num(p, "InitialSpeed").unwrap_or(0.0) * a.pm.unwrap_or(1.0);
    let cls = common::lit(w, p.get("ActorClass").and_then(Value::as_str).unwrap_or(""));
    let aim = AimedSpawn {
        y: if w.fighter(f).body.facing_dir < 0.0 { a.ay } else { -a.ay },
        z: a.az,
        initial_speed: speed,
        // FMax @10311.
        max_speed: math::max(num(p, "MaxSpeed").unwrap_or(0.0), speed),
    };
    let charge = elem::instigator_charge(w, f);
    // CompSpawnActor's HandleActorPostSpawn reads the socket on the disguise's mesh while BPI_CheckIsDisguised
    // (Comp_Spawner @651-720), as a Spawner notify's: a disguised Reindog's spit at the copy's Gem.
    let copy = if disguise::disguise_mesh_read(w, f) {
        let mut p = p.clone();
        p.insert("AimOn".to_owned(), Value::Bool(false));
        let dir = [("X", 0.0), ("Y", aim.y), ("Z", aim.z)].map(|(k, v)| (k.to_owned(), Value::Num(f64::from(v))));
        p.insert("InitialMovementDirection".to_owned(), Value::Obj(dir.into_iter().collect()));
        p.insert("InitialSpeed".to_owned(), Value::Num(f64::from(aim.initial_speed)));
        p.insert("MaxSpeed".to_owned(), Value::Num(f64::from(aim.max_speed)));
        Some(disguise::copy_spawn_point(w, f, &p))
    } else {
        None
    };
    let r = SpawnRequest {
        cls,
        n,
        attack_id: a.attack_id,
        foreign: Foreign::None,
        copy,
        charge,
        aim: Some(aim),
        props: None,
        instigator: None,
        origin: None,
    };
    w.fighter_mut(f).requests.spawn_requests.push(r);
}

// ---- sound

/// `surfaceSocket`: the socket shape of a box index by an Ak_SurfaceEffect's TraceFrom.
/// Socket names are FNames (compared without case): Shaggy_Idle_Sit_Intro names L_Foot_End for L_Foot_end.
pub fn surface_socket(w: &World, b: &BoxSampler, sock: &str) -> Option<u32> {
    let by = |s: &str| -> Option<u32> { b.by_name.get(&w.names.get(s)?).copied() };
    let mut cap = sock.chars();
    let up = cap.next().map_or_else(String::new, |c| c.to_uppercase().chain(cap).collect());
    by(&format!("socket:{sock}"))
        .or_else(|| by(&format!("socket:{up}")))
        .or_else(|| b.by_lower.get(&format!("socket:{}", sock.to_lowercase())).copied())
}

/// `flushSurfaceTraces`: the pawn's Mesh's traces. A Disguise's second mesh traces its own
/// from its skeleton (`combat::disguise` flushCopyTraces, in that mesh's tick).
pub fn flush_surface_traces(w: &mut World, f: FighterId, prev_anim: Name, prev_time: Real) {
    let traces = std::mem::take(&mut w.fighter_mut(f).frame.surface_traces);
    for t in &traces {
        let sock = w.names.str(t.sock).to_owned();
        let at = {
            let sampler = &w.fixed.samplers[&w.fighter(f).ch];
            let i = surface_socket(w, sampler, &sock);
            let pose = if t.queued { anim::sampled_boxes(w, f) } else { anim::posed_boxes(w, f, prev_anim, prev_time) };
            let at = match (i, &pose) {
                (Some(i), Some(pose)) => {
                    let o = sampler.offsets[i as usize] as usize;
                    Ok((crate::real(pose[o]), crate::real(pose[o + 1])))
                }
                (i, _) => Err(i.is_none()),
            };
            if let Some(pose) = pose {
                anim::put_pose(w, pose);
            }
            at
        };
        match at {
            Ok((px, py)) => {
                let facing = w.fighter(f).body.facing_dir;
                let oy = fighter::mesh_wy(w, f) + py;
                let x = fighter::mesh_wx(w, f) + px * facing;
                surface_sfx(w, f, t.ev, oy, false, None, Some(x));
            }
            Err(no_socket) => {
                // No pose: the clip is not in the box data (a clip of another skeleton: C016_GroundAimSlam_Montage's).
                common::warn_text(
                    w,
                    &if no_socket {
                        format!("surface trace socket not sampled: {sock}")
                    } else {
                        format!("surface trace: no pose to read {sock} from")
                    },
                );
                // The port's fallback: the capsule's bottom as it is now.
                let oy = w.fighter(f).body.y - fighter::cap_half(w, f);
                surface_sfx(w, f, t.ev, oy, false, None, None);
            }
        }
    }
    // Hand the list back for reuse (it is emptied in place).
    let mut traces = traces;
    traces.clear();
    let l = &mut w.fighter_mut(f).frame.surface_traces;
    if l.is_empty() {
        *l = traces;
    }
}

/// `matchStartVO`: PlayMatchStartVO's call: pfgPlayVO(matchstart, "", AlwaysPlayVO true);
/// Success ends the game mode's loop.
pub fn match_start_vo(w: &mut World, f: FighterId) -> bool {
    let alias = crate::name!("matchstart");
    play_vo(w, f, alias, &[], true)
}

/// `moveFx`: the BaseFighter functions behind these spawn the dust and post the sounds:
/// SFX_Jump (SFX_Jump by PF_Surface_Effect_Trace, VO_Jump on the root AkComponent), SFX_Jump_Double (SFX_Jump_Double,
/// VO_Jump), VFX_SFX_Land (SFX_Land by surface trace, VO_Land), SFX_FastFall (sfx_shared_combat_fastfall once the
/// emitter is valid), SFX_Wall_Enter (SFX Wall Slide, kept as WallSlideID, and SFX_Wall_Attach) and SFX_Wall_Exit
/// (stops WallSlideID over 100 ms, SFX_Wall_Exit). Jump, land and wall jump trace a line from the actor location -
/// Trace Offset 01; the wall sounds a sphere from the actor location (`surface_sfx`).
pub fn move_fx(w: &mut World, f: FighterId, kind: MoveFxKind) {
    let (fighter, x, y) = {
        let fm = w.fighter(f);
        (fm.id, fm.body.x, fm.body.y)
    };
    push(w, f, Event::MoveFx { fighter, kind, x, y });
    let key = own_key(w, f, TAG_WALL_SLIDE);
    let sfx_of = |w: &World, s: &str| common::lit(w, s);
    let opt = |w: &World, s: &Option<String>| s.as_deref().map(|s| common::lit(w, s));
    let d = &common::data_of(w, f).def.sfx;
    match kind {
        MoveFxKind::Jump => {
            let (ev, vo) = (sfx_of(w, &d.jump), opt(w, &d.vo_jump));
            surface_sfx(w, f, ev, y - C::TRACE_OFFSET_01, false, None, None);
            sfx(w, f, vo);
        }
        MoveFxKind::DoubleJump => {
            let (ev, vo) = (sfx_of(w, &d.jump_double), opt(w, &d.vo_jump));
            sfx(w, f, Some(ev));
            sfx(w, f, vo);
        }
        MoveFxKind::Land => {
            let (ev, vo) = (sfx_of(w, &d.land), opt(w, &d.vo_land));
            surface_sfx(w, f, ev, y - C::TRACE_OFFSET_01, false, None, None);
            sfx(w, f, vo);
        }
        MoveFxKind::FastFall => {
            let ev = crate::name!("sfx_shared_combat_fastfall");
            sfx(w, f, Some(ev));
        }
        MoveFxKind::WallOn => {
            let (slide, attach) = (crate::name!("sfx_shared_foley_medium_gen_wall_slide"), sfx_of(w, &d.wall_attach));
            surface_sfx(w, f, slide, y, true, Some(key), None);
            surface_sfx(w, f, attach, y, true, None, None);
        }
        MoveFxKind::WallOff => {
            let exit = sfx_of(w, &d.wall_exit);
            sound_stop(w, f, key, C::WALL_SLIDE_STOP_FADE);
            surface_sfx(w, f, exit, y, true, None, None);
        }
    }
}

/// `windRush`: BaseFighter ReceiveTick (not dead): falling (Velocity.Z < 0) sets
/// cha_wind_rush from the per-frame drop and, once, posts SFX_FallWind (PF_Post_Event, kept as WindRushID);
/// otherwise, if it was on, cha_wind_rush 0 and WindRushID stops over 10 ms. GetVelocity is the last CMC result; DT
/// is dilated in hitpause (0.005). SFX_Death stops WindRushID but leaves SetWindAudioTo1 set.
pub fn wind_rush(w: &mut World, f: FighterId) {
    let fm = w.fighter(f);
    let (vy, paused, on) = (fm.body.vy, fm.hit.hitpause > 0.0, fm.hit.wind_on);
    let key = own_key(w, f, TAG_WIND);
    if vy < 0.0 {
        let dt = C::DT * if paused { 0.005 } else { 1.0 };
        let v = math::min(1.0, math::max(0.0, 1.0 - (vy * dt) / C::WIND_RUSH_RANGE));
        rtpc(w, f, crate::name!("cha_wind_rush"), v);
        if !on {
            w.fighter_mut(f).hit.wind_on = true;
            let fighter = fid(w, f);
            let ev = crate::name!("sfx_shared_gen_fallwind");
            push(w, f, Event::SoundStart { fighter, ev, key });
        }
    } else if on {
        w.fighter_mut(f).hit.wind_on = false;
        rtpc(w, f, crate::name!("cha_wind_rush"), 0.0);
        sound_stop(w, f, key, C::WIND_STOP_FADE);
    }
}

/// `stopWindForDeath`.
pub fn stop_wind_for_death(w: &mut World, f: FighterId) {
    let key = own_key(w, f, TAG_WIND);
    sound_stop(w, f, key, C::WIND_STOP_FADE);
}

/// `postVO`: Lib_Audio PF_Post_VO_Event: RandomFloat <= Probability (@28), then BPI_PlayVo
/// with, for a context fighter, the context's BPI_GetVoContextTags (@238; BaseFighter's returns
/// Comp_Actor_VO.GetVOTagForEnemies, the context's own CharacterVO_TagForEnemies) as the extra tags, and none without
/// one (@441). The tags and rows are the applied skin's (InitVOData @863 / @1005 / @1301).
pub fn post_vo(w: &mut World, f: FighterId, alias: Name, context: Option<FighterId>, probability: Real) {
    if w.fighter_mut(f).rng.next() <= probability {
        match context {
            Some(c) => {
                let data = Arc::clone(&w.data);
                let tag = vo_tag_in(&data, w, c, false);
                play_vo(w, f, alias, &[tag], false);
            }
            None => {
                play_vo(w, f, alias, &[], false);
            }
        }
    }
}

/// `allyOpponentTags`: Comp_Actor_VO.SetAllyOpponentTags: every other FighterCharacter
/// (GetAllActorsOfClass, the world's fighters) with a Comp_Actor_VO adds, on the speaker's team
/// (Comp_Actor_Destructible.TeamIndex), its CharacterVO_TagForAllies and AnyFriend (@905-1087), else its
/// CharacterVO_TagForEnemies and AnyEnemy (@1262-1443); an empty tag adds neither (the pop_flow at @958 / @1315
/// comes before both adds). AnyFriend / AnyEnemy go in once (Array_AddUnique).
/// A fighter between its ring-out and its respawn is a peer: the build reuses the pawn (RespawnCharacter:
/// BPI_ResetFighter @432, isDead false @1214, the same pawn returned @1583; @2973 is another class's branch).
fn ally_opponent_tags<'a>(data: &'a GameData, w: &World, f: FighterId, tags: &mut Vec<&'a str>) {
    let team = w.fighter(f).team.team;
    for &o in common::peers(w) {
        if o == f {
            continue;
        }
        let ally = w.fighter(o).team.team == team;
        let tag = vo_tag_in(data, w, o, ally);
        if tag.is_empty() {
            continue;
        }
        tags.push(tag);
        let any = if ally { "AnyFriend" } else { "AnyEnemy" };
        if !tags.contains(&any) {
            tags.push(any);
        }
    }
}

/// `playVO`: Comp_Actor_VO.pfgPlayVO: nothing while this fighter speaks
/// (IsCurrentlySpeaking) or, without AlwaysPlayVO (only the respawn line passes it), while any VO speaker is counted
/// (VoiceOverEnabled is on). A bot-controlled pawn goes straight to the row search (`select_vo`), past all three
/// checks, so its lines can overlap. The rows' tags are looked up in VO_TagContainer (@4488) and the extra tags
/// (ParseIntoArray on '|', empty parts culled, @415; @4768). With both empty (@669) SetAllyOpponentTags fills the
/// extra tags (SetMapTags adds none: no MapNameToVOTags entry is used by these rows).
/// The tag compare ignores the case of ASCII letters: the LeBronFriend rows match LeBron's LebronFriend.
/// VOLocal posts the line (the bot's through VOMulticast, the same offline) with an end-of-event callback; an event
/// without "eff" becomes CurrentAkEventID, is added to PreviousAkEventIDs, adds a speaker (PandaGameState
/// BPI_AddVOSpeaker: +1) and sets IsCurrentlySpeaking. No DTVO_Shaggy row has an "eff" event, so every callback here
/// belongs to a tracked line.
/// VO_TagContainer ([`set_attack_vo_tags`], [`set_help_vo_tags`]): a row's tag passes when it is in the container
/// (@4488) or in the extra tags (@4768), so the two lists are searched as one; SetAllyOpponentTags runs only with both
/// empty (@669). The container is the build's.
pub fn play_vo(w: &mut World, f: FighterId, alias: Name, extra: &[&str], always: bool) -> bool {
    // The data and the names are shared, not state: held apart from the World so the fighter's random stream can be
    // drawn while the rows are read.
    let data = Arc::clone(&w.data);
    let names = Arc::clone(&w.names);
    let Some(rows) = vo_rows_in(&data, w, f) else { return false };
    let fm = w.fighter(f);
    if !fm.team.bot_controlled && (fm.vo.vo_speaking || (!always && w.flow.vo_shared.speakers != 0)) {
        return false;
    }
    let container = fm.vo.vo_tags.clone();
    let mut tags: Vec<&str> = extra.iter().flat_map(|t| t.split('|')).filter(|t| !t.is_empty()).collect();
    if tags.is_empty() && container.is_empty() {
        ally_opponent_tags(&data, w, f, &mut tags);
    }
    tags.extend(container.iter().map(String::as_str));
    let recent = fm.vo.vo_recent;
    let rng = &mut w.fighter_mut(f).rng;
    let pick = select_vo(rows, names.str(alias), &tags, recent, |n| rng.int(n));
    let Ok(pick) = usize::try_from(pick) else { return false };
    if rows[pick].3.as_deref().is_none_or(str::is_empty) {
        return false;
    }
    w.fighter_mut(f).vo.vo_recent = crate::real(i32::try_from(pick).expect("a row index"));
    vo_local_post(w, f, &rows[pick])
}

/// Comp_Actor_VO.VOLocal(VOIndex) called by a graph (ubergraph @717): VO_DataStructArray[VOIndex] (the VO table's rows
/// in their row order) posted on the owner as `play_vo`'s pick is, with no speaking or speaker test, no row search and
/// no SetMostRecentLine. The mature language setting is off (the port has none): a row whose Alias holds "mature"
/// posts nothing (@1664). An index past the rows reads an empty row: nothing.
/// Not ported: ShowVOSubtitle (@898).
pub fn vo_local(w: &mut World, f: FighterId, index: usize) -> bool {
    let data = Arc::clone(&w.data);
    let Some(row) = vo_rows_in(&data, w, f).and_then(|r| r.get(index)) else { return false };
    if row.0.to_ascii_lowercase().contains("mature") {
        return false;
    }
    vo_local_post(w, f, row)
}

/// VOLocal's post (@839-1652): the row's event on the owner's AkComponent; an event without "eff" becomes the tracked
/// line (CurrentAkEventID, PreviousAkEventIDs, BPI_AddVOSpeaker, IsCurrentlySpeaking).
fn vo_local_post(w: &mut World, f: FighterId, row: &VoRow) -> bool {
    let names = Arc::clone(&w.names);
    let (_, _, _, ev, dur) = row;
    let Some(ev) = ev.as_deref().filter(|e| !e.is_empty()) else { return false };
    let tracked = if ev.contains("eff") { None } else { *dur };
    // Unverified: a VO row's event is taken to be interned at the bind (the data's strings); one that is not posts
    // the empty name.
    let ev = names.get(ev).unwrap_or(Name::NONE);
    let fighter = fid(w, f);
    let time = w.fighter(f).clock.time;
    match tracked {
        Some(dur) => {
            let vo = &mut w.fighter_mut(f).vo;
            let n = vo.vo_seq;
            vo.vo_seq += 1;
            let key = SoundKey { tag: TAG_VO, n };
            push(w, f, Event::SoundStart { fighter, ev, key });
            let vo = &mut w.fighter_mut(f).vo;
            vo.vo_current = Some(n);
            vo.vo_lines.push(VoLine { key: n, until: time + crate::math::narrow(dur) });
            w.flow.vo_shared.speakers += 1;
            w.fighter_mut(f).vo.vo_speaking = true;
        }
        None => sound(w, f, ev, None),
    }
    true
}

/// Comp_Actor_VO.SetAttackVOTags, bound at its BeginPlay (@374-468) to the owner's Comp_HitBoxHandler HitEnemyPlayer
/// (pfgDoDamage @22770): RemoveAllVOTagsWithSubstring(attack) @2819, then, with the owner IsLocallyControlled
/// (@2497) and a Comp_Actor_VO on the hit player, AddVOTag(its CharacterVO_TagForEnemies) @2694 and
/// RetriggerableDelay(1.0) @2739, whose link @15 removes every tag holding "enemy".
/// IsLocallyControlled is taken as true for every fighter: offline every pawn is (AController /
/// APlayerController IsLocalController), and the sim must be the same on every peer (the pick draws the fighter's
/// random stream).
// Deviation: online the build adds the tag only on the attacker's own machine (and the server for a bot); here on
// every peer.
// Unverified: Comp_Actor_VO's listener is taken to run ahead of the buffs' and the pawn's own (its BeginPlay binds
// before them).
pub fn set_attack_vo_tags(w: &mut World, f: FighterId, hit: FighterId) {
    remove_vo_tags_with(w, f, "attack");
    let data = Arc::clone(&w.data);
    let tag = vo_tag_in(&data, w, hit, false).to_owned();
    add_vo_tag(w, f, tag);
    w.fighter_mut(f).vo.vo_enemy_delay = 1.0;
}

/// Comp_Actor_VO.SetHelpVOTags, bound (@580-603) to the owner's Comp_HitBoxHandler HitAlly (NotifyAllyOfHit @886):
/// RemoveAllVOTagsWithSubstring(help) @2794, then, with the owner IsLocallyControlled (@2105, taken as true: see
/// [`set_attack_vo_tags`]) and a Comp_Actor_VO on the ally, AddVOTag(its CharacterVO_TagForAllies) @2302 and
/// RetriggerableDelay(1.0) @2347, whose link @186 removes every tag holding "friend".
pub fn set_help_vo_tags(w: &mut World, f: FighterId, ally: FighterId) {
    remove_vo_tags_with(w, f, "help");
    let data = Arc::clone(&w.data);
    let tag = vo_tag_in(&data, w, ally, true).to_owned();
    add_vo_tag(w, f, tag);
    w.fighter_mut(f).vo.vo_friend_delay = 1.0;
}

/// Comp_Actor_VO's two RetriggerableDelays (FDelayAction: TimeRemaining -= the world's delta, done at <= 0, a
/// retrigger sets it back), in the world's leftover latent pass. A delay set by a hit counts the hit's frame.
// Unverified: Comp_Actor_VO is taken not to tick (its graph has no ReceiveTick; its CDO's PrimaryComponentTick is
// not dumped), so its latent actions run in UWorld::Tick's leftover pass, after the frame's hits.
pub fn vo_tags_latent(w: &mut World, f: FighterId) {
    let vo = &mut w.fighter_mut(f).vo;
    let enemy = latent_step(&mut vo.vo_enemy_delay);
    let friend = latent_step(&mut vo.vo_friend_delay);
    // Unverified: with both done in one pass, the attack delay's link is taken first (its action was made first
    // only when its first hit came first; the two links touch different tags, so the order changes nothing).
    if enemy {
        remove_vo_tags_with(w, f, "enemy");
    }
    if friend {
        remove_vo_tags_with(w, f, "friend");
    }
}

/// One FDelayAction update of a pending delay (> 0): true when it is done.
fn latent_step(t: &mut Real) -> bool {
    if *t <= 0.0 {
        return false;
    }
    *t -= C::DELTA_SECONDS;
    *t <= 0.0
}

/// AddVOTag: Array_AddUnique(VO_TagContainer, tag) (FString ==: the case of ASCII letters ignored). An empty tag goes
/// in too, and no removal takes it out again (FindSubstring in an empty string fails).
fn add_vo_tag(w: &mut World, f: FighterId, tag: String) {
    let tags = &mut w.fighter_mut(f).vo.vo_tags;
    if !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
        tags.push(tag);
    }
}

/// RemoveAllVOTagsWithSubstring: every tag holding `sub` (FindSubstring, bUseCase false) is removed.
fn remove_vo_tags_with(w: &mut World, f: FighterId, sub: &str) {
    w.fighter_mut(f).vo.vo_tags.retain(|t| !t.to_ascii_lowercase().contains(sub));
}

/// `interruptVO`: Comp_Actor_VO InterruptVO: stops CurrentAkEventID (the last line posted;
/// nothing at -1) over the transition time; the line's end-of-event callback (VODonePlaying) then comes at the end
/// of the fade (captured: the gate frees in the ring-out frame).
pub fn interrupt_vo(w: &mut World, f: FighterId, fade: Real) {
    let fm = w.fighter(f);
    let Some(cur) = fm.vo.vo_current else { return };
    let Some(i) = fm.vo.vo_lines.iter().position(|l| l.key == cur) else { return };
    let time = fm.clock.time;
    let key = SoundKey { tag: TAG_VO, n: cur };
    sound_stop(w, f, key, fade);
    let l = &mut w.fighter_mut(f).vo.vo_lines[i];
    l.until = math::min(l.until, time + fade);
}

/// `tickVO`: VODonePlaying, once per ended line: Array_Remove(PreviousAkEventIDs, 0) (the
/// oldest entry, whichever line ended), and only when the array is then empty: CurrentAkEventID = -1,
/// BPI_RemoveVOSpeaker (one Max(ActiveVOSpeakers - 1, 0), however many lines added a speaker) and
/// IsCurrentlySpeaking = false. A bot whose lines overlapped so leaves the count raised, which keeps the player's
/// lines (all but respawn) silent.
pub fn tick_vo(w: &mut World, f: FighterId) {
    let mut i = w.fighter(f).vo.vo_lines.len();
    while i > 0 {
        i -= 1;
        let fm = w.fighter_mut(f);
        if fm.clock.time < fm.vo.vo_lines[i].until {
            continue;
        }
        // Only the array's length is read, so which entry goes makes no difference.
        fm.vo.vo_lines.remove(i);
        if fm.vo.vo_lines.is_empty() {
            fm.vo.vo_current = None;
            fm.vo.vo_speaking = false;
            let s = &mut w.flow.vo_shared.speakers;
            *s = (*s - 1).max(0);
        }
    }
}

/// `surfaceBlocked`: Lib_Audio PF_Surface_Effect_Trace (trace channel 3 =
/// ProjectileGroundTrace, which the Lab's Floor profile box blocks; nothing else in the Lab does) and
/// PF_Surface_Effect. Line: from the origin 200 cm down. Sphere: radius 150 from the origin to 5 cm above it, an
/// initial overlap counting. A hit returns the box's physical material, the engine DefaultPhysicalMaterial (no
/// override; SurfaceType 0 -> DefaultPhysmatInGame 6); a miss leaves the PhysMat invalid -> PM_Rock (SurfaceType1).
/// PF_Set_Physmat sets the physmat switch on the fighter's root AkComponent (6 flesh, 1 rock), then
/// cha_foley_land_intensity = BPI_GetVelocityClamped, then PF_Post_Event_Surface (cha_locally_controlled 1: every
/// pawn is local offline, also its default) posts it.
/// Flesh on the floor is the build's answer, not a hit on a body. Both traces are simple (bTraceComplex false) with
/// an empty ignore list; a fighter's capsule (PhysMaterialOverride PM_Flesh) and hurtboxes leave the channel at its
/// default, Ignore, so only world boxes answer. No floor, platform or floor material of the Lab or Sky Arena names a
/// physical material, and switch value 6 and the default surface 0 both play physmat-flesh. The other blockers of the
/// channel count too: the stage's pass-through platforms (profile Platform) and a ship's pieces (the run under the
/// trace; the 12 / 24 gaps between pieces are not modelled).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn surface_blocked(
    x: Real,
    oy: Real,
    sphere: bool,
    left: Real,
    right: Real,
    top: Real,
    bottom: Real,
    len: Real,
) -> bool {
    if !sphere {
        return x >= left && x <= right && oy - len <= top && oy >= bottom;
    }
    let dx = math::max(math::max(left - x, 0.0), x - right);
    let dy = math::max(math::max(bottom - (oy + 5.0), 0.0), oy - top);
    dx * dx + dy * dy <= C::SURFACE_SPHERE_RADIUS * C::SURFACE_SPHERE_RADIUS
}

/// `groundTrace`: a trace of type 3 (ProjectileGroundTrace) against the world boxes that
/// block it: the line from (x, oy) `len` down, or the surface sounds' sphere.
pub fn ground_trace(w: &mut World, f: FighterId, x: Real, oy: Real, sphere: bool, len: Real) -> bool {
    let g = w.stage.ground;
    if surface_blocked(
        x,
        oy,
        sphere,
        crate::math::narrow(g.left),
        crate::math::narrow(g.right),
        crate::math::narrow(g.top),
        crate::math::narrow(g.bottom),
        len,
    ) {
        return true;
    }
    for &i in &w.fighter(f).platforms.stage_platforms {
        let p = &w.stage.stage_platforms[i as usize];
        if surface_blocked(x, oy, sphere, p.left, p.right, p.top, p.bottom, len) {
            return true;
        }
    }
    let mut i = 0;
    while let Some(&p) = w.fighter(f).platforms.platforms.get(i) {
        i += 1;
        let near = sphere.then_some(C::SURFACE_SPHERE_RADIUS);
        let Some(s) = body::trace_span(w, p, x, near) else { continue };
        if surface_blocked(x, oy, sphere, s.left, s.right, s.top, s.bottom, len) {
            return true;
        }
    }
    false
}

/// `surfaceSfx`: the physmat switch, the land intensity and the post (a kept sound with
/// `key`). `x`: the trace's origin, the fighter's place by default.
pub fn surface_sfx(
    w: &mut World,
    f: FighterId,
    ev: Name,
    oy: Real,
    sphere: bool,
    key: Option<SoundKey>,
    x: Option<Real>,
) {
    let x = x.unwrap_or(w.fighter(f).body.x);
    let hit = ground_trace(w, f, x, oy, sphere, C::SURFACE_LINE_LENGTH);
    let fighter = fid(w, f);
    let (group, value) = (crate::name!("physmat"), if hit { crate::name!("flesh") } else { crate::name!("rock") });
    push(w, f, Event::Switch { fighter, group, value });
    let v = w.fighter(f).body.vel_clamped;
    rtpc(w, f, crate::name!("cha_foley_land_intensity"), v);
    match key {
        Some(key) => push(w, f, Event::SoundStart { fighter, ev, key }),
        None => sfx(w, f, Some(ev)),
    }
}

/// `sfx`: an AkEvent the class defaults leave None (Pawn_Reindog's audio container has no
/// VO_Jump / VO_Land / VO_Evade / VO_Death, nor its parent's) posts nothing.
pub fn sfx(w: &mut World, f: FighterId, ev: Option<Name>) {
    let Some(ev) = ev.filter(|e| !e.is_none()) else { return };
    sound(w, f, ev, None);
}

// ---- vo

/// `selectVO`: Comp_Actor_VO.pfgPlayVO's line search (@312..3266). Rows whose Alias equals the
/// alias (case-insensitive; the _mature variants need the mature language setting, off) qualify when untagged, or
/// when every '|' tag is in the tag set (Array_Find on FString: FStrProperty Identical 1415a7a60 ignores the case of
/// ASCII letters); only the highest Priority is kept. Array_Random picks a line, re-rolled up to twice more while it
/// equals MostRecentVOLineIndex. The row's index, -1 when no row qualifies.
pub fn select_vo(
    rows: &[VoRow],
    alias: &str,
    tags: &[&str],
    recent: Real,
    mut rand_int: impl FnMut(Real) -> Real,
) -> i32 {
    let mut best = 0.0;
    let mut valid: Vec<usize> = Vec::new();
    for (i, (al, tg, pr, _, _)) in rows.iter().enumerate() {
        if !lower_eq(al, alias) {
            continue;
        }
        if !tg.is_empty()
            && !tg.split('|').filter(|t| !t.is_empty()).all(|t| tags.iter().any(|s| s.eq_ignore_ascii_case(t)))
        {
            continue;
        }
        if *pr > best {
            best = *pr;
            valid.clear();
        }
        if *pr == best && !valid.contains(&i) {
            valid.push(i);
        }
    }
    if valid.is_empty() {
        return -1;
    }
    let len = valid.len() as Real;
    let at = |r: Real| valid[index(r).expect("a pick within the rows")];
    let mut pick = at(rand_int(len));
    let mut k = 1;
    while k < 3 && pick as Real == recent {
        pick = at(rand_int(len));
        k += 1;
    }
    i32::try_from(pick).expect("a row index")
}

// ---- skin audio

/// `skinAudio`: a character's skin audio (bound from the build at the import:
/// `CharData::skin_audio`); `None` for a key with no table (a data-only key).
// The tables are not entered per key as a character loads; they are the
// bound data's, so there is nothing to enter.
pub fn skin_audio(w: &World, key: Name) -> Option<&SkinAudio> {
    w.data.chars.get(w.names.str(key)).and_then(|c| c.skin_audio.as_ref())
}

/// `appliedSkinAudio`: the applied skin's entry of a character: the picked skin
/// (MatchPlayer.skin), else the port's default (`FighterData.skin.name`). `None` for a key with no table or a skin
/// the table lacks (warned: no default for an unknown name; the default skin's audio is used then). An asset name is
/// an FName, compared without case: LeBron's skin data names its default c016, the asset is C016.
pub fn applied_skin_audio<'a>(
    w: Option<&World>,
    key: &'a CharData,
    data: &CharData,
    skin: Option<&str>,
) -> Option<&'a mvs_data::tables::SkinAudioSkin> {
    let t = key.skin_audio.as_ref()?;
    let name = skin.filter(|s| !s.is_empty()).unwrap_or(&data.fighter.skin.name);
    let s = t.skins.get(name).or_else(|| t.skins.iter().find(|(k, _)| lower_eq(k, name)).map(|(_, v)| v));
    if s.is_none() {
        let msg = format!("skin audio: {} has no skin {name}", key.key);
        match w {
            Some(w) => common::warn_text(w, &msg),
            // Outside a match (a preview actor): no World to keep the warned set.
            None => eprintln!("{msg}"),
        }
    }
    s
}

/// `replaceIgnoreCase`: FString::Replace with SearchCase 1: every occurrence of `from` in
/// `src`, left to right without overlap, letters compared without case, `to` written as given; an empty `from` (or
/// `src`) gives `src`.
pub fn replace_ignore_case(src: &str, from: &str, to: &str) -> String {
    if from.is_empty() || src.is_empty() {
        return src.to_owned();
    }
    let s = src.to_lowercase();
    let fl = from.to_lowercase();
    // A lower-case copy keeps the byte offsets of an ASCII source, which every asset and event name is.
    if s.len() != src.len() {
        return src.to_owned();
    }
    let mut out = String::new();
    let mut at = 0;
    while let Some(i) = s[at..].find(&fl).map(|i| i + at) {
        out.push_str(&src[at..i]);
        out.push_str(to);
        at = i + fl.len();
    }
    if at == 0 {
        return src.to_owned();
    }
    out.push_str(&src[at..]);
    out
}

/// `swapSkinEvent`: PF_Skin_Manager for a fighter's post
/// (BPI_GetDefaultAndAppliedAudioSkin, BaseFighter @13151: the default skin's AudioName and the applied skin's;
/// Conv_ObjectToString gives the event's name), then the post by that name, which Wwise answers with 0 when the
/// name's event is not loaded, and the caller then posts the event itself (PF_Post_Event @577-625,
/// Post_VO_On_AkComponent @351-399): `loaded(name)` says whether the renamed event would post; the original name
/// otherwise. A name the swap leaves as it is, or changes only in case (Tom and Jerry's TomandJerry / TomAndJerry:
/// Wwise lower-cases the name for its id), is the original.
pub fn swap_skin_event(ev: &str, default_name: &str, applied: &str, loaded: impl Fn(&str) -> bool) -> String {
    let r = replace_ignore_case(ev, default_name, applied);
    if r == ev {
        return r;
    }
    let low = r.to_lowercase();
    if low != ev.to_lowercase() && loaded(&low) { r } else { ev.to_owned() }
}

/// The VO data of a fighter (InitVOData): the applied skin's table and tags.
#[derive(Clone, Copy, Debug)]
pub struct SkinVo<'a> {
    pub rows: Option<&'a [VoRow]>,
    pub ally: &'a str,
    pub enemy: &'a str,
}

/// `voOfSkin`: a character's VO data in a skin (`applied_skin_audio`'s pick); `None`: no
/// table, or a skin the table lacks.
fn vo_of_skin<'a>(w: Option<&World>, key: &'a CharData, data: &'a CharData, skin: Option<&str>) -> Option<SkinVo<'a>> {
    let s = applied_skin_audio(w, key, data, skin)?;
    let t = key.skin_audio.as_ref()?;
    let rows = t.vo.get(&s.vo).map(Vec::as_slice).or(data.fighter.vo.as_deref());
    Some(SkinVo { rows, ally: &s.ally, enemy: &s.enemy })
}

/// `skinVO`: the VO data of fighter `f` from its player's skin; a minion (no player of the
/// set-up) and a fighter of a key with no skin audio keep the character's.
fn skin_vo_in<'a>(data: &'a GameData, w: &World, f: FighterId) -> Option<SkinVo<'a>> {
    let id = usize::try_from(w.fighter(f).id).ok()?;
    if id >= w.players.len() {
        return None;
    }
    let p = w.setup.players.get(id)?;
    let key = data.chars.get(p.character.as_deref().unwrap_or(crate::world::DEFAULT_CHARACTER))?;
    let own = data.chars.get(w.names.str(w.fighter(f).ch))?;
    vo_of_skin(Some(w), key, own, p.skin.as_deref())
}

/// `skinVoData`: the VO data of a character in a skin outside a match (a preview actor's
/// InitVOData from the applied skin, Comp_Pawn_SkinManager @3199: the pre-match voices, the podium, the results
/// screen's victory line): the skin's table and tags, else the character's. `skin`: its SkinData name, `None` or
/// empty the port's default.
pub fn skin_vo_data<'a>(key: &'a CharData, data: &'a CharData, skin: Option<&str>) -> SkinVo<'a> {
    let v = vo_of_skin(None, key, data, skin);
    SkinVo {
        rows: Some(v.and_then(|v| v.rows).or(data.fighter.vo.as_deref()).unwrap_or(&[])),
        ally: v.map_or(&data.fighter.vo_tags.ally, |v| v.ally),
        enemy: v.map_or(&data.fighter.vo_tags.enemy, |v| v.enemy),
    }
}

/// `voRows`.
fn vo_rows_in<'a>(data: &'a GameData, w: &World, f: FighterId) -> Option<&'a [VoRow]> {
    match skin_vo_in(data, w, f) {
        Some(v) => v.rows,
        None => data.chars.get(w.names.str(w.fighter(f).ch))?.fighter.vo.as_deref(),
    }
}

/// `voRows`: the rows of fighter `f`'s VO table.
pub fn vo_rows(w: &World, f: FighterId) -> Option<&[VoRow]> {
    vo_rows_in(&w.data, w, f)
}

/// `voTagAlly`.
pub fn vo_tag_ally(w: &World, f: FighterId) -> &str {
    vo_tag_in(&w.data, w, f, true)
}

/// `voTagEnemy`.
pub fn vo_tag_enemy(w: &World, f: FighterId) -> &str {
    vo_tag_in(&w.data, w, f, false)
}

/// `voTagAlly` (`ally`) or `voTagEnemy` over the shared data, so the tag outlives a borrow of the World.
fn vo_tag_in<'a>(data: &'a GameData, w: &World, f: FighterId, ally: bool) -> &'a str {
    if let Some(v) = skin_vo_in(data, w, f) {
        return if ally { v.ally } else { v.enemy };
    }
    let d = &data.chars.get(w.names.str(w.fighter(f).ch)).expect("the fighter's character data").def;
    if ally { &d.vo_tag_ally } else { &d.vo_tag_enemy }
}

// ---- tutorial

impl TutorialEvent {
    /// The event of byte `b` (TutorialEventTypes); a byte past the enum's is None.
    pub fn from_byte(b: i32) -> TutorialEvent {
        usize::try_from(b).ok().and_then(|i| TUTORIAL_EVENTS.get(i)).copied().unwrap_or(TutorialEvent::None)
    }

    /// The event's byte.
    pub fn byte(self) -> i32 {
        self as i32
    }
}

/// TutorialEventTypes by byte.
const TUTORIAL_EVENTS: [TutorialEvent; 90] = {
    use TutorialEvent as T;
    [
        T::None,
        T::NeutralNormalHit,
        T::DownNormalHit,
        T::UpNormalHit,
        T::ForwardNormalHit,
        T::NeutralSpecialHit,
        T::DownSpecialHit,
        T::UpSpecialHit,
        T::ForwardSpecialHit,
        T::NeutralAerialNormalHit,
        T::DownAerialNormalHit,
        T::UpAerialNormalHit,
        T::ForwardAerialNormalHit,
        T::NeutralsAerialSpecialHit,
        T::DownAerialSpecialHit,
        T::UpAerialSpecialHit,
        T::ForwardAerialSpecialHit,
        T::Jump,
        T::HoldJump,
        T::DoubleJump,
        T::WallJump,
        T::LedgeSlide,
        T::RingOut,
        T::FastFall,
        T::ChargeUp,
        T::ComboChain,
        T::Equip,
        T::Unequip,
        T::CustomActorEvent,
        T::NeutralNormalUse,
        T::DownNormalUse,
        T::UpNormalUse,
        T::ForwardNormalUse,
        T::NeutralSpecialUse,
        T::DownSpecialUse,
        T::UpSpecialUse,
        T::ForwardSpecialUse,
        T::NeutralAerialNormalUse,
        T::DownAerialNormalUse,
        T::UpAerialNormalUse,
        T::ForwardAerialNormalUse,
        T::NeutralAerialSpecialUse,
        T::DownAerialSpecialUse,
        T::UpAerialSpecialUse,
        T::ForwardAerialSpecialUse,
        T::MoveRight,
        T::MoveLeft,
        T::Crouch,
        T::Up,
        T::DropThroughPlatform,
        T::EvadeThroughPlatform,
        T::Evade,
        T::EvadeGroundNeutral,
        T::EvadeGroundRight,
        T::EvadeGroundLeft,
        T::EvadeAirNeutral,
        T::EvadeAirUp,
        T::EvadeAirUpRight,
        T::EvadeAirRight,
        T::EvadeAirDownRight,
        T::EvadeAirDown,
        T::EvadeAirDownLeft,
        T::EvadeAirLeft,
        T::EvadeAirUpLeft,
        T::EvadeGroundToAirUp,
        T::EvadeGroundToAirUpRight,
        T::EvadeGroundToAirUpLeft,
        T::EvadeAir,
        T::ArmorBreak,
        T::FastFallJump,
        T::EvadeJump,
        T::UsedUpAllAirMoves,
        T::UniqueCharacterMechanic0,
        T::UniqueCharacterMechanic1,
        T::UniqueCharacterMechanic2,
        T::UniqueCharacterMechanic3,
        T::UniqueCharacterMechanic4,
        T::UniqueCharacterMechanic5,
        T::DodgeJumpCancel,
        T::FastfallDuringAttack,
        T::MaximumAttackDecay,
        T::NoAttackDecay,
        T::AnyAttackDecay,
        T::DodgeSuccess,
        T::DodgeMeterEmpty,
        T::DodgeMeterFull,
        T::KBILeft,
        T::KBIRight,
        T::KBIUp,
        T::ElementalFireBuff,
    ]
};

/// `tutorialWorld`: whether a tutorial runs on the fighter's World (`World.tutorial`
/// set).
pub fn tutorial_world(w: &World, _f: FighterId) -> bool {
    crate::modes::tutorial::running(w)
}

/// `tutorialEventOf`: BPI_ProcessTutorialEvent from a fighter or one of its components.
pub fn tutorial_event_of(w: &mut World, _f: FighterId, ev: TutorialEvent) {
    flow::tutorial_event(w, ev);
}

/// HandleAirMoveDesaturation (BaseFighter, after the double jump @8383, the air special's adds @31055 / @99334 and the
/// air evade's @43591; `airMovesUsedTutorial`, for the tutorial event): with
/// AirSpecialNumber == AirSpecialMax, CurrentJumpNumber == MaxJumpNumber and AirEvadeNumber == AirEvadeMax,
/// CharacterMaterialSaturation(0.7) @204 (ubergraph @143877: Mesh.SetScalarParameterValueOnMaterials(Diffuse_Mult);
/// only BatmanBase_MAT, 0_C020_MAT and Creature_Mat have the parameter), then UsedUpAllAirMoves (@323) in a
/// tutorial. ResetJump sets it back to 1.0 ([`air_saturation_reset`]).
// Deviation: the 0.7 is sent only when the mesh is not already at it (`AirControl::desaturated`); the build sets it
// again at each evaluation that passes, which draws the same.
pub fn handle_air_move_desaturation(w: &mut World, f: FighterId) {
    let fm = w.fighter(f);
    if crate::real(fm.air.air_specials) == fighter::air_special_max(w, f)
        && crate::real(fm.jump.jumps) == moves::max_jumps(w, f)
        && crate::real(fm.air.air_evades) == C::AIR_EVADE_MAX
    {
        if !w.fighter(f).air.desaturated {
            w.fighter_mut(f).air.desaturated = true;
            control::mat_scalar(w, f, crate::name!("Diffuse_Mult"), 0.7);
        }
        if tutorial_world(w, f) {
            flow::tutorial_event(w, TutorialEvent::UsedUpAllAirMoves);
        }
    }
}

/// ResetJump's CharacterMaterialSaturation(1.0) (BaseFighter @2256; SetJumpData's @21424 goes there too).
// Deviation: sent only when HandleAirMoveDesaturation left the mesh at 0.7; the build sets 1.0 at every ResetJump,
// which draws the same.
pub fn air_saturation_reset(w: &mut World, f: FighterId) {
    if w.fighter(f).air.desaturated {
        w.fighter_mut(f).air.desaturated = false;
        control::mat_scalar(w, f, crate::name!("Diffuse_Mult"), 1.0);
    }
}

/// The move input's call: the MoveX call's MoveLeft for MoveXAxisValue < 0 (@127368), MoveRight for > 0 (@127548);
/// then MoveY's: Up for MoveYAxisValue > 0 (@128789). `x_event` / `y_event`: the frame made that call.
// Unverified: a bot's MoveX / MoveY calls of one frame are one call here.
pub fn move_axis_tutorial(w: &mut World, f: FighterId, x_event: bool, y_event: bool) {
    if !((x_event || y_event) && tutorial_world(w, f)) {
        return;
    }
    let inp = w.fighter(f).controls.inp;
    if x_event && inp.x != 0.0 {
        let ev = if inp.x < 0.0 { TutorialEvent::MoveLeft } else { TutorialEvent::MoveRight };
        tutorial_event_of(w, f, ev);
    }
    if y_event && inp.y > 0.0 {
        tutorial_event_of(w, f, TutorialEvent::Up);
    }
}

/// The Hit bytes by EActionDirection (0 Up, 1 Down, 2 Forward, 3 Back, 4 Neutral, 5 Default, 6 ForwardUp,
/// 7 ForwardDown, 8 BackUp, 9 BackDown); a Use byte is its Hit byte + 28. BaseFighter's free attacks (the switches
/// ahead of @28155, @31296, @58279, @65482): ground normal, air normal, ground special, air special.
const FREE_HIT: [[i32; 10]; 4] = [
    [3, 2, 4, 4, 1, 1, 3, 2, 3, 2],
    [11, 10, 12, 12, 9, 9, 11, 10, 11, 10],
    [7, 6, 8, 8, 5, 5, 7, 6, 7, 6],
    [15, 14, 16, 16, 13, 13, 15, 14, 15, 14],
];
const USE_OFFSET: i32 = TutorialEvent::NeutralNormalUse as i32 - TutorialEvent::NeutralNormalHit as i32;

/// `FREE_HIT[t][dir] ?? 0`.
fn free_hit(t: usize, dir: Real) -> i32 {
    index(dir).and_then(|i| FREE_HIT[t].get(i)).copied().unwrap_or(0)
}

/// `freeAttackTutorial`: BaseFighter's free attack start (the attack table's montage
/// played): LastAttackTutorialEvent = the Hit byte (@58279 ground normal, @65482 air normal, @28155 ground special,
/// @31296 air special), then BPI_ProcessTutorialEvent(the Use byte) (@58822, @66025, @28698, @31839). `air`: the
/// table the press took, `dir`: the press's direction.
// Deviation: LastAttackTutorialEvent is written only while a tutorial runs; its other readers (the quests) are not
// ported, so outside a tutorial the write is unobservable.
pub fn free_attack_tutorial(w: &mut World, f: FighterId, special: bool, air: bool, dir: Real) {
    if !tutorial_world(w, f) {
        return;
    }
    let hit = free_hit(usize::from(special) * 2 + usize::from(air), dir);
    w.fighter_mut(f).tutorial.last_attack_tutorial_event = Some(TutorialEvent::from_byte(hit));
    flow::tutorial_event(w, TutorialEvent::from_byte(hit + USE_OFFSET));
}

/// EActionDirection's bytes by name.
fn dir_byte(s: &str) -> Option<i32> {
    Some(match s {
        "AD_Up" => 0,
        "AD_Down" => 1,
        "AD_Forward" => 2,
        "AD_Back" => 3,
        "AD_Neutral" => 4,
        "AD_Default" => 5,
        "AD_ForwardUp" => 6,
        "AD_ForwardDown" => 7,
        "AD_BackUp" => 8,
        "AD_BackDown" => 9,
        _ => return None,
    })
}

/// `branchRowTutorial`: the branch row's event (`branch_attack_tutorial`), its bytes
/// read only in a tutorial.
pub fn branch_row_tutorial(w: &mut World, f: FighterId, button: &str, direction: &str) {
    if !tutorial_world(w, f) {
        return;
    }
    let (b, d) = branch_row_bytes(button, direction);
    branch_attack_tutorial(w, f, b, d);
}

/// `branchRowBytes`: a branch row's button and direction (EActionButton /
/// EActionDirection names) as bytes: 0 normal, 1 special, -1 for any other button (the taunt's row).
fn branch_row_bytes(button: &str, direction: &str) -> (Real, Real) {
    let b = common::strip_enum(button);
    let b = match b {
        "AB_BasicAttack" | "AB_HeavyAttack" => 0.0,
        "AB_SpecialAttack" => 1.0,
        _ => -1.0,
    };
    (b, crate::real(dir_byte(common::strip_enum(direction)).unwrap_or(5)))
}

/// `branchAttackTutorial`: Comp_Pawn_BranchManager HandleBranchedAttackTutorialEvents
/// (Trigger Current Attack Client @8689, after AddRecentAttackToQueue): the triggering row's button and
/// RequiredDirectionInput pick the byte from the free tables, except Default, which gives 0 (None);
/// CharacterMovement.IsFalling picks air or ground. LastAttackTutorialEvent = the Hit byte (@692 normal, @4145
/// special), BPI_ProcessTutorialEvent(the Use byte) (@2833 / @6286), then ComboChain for a Forward normal row
/// (@3409). `button` -1 (a taunt): nothing.
// Deviation: the write is gated as freeAttackTutorial's.
pub fn branch_attack_tutorial(w: &mut World, f: FighterId, button: Real, dir: Real) {
    if button != 0.0 && button != 1.0 {
        return;
    }
    if !tutorial_world(w, f) {
        return;
    }
    let air = moves::is_falling(w, f);
    let hit = if dir == 5.0 { 0 } else { free_hit(if button == 1.0 { 2 } else { 0 } + usize::from(air), dir) };
    w.fighter_mut(f).tutorial.last_attack_tutorial_event = Some(TutorialEvent::from_byte(hit));
    flow::tutorial_event(w, TutorialEvent::from_byte(if hit == 0 { 0 } else { hit + USE_OFFSET }));
    if button == 0.0 && dir == 2.0 {
        flow::tutorial_event(w, TutorialEvent::ComboChain);
    }
}

/// The evade bytes by Get8WayDirectionNoFacing (4 for a neutral evade press): BaseFighter's evade start, air
/// (EvadeAir's table), ground to air and plain ground.
const EVADE_AIR: [i32; 10] = [56, 60, 58, 62, 55, 55, 57, 59, 63, 61];
const EVADE_GTA: [i32; 10] = [64, 52, 53, 54, 52, 52, 65, 53, 66, 54];
const EVADE_GROUND: [i32; 10] = [52, 52, 53, 54, 52, 52, 53, 53, 54, 54];

/// `evadeTutorialByte`.
pub fn evade_tutorial_byte(kind: EvadeTutorialByteKind, d: Real) -> TutorialEvent {
    let t = match kind {
        EvadeTutorialByteKind::Air => &EVADE_AIR,
        EvadeTutorialByteKind::Gta => &EVADE_GTA,
        EvadeTutorialByteKind::Ground => &EVADE_GROUND,
    };
    TutorialEvent::from_byte(index(d).and_then(|i| t.get(i)).copied().unwrap_or(0))
}

// ---- ally

/// `allyProps`: the pawn's Comp_Pawn_TeleportToAlly.
fn ally_props(w: &World, f: FighterId) -> &Props {
    pawn_comp(common::data_of(w, f), "Comp_Pawn_TeleportToAlly_GEN_VARIABLE").unwrap_or(&NO_PROPS)
}

/// The distance of two fighters' actor locations (float).
fn ally_distance(w: &World, a: FighterId, b: FighterId) -> Real {
    let (ax, bx) = (w.fighter(a).body.x, w.fighter(b).body.x);
    ue::size2d([bx - ax, fighter::actor_y(w, b) - fighter::actor_y(w, a)])
}

/// GetAllies' entry test: another player of the team, not waiting for its respawn.
fn is_ally(w: &World, f: FighterId, o: FighterId) -> bool {
    let (a, b) = (w.fighter(f), w.fighter(o));
    o != f && b.team.team == a.team.team && b.life.dead <= 0.0 && !b.life.is_minion
}

/// `allyNotWithinRange`: AllyNotWithinRangeCondition.Check (@5-1466): AllyNotWithinRange =
/// false, then for every valid entry of the owner's Comp_Actor_Destructible.GetAllies: AllyNotWithinRange =
/// GetDistanceTo(owner) >= 2250 (the last one stays) and Array_Add to the condition's ListOfAllies; the result is
/// ListOfAllies empty OR AllyNotWithinRange.
// Deviation: ListOfAllies is never cleared and the condition object is the montage notify's instance, so in the
// build a check without allies answers false once any earlier check (of any match of the session) had one; taken
// here as empty at every check. With two or more allies the last one of GetAllies (`common::allies`' order) answers.
pub fn ally_not_within_range(w: &World, f: FighterId) -> bool {
    common::last_ally(w, f).is_none_or(|o| ally_distance(w, f, o) >= 2250.0)
}

/// `alliesWithinDashDistance`: AlliesWithinDashDistance.Check: false without the owner's
/// Comp_Actor_Destructible (@5-85) or its Comp_Pawn_TeleportToAlly (@169-249), or with GetAllies empty (@333-471);
/// else true at the first ally with Vector_Distance(ally location, owner location) <= the component's DashDistance
/// (@752-1027, inclusive, a break at the first), false when none is (@1043). No IsValid on an entry.
/// GetAllies returns the component's Allies as they stand (Comp_Actor_Destructible GetAllies @0). The list is kept by
/// the component: every player of the owner's TeamIndex with a valid pawn other than the owner is added at BeginPlay
/// (after a Delay(0), @964 -> @132-558 -> @1821-2269) and at the game mode's player-respawned broadcast
/// (@1420-1796), and removed at its player-knocked-out broadcast (@1019-1395 -> @2338). So a knocked-out ally is not
/// in the list while it waits: left out here as in `ally_not_within_range`.
/// DashDistance's class default is 1500 (Comp_Pawn_TeleportToAlly CDO; every exported instance serializes 2250).
// Unverified: the allies in the sim's order (the build's is GetAllPlayers order, then respawn order: any ally in
// range answers the same); the removal needs the loser's pawn still valid at the knock-out broadcast (@1959), which
// is native order, not read: a stale entry would be measured where the pawn was.
pub fn allies_within_dash_distance(w: &World, f: FighterId) -> bool {
    let Some(comp) = common::data_of(w, f).fighter.components.pawn.get("Comp_Pawn_TeleportToAlly_GEN_VARIABLE") else {
        return false;
    };
    let d = comp.p.as_obj().and_then(|p| num(p, "DashDistance")).unwrap_or(1500.0);
    common::peers(w).iter().any(|&o| is_ally(w, f, o) && ally_distance(w, f, o) <= d)
}

/// The place of a spawned actor in the sim (`a.x`), `None` for a class the sim gives none.
fn spawned_x(w: &World, a: ActorRef) -> Option<Real> {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|p| p.x),
        ActorRef::Item(id) => w.actors.items.get(id).map(|p| p.x),
        ActorRef::Actor(id) => crate::actors::world_actor::world_actor_x(w, id),
        ActorRef::Marker(id) => w.actors.markers.get(id).map(|p| p.x),
        ActorRef::Ship(id) => w.actors.ships.get(id).map(|p| p.x),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).map(|p| p.x),
        ActorRef::AimPunch(id) => w.actors.aim_punches.get(id).map(|p| p.x),
        ActorRef::Accelerator(id) => w.actors.accelerators.get(id).map(|p| p.x),
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => None,
    }
}

/// A spawned actor's `dead` and `cls`; `None` for one gone from its arena.
fn spawned_state(w: &World, a: ActorRef) -> Option<(bool, Name)> {
    match a {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|p| (p.dead, p.cls)),
        ActorRef::Item(id) => w.actors.items.get(id).map(|p| (p.dead, p.cls)),
        ActorRef::Actor(id) => w.actors.actors.get(id).map(|p| (p.dead, p.cls)),
        ActorRef::Marker(id) => w.actors.markers.get(id).map(|p| (p.dead, p.cls)),
        ActorRef::Ship(id) => w.actors.ships.get(id).map(|p| (p.dead, p.cls)),
        ActorRef::Mortar(id) => w.actors.mortars.get(id).map(|p| (p.dead, p.cls)),
        ActorRef::AimPunch(id) => w.actors.aim_punches.get(id).map(|p| (p.dead, p.cls)),
        ActorRef::Accelerator(id) => w.actors.accelerators.get(id).map(|p| (p.dead, p.cls)),
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => None,
    }
}

/// `faceTowards`: AnimNotify_FaceTowards (Enum_AimTowards bytes from the asset: 0 None,
/// 1 Ally, 2 Closest Enemy, 3 Farthest Enemy, 4 FirstSpawned, 5 LastSpawned; the data exports Target as the byte).
/// Received_NotifyBegin, a switch on Target (@0-225): 1 GetAlly, 2 and 3 GetEnemy, 4 and 5 GetSpawned, anything else
/// returns; a target that is not IsValid does nothing; else RotateOwner(owner, target).
/// GetAlly (@5-1055): the first BP_FighterCharacter (GetAllActorsOfClass) of the owner's TeamIndex that is not the
/// owner and not a NotPlayerCharacter; with UseMaxDistance none when it is further than MaxDistance
/// (IsWithinDistance: Vector_Distance of the actor locations <= MaxDistance), without trying the next.
/// GetEnemy (@5-1047): the same loop with a TeamIndex that differs (@732). "Closest" and "Farthest" are one graph and
/// compare no distance. With UseMaxDistance IsWithinDistance is called @1004 but the test @1045 is on the literal
/// true: the distance never rejects an enemy.
/// GetSpawned (@5-732): the owner's Comp_Spawner "Get Spawned Actor"(SpawnedActorClass, AllObjects false, LastObject
/// = Target == 5, FirstObject = Target == 4) @118, which gives one entry: the first or the last of SpawnedActors
/// filtered by the class (FilterArray @95: a child class passes), of the whole list without a valid class (@955), an
/// empty item when there is none. Without UseMaxDistance entry 0 (@594); with it the first entry within the distance
/// and, when none is, the entry at the last tested index (@673-732): of a list of one that is the same entry, so the
/// distance never rejects a spawned actor either.
/// RotateOwner @158-430: BPI_SetFacingLogic(target Y - owner Y >= 0) (the argument is "facing left"), i.e. left when
/// the target is not at a greater sim x. @476-934: Mesh.K2_SetWorldRotation(FindLookAtRotation(owner, the target's
/// location on the aimed axes)). Received_NotifyEnd @121: Mesh.K2_SetRelativeRotation((0, 0, 0))
/// (`face_towards_end`).
/// SpawnedActors is `Items::spawned_actors` (the world appends at a spawn, an actor counts until it is marked dead,
/// as World.pickSpawned); an actor without a place warns.
// Deviation: the mesh's world rotation (the pose and the hurt shapes turn with it) is not ported, so the end has
// nothing to put back; the fighters in the sim's order, one waiting for its respawn left out.
pub fn face_towards(w: &mut World, f: FighterId, n: NotifyRef) {
    if let Some(x) = face_towards_x(w, f, notify_props(w, f, n)) {
        fighter::set_facing(w, f, if x <= w.fighter(f).body.x { -1.0 } else { 1.0 });
    }
}

/// The target's x of `face_towards`, `None` for no target.
fn face_towards_x(w: &World, f: FighterId, p: &Props) -> Option<Real> {
    let t = match p.get("Target") {
        None | Some(Value::Null) => 0.0,
        Some(v) => match v.as_f64() {
            Some(t) => t,
            None => {
                common::warn_text(w, &format!("FaceTowards: Target is not a byte ({v:?})"));
                return None;
            }
        },
    };
    if t == 1.0 || t == 2.0 || t == 3.0 {
        let team = w.fighter(f).team.team;
        let o = common::peers(w).iter().copied().find(|&v| {
            let vf = w.fighter(v);
            // @458 / @652.
            v != f && (vf.team.team == team) == (t == 1.0) && vf.life.dead <= 0.0 && !vf.life.not_player_character
        })?;
        if t == 1.0
            && flag(p, "UseMaxDistance")
            && !matches!(
                ally_distance(w, f, o).partial_cmp(&num(p, "MaxDistance").unwrap_or(0.0)),
                Some(Ordering::Less | Ordering::Equal)
            )
        {
            return None;
        }
        Some(w.fighter(o).body.x)
    } else if t == 4.0 || t == 5.0 {
        let cls = p.get("SpawnedActorClass").and_then(Value::as_str).filter(|s| !s.is_empty());
        let mut s: Option<(ActorRef, Name)> = None;
        for &a in &w.fighter(f).items.spawned_actors {
            let Some((dead, acls)) = spawned_state(w, a) else { continue };
            if dead {
                continue;
            }
            if let Some(c) = cls {
                let an = w.names.str(acls);
                if an != c && !w.registry.actors.get(an).is_some_and(|e| e.parents.contains(&c)) {
                    continue;
                }
            }
            if t == 5.0 || s.is_none() {
                s = Some((a, acls));
            }
        }
        let (a, acls) = s?;
        let x = spawned_x(w, a);
        if x.is_none() {
            common::warn_text(
                w,
                &format!("FaceTowards: the spawned actor has no place in the sim ({})", w.names.str(acls)),
            );
        }
        x
    } else {
        None
    }
}

/// `faceTowardsEnd`: AnimNotify_FaceTowards Received_NotifyEnd (@0-199): the owner's
/// Mesh.K2_SetRelativeRotation((0, 0, 0)), whether the begin found a target or not (a failed cast @107 falls on the
/// same statement). The facing stays.
// Deviation: the sim holds no mesh rotation (the begin's K2_SetWorldRotation is not ported), so there is nothing to
// clear; the port of the begin's aim brings this with it (sim for the hurt shapes, render for the pose).
pub fn face_towards_end(_w: &World, _f: FighterId) {}

/// `teleportToAlly`: TeleportToAlly (ubergraph 6472): Allies cleared, ReachedAlly = false;
/// Allies = the BP_FighterCharacters overlapping the first BlastBoxPlayerExit actor that have the owner's TeamIndex,
/// are not the owner and not a NotPlayerCharacter (@4754-6194). None (@6199): MulticastMoveSuccess(false), NoAlly =
/// true, Delay(0) -> Shield. Else CurrentClosest = the one of the least squared distance (strict <, from 1e12: the
/// first of a tie, @1366-2279); GetDistanceTo it not < DashDistance (@2284-2541): Delay(0) -> Shield. In range:
/// DashMovement (@6758: DashedVFX = SpawnEmitterAttached(DashVFX, Mesh, Root) with the Team parameter; its
/// PlayMontage(null, AirDash) has no mesh and plays nothing), EndRotation = the actor's rotation,
/// SpawnBuffActor(Buff_MoveToPosition, owner, owner, 0, 0.0, true, false) and on it MulticastBeginMove(the ally's
/// location + (X, FacingLeft ? +Y : -Y, Z) of TeleportOffset, false, false, MoveToTime) (@2996-3591); then
/// K2_SetActorRotation(EndRotation) (no change), BPI_IncrementCoopActions, Delay(0) -> Shield (@26-455), and
/// Delay(MoveToTime) -> ReachedAlly = true (@3657).
/// Buff_MoveToPosition: BeginMove (@969-1685) calls RootMotion_ForceMoveToPoint(None, the destination with X 0,
/// OverTime, LaunchAtEnd ? SetVelocity : ClampVelocity, EndingLaunchVector, 0.0, no path curve, no time curve
/// without an ease, not additive), keeps the id and Delay(OverTime) -> K2_DestroyActor; ReceiveEndPlay (@468)
/// removes the source by id; OnOwnerKnockedBack destroys the actor (receiveHit). Its class defaults: LaunchAtEnd
/// off, EndingLaunchVector 0, CanOverride on, state group Move, breaking Move and Held, no blockers.
/// The Delays: one started by the mesh tick's notify is taken to count that frame (the latent actions of an object
/// run after the mesh tick, in its own tick or the world's pass), one started by a resumed Delay counts from the next
/// frame. So with the notify in frame F: Shield in F, the source active from the CMC tick of F + 1, ReachedAlly and
/// the buff's end in F + 12 (0.21 s = 13 counts), the source removed and the velocity clamped by the CMC tick of
/// F + 13, after 12 of the 12.6 ticks of the way, the launch (0, 0, 1) in the pass of F + 13 and taken by the CMC
/// tick of F + 14.
/// Buff_MoveToPosition's graph (BeginMove, the Delay -> K2_DestroyActor, ReceiveEndPlay, OnOwnerKnockedBack) is
/// `Ally::move_id` / `buff`, not a buff actor.
// Unverified: that frame count (no game read). Deviation: the allies in the sim's order and one waiting for its
// respawn left out; the buff is not in the buff list (no StatusUpdate, and a buff that breaks the Move group does
// not end it; the code reads no def); the coop action count is not kept; a spawn blocked by a state group (none of
// the ported buffs blocks Move) would return before the Delays.
pub fn teleport_to_ally(w: &mut World, f: FighterId) {
    w.fighter_mut(f).team.ally.reached = false;
    let (fx, fy, team) = (w.fighter(f).body.x, fighter::actor_y(w, f), w.fighter(f).team.team);
    let mut best = None;
    let mut bd = 1e12;
    for &o in common::peers(w) {
        let of = w.fighter(o);
        // @5013-5254.
        if o == f
            || of.team.team != team
            || of.life.dead > 0.0
            || (of.life.blast_in & 1) == 0
            || of.life.not_player_character
        {
            continue;
        }
        let (dx, dy) = (of.body.x - fx, fighter::actor_y(w, o) - fy);
        let d2 = dx * dx + dy * dy;
        if d2 < bd {
            bd = d2;
            best = Some(o);
        }
    }
    let pr = ally_props(w, f);
    let dash = num(pr, "DashDistance").unwrap_or(0.0);
    let t = num(pr, "MoveToTime").unwrap_or(1.0);
    let off = pr.get("TeleportOffset").and_then(Value::as_obj).unwrap_or(&NO_PROPS);
    let (oy, oz) = (num(off, "Y").unwrap_or(0.0), num(off, "Z").unwrap_or(0.0));
    if best.is_none() {
        w.fighter_mut(f).team.ally.no_ally = true;
    }
    let best = best.filter(|&b| ally_distance(w, f, b) < dash);
    let Some(best) = best else {
        let a = &mut w.fighter_mut(f).team.ally;
        if a.shield <= 0.0 {
            a.shield = 1.0;
        }
        return;
    };
    let tick = crate::real(w.fighter(f).clock.tick);
    w.fighter_mut(f).team.ally.vfx_born = tick;
    // CanOverride: the old actor goes first.
    end_move_buff(w, f);
    let facing = w.fighter(f).body.facing_dir;
    let (bx, by) = (w.fighter(best).body.x, fighter::actor_y(w, best));
    let id = moves::force_move_to_point(
        w,
        f,
        bx + if facing < 0.0 { -oy } else { oy },
        by + oz,
        t,
        2.0,
        0.0,
        0.0,
        0.0,
        false,
    );
    let best_id = crate::real(w.fighter(best).id);
    let a = &mut w.fighter_mut(f).team.ally;
    a.move_id = math::to_int32(id);
    a.buff = common::latent_frames(t);
    a.cast = best_id;
    if a.shield <= 0.0 {
        a.shield = 1.0;
    }
    if a.reach <= 0.0 {
        a.reach = common::latent_frames(t);
    }
}

/// `endMoveBuff`.
pub fn end_move_buff(w: &mut World, f: FighterId) {
    let id = w.fighter(f).team.ally.move_id;
    if id != 0 {
        moves::remove_force_move(w, f, id);
    }
    let a = &mut w.fighter_mut(f).team.ally;
    a.move_id = 0;
    a.buff = 0.0;
    a.safe = None;
}

/// `allyTransferTick`: the graph's pending Delays, after the frame's mesh tick. Shield
/// (@7697-8558): PlayMontage(Mesh, the montage, 'Montage Play Rate', 0.0); with UseNewMontageForNoAlly and NoAlly the
/// NoAlly montages by the ally's IsMovingOnGround, else GroundShield / AirShield by it (without
/// UseNewMontageForNoAlly a NoAlly picks them by the owner's). Then (@803-1053) Delay(MoveToTime) ->
/// LaunchCharacter((0, 0, 1), XY and Z override) -> Delay(0.5) -> DestroyVFX, on every path, also the ones without a
/// move.
// Deviation: the montage starts as an attack's does here (startMontage: a new AttackID and BeginAttack); the build
// plays it on the mesh without the BranchManager.
pub fn ally_transfer_tick(w: &mut World, f: FighterId) {
    {
        let fm = w.fighter_mut(f);
        let dead = fm.life.dead > 0.0;
        let a = &mut fm.team.ally;
        if a.vfx > 0.0 {
            a.vfx -= 1.0;
            if a.vfx == 0.0 {
                a.vfx_born = -1.0;
            }
        }
        if a.launch > 0.0 {
            a.launch -= 1.0;
            if a.launch == 0.0 {
                if !dead {
                    fm.moves.impulse = Some(moves::Impulse { vx: 0.0, vy: 1.0, xy: true, kind: 0, z: true, d: None });
                    // A direct LaunchCharacter, as CancelJump's.
                    fm.moves.hit_launch = true;
                }
                fm.team.ally.vfx = common::latent_frames(0.5);
            }
        }
        let a = &mut fm.team.ally;
        if a.reach > 0.0 {
            a.reach -= 1.0;
            if a.reach == 0.0 {
                a.reached = true;
            }
        }
    }
    let a = &w.fighter(f).team.ally;
    // Destroyed by its tick when it answers true.
    if !(a.safe.is_some() && a.buff > 0.0 && safe_move_tick(w, f)) {
        let a = &mut w.fighter_mut(f).team.ally;
        if a.buff > 0.0 {
            a.buff -= 1.0;
            if a.buff == 0.0 {
                if a.safe.is_some() {
                    safe_move_expired(w, f);
                } else {
                    end_move_buff(w, f);
                }
            }
        }
    }
    let a = &mut w.fighter_mut(f).team.ally;
    if a.shield.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return;
    }
    a.shield -= 1.0;
    if a.shield != 0.0 {
        return;
    }
    let (launch, no_ally, cast) = (a.launch, a.no_ally, a.cast);
    let pr = ally_props(w, f);
    let t = num(pr, "MoveToTime").unwrap_or(1.0);
    if launch <= 0.0 {
        w.fighter_mut(f).team.ally.launch = common::latent_frames(t);
    }
    if w.fighter(f).life.dead > 0.0 {
        return;
    }
    let o = if cast >= 0.0 { peer_by_id(w, math::to_int32(cast)) } else { None };
    let on_ground = o.is_some_and(|o| fighter::grounded(w, o));
    let pr = ally_props(w, f);
    let pick = |k: &str| pr.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    let name = if no_ally {
        if flag(pr, "UseNewMontageForNoAlly") {
            pick(if on_ground { "GroundNoAllyMontage" } else { "AirNoAllyMontage" })
        } else {
            pick(if fighter::grounded(w, f) { "GroundShield" } else { "AirShield" })
        }
    } else {
        pick(if on_ground { "GroundShield" } else { "AirShield" })
    };
    let Some(name) = name else { return };
    let rate = num(pr, "Montage Play Rate").unwrap_or(1.0);
    let name = common::lit(w, name);
    anim::start_montage(w, f, name, &0, None);
    if let Some(m) = w.fighter_mut(f).montages.montage.as_mut() {
        m.play_rate = f64::from(rate);
    }
}

// ---- safe move

/// `safeMoveBegin`: Buff_SafeMoveToPosition (Buffs/Buff_SafeMoveToPosition), a child of
/// Buff_MoveToPosition: the same force move on the same fields (`Ally::buff` the Delay, `move_id` the source), so a
/// test of Buff_MoveToPosition (CheckHasBuff finds a child; Batman's CancelMoveBuffOnEnd, `ally.buff > 0`) sees it,
/// and `Ally::safe` holds what the child adds: the Destination and MovementSpeed. Its class defaults:
/// BufferDistanceToCheckArrive 100, LaunchAtEnd off (finish ClampVelocity 0), CanOverride, DestroyOnKnockedBack
/// (`end_move_buff` at a hit), StateInfo Group 13, BreakGroups 13 and 12.
/// MulticastBeginMove(Destination, bEaseOut, bEaseIn, OverTime) -> BeginMove @1744-2127 (the parent's): MovementSpeed
/// = OverTime / Vector_Distance(owner, (0, Y, Z)) (Divide_FloatFloat: 0 for a distance of 0), Destination = (0, Y,
/// Z); @969-1685: RootMotion_ForceMoveToPoint(None, Destination, OverTime, LaunchAtEnd ? 1 : 2, EndingLaunchVector,
/// 0, no path curve, EaseInOutFloatCurve with both eases, not additive), RootMotionID kept, Delay(OverTime) ->
/// DurationExpired (virtual: the child's @1817). `frames`: the Delay's count as the caller's start makes it
/// (`latent_frames(over_time)` for one started in the frame's mesh tick or pawn graph, as `teleport_to_ally`'s).
// Deviation: no buff entry, so its StateInfo breaks nothing and blocks nothing (Buff_Caught's BlockerGroups [13]
// would refuse Caught while this move is on in the build), and the buff list has no entry for it.
pub fn safe_move_begin(
    w: &mut World,
    f: FighterId,
    dest_x: Real,
    dest_actor_y: Real,
    over_time: Real,
    ease: bool,
    frames: Option<Real>,
) {
    // CanOverride: an older Buff_MoveToPosition goes first.
    end_move_buff(w, f);
    let frames = frames.unwrap_or_else(|| common::latent_frames(over_time));
    safe_begin_move(w, f, dest_x, dest_actor_y, over_time, ease, frames);
}

fn safe_begin_move(
    w: &mut World,
    f: FighterId,
    dest_x: Real,
    dest_actor_y: Real,
    over_time: Real,
    ease: bool,
    frames: Real,
) {
    let d = ue::size2d([dest_x - w.fighter(f).body.x, dest_actor_y - fighter::actor_y(w, f)]);
    let t = over_time;
    w.fighter_mut(f).team.ally.safe =
        Some(AllySafe { x: dest_x, y: dest_actor_y, speed: if d == 0.0 { 0.0 } else { t / d } });
    let id = moves::force_move_to_point(w, f, dest_x, dest_actor_y, t, 2.0, 0.0, 0.0, 0.0, ease);
    let a = &mut w.fighter_mut(f).team.ally;
    a.move_id = math::to_int32(id);
    a.buff = frames;
}

/// The object type WorldStatic of the queries.
const OBJ_WORLD_STATIC: Real = 0.0;

/// `safeMoveTick`: ReceiveTick @25-1783 (authority, the owner a BP_FighterCharacter):
/// SphereTraceSingleForObjects from the owner's location to location + Normal(Destination - location) x 100, radius
/// 30, object types [WorldStatic], no actors to ignore but self (the buff). A hit whose HitActor is no
/// BP_FighterCharacter (@761), whose HitComponent's GetCollisionEnabled is 3 QueryAndPhysics (@1008), whose
/// CopyArrayOfMoveIgnoreComponents does not hold the owner's CapsuleComponent (@1270) and that is not in the
/// capsule's own list (@1445-1769) -> K2_DestroyActor @1783 (EndPlay: the source removed by its id). The capsule's
/// list is the stage platforms the fighter ignores (MoveIgnoreComponents, `plat_ignored`). True when it ended the
/// move.
// Unverified: a body's CollisionEnabled is not kept by the trace: every WorldStatic body is taken as QueryAndPhysics,
// and a spawned actor's body as holding no move-ignore list; the tick's place in the frame (here after the mesh tick,
// before the latent Delay, as AActor::Tick runs ReceiveTick before ProcessLatentActions) and whether a buff spawned
// in the frame ticks in it are not read.
pub fn safe_move_tick(w: &mut World, f: FighterId) -> bool {
    let Some(s) = w.fighter(f).team.ally.safe.as_ref() else { return false };
    let (sx, sy) = (s.x, s.y);
    let (fx, fy) = (w.fighter(f).body.x, fighter::actor_y(w, f));
    let (dx, dy) = (sx - fx, sy - fy);
    // Normal(V, 0.0001) (142f7adf0): InvSqrt, zero for a squared size under the tolerance.
    let (nx, ny) = unit(dx, dy);
    let mut hits = std::mem::take(&mut w.fighter_mut(f).bufs.hits);
    traces::sphere_trace_multi_for_objects(
        w,
        fx,
        fy,
        fx + nx * 100.0,
        fy + ny * 100.0,
        30.0,
        &[OBJ_WORLD_STATIC],
        &[],
        None,
        &mut hits,
    );
    // The Single trace: the first blocking hit.
    let first = hits.first().copied();
    hits.clear();
    w.fighter_mut(f).bufs.hits = hits;
    let Some(h) = first else { return false };
    if let Some(ActorRef::Fighter(o)) = h.actor
        && common::peers(w).contains(&o)
    {
        return false;
    }
    if h.platform >= 0 && (w.fighter(f).platforms.plat_ignored.wrapping_shr(h.platform.cast_unsigned()) & 1) == 1 {
        return false;
    }
    end_move_buff(w, f);
    true
}

/// `safeMoveExpired`: DurationExpired @1817-2147 (the child's override):
/// Vector_Distance(owner, Destination) > BufferDistanceToCheckArrive (100): MulticastBeginMove(Destination, true,
/// true, that distance x MovementSpeed), the parent's BeginMove again (a new source and a new Delay, started by the
/// resumed Delay: counted from the next frame); else the parent's DurationExpired @10 -> K2_DestroyActor.
pub fn safe_move_expired(w: &mut World, f: FighterId) {
    let s = w.fighter(f).team.ally.safe.as_ref().expect("a safe move");
    let (sx, sy, speed) = (s.x, s.y, s.speed);
    let d = ue::size2d([sx - w.fighter(f).body.x, sy - fighter::actor_y(w, f)]);
    if d > 100.0 {
        let t = d * speed;
        safe_begin_move(w, f, sx, sy, t, true, common::latent_frames(t));
    } else {
        end_move_buff(w, f);
    }
}

// ---- the call surface's argument and return types

/// `evadeTutorialByte`'s 'air' | 'gta' | 'ground'.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvadeTutorialByteKind {
    Air,
    Gta,
    Ground,
}

// ---- stubs a hook of the characters' and notifies' code calls

/// `SpawnRequest.origin`, the place a request made outside a notify spawns at
/// (scoobySnack's box); `None` for a notify's request.
pub fn spawn_origin(r: &SpawnRequest) -> Option<crate::fighter::moves::V2> {
    r.origin
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(alias: &str, tags: &str, pr: f64) -> VoRow {
        (alias.to_owned(), tags.to_owned(), pr, Some(format!("vo_{alias}")), Some(1.0))
    }

    #[test]
    fn select_vo_keeps_the_best_priority_and_rerolls_the_recent_line() {
        let rows = [
            row("respawn", "", 1.0),
            row("Respawn", "LebronFriend", 2.0),
            row("respawn", "LeBronFriend|AnyFriend", 2.0),
            row("respawn", "Missing", 3.0),
            row("other", "", 9.0),
        ];
        // The tag compare ignores ASCII case; the untagged row loses to the priority 2 rows.
        let mut draws = vec![0.0, 0.0, 1.0].into_iter();
        let pick = select_vo(&rows, "RESPAWN", &["lebronfriend", "AnyFriend"], 1.0, |n| {
            assert_eq!(n, 2.0);
            draws.next().expect("a draw")
        });
        // Rows 1 and 2 qualify; the first two draws give row 1 (the recent one), the third row 2.
        assert_eq!(pick, 2);
        assert_eq!(select_vo(&rows, "none", &[], -1.0, |_| 0.0), -1);
        // At most two re-rolls.
        assert_eq!(select_vo(&rows, "respawn", &["LebronFriend", "AnyFriend"], 1.0, |_| 0.0), 1);
    }

    #[test]
    fn replace_and_swap_follow_fstring_replace() {
        assert_eq!(replace_ignore_case("sfx_C017_S01_jump_c017", "c017", "C017_S02"), "sfx_C017_S02_S01_jump_C017_S02");
        assert_eq!(replace_ignore_case("abc", "", "x"), "abc");
        assert_eq!(replace_ignore_case("abc", "d", "x"), "abc");
        assert_eq!(replace_ignore_case("aaa", "aa", "b"), "ba");
        // A swap that only changes case is the original; an unloaded event is the original.
        assert_eq!(swap_skin_event("TomandJerry_hit", "TomandJerry", "TomAndJerry", |_| true), "TomandJerry_hit");
        assert_eq!(swap_skin_event("vo_S00_hit", "S00", "S01", |_| false), "vo_S00_hit");
        assert_eq!(swap_skin_event("vo_S00_hit", "s00", "S01", |n| n == "vo_s01_hit"), "vo_S01_hit");
    }

    #[test]
    fn tutorial_bytes() {
        for b in 0..90 {
            assert_eq!(TutorialEvent::from_byte(b).byte(), b);
        }
        assert_eq!(TutorialEvent::from_byte(90), TutorialEvent::None);
        assert_eq!(TutorialEvent::from_byte(-1), TutorialEvent::None);
        assert_eq!(USE_OFFSET, 28);
        assert_eq!(evade_tutorial_byte(EvadeTutorialByteKind::Air, 0.0), TutorialEvent::EvadeAirUp);
        assert_eq!(evade_tutorial_byte(EvadeTutorialByteKind::Gta, 8.0), TutorialEvent::EvadeGroundToAirUpLeft);
        assert_eq!(evade_tutorial_byte(EvadeTutorialByteKind::Ground, 10.0), TutorialEvent::None);
        assert_eq!(evade_tutorial_byte(EvadeTutorialByteKind::Ground, 1.5), TutorialEvent::None);
        assert_eq!(free_hit(3, 4.0), 13);
        assert_eq!(free_hit(0, -1.0), 0);
        assert_eq!(branch_row_bytes("EActionButton::AB_SpecialAttack", "EActionDirection::AD_Back"), (1.0, 3.0));
        assert_eq!(branch_row_bytes("AB_Taunt", "nothing"), (-1.0, 5.0));
    }

    #[test]
    fn surface_traces_hit_the_line_or_the_sphere() {
        // A box from x -100..100, y -50..0.
        let b = |x, oy, sphere| surface_blocked(x, oy, sphere, -100.0, 100.0, 0.0, -50.0, C::SURFACE_LINE_LENGTH);
        assert!(b(0.0, 100.0, false));
        assert!(!b(0.0, C::SURFACE_LINE_LENGTH + 0.5, false));
        assert!(!b(101.0, 10.0, false));
        assert!(!b(0.0, -60.0, false));
        assert!(b(100.0 + C::SURFACE_SPHERE_RADIUS, 0.0, true));
        assert!(!b(100.0 + C::SURFACE_SPHERE_RADIUS + 1.0, 0.0, true));
        // The sphere runs from the origin to 5 above it.
        assert!(b(0.0, -50.0 - 5.0 - C::SURFACE_SPHERE_RADIUS, true));
    }

    #[test]
    fn aim_helpers() {
        assert_eq!(stick_sign_down(0.0).to_bits(), 0.0f32.to_bits());
        assert_eq!(stick_sign_down(0.5), -1.0);
        assert_eq!(stick_sign(-0.2), -1.0);
        let n = near(170.0, -170.0);
        assert_eq!((n.mult, n.pos, n.diff), (1.0, true, -20.0));
        assert_eq!(index(2.0), Some(2));
        assert_eq!(index(-0.0), Some(0));
        assert_eq!(index(0.5), None);
        assert_eq!(index(-1.0), None);
    }
}
