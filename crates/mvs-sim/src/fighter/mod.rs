//! A fighter: every field of the fighter, grouped by subject into
//! structs, each group in the file of the module that owns its behaviour. A field is named in snake case;
//! a group's doc names the modules that write it.
//!
//! A reference to an object is an id: another fighter a [`FighterId`], a spawned actor an
//! [`crate::actors::ActorRef`], a notify a [`crate::events::NotifyRef`], a montage's data its [`Name`]. A string is a
//! [`Name`]. `Default` is the zeroed group; `Fighter::new` (with the fighter's set-up) gives the initial values.

pub mod anim;
pub mod body;
pub mod common;
pub mod control;
pub mod match_side;
pub mod moves;
pub mod tick;
pub mod traces;

pub use anim::{Anim, Branching, MontageState, Montages, Pose, RootMotion, StateMachines, Visibility};
pub use body::{Body, Capsule, PlatformRef, Platforms, StagePlatform};
pub use control::{Abilities, ChargeAttack, Controls, DodgeState, Evade};
pub use match_side::{Clock, Items, Life, Requests, Stats, Team, TutorialFlags, Vo};
pub use moves::{AirControl, Crouch, FastFall, Jump, MoveCommands, Wall};

use crate::Real;
use crate::actors::ActorRef;
use crate::consts as C;
use crate::math;
use crate::registry::signals::FighterSignal;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use mvs_data::def::AnimSet;
use std::sync::LazyLock;

use crate::State;
use crate::chars::CharState;
use crate::combat::{Armor, Attackers, BuffShapes, Buffs, ComboDecay, HitReact, HitState, Holds, Invuln, Rage};
use crate::events::Event;
use crate::fighter::control::AbilityState;
use crate::fighter::tick::FrameMode;
use crate::ids::Id;
use crate::input::InputState;
use crate::names::Name;
use crate::registry::signals::OwnerSignal;
use crate::registry::{BindError, CharHooks, PickupEntry, Registry};
use crate::rng::Rng;
use crate::state::{StateHash, StateHasher};
use crate::world::World;
use mvs_data::def::CharDef;
use mvs_data::value::Props;

pub type FighterId = Id<Fighter>;

/// `Fighter`. What a fighter would hold as a reference to an object of its World is the World's, one for all its
/// fighters: `voShared` (`MatchState::vo_shared`), `peers` (the World's fighter lists), `comboReset`
/// (`World::combo_reset`).
#[derive(State, Debug)]
pub struct Fighter {
    /// The `id`: the player's slot, a pawn's number (`World.pawnSeq`).
    pub id: i32,
    /// The character key (`ch.key`, the data's), and the definition the fighter was made with (`baseCh`).
    pub ch: Name,
    pub base_ch: Name,
    /// The members of `ch` the actor's scale changes (`setSizeScale`, a Polymorph's `setActorScale`); the rest of
    /// `ch` is the character's definition as it is.
    pub scaled: ScaledDef,
    /// `baseCh.meshScale` where a class replaced it on the fighter's own copy of the definition (the Watermelon
    /// Steven's Mesh world scale 0.85): the scale the size changes and a Polymorph multiply. `None`: the definition's.
    pub base_mesh_scale: Option<Real>,
    /// The character's hooks, bound from `ch` at the fighter's set-up.
    pub hooks: Hooks,
    /// `cs`: the character's own state.
    pub cs: CharState,
    /// The fighter's own random stream (`new Rng((seed * 7919 + i * 104729) | 0)`).
    pub rng: Rng,
    // (`data`, `boxes` and `ground` are the World's, by `ch`: `World.data`, `World.fixed.samplers`, `World.stage`.)

    // fighter: where it is and how it moves
    pub body: Body,
    pub platforms: Platforms,
    pub capsule: Capsule,
    pub moves: MoveCommands,
    pub jump: Jump,
    pub crouch: Crouch,
    pub fast_fall: FastFall,
    pub wall: Wall,
    pub air: AirControl,
    pub evade: Evade,
    // fighter: input, abilities, attacks
    pub controls: Controls,
    pub abilities: Abilities,
    pub charge: ChargeAttack,
    // fighter: montages, branches, pose
    pub montages: Montages,
    pub branching: Branching,
    pub root: RootMotion,
    pub anim: Anim,
    pub pose: Pose,
    pub sm: StateMachines,
    pub vis: Visibility,
    pub frame: tick::FrameFlags,

    // combat: being hit, attacking, defending
    pub hit: HitState,
    pub react: HitReact,
    pub attackers: Attackers,
    pub decay: ComboDecay,
    pub armor: Armor,
    pub invuln: Invuln,
    pub rage: Rage,
    pub buffs: Buffs,
    pub holds: Holds,
    pub shapes: BuffShapes,

    // world: the match's side of the fighter
    pub stats: Stats,
    pub life: Life,
    pub team: Team,
    pub vo: Vo,
    pub items: Items,
    pub requests: Requests,
    pub clock: Clock,
    pub tutorial: TutorialFlags,
    /// `PAWN`: its multicast delegates.
    pub delegates: crate::world::flow::PawnDelegates,

    /// `events`: the fighter's events of the step, flushed into the World's (`Pawns.flushEvents`). Not state.
    #[state(skip)]
    pub events: Vec<Event>,
    /// Buffers the fighter's per-tick work reuses (empty between calls). Not state.
    #[state(skip)]
    pub bufs: Buffers,
}

/// The fighter's reused buffers: a call takes one (`std::mem::take`), fills it, and puts it back cleared, so a
/// tick allocates nothing once each has grown.
#[derive(Debug, Default)]
pub struct Buffers {
    /// The fly tick's trace hits (`fly_tick`).
    pub hits: Vec<crate::fighter::traces::TraceHit>,
    /// The shape indices of `update_hurt_boxes`.
    pub shapes: Vec<i32>,
}

/// A copy starts with empty buffers: they hold nothing between calls.
impl Clone for Buffers {
    fn clone(&self) -> Self {
        Buffers::default()
    }
}

/// `ch`'s size-scaled copy (`{ ...baseCh, capHalf, capRadius, crouchedHalf, meshScale }`, each the base's times the
/// scale, float32): the four members it changes. At scale 1 the base's own.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct ScaledDef {
    pub cap_half: Real,
    pub cap_radius: Real,
    pub crouched_half: Real,
    pub mesh_scale: Real,
}

impl ScaledDef {
    /// The base definition's (`f.ch = baseCh`).
    pub fn of(def: &CharDef) -> Self {
        ScaledDef {
            cap_half: crate::math::narrow(def.cap_half),
            cap_radius: crate::math::narrow(def.cap_radius),
            crouched_half: crate::math::narrow(def.crouched_half),
            mesh_scale: crate::math::narrow(def.mesh_scale),
        }
    }
}

/// A fighter's hooks, resolved once from its character key (`CHARS.get(f.ch.key)`, `PICKUPS.get(f.ch.key)`): copied
/// with the fighter, left out of the hash (they follow from `ch`, which is hashed).
#[derive(Clone, Copy, Debug)]
pub struct Hooks {
    pub char: &'static CharHooks,
    pub pickup: Option<&'static PickupEntry>,
}

impl Hooks {
    /// The hooks of character `key`; a key the registry has no character entry for is a bind error naming `asset`.
    pub fn bind(r: &'static Registry, key: &str, asset: &str) -> Result<Hooks, BindError> {
        Ok(Hooks { char: r.chars.resolve(key, asset)?, pickup: r.pickups.get(key) })
    }
}

impl StateHash for Hooks {
    fn hash_state(&self, _h: &mut StateHasher) {}
}

/// The empty hooks, for a fighter whose character registered none.
pub static NO_HOOKS: LazyLock<CharHooks> = LazyLock::new(CharHooks::default);

impl Fighter {
    /// A fighter of character `ch` in slot `id`, its hooks bound and the field initialisers set. What the
    /// the constructor reads from the character's data and its hooks is [`set_up`]'s, called once the fighter is in
    /// the World's arena.
    pub fn new(id: i32, ch: Name, hooks: Hooks, rng: Rng) -> Self {
        let mut f = Self::zeroed(id, ch, hooks, rng);
        f.body.facing_dir = 1.0;
        f.body.size_scale = 1.0;
        f.platforms.plat_ignored = -1;
        f.platforms.down_window_tick = -1;
        // CreateAbilities' Delay(1.0).
        f.life.pre_spawn_delay = 60;
        f.life.outline_count = 1;
        f.abilities.attack_slot = -1;
        f.abilities.cooldown_multiplier = 1.0;
        f.charge.charge_mult = 1.0;
        f.charge.charge_dmg_mult = 1.0;
        f.charge.charge_buff_mult = 1.0;
        f.charge.charge_notify = -1;
        f.vis.on_camera = true;
        f.air.dash_tick = -1e9;
        f.air.air_control_gate = true;
        f.air.air_control = C::AIR_CONTROL;
        f.air.air_boost_mult = C::AIR_CONTROL_BOOST_MULT;
        f.air.air_attack_available = true;
        f.moves.gravity_scale = C::GRAVITY_SCALE;
        f.jump.jump_frame = -1;
        f.jump.jump_min_time = C::JUMP_MIN_TIME;
        f.jump.jump_cut_at = -1;
        f.jump.quick_fall_at = -1;
        f.jump.ground_jump_tick = -1;
        f.fast_fall.ff_check_tick = -1;
        f.anim.fade_rate = 1.0;
        f.anim.loco_rate = 1.0;
        f.anim.loco_loop = true;
        f.anim.loco_blend = (0.1, false);
        f.hit.true_hitstun_set_tick = -1;
        f.hit.true_hitstun_tick_at = -1;
        f.hit.bounce_fx_until = -1e9;
        f.wall.wall_mult = 1.0;
        f.wall.wall_stall = -1;
        f.wall.wall_fatigue = -1;
        f.wall.no_wall_slide_until = -1;
        f.attackers.assist_clear_tick = -1;
        f.attackers.combo_reset_tick = -1;
        f.attackers.last_knocked_back = -1.0;
        f.evade.evade_invuln_time = C::EVADE_INVULN;
        f.evade.evade_regen_start = -1.0;
        f.controls.rot_lock_until = -1.0;
        f.buffs.speed_mod = 1.0;
        f.sm.gs_rel = 1.0;
        f.sm.dash_stop_blend = true;
        f.sm.dash_stop_delay = -1.0;
        f.team.ally.cast = -1.0;
        f.team.ally.vfx_born = -1.0;
        f
    }

    fn zeroed(id: i32, ch: Name, hooks: Hooks, rng: Rng) -> Self {
        Fighter {
            id,
            ch,
            base_ch: ch,
            scaled: ScaledDef::default(),
            base_mesh_scale: None,
            hooks,
            cs: CharState::None,
            rng,
            body: Body::default(),
            platforms: Platforms::default(),
            capsule: Capsule::default(),
            moves: MoveCommands::default(),
            jump: Jump::default(),
            crouch: Crouch::default(),
            fast_fall: FastFall::default(),
            wall: Wall::default(),
            air: AirControl::default(),
            evade: Evade::default(),
            controls: Controls::default(),
            abilities: Abilities::default(),
            charge: ChargeAttack::default(),
            montages: Montages::default(),
            branching: Branching::default(),
            root: RootMotion::default(),
            anim: Anim::default(),
            pose: Pose::default(),
            sm: StateMachines::default(),
            vis: Visibility::default(),
            frame: tick::FrameFlags::default(),
            hit: HitState::default(),
            react: HitReact::default(),
            attackers: Attackers::default(),
            decay: ComboDecay::default(),
            armor: Armor::default(),
            invuln: Invuln::default(),
            rage: Rage::default(),
            buffs: Buffs::default(),
            holds: Holds::default(),
            shapes: BuffShapes::default(),
            stats: Stats::default(),
            life: Life::default(),
            team: Team::default(),
            vo: Vo::default(),
            items: Items::default(),
            requests: Requests::default(),
            clock: Clock::default(),
            tutorial: TutorialFlags::default(),
            delegates: crate::world::flow::PawnDelegates::default(),
            events: Vec::new(),
            bufs: Buffers::default(),
        }
    }
}

/// The rest of the constructor (`new Fighter(id, data, ch, boxes, ground, rng)`), once the fighter is in the
/// World's arena: what it reads from the character's definition and data, the character's own state
/// (`CharHooks.state`), the idle of the set the AnimBP shows on a new pawn and `CharHooks.init`.
pub fn set_up(w: &mut World, f: FighterId) {
    let ch = w.fighter(f).ch;
    let d = common::char_data(w, ch);
    let def = &d.def;
    let cap_half = def.cap_half;
    let cap_radius = def.cap_radius;
    let jump_v = def.jump_velocity;
    let mesh_x = def.mesh_x;
    let scaled = ScaledDef::of(def);
    let hold_pack: Vec<anim::HoldOverride> = def
        .nav
        .hold
        .iter()
        .map(|h| anim::HoldOverride {
            air: h.air,
            dir: h.dir,
            montage: h.montage.as_deref().map_or(Name::NONE, |m| common::lit(w, m)),
        })
        .collect();
    let terminal = common::default_terminal(d);
    // The AnimBP class default (Default__Base_AnimBP_C017_C has it false, the rest Base_AnimBP's true).
    let dash_stop_blend =
        d.fighter.anim_set.get("DashStopBlend").and_then(mvs_data::value::Value::as_bool) != Some(false);
    let abilities = new_ability_containers(def);
    let sub_hidden: indexmap::IndexSet<Name> = def.sub_hidden.iter().map(|s| common::lit(w, s)).collect();
    {
        let fm = w.fighter_mut(f);
        fm.base_ch = ch;
        fm.scaled = scaled;
        fm.capsule.cap_half_u = crate::math::narrow(cap_half);
        fm.capsule.cap_radius_u = crate::math::narrow(cap_radius);
        fm.jump.jump_v = crate::math::narrow(jump_v);
        fm.hit.mesh_x = crate::math::narrow(mesh_x);
        fm.anim.nav = anim::SetRef::Base;
        fm.anim.hold_pack = hold_pack;
        // BeginPlay: SetTerminalVelocity's Reset.
        fm.hit.terminal_v = terminal;
        fm.sm.dash_stop_blend = dash_stop_blend;
        fm.abilities.abilities = abilities;
        fm.pose.sub_hidden = sub_hidden;
    }
    let hooks = w.fighter(f).hooks.char;
    if let Some(state) = hooks.state {
        let cs = state(w, f);
        w.fighter_mut(f).cs = cs;
    }
    // The idle of the set the AnimBP shows on a new pawn (CharHooks.animSet, read with the character's new state).
    let idle = anim_set_def(w, f).idle.as_deref().map_or(Name::NONE, |s| common::lit(w, s));
    let fm = w.fighter_mut(f);
    fm.anim.anim = idle;
    fm.anim.loco_anim = idle;
    if let Some(init) = hooks.init {
        init(w, f);
    }
}

// ---- the getters and the small methods

/// `facing`.
pub fn facing(w: &World, f: FighterId) -> Real {
    w.fighter(f).body.facing_dir
}

/// `set facing` (SetRotationLogic): a changed facing is a rotation-only root move, which updates the overlaps
/// inside the call (`onTurn`), then the parent's broadcast FacingChanged to the spawned actors, the bound
/// signal and the pawn's own hook.
pub fn set_facing(w: &mut World, f: FighterId, v: Real) {
    let fm = w.fighter_mut(f);
    if v == fm.body.facing_dir {
        return;
    }
    fm.body.facing_dir = v;
    if fm.frame.on_turn == tick::OnTurn::Overlaps {
        crate::combat::phases::overlaps(w, f, false);
    }
    signal_actors(w, f, OwnerSignal::Facing, v < 0.0);
    signal_bound(w, f, FighterSignal::Facing, v < 0.0, 0.0, Name::NONE, None);
    if let Some(turned) = w.fighter(f).hooks.char.turned {
        turned(w, f);
    }
}

/// `grounded`.
pub fn grounded(w: &World, f: FighterId) -> bool {
    w.fighter(f).body.on_floor
}

/// `set grounded`: leaving the floor drops the movement base.
pub fn set_grounded(w: &mut World, f: FighterId, v: bool) {
    let fm = w.fighter_mut(f);
    fm.body.on_floor = v;
    if !v {
        fm.platforms.base = None;
    }
}

/// A spawned actor's `dead` and `cls`; `None` for an actor gone from its arena.
fn actor_state(w: &World, a: ActorRef) -> Option<(bool, Name)> {
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

/// The registry's entry of an actor class.
fn actor_entry(w: &World, cls: Name) -> Option<&'static crate::registry::entries::ActorEntry> {
    w.registry.actors.get(w.names.str(cls))
}

/// `signalActors`: the owner's broadcasts to its spawned actors whose class binds them
/// (`ActorKind.ownerSignal`). The list is read live, by index.
pub fn signal_actors(w: &mut World, f: FighterId, sig: OwnerSignal, value: bool) {
    let mut i = 0;
    while let Some(&a) = w.fighter(f).items.spawned_actors.get(i) {
        i += 1;
        let ActorRef::Actor(id) = a else { continue };
        let Some((dead, cls)) = actor_state(w, a) else { continue };
        if dead {
            continue;
        }
        if let Some(h) = actor_entry(w, cls).and_then(|e| match e.hooks {
            crate::registry::entries::ActorHooks::World(h) => h.owner_signal,
            _ => None,
        }) {
            h(w, id, f, sig, value);
        }
    }
}

/// `signalBound`: this fighter's broadcasts to the actors of any fighter of the match
/// whose class binds them on a fighter (`ActorKind.fighterSignal`): its own spawned actors, then its peers'.
pub fn signal_bound(
    w: &mut World,
    f: FighterId,
    sig: FighterSignal,
    flag: bool,
    num: Real,
    tag: Name,
    by: Option<FighterId>,
) {
    if sig == FighterSignal::TakeAnyDamage && !w.fighter(f).buffs.buffs.is_empty() {
        crate::combat::buffs::buff_take_any_damage(w, f, num, by);
    }
    signal_bound_of(w, f, f, sig, flag, num, tag, by);
    let mut i = 0;
    while let Some(&o) = common::peers(w).get(i) {
        i += 1;
        if o != f {
            signal_bound_of(w, o, f, sig, flag, num, tag, by);
        }
    }
}

#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn signal_bound_of(
    w: &mut World,
    o: FighterId,
    f: FighterId,
    sig: FighterSignal,
    flag: bool,
    num: Real,
    tag: Name,
    by: Option<FighterId>,
) {
    if let Some(h) = w.fighter(o).hooks.char.bound_signal {
        h(w, o, f, sig, flag, num, tag, by);
    }
    let mut i = 0;
    while let Some(&a) = w.fighter(o).items.spawned_actors.get(i) {
        i += 1;
        let Some((dead, cls)) = actor_state(w, a) else { continue };
        if dead {
            continue;
        }
        // (a world actor's kind and a projectile's alike: C023A_Car_SpawnedActor and the item cars)
        match (a, actor_entry(w, cls).map(|e| e.hooks)) {
            (ActorRef::Actor(id), Some(crate::registry::entries::ActorHooks::World(h))) => {
                if let Some(h) = h.fighter_signal {
                    h(w, id, f, sig, flag, num, tag, by);
                }
            }
            (ActorRef::Projectile(id), Some(crate::registry::entries::ActorHooks::Projectile(h))) => {
                if let Some(h) = h.fighter_signal {
                    h(w, id, f, sig, flag, num, tag, by);
                }
            }
            _ => {}
        }
    }
}

/// `airSpecialMax`: BaseFighter AirSpecialMax as a graph wrote it, else the class default.
pub fn air_special_max(w: &World, f: FighterId) -> Real {
    w.fighter(f).air.air_special_max_set.unwrap_or(C::AIR_SPECIAL_MAX)
}

/// `capHalf`: the root capsule's half height at the actor's scale (float).
pub fn cap_half(w: &World, f: FighterId) -> Real {
    let fm = w.fighter(f);
    if let Some(a) = fm.body.actor_scale {
        // UCapsuleComponent: the half height times the scale's Z.
        return fm.capsule.cap_half_u * a[1];
    }
    let k = fm.body.size_scale;
    if k == 1.0 { fm.capsule.cap_half_u } else { fm.capsule.cap_half_u * k }
}

/// `capRadius`: the root capsule's radius at the actor's scale (times min(X, Y) of the scale).
pub fn cap_radius(w: &World, f: FighterId) -> Real {
    let fm = w.fighter(f);
    if let Some(a) = fm.body.actor_scale {
        return fm.capsule.cap_radius_u * a[0];
    }
    let k = fm.body.size_scale;
    if k == 1.0 { fm.capsule.cap_radius_u } else { fm.capsule.cap_radius_u * k }
}

/// `capsuleHalf`: the crouched capsule's half height (max(CrouchedHalfHeight, radius)) while bIsCrouched.
pub fn capsule_half(w: &World, f: FighterId) -> Real {
    let fm = w.fighter(f);
    if fm.capsule.cmc_crouched { math::max(fm.scaled.crouched_half, cap_radius(w, f)) } else { cap_half(w, f) }
}

/// `actorY`: the actor location's height (the standing centre `y` less the crouch's drop).
pub fn actor_y(w: &World, f: FighterId) -> Real {
    w.fighter(f).body.y - (cap_half(w, f) - capsule_half(w, f))
}

/// `extraHurt`: HurtBoxFloorBounce while its overlap events are on, as the segment of its capsule.
pub fn extra_hurt(w: &mut World, f: FighterId) -> Option<crate::combat::boxes::ExtraHurt> {
    if !w.fighter(f).hit.floor_bounce_hurt {
        return None;
    }
    let c = crate::combat::hit::floor_bounce_capsule(w, f)?;
    let y = actor_y(w, f) - c.dz;
    let h = math::max(c.half - c.r, 0.0);
    Some(crate::combat::boxes::ExtraHurt { x: w.fighter(f).body.x, y0: y - h, y1: y + h, r: c.r })
}

/// `meshWX`: the Mesh component's world X (the relative offset along the facing).
pub fn mesh_wx(w: &World, f: FighterId) -> Real {
    let fm = w.fighter(f);
    fm.body.x + fm.body.facing_dir * fm.hit.mesh_x
}

/// `meshWY`.
pub fn mesh_wy(w: &World, f: FighterId) -> Real {
    let fm = w.fighter(f);
    fm.body.y + fm.hit.mesh_z
}

/// `outlinesOn`.
pub fn outlines_on(w: &World, f: FighterId) -> bool {
    w.fighter(f).life.outline_count > 0
}

/// `animSet`: the anim set the base machines read now: the one the pawn's hook names, else the definition's.
pub fn anim_set(w: &World, f: FighterId) -> anim::SetRef {
    match w.fighter(f).hooks.char.anim_set {
        Some(h) => h(w, f),
        None => anim::SetRef::Base,
    }
}

/// The definition's anim set [`anim_set`] names.
pub fn anim_set_def(w: &World, f: FighterId) -> &AnimSet {
    let def = &common::data_of(w, f).def;
    match anim_set(w, f) {
        anim::SetRef::Base => &def.anim,
        anim::SetRef::Named(n) => def.anim_sets.as_ref().and_then(|m| m.get(w.names.str(n))).unwrap_or(&def.anim),
    }
}

/// `maxWalk`: CharacterMovement.MaxWalkSpeed (the speed override, else MaxWalkSpeed x the buffs' speed mod;
/// Pawn_C017's IsBlocking x 1.5 and isGliding x HoverSpeedMult).
// Deviation: the build's two writers are folded into one product (the build keeps whichever wrote last).
pub fn max_walk(w: &World, f: FighterId) -> Real {
    let fm = w.fighter(f);
    let def = &common::data_of(w, f).def;
    let mut v = fm.moves.speed_override.unwrap_or(crate::math::narrow(def.max_walk * f64::from(fm.buffs.speed_mod)));
    let mut gliding = false;
    if crate::chars::iron_giant::giant_of(w, f).is_some() {
        if crate::chars::iron_giant::giant_blocking(w, f) {
            v *= 1.5;
        }
        gliding = crate::chars::iron_giant::giant_gliding(w, f);
    }
    if gliding { v * crate::math::narrow(def.hover_speed_mult) } else { v }
}

/// `inHitstun` (CheckHitstunActive): hitstun, a hit's pause with its launch pending, a pull or a buff stun.
pub fn in_hitstun(w: &World, f: FighterId) -> bool {
    let fm = w.fighter(f);
    fm.hit.hitstun > 0.0
        || (fm.hit.hitpause > 0.0 && fm.hit.launch.is_some() && !fm.hit.free_launch)
        || fm.moves.pull.is_some()
        || fm.buffs.buff_stunned
}

/// `isEvading`.
pub fn is_evading(w: &World, f: FighterId) -> bool {
    w.fighter(f).evade.evading
}

/// `isJumping` (BPI_GetJumpInfo Jumping).
pub fn is_jumping(w: &World, f: FighterId) -> bool {
    w.fighter(f).moves.cmd_jumped
}

/// `canJump` (ACharacter::CanJump): false while crouching, in the knockback mode and while flying.
pub fn can_jump(w: &World, f: FighterId) -> bool {
    let falling = crate::fighter::moves::is_falling(w, f);
    let fm = w.fighter(f);
    if fm.capsule.cmc_crouched || fm.crouch.crouching || !(fm.body.on_floor || falling) {
        return false;
    }
    (if falling { 1.0 } else { 0.0 }) < C::JUMP_MAX_COUNT
}

/// `isRageCharging`.
pub fn is_rage_charging(w: &World, f: FighterId) -> bool {
    w.fighter(f).rage.rage_charging
}

/// `animIsAttacking`: Base_AnimBP's IsAttacking variable.
pub fn anim_is_attacking(w: &World, f: FighterId) -> bool {
    w.fighter(f).charge.anim_attack_off != 2 && is_attacking(w, f)
}

/// `isAttacking` (Comp_Pawn_BranchManager.IsAttacking): AttackIsActive > 0 || BeginAttack.
pub fn is_attacking(w: &World, f: FighterId) -> bool {
    let c = &w.fighter(f).charge;
    c.attack_is_active > 0 || c.begin_attack
}

/// `recentlyHitstunned` (WasRecentlyHitStunned): GetGameTimeInSeconds - LastHitStunTime < 0.5, in float.
pub fn recently_hitstunned(w: &World, f: FighterId) -> bool {
    let fm = w.fighter(f);
    (fm.clock.time - fm.hit.last_hitstun_time) < 0.5
}

/// `hasAttackInst`: `montage`, or a grouped instance played as an attack that is not fading.
pub fn has_attack_inst(w: &World, f: FighterId) -> bool {
    let m = &w.fighter(f).montages;
    if m.montage.is_some() {
        return true;
    }
    m.grouped.as_ref().is_some_and(|(list, _)| list.iter().any(|e| e.inst.is_some() && e.fade.is_none()))
}

/// `inAir` (GetIsInAir): not on the floor, Jumping, a ground jump from its press, a hit's pending launch, the
/// jump's PendingLaunch, a pending launch with Z > 0, or move commands whose summed MoveDelta points up.
pub fn in_air(w: &World, f: FighterId) -> bool {
    let fm = w.fighter(f);
    if !fm.body.on_floor
        || fm.moves.cmd_jumped
        || fm.moves.ground_jump.is_some()
        || (fm.hit.launch.is_some() && !fm.hit.free_launch)
        || fm.moves.jump_launched
    {
        return true;
    }
    if crate::fighter::moves::pending_launch_vector(w, f).map_or(0.0, |v| v.1) > 0.0 {
        return true;
    }
    let mut z = 0.0;
    for c in &fm.moves.move_cmds {
        z += c.by * c.total;
    }
    z > 0.0
}

/// `pickupMontage`: the PickUpOverride pack's entry for the fighter's state.
pub fn pickup_montage(w: &World, f: FighterId) -> Name {
    let def = &common::data_of(w, f).def;
    common::lit(w, if in_air(w, f) { &def.pickup_air } else { &def.pickup_ground })
}

/// `activeMontageName`: GetCurrentActiveMontage's among `montage` and the grouped instances.
pub fn active_montage_name(w: &World, f: FighterId) -> Option<Name> {
    match crate::fighter::anim::active_grouped(w, f) {
        Some(i) => w.fighter(f).montages.grouped.as_ref().map(|(l, _)| l[i].m.montage.name),
        None => w.fighter(f).montages.montage.as_ref().map(|m| m.montage.name),
    }
}

/// `isDashing`: until DashEndTime = the dash + 0.25 s.
pub fn is_dashing(w: &World, f: FighterId) -> bool {
    let fm = w.fighter(f);
    (crate::real(fm.clock.tick) - fm.air.dash_tick) * C::DT <= C::DASH_TIME
}

/// `rescueBy`: the instigator of the live rescue buff whose StartMoveToAlly has not run yet, or -1.
pub fn rescue_by(w: &World, f: FighterId) -> i32 {
    for b in &w.fighter(f).buffs.buffs {
        if b.rescue.is_some() && b.frames > 0.0 && b.pulled != Some(true) {
            return b.by.map_or(-1, math::to_int32);
        }
    }
    -1
}

/// `rescueVfxBorn`.
pub fn rescue_vfx_born(w: &World, f: FighterId) -> Real {
    for b in &w.fighter(f).buffs.buffs {
        if b.rescue.is_some() && b.frames > 0.0 {
            return b.born.unwrap_or(-1.0);
        }
    }
    -1.0
}

/// `healVfxBorn`.
pub fn heal_vfx_born(w: &World, f: FighterId) -> Real {
    for b in &w.fighter(f).buffs.buffs {
        if b.heal.is_some() && b.frames > 0.0 {
            return b.born.unwrap_or(-1.0);
        }
    }
    -1.0
}

/// `buffStunRemaining` (K2_GetTimerRemainingTimeHandle(BuffstunTimer)): the expire time less the timer manager's
/// double clock, rounded to float once (the duration less the ticks on that clock).
pub fn buff_stun_remaining(w: &World, f: FighterId) -> Real {
    let b = &w.fighter(f).buffs;
    crate::math::narrow(f64::from(b.buff_stun_len) - C::clock(b.buff_stun_age))
}

/// `actorCollisionOff` (SetActorEnableCollision(false)): the pull's move, or a live buff that switched it off.
pub fn actor_collision_off(w: &World, f: FighterId) -> bool {
    w.fighter(f).moves.pull.is_some() || crate::combat::buffs::buff_collision_off(w, f)
}

/// `noCollision`.
pub fn no_collision(w: &World, f: FighterId) -> bool {
    w.fighter(f).capsule.capsule_off || actor_collision_off(w, f)
}

/// `noHit`.
pub fn no_hit(w: &World, f: FighterId) -> bool {
    w.fighter(f).invuln.damage_disabled || actor_collision_off(w, f)
}

/// [`no_hit`] for what `owner` deals: the port's own Dodgeball rule spares an opponent too
/// (`combat::phases::no_hit_by`).
pub fn no_hit_by(w: &World, owner: FighterId, vic: FighterId) -> bool {
    crate::combat::phases::no_hit_by(w, owner, vic)
}

/// `mostRecentAssistAttacker`.
pub fn most_recent_assist_attacker(w: &World, f: FighterId) -> Option<FighterId> {
    let a = &w.fighter(f).attackers;
    if w.fighter(f).clock.tick < a.assist_clear_tick { a.assist_attacker } else { None }
}

/// `invulnerable`.
pub fn invulnerable(w: &World, f: FighterId) -> bool {
    w.fighter(f).invuln.invuln_count > 0
}

/// `weight`: Comp_Actor_Damage Weight (what a graph wrote, the pawn's, BaseFighter's).
pub fn weight(w: &World, f: FighterId) -> Real {
    if let Some(v) = w.fighter(f).body.weight_set {
        return v;
    }
    let cs = &common::data_of(w, f).fighter.components;
    let read = |m: &mvs_data::fighter::Map<mvs_data::fighter::Component>| {
        m.get("Comp_Actor_Damage_GEN_VARIABLE")
            .and_then(|c| c.p.as_obj())
            .and_then(|p| p.get("Weight"))
            .and_then(mvs_data::value::Value::as_f64)
    };
    crate::math::narrow(read(&cs.pawn).or_else(|| read(&cs.base)).unwrap_or(f64::from(C::KB_WEIGHT)))
}

/// `pendingLaunch` (DamageManager KnockbackLaunchVelocity): a launch waiting for the end of hitpause, not a DI one.
pub fn pending_launch(w: &World, f: FighterId) -> Option<(Real, Real)> {
    w.fighter(f).hit.launch.as_ref().filter(|l| !l.di).map(|l| (l.vx, l.vy))
}

/// `isMontagePlaying` (MontagePlayingCount): `montage` or the dash montage, not blending out, without a track in
/// a DontStopNavMontageSlots slot.
pub fn is_montage_playing(w: &World, f: FighterId) -> bool {
    let m = &w.fighter(f).montages;
    let playing = |s: &Option<MontageState>, dash: bool| {
        s.as_ref().is_some_and(|s| {
            !crate::fighter::anim::blending_out(w, f, dash) && !montage_keeps_nav(w, f, s.montage.name)
        })
    };
    playing(&m.montage, false) || playing(&m.dash_montage, true)
}

fn montage_keeps_nav(w: &World, f: FighterId, m: Name) -> bool {
    common::data_of(w, f).fighter.montages.get(w.names.str(m)).and_then(|m| m.keeps_nav).unwrap_or(false)
}

/// `shownMontage`'s test: `montage` or the dash montage, not terminated, not one the character's graph lays over
/// the locomotion pose (`CharHooks.navPose`). `Some(true)`: the dash montage.
pub fn shown_montage(w: &World, f: FighterId) -> Option<bool> {
    let m = &w.fighter(f).montages;
    let (s, dash) = match (&m.montage, &m.dash_montage) {
        (Some(s), _) => (s, false),
        (None, Some(s)) => (s, true),
        (None, None) => return None,
    };
    if s.terminated {
        return None;
    }
    if let Some(nav) = w.fighter(f).hooks.char.nav_pose
        && let Some(md) = common::data_of(w, f).fighter.montages.get(w.names.str(s.montage.name))
        && nav(md)
    {
        return None;
    }
    Some(dash)
}

/// `slotMontage`: the same slot's instance, a terminated one included. `Some(true)`: the dash montage.
pub fn slot_montage(w: &World, f: FighterId) -> Option<bool> {
    let m = &w.fighter(f).montages;
    if m.montage.is_some() {
        Some(false)
    } else if m.dash_montage.is_some() {
        Some(true)
    } else {
        None
    }
}

/// `tenticleOverride`: C017_AnimBP TenticleOverride, the weight of the pilot's tentacle
/// pose (AnimNotify_C017_TenticleOveride NotifyBegin @121). For the renderer.
pub fn tenticle_override(w: &World, f: FighterId) -> bool {
    w.fighter(f).montages.grouped.as_ref().is_some_and(|g| g.1)
}

/// `tenticleOverride`'s setter: on makes the grouped state if there is none; off clears
/// the flag of one that is there and lets `groupedDone` drop it.
pub fn set_tenticle_override(w: &mut World, f: FighterId, on: bool) {
    let g = &mut w.fighter_mut(f).montages.grouped;
    if on {
        g.get_or_insert_with(|| (Vec::new(), false)).1 = true;
    } else if let Some(g) = g {
        g.1 = false;
        crate::fighter::anim::grouped_done(w, f);
    }
}

/// `hasGrouped`: a grouped instance is listed (shown or not).
pub fn has_grouped(w: &World, f: FighterId) -> bool {
    w.fighter(f).montages.grouped.as_ref().is_some_and(|g| !g.0.is_empty())
}

/// `percentCharged`: Comp_Pawn_ChargeAttack.GetChargeValue(4), PercentCharged while the
/// charge's AttackMontage is the active montage, else 0 (`chargePct` is cleared with every montage start).
pub fn percent_charged(w: &World, f: FighterId) -> Real {
    w.fighter(f).charge.charge_pct
}

/// `bindJump`: AddUnique on BaseFighter JumpPressedDispatch.
pub fn bind_jump(w: &mut World, f: FighterId, a: ActorRef) {
    let l = &mut w.fighter_mut(f).frame.jump_listeners;
    if l.iter().any(|j| j.actor == a) {
        return;
    }
    l.push(tick::JumpListener { actor: a });
}

/// `unbindJump`: a new list is built so a broadcast running over the old one goes on with it; the
/// broadcaster (`fighter/control.rs`, JumpPressedDispatch) iterates a copy of the list taken before its first call,
/// so a removal here during it is safe the same way.
pub fn unbind_jump(w: &mut World, f: FighterId, a: ActorRef) {
    w.fighter_mut(f).frame.jump_listeners.retain(|j| j.actor != a);
}

// ---- the call surface of index

/// `newAbilityContainers`: a new pawn's AbilityContainers (no cooldown, the starting
/// ammo, the row's cooldowns).
pub fn new_ability_containers(ch: &CharDef) -> Vec<AbilityState> {
    ch.abilities
        .iter()
        .map(|a| AbilityState {
            cooldown: false,
            cd_frames: 0.0,
            ammo: crate::math::narrow(a.start_count),
            ammo_cooldown: false,
            ammo_frames: 0.0,
            default_cd: crate::math::narrow(a.cooldown),
            active_cd: crate::math::narrow(a.cooldown),
            ammo_max: crate::math::narrow(a.count_cooldown),
        })
        .collect()
}

/// `addActiveAttack`: BeginAttack off, the count up, and when it became 1 OnIsAttacking,
/// BaseFighter's AttackStarted body (SetCrouching(false)), the pawn's hook and IsAttackingStart.
// Deviation: OnIsAttacking's other binder (Base_AnimBP) is the animAttackOff write alone.
pub fn add_active_attack(w: &mut World, f: FighterId) {
    let fm = w.fighter_mut(f);
    fm.charge.begin_attack = false;
    fm.charge.attack_is_active += 1;
    if fm.charge.attack_is_active == 1 {
        fm.charge.anim_attack_off = 0;
        crate::fighter::moves::set_crouching(w, f, false);
        if let Some(h) = w.fighter(f).hooks.char.attack_started {
            h(w, f);
        }
        if !w.fighter(f).buffs.buffs.is_empty() {
            crate::combat::buffs::buff_signal(w, f, BuffSignal::IsAttackingStart, BuffSignalArg::default());
        }
    }
}

/// `removeActiveAttack`: OnIsAttacking at every call; at 0 BaseFighter's AttackEnded body
/// (isPushingDown and walking: SetCrouching(true), owed to the dispatch after a stop ahead of it), the pawn's hook
/// and IsAttackingEnd.
pub fn remove_active_attack(w: &mut World, f: FighterId) {
    let fm = w.fighter_mut(f);
    fm.charge.anim_attack_off = 0;
    fm.charge.attack_is_active -= 1;
    if fm.charge.attack_is_active == 0 {
        if fm.crouch.stop_ahead {
            fm.crouch.crouch_owed = true;
        } else if fm.crouch.pushing_down && fm.body.on_floor {
            crate::fighter::moves::set_crouching(w, f, true);
        }
        if let Some(h) = w.fighter(f).hooks.char.attack_ended {
            h(w, f);
        }
        if !w.fighter(f).buffs.buffs.is_empty() {
            crate::combat::buffs::buff_signal(w, f, BuffSignal::IsAttackingEnd, BuffSignalArg::default());
        }
    }
}

/// `owedAttackEndCrouch`: the dispatch's part of an AttackEnded whose flag ended at a stop ahead of it.
pub fn owed_attack_end_crouch(w: &mut World, f: FighterId) {
    let fm = w.fighter_mut(f);
    if !fm.crouch.crouch_owed {
        return;
    }
    fm.crouch.crouch_owed = false;
    if fm.charge.attack_is_active == 0 && fm.crouch.pushing_down && fm.body.on_floor {
        crate::fighter::moves::set_crouching(w, f, true);
    }
}

/// `updateHurtBoxes`: the HurtBox shapes of the tags on or off (a disguise's clone
/// shapes too).
pub fn update_hurt_boxes(w: &mut World, f: FighterId, tags: &[Name], on: bool) {
    let ch = w.fighter(f).ch;
    let mut idx = std::mem::take(&mut w.fighter_mut(f).bufs.shapes);
    {
        let sampler = &w.fixed.samplers[&ch];
        let shapes = &common::char_data(w, ch).boxes.shapes;
        for &t in tags {
            common::with_lower(w.names.str(t), |lower| {
                for &i in sampler.by_tag.get(lower).map_or(&[][..], Vec::as_slice) {
                    let Some(s) = usize::try_from(i).ok().and_then(|i| shapes.get(i)) else { continue };
                    if !s.tags.iter().any(|t| t == "HurtBox") {
                        continue;
                    }
                    idx.push(i32::try_from(i).expect("a shape index"));
                }
            });
        }
    }
    let fm = w.fighter_mut(f);
    for &i in &idx {
        if on {
            fm.hit.hurt_off.shift_remove(&i);
        } else {
            fm.hit.hurt_off.insert(i);
        }
    }
    idx.clear();
    fm.bufs.shapes = idx;
    if w.fighter(f).shapes.disguise.is_some() {
        crate::combat::disguise::clone_hurt_boxes(w, f, tags, on);
    }
}

/// The lower-case name of `tag` (`tag.toLowerCase()`: the equipped list keeps names in lower case). The bind interns
/// the lower-case form of every tag of the data; a tag whose form it lacks is a bind bug: it warns and keeps the tag
/// as it is, so two tags never share an entry.
fn lower_name(w: &World, tag: Name) -> Name {
    let s = w.names.str(tag);
    common::with_lower(s, |l| w.names.get(l)).unwrap_or_else(|| {
        debug_assert!(false, "the bind did not intern the lower-case form of tag {s}");
        common::warn_text(w, &format!("tag {s}: the bind did not intern its lower-case form"));
        tag
    })
}

/// `hasEquipped`: `tag` in lower case.
pub fn has_equipped(w: &World, f: FighterId, tag: Name) -> bool {
    w.fighter(f).pose.equipped.contains(&tag)
}

/// `equipAdd`: AddEquippedComponentMulticast, the list alone.
// Deviation: the ItemEquipped / ItemUnequipped broadcasts are not sent: no ported class binds them.
pub fn equip_add(w: &mut World, f: FighterId, tag: Name) {
    let t = lower_name(w, tag);
    w.fighter_mut(f).pose.equipped.push(t);
}

/// `equipRemove`: Array_RemoveItem, every entry equal to the tag.
pub fn equip_remove(w: &mut World, f: FighterId, tag: Name) {
    let t = lower_name(w, tag);
    w.fighter_mut(f).pose.equipped.retain(|&e| e != t);
}

/// `equipObject`: the list, SetSubMeshVisibility(tag, on) and the AnimBP's BPI_HoldingItem.
pub fn equip_object(w: &mut World, f: FighterId, tag: Name) {
    equip_add(w, f, tag);
    crate::fighter::anim::set_sub_mesh(w, f, tag, true);
    holding_item(w, f);
}

/// `unequipObject`.
pub fn unequip_object(w: &mut World, f: FighterId, tag: Name) {
    equip_remove(w, f, tag);
    crate::fighter::anim::set_sub_mesh(w, f, tag, false);
    holding_item(w, f);
}

fn holding_item(w: &mut World, f: FighterId) {
    if let Some(h) = w.fighter(f).hooks.char.holding_item {
        let rock = crate::name!("rockmesh");
        let on = has_equipped(w, f, rock);
        h(w, f, on);
    }
}

/// `step`: a whole frame for one fighter alone (the phases back to back, the
/// fighter's own parts of them: no overlaps, no item reach, no fly tick).
pub fn step(w: &mut World, f: FighterId, inp: InputState) {
    tick::begin_frame_input(w, f, inp);
    tick::controller_tick(w, f);
    tick::cmc_tick_own(w, f);
    tick::pawn_tick_own(w, f, false);
    tick::mesh_tick_own(w, f);
    tick::during_physics_tick_own(w, f);
}

/// `standY`: the walking height, FLOOR_REST_GAP over the floor contact.
pub fn stand_y(w: &World, f: FighterId) -> Real {
    let top = match w.fighter(f).platforms.base {
        Some(b) => crate::fighter::body::base_top(w, b, w.fighter(f).body.x, cap_radius(w, f)),
        None => crate::math::narrow(w.stage.ground.top),
    };
    top + cap_half(w, f) + crate::math::narrow(common::data_of(w, f).def.rest_gap)
}

/// `mode`: the branch the frame boundary picked, or 'paused' once a hit of this frame
/// set a pause.
pub fn mode(w: &World, f: FighterId) -> FrameMode {
    let fm = w.fighter(f);
    let m = fm.frame.frame_mode;
    if m != FrameMode::Dead && m != FrameMode::Paused && fm.hit.hitpause > 0.0 && fm.frame.hit_this_frame {
        return FrameMode::Paused;
    }
    m
}

/// `warn`.
pub fn warn(w: &World, _f: FighterId, msg: Name) {
    common::warn_once(w, msg);
}

/// `setJumping`: a write of Jumping and its broadcast OnIsJumping.
pub fn set_jumping(w: &mut World, f: FighterId, v: bool) {
    w.fighter_mut(f).moves.cmd_jumped = v;
    signal_actors(w, f, OwnerSignal::Jumping, v);
    if !w.fighter(f).buffs.buffs.is_empty() {
        crate::combat::buffs::buff_signal(
            w,
            f,
            BuffSignal::IsJumping,
            BuffSignalArg { value: Some(v), ..BuffSignalArg::default() },
        );
    }
}

/// `latent`.
pub fn latent(_w: &World, _f: FighterId, s: Real) -> Real {
    common::latent_frames(s)
}

static NO_PROPS: LazyLock<Props> = LazyLock::new(Props::default);

/// `holdDef`: the buff class's defaults, empty for none.
pub fn hold_def(w: &World, f: FighterId, cls: Name) -> &Props {
    common::buff_def(w, ActorRef::Fighter(f), cls).unwrap_or(&NO_PROPS)
}

/// `grappleOn`.
pub fn grapple_on(w: &mut World, f: FighterId) {
    crate::combat::holds::initiate_grapple(w, f);
}

/// `grappleOff`.
pub fn grapple_off(w: &mut World, f: FighterId) {
    crate::combat::holds::remove_grapple(w, f);
}
