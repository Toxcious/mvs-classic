//! Iron Giant's ally turret: Comp_C017_Ally_Turret (the component
//! 'Comp_IronGiant_Ally_Turret' of Pawn_C017) and Buff_C017_Ally_Pilot, the buff it puts on the teammate who climbs
//! in. Its reticle and bomb actors are `turret_actors.rs`.
//!
//! No other character's code reads the turret: a teammate of any class climbs in through the same buff (there is no
//! per-character branch for the pilot).
#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-keeping negated tests

use mvs_data::value::{Props, Value};

use super::state::{giant_mut, giant_of, giant_w};
use super::turret_actors::{spawn_pilot_reticle, spawn_turret_bomb, sync_reticles};
use crate::Real;
use crate::State;
use crate::actors::projectile::{MakeOpts, make_projectile};
use crate::actors::{ActorClass, ActorRef, ProjectileClass};
use crate::combat::BuffRef;
use crate::combat::boxes::CH_HURT;
use crate::combat::buffs::{BuffExt, buff, buff_mut, buff_of, end_buff, spawn_buff};
use crate::combat::hit::hurt::hurt_hit;
use crate::combat::hits::hitframe_packet;
use crate::combat::holds::apply_stun_buffs;
use crate::combat::phases::{give_vbuf, hit_spheres, victim_pose};
use crate::consts::{DELTA_SECONDS, DT};
use crate::events::Event;
use crate::fighter::anim::grouped::play_grouped;
use crate::fighter::anim::visibility::{add_invisibility, remove_invisibility};
use crate::fighter::anim::{OpenPose, put_pose, sampled_boxes};
use crate::fighter::common::{actor_def, data_of, warn_text};
use crate::fighter::control::{has_ability_resources, raw_press_dir, trigger_ability};
use crate::fighter::moves::{
    V2, clear_gravity_override, disable_movement, enable_falling, override_gravity, recall_move, reset_jump_counts,
};
use crate::fighter::{
    FighterId, actor_collision_off, actor_y, cap_radius, capsule_half, facing, grapple_off, grapple_on, hold_def,
    in_hitstun, latent, mesh_wx, mesh_wy, no_collision,
};
use crate::input::Dir;
use crate::math::ue::{SMALL_NUMBER, safe_normal, size2d};
use crate::math::{max, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks};
use crate::registry::props::{num, truthy};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::BlastBounds;
use crate::world::pawns::match_fighters;
use crate::world::spawning::add_projectile;

const PILOT_BUFF: &str = "Buff_C017_Ally_Pilot_C";
const FAST_SHOT: &str = "C017_TenticalFastShot_C";
const COMPONENT: &str = "Comp_IronGiant_Ally_Turret_GEN_VARIABLE";

/// PilotFastAttackMulticast's montage by AttackMontageNumber (@12610) and its socket (@10484-10624).
fn shot_parts(n: i32) -> (Name, Name, &'static str) {
    match n {
        0 => (
            name!("C017_WeaponMode_Tenticle_Attack_01_Montage"),
            name!("socket:fml_un_R_tentacle8"),
            "fml_un_R_tentacle8",
        ),
        1 => (
            name!("C017_WeaponMode_Tenticle_Attack_03_Montage"),
            name!("socket:fml_un_L_tentacle8"),
            "fml_un_L_tentacle8",
        ),
        _ => (
            name!("C017_WeaponMode_Tenticle_Attack_02_Montage"),
            name!("socket:fml_un_C_tentacle8"),
            "fml_un_C_tentacle8",
        ),
    }
}

/// The component's variables (`AllyTurret`), on the Giant as `GiantState.ally_turret` from his first
/// ToggleAllyTurret on. For the renderer and the HUD:
/// - `jump_on_ally`: the teammate with the "climb in" input label (JumpOnUiMulticast @6988: a widget at the ally's (0,
///   0, 400), tint (0.68, 0.8, 0.85, 1), icon 32 x 32, on the ally's own screen only), or `None`
/// - `has_pilot`: HasPilot (Set Pilot Multicast); `pilot`: ActivePilot, `None` from the kick on
/// - `reticle`: C017_Pilot_Reticle lives (the actor of that class in the World's actors: PS_C017_UI_Reticule and its
///   arrow mesh at aim_x / aim_y, the AnimBP's BPI_SetAimTarget); aim_x / aim_y: AimLocation in sim axes
/// - `bomb`: latent ticks left of PilotLargeAttackVisuals' Delay(0.3): above 0, PS_AttackChargeUP_Short_nonLoop is
///   attached to giantEmblem_att (offset (150, 0, 0), scale 1.5); at its end the C017_Ally_Turret_Bomb actor joins the
///   World's actors
/// - `shot`: AttackMontageNumber, the tentacle of the last fast shot (0 R, 1 L, 2 C)
///
/// The pilot's side is the live buff Buff_C017_Ally_Pilot_C on him (`buff_of`): invisible, his HUD widgets hidden,
/// PS_C017_UI on his camera ([`pilot_stage`] 2: placed).
#[derive(State, Debug)]
pub struct AllyTurret {
    /// IronGiantInWeaponMode.
    pub in_weapon_mode: bool,
    /// AllowPilotEnter's two overlap delegates are bound.
    pub enter: bool,
    /// Bound since the last tick: the actors overlapping now get no begin event.
    pub seed: bool,
    /// The teammates overlapping at the last tick, by their ids (`Fighter.id`).
    pub over: Vec<i32>,
    pub jump_on_ally: Option<FighterId>,
    /// jumpOnAlly's attack (1) and item (2) buttons at the last tick.
    pub ally_prev: i32,
    pub has_pilot: bool,
    pub pilot: Option<FighterId>,
    /// PilotBuff, on `buff_on`.
    pub buff: Option<BuffRef>,
    pub buff_on: Option<FighterId>,
    /// The pilot's attack (1), special (2), jump (4) and dodge (8) buttons at the last tick.
    pub pilot_prev: i32,
    pub reticle: bool,
    pub aim_x: Real,
    pub aim_y: Real,
    /// AimVector, sim axes.
    pub aim_vx: Real,
    pub aim_vy: Real,
    /// 'Top Left Corner' / 'Bottom Right Corner', sim axes.
    pub left: Real,
    pub right: Real,
    pub bottom: Real,
    pub top: Real,
    /// IsNormalAttackHeld.
    pub held: bool,
    /// AutoFireTimer as the timer manager holds it ([`ally_turret_timers`]). `fire`: 0 not set, 1 pending (set since
    /// the last timer pass), 2 active; `fire_rate`: its Rate (float32); `fire_t` / `fire_at`: InternalTime and
    /// ExpireTime, in seconds since the pass that made it active (doubles, as the manager's).
    pub fire: i32,
    pub fire_rate: Real,
    pub fire_t: crate::Clock,
    pub fire_at: crate::Clock,
    /// AttackMontageNumber, the tentacle of the last fast shot (0 R, 1 L, 2 C).
    pub shot: i32,
    /// Latent ticks left of PilotLargeAttackVisuals' Delay(0.3).
    pub bomb: i32,
    /// Latent ticks left of KickPilot's Delay(NewPilotDelay), 0 none.
    pub kick: i32,
    /// The tick's two DoOnce gates (@2938-2998 the move sounds, @2703-2738 the loop's stop).
    pub move_open: bool,
    pub stop_open: bool,
}

impl Default for AllyTurret {
    /// The first object.
    fn default() -> Self {
        AllyTurret {
            in_weapon_mode: false,
            enter: false,
            seed: false,
            over: Vec::new(),
            jump_on_ally: None,
            ally_prev: 0,
            has_pilot: false,
            pilot: None,
            buff: None,
            buff_on: None,
            pilot_prev: 0,
            reticle: false,
            aim_x: 0.0,
            aim_y: 0.0,
            aim_vx: 0.0,
            aim_vy: 0.0,
            left: 0.0,
            right: 0.0,
            bottom: 0.0,
            top: 0.0,
            held: false,
            fire: 0,
            fire_rate: 0.0,
            fire_t: 0.0,
            fire_at: 0.0,
            shot: 0,
            bomb: 0,
            kick: 0,
            move_open: true,
            stop_open: true,
        }
    }
}

/// Buff_C017_Ally_Pilot's own variables (`PilotHold`, the buff entry's extension); `begun` false before its begin
/// (an undefined extension).
#[derive(State, Debug, Default)]
pub struct PilotHold {
    pub begun: bool,
    /// 0: polling the BuffstunTimer (@226), 1: its Delay(0.033) @363 runs, 2: placed (@1092-).
    pub stage: i32,
    pub wait: i32,
    /// Temp_int_Variable of the retry gate (@443-556).
    pub tries: i32,
    /// Latent ticks left of the Delay(0.06) before the second K2_SetActorLocation (@2733), 0 none.
    pub snap: i32,
    /// The blast limits (@5561-6516).
    pub lim: Option<BlastBounds>,
    pub ended: bool,
}

fn hold(w: &World, b: BuffRef) -> Option<&PilotHold> {
    match &buff(w, b)?.ext {
        BuffExt::AllyPilot(e) if e.begun => Some(e),
        _ => None,
    }
}

fn hold_mut(w: &mut World, b: BuffRef) -> Option<&mut PilotHold> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::AllyPilot(e) if e.begun => Some(e),
        _ => None,
    }
}

/// `pilotStage`: the stage of the pilot buff on `f`, -1 without one.
pub fn pilot_stage(w: &World, f: FighterId) -> i32 {
    buff_of(w, f, name!("Buff_C017_Ally_Pilot_C")).and_then(|b| hold(w, b)).map_or(-1, |e| e.stage)
}

/// The frames left of a buff entry: 0 for one no longer listed (an ended entry).
fn frames(w: &World, b: BuffRef) -> Real {
    buff(w, b).map_or(0.0, |x| x.frames)
}

/// Comp_C017_Ally_Turret's values: its class defaults (its CDO; the fighter data's `components` holds a component's
/// own tagged properties without its class CDO, as for Comp_C017_Hover_Rage's in hover_rage.rs), the pawn's template
/// overriding none of them; an override in the data wins.
// Deviation: held here as constants until the fighter bind merges a component's class defaults.
#[derive(Clone, Copy)]
struct Vals {
    new_pilot_delay: Real,
    blast_box_buffer: Real,
    aim_speed: Real,
    projectile_speed: Real,
    auto_fire_rate: Real,
}

/// The component's values and the buff's: without the buff's entry in the data no teammate is offered the turret.
fn defaults(w: &World, f: FighterId) -> Option<Vals> {
    let c = data_of(w, f).fighter.components.pawn.get(COMPONENT)?;
    if !hold_def(w, f, name!("Buff_C017_Ally_Pilot_C")).get("Stun").is_some_and(truthy) {
        return None;
    }
    let p = c.p.as_obj();
    let g = |k: &str, d: Real| p.and_then(|p| num(p, k)).unwrap_or(d);
    Some(Vals {
        new_pilot_delay: g("NewPilotDelay", 0.75),
        blast_box_buffer: g("BlastBoxBuffer", 150.0),
        aim_speed: g("AimSpeed", 2000.0),
        projectile_speed: g("ProjectileSpeed", 4000.0),
        auto_fire_rate: g("AutoFireRate", 0.3),
    })
}

/// `state(f)`: the turret, made at its first use.
fn state(w: &mut World, f: FighterId) -> &mut AllyTurret {
    giant_w(w, f).ally_turret.get_or_insert_with(AllyTurret::default)
}

/// The turret while it exists.
fn turret(w: &mut World, f: FighterId) -> Option<&mut AllyTurret> {
    giant_mut(w, f).and_then(|g| g.ally_turret.as_mut())
}

/// `sfx`: an event on the fighter, without a place.
fn sfx(w: &mut World, f: FighterId, ev: Name) {
    crate::fighter::match_side::sfx(w, f, Some(ev));
}

/// AllowPilotEnter (@3614): the owner's OnActorBeginOverlap -> TurnOnPilotActivationPrompt and OnActorEndOverlap ->
/// TurnOffPilotActivationPrompt.
fn allow_pilot_enter(t: &mut AllyTurret) {
    if !t.enter {
        t.seed = true;
    }
    t.enter = true;
}

/// BlockPilotEnter (@4640): both removed, BindInputMulticast(null, false) (@11887: the label's ally unbound).
fn block_pilot_enter(t: &mut AllyTurret) {
    t.enter = false;
    t.seed = false;
    t.over.clear();
    t.jump_on_ally = None;
}

/// JumpOnUiMulticast(ally, false) (@8055): AutoFireTimer cleared, the label destroyed.
fn jump_on_ui_off(t: &mut AllyTurret) {
    t.fire = 0;
}

/// `toggleAllyTurret`: Pawn_C017 SetAlternateModeVisuals -> ToggleAllyTurret(InWeaponMode) (@8199):
/// IronGiantInWeaponMode, then AllowPilotEnter (@8232) or KickPilot(false) (@8247). The off call comes at every end
/// of the mode and at the pawn's death, with or without a pilot, and KickPilot always plays the Out montage and posts
/// the loop's stop. Called by pawn.rs (setAlternateMode, the death).
pub fn toggle_ally_turret(w: &mut World, f: FighterId, on: bool) {
    let t = state(w, f);
    t.in_weapon_mode = on;
    if on {
        allow_pilot_enter(t);
    } else {
        kick_pilot(w, f, false);
    }
}

/// KickPilot(InWeaponMode) (@4989): the label off for ActivePilot (@4994), ActivePilot = null, BlockPilotEnter; with
/// PilotBuff valid its K2_DestroyActor (@5086: the buff's EndPlay puts the pilot back) and, for a kick in weapon mode,
/// Delay(NewPilotDelay) before HasPilot = false and AllowPilotEnter (@15, the latter with IronGiantInWeaponMode); else
/// HasPilot = false at once (@5583). Then PlayMontageMulticast(C017_WeaponMode_Out_Tenticle_Montage) (@5387 ->
/// Montage_Play(m, 1, 0, 0, false) @11557: no instance is stopped), the reticle's K2_DestroyActor and
/// SetReticleControl(null) (@5515-5551: the tick off), and sfx_..._tenticle_attack_loop_stop on the owner (@5634).
/// The Delay is not retriggerable: a kick while one runs (the mode's end inside NewPilotDelay) leaves it running.
fn kick_pilot(w: &mut World, f: FighterId, in_weapon_mode: bool) {
    let t = state(w, f);
    jump_on_ui_off(t);
    let (b, on) = (t.buff.take(), t.buff_on.take());
    t.pilot = None;
    block_pilot_enter(t);
    match (b, on) {
        (Some(b), Some(on)) if frames(w, b) > 0.0 => {
            end_buff(w, on, b);
            let p = defaults(w, f);
            match p {
                Some(p) if in_weapon_mode => {
                    if state(w, f).kick <= 0 {
                        let k = to_int32(latent(w, f, p.new_pilot_delay));
                        state(w, f).kick = k;
                    }
                }
                _ => state(w, f).has_pilot = false,
            }
        }
        _ => state(w, f).has_pilot = false,
    }
    play_grouped(w, f, name!("C017_WeaponMode_Out_Tenticle_Montage"), false, false);
    state(w, f).reticle = false;
    sync_reticles(w, f);
    sfx(w, f, name!("sfx_cha_C017_weaponsmode_tenticle_attack_loop_stop"));
}

/// SetActorEnableCollision(false) is on: the pilot from his placing on (Buff_C017_Ally_Pilot @1502, back on @4446)
/// and a fighter in Buff_StunnedMoveToPositionThroughGeo's move or Buff_Teleporting3 (`actor_collision_off`). No
/// component of his has a pair.
fn collision_off(w: &World, f: FighterId) -> bool {
    actor_collision_off(w, f) || pilot_stage(w, f) == 2
}

/// An active hit shape of `a` over a hurt box of `b` that generates overlap events: the shapes of a's live Hitframes
/// at their HitBoxScale (Comp_Pawn_HitBoxHandler @16817-16855: SetCollisionEnabled(3), SetRelativeScale3D) against
/// b's hurt capsules, HurtBoxFloorBounce during a floor or ceiling bounce and without those UpdateHurtBoxes turned off,
/// each in the pose it holds now: the geometry of the World's melee test. The Hitframe's hit set and the teams do not
/// matter to the component overlap.
fn hit_over_hurt(w: &mut World, a: FighterId, b: FighterId) -> bool {
    if w.fighter(a).montages.active_hits.is_empty() {
        return false;
    }
    let Some(vp) = victim_pose(w, b) else { return false };
    let Some(pose) = sampled_boxes(w, a) else {
        give_vbuf(w, vp);
        return false;
    };
    let hs: Vec<_> = w
        .fighter(a)
        .montages
        .active_hits
        .values()
        .map(|h| (h.n, h.copy_pose.map(|(anim, time)| OpenPose { anim, time })))
        .collect();
    let mut met = false;
    for (n, copy) in hs {
        let p = hitframe_packet(w, a, n);
        // the World's own shape read
        let sp = hit_spheres(w, a, &p, &pose, CH_HURT, 0.0, Real::INFINITY, copy);
        if !sp.is_empty() && hurt_hit(w, b, &sp, &vp).is_some() {
            met = true;
            break;
        }
    }
    put_pose(w, pose);
    give_vbuf(w, vp);
    met
}

/// Whether the owner and a teammate overlap as actors (OnActorBeginOverlap at the first overlapping component pair,
/// OnActorEndOverlap at the last). The pairs of two fighters that generate events and answer Overlap both ways
/// (DefaultEngine.ini's profiles and the templates): root capsule against root capsule (Pawn / Pawn), and an active
/// hit shape against a hurt box, either way round (HitBox / HurtBox). A hit shape against a root capsule gives none
/// (BaseFighter's construction script sets the capsule's HitBox response to Ignore, @1639), nor do two hurt boxes or
/// two hit shapes. A capsule without collision has no capsule pair; an actor without collision has none.
// Deviation: the pairs are read once a frame, at the component's tick, in the poses the fighters hold there: the build
// fires the events inside the move or the SetCollisionEnabled that changes a pair, so a pair that both begins and ends
// between two of these reads is missed, and when the end comes after a hit shape's SetCollisionEnabled(0) (in the call
// or at the next move) is not read. The hit shapes' class default bGenerateOverlapEvents is taken as on.
fn touching(w: &mut World, f: FighterId, o: FighterId) -> bool {
    if w.fighter(o).life.dead > 0.0 || w.fighter(f).life.dead > 0.0 || collision_off(w, o) || collision_off(w, f) {
        return false;
    }
    if !no_collision(w, o) && !no_collision(w, f) {
        let (r1, r2) = (cap_radius(w, f), cap_radius(w, o));
        let s1 = max(capsule_half(w, f) - r1, 0.0);
        let s2 = max(capsule_half(w, o) - r2, 0.0);
        let (c1, c2) = (actor_y(w, f), actor_y(w, o));
        let dy = max(max(c1 - s1 - (c2 + s2), c2 - s2 - (c1 + s1)), 0.0);
        if size2d([w.fighter(f).body.x - w.fighter(o).body.x, dy]) <= r1 + r2 {
            return true;
        }
    }
    hit_over_hurt(w, o, f) || hit_over_hurt(w, f, o)
}

/// TurnOnPilotActivationPrompt (@3855, the begin overlap): not HasPilot, another actor that is a fighter of the
/// owner's team (@3855-4300), JumpOnAlly not valid, and not a Pawn_C017 in AlternateMode (@4320-4480) ->
/// BindInputMulticast(actor, true) (@4516 -> @11650: JumpOnAlly, the ally's ItemInputPressed -> JumpIn and
/// InputPreTriggered -> JumpInCheck) and JumpOnUiMulticast(actor, true) (@4615).
/// TurnOffPilotActivationPrompt (@4825, the end overlap): for JumpOnAlly, the label off (@8055) and the unbind.
fn overlap_events(w: &mut World, f: FighterId, ready: bool) {
    let all = match_fighters(w);
    let team = w.fighter(f).team.team;
    let mut now: Vec<i32> = Vec::new();
    for &o in &all {
        if o != f && w.fighter(o).team.team == team && touching(w, f, o) {
            now.push(w.fighter(o).id);
        }
    }
    let t = state(w, f);
    if t.seed {
        t.seed = false;
        t.over = now;
        return;
    }
    for &o in &all {
        let oid = w.fighter(o).id;
        let alt = giant_of(w, o).is_some_and(|g| g.alternate_mode);
        let bits = {
            let i = &w.fighter(o).controls.inp;
            i32::from(i.attack) | (i32::from(i.item) << 1)
        };
        let t = state(w, f);
        let (was, is) = (t.over.contains(&oid), now.contains(&oid));
        if is && !was {
            if t.has_pilot || t.jump_on_ally.is_some() || alt {
                continue;
            }
            if !ready {
                warn_text(w, "ally turret: Comp_C017_Ally_Turret class defaults / Buff_C017_Ally_Pilot_C not exported");
                continue;
            }
            t.jump_on_ally = Some(o);
            t.ally_prev = bits;
        } else if was && !is && t.jump_on_ally == Some(o) {
            jump_on_ui_off(t);
            t.jump_on_ally = None;
        }
    }
    state(w, f).over = now;
}

/// The ally's bound inputs. JumpInCheck (@13394), on InputPreTriggered: input type 0 (the normal attack) with
/// direction 4 (neutral), the ally's player data found and its AttackPicksUpItems preference -> JumpIn.
/// ItemInputPressed -> JumpIn (@12067): refused while the ally's BPI_HitstunActive (@12192) or, for a Pawn_C017, its
/// AlternateMode; else @6790: ActivePilot = JumpOnAlly, the ally's Comp_State_Manager.SpawnBuffActor(
/// Buff_C017_Ally_Pilot, ally, the owner, 0, 0.0, true, false) kept as PilotBuff (@6880), then @5708: the label off,
/// HasPilot, BlockPilotEnter, the blast box's two corners (@5772-6199), CompSpawnActor(C017_Pilot_Reticle at (0, 500,
/// 500)) (@6364) and PlayMontageMulticast(C017_WeaponMode_In_Tenticle_Montage) (@6669). The reticle's BeginPlay calls
/// SetReticleControl(self) (C017_Pilot_Reticle @1204 -> @9226): AimLocation = its location and the component's tick
/// on.
// Deviation: the ally's presses are read at the component's tick from the buttons' rising edges (InputPreTriggered is
// the first statement of NormalAttackPressedLogic, BaseFighter @126257, before any state test; the press's own attack
// has started by then and the buff's stun stops it); ItemInputPressed (ItemInputPressMulticast @173979, from the item
// press's flow @68465) is taken as every Item press, the flow's own gate ahead of it is not read. A bot ally never
// climbs in.
// Unverified: the corners are taken as the bounds of BlastBoxPlayerExit's CollisionComp (the World's blast).
/// The reticle's start is the spawner's place for a spawn without a socket and without a world transform (Comp_Spawner
/// HandleActorPostSpawn @1083-2417: the owner's location - (0, 0, 100) + the translation, its Y to the owner's front).
fn ally_inputs(w: &mut World, f: FighterId) {
    let Some(a) = state(w, f).jump_on_ally else { return };
    let i = w.fighter(a).controls.inp;
    let cur = i32::from(i.attack) | (i32::from(i.item) << 1);
    let t = state(w, f);
    let pressed = cur & !t.ally_prev;
    t.ally_prev = cur;
    if w.fighter(a).controls.bot_controlled || pressed == 0 {
        return;
    }
    // The attack press (@13394-13989): Neutral and the ally's own AttackPicksUpItems.
    if pressed & 2 == 0 && (raw_press_dir(w, a) != Dir::Neutral || !w.fighter(a).controls.prefs.attack_picks_up_items) {
        return;
    }
    if in_hitstun(w, a) || giant_of(w, a).is_some_and(|g| g.alternate_mode) {
        return;
    }
    state(w, f).pilot = Some(a);
    let b = spawn_buff(w, a, name!("Buff_C017_Ally_Pilot_C"), 0.0, Some(0), Some(f), true, None);
    let t = state(w, f);
    t.buff = b;
    t.buff_on = b.map(|_| a);
    if let Some(b) = b {
        let k = num(hold_def(w, a, name!("Buff_C017_Ally_Pilot_C")), "BlastzoneBuffer").unwrap_or(0.0);
        let bl = &w.stage.blast;
        let lim = BlastBounds { left: bl.left + k, right: bl.right - k, bottom: bl.bottom + k, top: bl.top - k };
        if let Some(e) = hold_mut(w, b) {
            e.lim = Some(lim);
        }
    }
    let bl = BlastBounds { ..w.stage.blast };
    let (fx, fy, fc) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
    let t = state(w, f);
    jump_on_ui_off(t);
    t.has_pilot = true;
    block_pilot_enter(t);
    t.left = bl.left;
    t.right = bl.right;
    t.bottom = bl.bottom;
    t.top = bl.top;
    // reticle goes on before the spawn: a reticle of an earlier pilot still listed counts as live there and is the
    // one NumberOfActorsAllowed destroys.
    t.reticle = true;
    t.aim_x = fx + 500.0 * fc;
    t.aim_y = fy - 100.0 + 500.0;
    let (ax, ay) = (t.aim_x, t.aim_y);
    sync_reticles(w, f);
    if spawn_pilot_reticle(w, f, ax, ay, Some(a)).is_none() {
        state(w, f).reticle = false;
        sync_reticles(w, f);
    }
    let i = w.fighter(a).controls.inp;
    state(w, f).pilot_prev =
        i32::from(i.attack) | (i32::from(i.special) << 1) | (i32::from(i.jump) << 2) | (i32::from(i.dodge) << 3);
    play_grouped(w, f, name!("C017_WeaponMode_In_Tenticle_Montage"), false, false);
}

/// FClamp: the min wins over a max below it.
fn clamp(v: Real, lo: Real, hi: Real) -> Real {
    if v < lo {
        lo
    } else if v < hi {
        v
    } else {
        hi
    }
}

/// ReceiveTick with ActivePilot valid (@9103 -> @1181): the pilot's BPI_GetMoveAxisValues; with either not 0 the
/// target (0, MoveX > 0 ? -1 : 1 or 0, MoveY > 0 ? 1 : -1 or 0) (@1440-1772), AimVector = VInterpTo_Constant(
/// AimVector, target, dt, 10) (@1823), AimLocation = (0, FClamp(Y, BR.Y + BlastBoxBuffer, TL.Y - BlastBoxBuffer),
/// FClamp(Z, BR.Z + BlastBoxBuffer, TL.Z - BlastBoxBuffer)) of AimLocation + AimVector x (AimSpeed x dt)
/// (@1910-2531), the reticle's K2_SetActorLocation (@2558). With AimSpeed > 0, once per stretch of input:
/// sfx_..._move_start then sfx_..._loop (@3034, @3150). Without input (@2644): AimVector zero and, once per stretch,
/// sfx_..._loop_stop (@2769; the first tick without input posts it whether a loop ran or not).
/// Before it (@9221 -> @1129): a polymorphed owner (Is Polymorphed) -> KickPilot(true) @1165, every tick, in place of
/// the aim ([`component_tick`]). The tick tests ActivePilot alone (@9130): it goes on without the reticle
/// (C017_Pilot_Reticle's arena exit), whose actor follows aim_x / aim_y.
// Unverified: MoveX above 0 is taken as the stick to screen right (sim +x = UE -Y).
fn aim_tick(w: &mut World, f: FighterId, pilot: FighterId, p: Vals) {
    let (mx, my) = (w.fighter(pilot).controls.inp.x, w.fighter(pilot).controls.inp.y);
    if mx == 0.0 && my == 0.0 {
        let t = state(w, f);
        t.aim_vx = 0.0;
        t.aim_vy = 0.0;
        t.move_open = true;
        if t.stop_open {
            t.stop_open = false;
            sfx(w, f, name!("sfx_cha_C017_weaponsmode_tenticle_attack_loop_stop"));
        }
        return;
    }
    let dt = DT;
    let sign = |v: Real| {
        if v == 0.0 {
            0.0
        } else if v > 0.0 {
            1.0
        } else {
            -1.0
        }
    };
    let (tx, ty) = (sign(mx), sign(my));
    let t = state(w, f);
    // FMath::VInterpConstantTo: the whole way when the distance is within speed x dt.
    let (dx, dy) = ((tx - t.aim_vx), (ty - t.aim_vy));
    let (dist, step) = (size2d([dx, dy]), (10.0 * dt));
    if dist > step {
        t.aim_vx += (dx / dist) * step;
        t.aim_vy += (dy / dist) * step;
    } else {
        t.aim_vx = tx;
        t.aim_vy = ty;
    }
    let (speed, buf) = ((p.aim_speed * dt), p.blast_box_buffer);
    t.aim_x = clamp(t.aim_x + (t.aim_vx * speed), t.left + buf, t.right - buf);
    t.aim_y = clamp(t.aim_y + (t.aim_vy * speed), t.bottom + buf, t.top - buf);
    if !(p.aim_speed > 0.0) || !t.move_open {
        return;
    }
    t.move_open = false;
    t.stop_open = true;
    sfx(w, f, name!("sfx_cha_C017_weaponsmode_tenticle_attack_move_start"));
    sfx(w, f, name!("sfx_cha_C017_weaponsmode_tenticle_attack_loop"));
}

/// The owner's socket `name` in his sampled pose, in sim space; `None` for a socket the data lacks or no pose.
fn socket_at(w: &World, f: FighterId, key: Name) -> Option<(Real, Real)> {
    let ch = w.fighter(f).ch;
    let o = w.fixed.samplers.get(&ch).and_then(|b| {
        let si = *b.by_name.get(&key)?;
        b.offsets.get(usize::try_from(si).ok()?).copied()
    })?;
    let pose = sampled_boxes(w, f)?;
    let o = usize::try_from(o).expect("a box offset");
    let r = (mesh_wx(w, f) + crate::real(pose[o]) * facing(w, f), mesh_wy(w, f) + crate::real(pose[o + 1]));
    put_pose(w, pose);
    Some(r)
}

/// TryPilotFastShot (@10040): AttackMontageNumber = 0 after 2, else + 1, then PilotFastAttack(the number)
/// (@10400-11212): the main mesh's GetSocketLocation(fml_un_R / L / C_tentacle8 for 0 / 1 / 2), CompSpawnActor(
/// C017_TenticalFastShot, world transform (0, socket Y, socket Z), direction = GetDirectionUnitVector(that,
/// AimLocation) with Y negated for an owner facing right, InitialSpeed = MaxSpeed = ProjectileSpeed, AttackID 0), and
/// PilotFastAttackMulticast: Montage_Play(C017_WeaponMode_Tenticle_Attack_01 / 03 / 02 for 0 / 1 / 2, 1, 0, 0, false)
/// (@12610), a TenticleGroup instance beside the others of its group.
// Unverified: the spawner's handling of the direction (HandleActorPostSpawn mirrors a notify's direction by the
// owner's facing) is taken to undo the graph's negation, so the shot leaves toward AimLocation for either facing. The
// socket is read from the owner's composed pose (pose.rs: the weapon mode override, the TenticleGroup instances and
// the aim blend space on the tentacle chain), the same name at either facing (the graph's literal on the main mesh,
// which has no mirror node).
/// A socket the data lacks spawns nothing.
fn fast_shot(w: &mut World, f: FighterId, p: Vals) {
    let t = state(w, f);
    t.shot = if t.shot == 2 { 0 } else { t.shot + 1 };
    let (m, socket, socket_s) = shot_parts(t.shot);
    play_grouped(w, f, m, false, false);
    let cls = name!("C017_TenticalFastShot_C");
    let has = actor_def(w, ActorRef::Fighter(f), cls).is_some();
    let at = socket_at(w, f, socket);
    let (true, Some((x, y))) = (has, at) else {
        let m = if has { format!("spawn socket missing: {socket_s}") } else { format!("{FAST_SHOT} not exported") };
        warn_text(w, &m);
        return;
    };
    let t = state(w, f);
    let (dx, dy) = (t.aim_x - x, t.aim_y - y);
    let n = safe_normal([0.0, dx, dy], SMALL_NUMBER); // GetDirectionUnitVector
    let fc = facing(w, f);
    let mut dir = Props::new();
    dir.insert("X".into(), Value::Num(0.0));
    dir.insert("Y".into(), Value::Num(f64::from(n[1] * fc)));
    dir.insert("Z".into(), Value::Num(f64::from(n[2])));
    let mut np = Props::new();
    np.insert("InitialMovementDirection".into(), Value::Obj(dir));
    np.insert("InitialSpeed".into(), Value::Num(f64::from(p.projectile_speed)));
    np.insert("MaxSpeed".into(), Value::Num(f64::from(p.projectile_speed)));
    let count = w.fighter(f).decay.attack_count;
    let class = ProjectileClass::C017TenticalFastShot;
    let made = make_projectile(w, cls, class, Some(f), count, &np, Some(V2 { x, y }), MakeOpts::default());
    let id = w.actors.projectiles.insert(made);
    add_projectile(w, id, None);
}

/// PilotLargeAttackVisuals' Delay(0.3) ends (@66): the charge emitter destroyed, the main mesh's
/// GetSocketLocation(giantEmblem_att) (@456) and CompSpawnActor(C017_Ally_Turret_Bomb, world transform ((0, socket Y,
/// socket Z), zero rotation, scale (ProjectileSpeed, 1, 1)), the fast shot's direction rule, InitialSpeed = MaxSpeed =
/// ProjectileSpeed) (@974). The bomb takes its speed from the scale and neither the direction nor the speeds
/// (turret_actors.rs), so the spawner's direction handling does not reach it. The Delay is the component's: it ends,
/// and the bomb is spawned, with or without a pilot. The emblem is read from the owner's sampled pose, as the fast
/// shot's socket; a socket the data lacks spawns nothing.
fn large_attack(w: &mut World, f: FighterId, p: Vals) {
    let Some((x, y)) = socket_at(w, f, name!("socket:giantEmblem_att")) else {
        warn_text(w, "spawn socket missing: giantEmblem_att");
        return;
    };
    spawn_turret_bomb(w, f, x, y, p.projectile_speed);
}

/// PilotInputs(InputType) (@8263), bound by the buff to the pilot's InputPreTriggered (Buff_C017_Ally_Pilot @4935 ->
/// @7529), and PilotInputReleased (@4999 -> @7659 -> @11441: button 0 -> IsNormalAttackHeld off).
/// 0 (@8475): IsNormalAttackHeld; with AutoFireTimer not active, the looping timer of AutoFireRate (callback @12822:
/// not held -> the timer cleared, else TryPilotFastShot) and TryPilotFastShot now (@8638).
/// 1 -> CheckCooldownLargeAttack (@9707): the owner's HasAbilityResources(1) -> TriggerAbilityActual(1) and
/// PilotLargeAttackServer (@8653: the charge emitter, sfx_cha_C017_weaponmode_bomb_leadin on the owner @9013,
/// Delay(0.3)); else LargeAttackCooldownServer (@12846: ui_ability_cooldown for a locally controlled pilot).
/// 2 and 3 (jump, evade) -> KickPilot(true) (@8444).
// Deviation: as the ally's, the pilot's presses are the buttons' edges at the component's tick, in the fixed order
// attack, special, jump, evade; a bot pilot's presses are not read.
/// The timer set here is pending until the frame's timer pass ([`ally_turret_timers`]); K2_IsTimerActiveHandle counts
/// a pending timer (fire not 0). A Rate of 0 or less sets no timer (knockback's timerTicks rule), so each press then
/// fires its own shot.
fn pilot_inputs(w: &mut World, f: FighterId, pilot: FighterId, p: Vals) {
    let i = w.fighter(pilot).controls.inp;
    let cur = i32::from(i.attack) | (i32::from(i.special) << 1) | (i32::from(i.jump) << 2) | (i32::from(i.dodge) << 3);
    let t = state(w, f);
    let (pressed, released) = (cur & !t.pilot_prev, t.pilot_prev & !cur);
    t.pilot_prev = cur;
    let buff = t.buff;
    if buff.is_none_or(|b| frames(w, b) <= 0.0) || w.fighter(pilot).controls.bot_controlled {
        return;
    }
    if pressed & 1 != 0 {
        let t = state(w, f);
        t.held = true;
        if t.fire == 0 {
            t.fire_rate = p.auto_fire_rate;
            if t.fire_rate > 0.0 {
                t.fire = 1;
            }
            fast_shot(w, f, p);
        }
    }
    if pressed & 2 != 0 {
        if has_ability_resources(w, f, 1) {
            trigger_ability(w, f, 1);
            sfx(w, f, name!("sfx_cha_C017_weaponmode_bomb_leadin"));
            if state(w, f).bomb <= 0 {
                let k = to_int32(latent(w, f, 0.3));
                state(w, f).bomb = k;
            }
        } else {
            sfx(w, pilot, name!("ui_ability_cooldown"));
        }
    }
    if released & 1 != 0 {
        state(w, f).held = false;
    }
    if pressed & 12 != 0 {
        kick_pilot(w, f, true);
    }
}

/// `allyTurretTimers`: AutoFireTimer in the world's FTimerManager::Tick (FUN_142e45520), which UWorld::Tick runs once a
/// frame after every tick group up to TG_PostPhysics and the leftover latent pass: after this frame's inputs, the
/// component's tick (the aim) and every actor's tick.
/// A timer set since the last pass becomes active here with ExpireTime = InternalTime + Rate, so the frame it was set
/// in does not count. An active one: InternalTime += DeltaSeconds; with InternalTime > ExpireTime (strict) the
/// delegate is called CallCount = trunc((InternalTime - ExpireTime) / Rate) + 1 times in a row (a call that clears the
/// timer ends them), then ExpireTime += (float)(CallCount x Rate): the loop is re-armed from its own expiry, not from
/// the clock, and the remainder carries. The delegate (@12841 -> @12822): IsNormalAttackHeld off -> the timer cleared
/// (@12801), else TryPilotFastShot (@10040).
/// The times are kept since the activating pass: sums of float32 deltas and Rates in a double are exact, so the
/// compare is the manager's on its absolute clock.
/// The World's timer pass (`CharHooks.timer_tick`): after the frame's fighters, projectiles, actors and items, so a
/// shot of the timer first moves in the next frame.
// Unverified: whether an actor spawned from the timer pass still ticks in that frame is not read. The timer's order
// against the frame's other timers (by ExpireTime) is not kept either.
pub fn ally_turret_timers(w: &mut World, f: FighterId) {
    let Some(t) = turret(w, f) else { return };
    if t.fire == 0 {
        return;
    }
    if t.fire == 1 {
        t.fire = 2;
        t.fire_t = 0.0;
        t.fire_at = f64::from(t.fire_rate);
        return;
    }
    t.fire_t += f64::from(DELTA_SECONDS);
    if !(t.fire_t > t.fire_at) {
        return;
    }
    let calls = crate::math::dbl::to_int32(((t.fire_t - t.fire_at) / f64::from(t.fire_rate)).trunc()) + 1;
    let p = defaults(w, f);
    let mut i = 0;
    while i < calls && state(w, f).fire == 2 {
        if !state(w, f).held {
            state(w, f).fire = 0;
        } else if let Some(p) = p {
            fast_shot(w, f, p);
        }
        i += 1;
    }
    let t = state(w, f);
    if t.fire == 2 {
        t.fire_at += f64::from(crate::real(calls) * t.fire_rate);
    }
}

/// `allyTurretTick`: the component's tick and its latent actions, once a tick after the fighters' own component ticks
/// (`CharHooks.world_tick`).
// Unverified: the component's place among the tick functions of its group is not read (as Comp_C017_Hover_Rage's).
pub fn ally_turret_tick(w: &mut World, f: FighterId) {
    component_tick(w, f);
}

fn component_tick(w: &mut World, f: FighterId) {
    if turret(w, f).is_none() {
        return;
    }
    let p = defaults(w, f);
    if state(w, f).enter {
        overlap_events(w, f, p.is_some());
    }
    // KickPilot's Delay (@15).
    let t = state(w, f);
    if t.kick > 0 {
        t.kick -= 1;
        if t.kick == 0 {
            t.has_pilot = false;
            if t.in_weapon_mode {
                allow_pilot_enter(t);
            }
        }
    }
    let Some(p) = p else { return };
    // PilotLargeAttackVisuals' Delay(0.3) (@8938 -> @66).
    let t = state(w, f);
    if t.bomb > 0 {
        t.bomb -= 1;
        if t.bomb == 0 {
            large_attack(w, f, p);
        }
    }
    if state(w, f).enter {
        ally_inputs(w, f);
    }
    let Some(pilot) = state(w, f).pilot else { return };
    pilot_inputs(w, f, pilot, p);
    if state(w, f).pilot != Some(pilot) {
        return;
    }
    if w.fighter(f).shapes.is_polymorphed {
        // @1129 -> @1165
        kick_pilot(w, f, true);
    } else {
        aim_tick(w, f, pilot, p);
    }
}

/// `allyTurretReset`: the component at the owner's reset (it has no handler of its own: its variables are a new
/// pawn's). Called by pawn.rs (the reset hook).
// Unverified: a match reset is taken to end the turret as the mode's end does, without the Out montage.
pub fn ally_turret_reset(w: &mut World, f: FighterId) {
    let Some(t) = turret(w, f) else { return };
    let (b, on) = (t.buff, t.buff_on);
    if let Some(b) = b
        && frames(w, b) > 0.0
        && let Some(on) = on
    {
        end_buff(w, on, b);
    }
    giant_w(w, f).ally_turret = None;
    sync_reticles(w, f);
}

/// K2_SetActorLocation(owner, (0, 0, 500), no sweep, teleport) (@2439, @2733).
fn pilot_place(w: &mut World, vic: FighterId) {
    recall_move(w, vic, 0.0, 500.0);
}

// Buff_C017_Ally_Pilot: a child of BuffParentActor on the teammate in the turret, its Instigator the Giant. Its tick is
// off; the graph is latent actions and delegates, the same frame as Buff_HeldByPlasmaBall's (hold.rs), whose notes on
// the grapple natives and the latent actions' tick hold here.

/// ReceiveBeginPlay @4811, after the parent's: CheckHitstunActive -> RemoveHitstunFromKnockBack (@2908), the blast
/// limits (@5561-6516: the blast box's corners inset by BlastzoneBuffer; set by the turret here, which has the world),
/// the owner's InputPreTriggered / InputReleased bound to the turret's PilotInputs / PilotInputReleased (@4935,
/// @4999), Comp_FighterVisibilityManager.AddInvisibility (@5134), BPI_ResetJump (@5269), the owner's widgets hidden
/// (@5399), then the BuffstunTimer poll (@226: active -> Delay(0.033) @363; else at most 5 times Delay(0.033) and the
/// test again, @443-556). SpawnBuffActor starts the buff's own stun (Stun.StunDuration 60000) after BeginPlay.
// Unverified: a knockback hitstun running at the spawn is not cleared here (JumpIn refuses an ally in hitstun, @12192,
// so none runs).
fn pilot_begin(w: &mut World, vic: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    let stunned = w.fighter(vic).buffs.buff_stunned;
    let wait = to_int32(max(latent(w, vic, 0.033) - 1.0, 1.0));
    if let Some(x) = buff_mut(w, b) {
        x.src = from;
        x.ext = BuffExt::AllyPilot(PilotHold {
            begun: true,
            stage: if stunned { 1 } else { 0 },
            wait,
            tries: if stunned { 0 } else { 1 },
            snap: 0,
            lim: None,
            ended: false,
        });
    }
    add_invisibility(w, vic);
    reset_jump_counts(w, vic);
}

fn pilot_tick(w: &mut World, vic: FighterId, b: BuffRef) -> bool {
    let stunned = w.fighter(vic).buffs.buff_stunned;
    let Some(e) = hold_mut(w, b) else { return false };
    if e.stage == 2 {
        // @2733, after the Delay(0.06): the same K2_SetActorLocation.
        if e.snap > 0 {
            e.snap -= 1;
            if e.snap == 0 {
                pilot_place(w, vic);
            }
        }
        return false;
    }
    if e.wait <= 0 {
        return false;
    }
    e.wait -= 1;
    if e.wait > 0 {
        return false;
    }
    if e.stage == 0 {
        if stunned {
            e.stage = 1;
        } else if e.tries < 5 {
            e.tries += 1;
        } else {
            // the gate is used up: the buff stays, the pilot is never placed
            return false;
        }
        let k = to_int32(latent(w, vic, 0.033));
        if let Some(e) = hold_mut(w, b) {
            e.wait = k;
        }
        return false;
    }
    // @611: PS_C017_UI on the owner's camera and sfx_cha_C017_weaponsmode_enter_ui (@700, a locally controlled owner),
    // the buff destroyed with the Giant's or the owner's EndPlay (@766-975). @1092: DisableMovement,
    // FMC.OverrideGravity(0.0), InitiateGrapple, SetActorEnableCollision(false), UI_IGv3_AttachedBottom hidden,
    // K2_SetActorLocation((0, 0, 500)) @2439, Delay(0.06) -> the same again @2733.
    e.stage = 2;
    let fid = w.fighter(vic).id;
    w.fighter_mut(vic).events.push(Event::Sound {
        fighter: fid,
        ev: name!("sfx_cha_C017_weaponsmode_enter_ui"),
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    });
    // @1345
    override_gravity(w, vic);
    grapple_on(w, vic);
    // MOVE_None from @1163 to EndPlay's Falling
    disable_movement(w, vic);
    w.fighter_mut(vic).capsule.capsule_off = true;
    pilot_place(w, vic);
    let k = to_int32(latent(w, vic, 0.06));
    if let Some(e) = hold_mut(w, b) {
        e.snap = k;
    }
    false
}

/// ReceiveEndPlay @6662, after the parent's (the stun's end): the widgets back; @3039 (authority): zero rotation
/// @3545, SetMovementMode(Falling) @3672, FMC.RemoveGrapple and ClearGravityOverride (@3748-4055), the owner's
/// K2_SetActorLocation((0, the Instigator's Y and Z, each FClamp'ed to the blast limits), teleport) @4107-4364,
/// SetActorEnableCollision(true) @4446, RemoveInvisibility @4550, the bottom widget shown @4586, Delay(0.2) @4620 ->
/// RemoveHitstunFromKnockBack with HitstunBuffActive (not resumed: Unverified, see hold.rs).
// Unverified: the Instigator is the Giant (SpawnBuffActor's argument, @6880); a Giant that is out (his death ends the
// turret: KickPilot(false)) is read where the port keeps him.
fn pilot_end(w: &mut World, vic: FighterId, b: BuffRef) {
    let Some(e) = hold_mut(w, b) else { return };
    if e.ended {
        return;
    }
    e.ended = true;
    let (stage, lim) = (e.stage, e.lim);
    let src = buff(w, b).and_then(|x| x.src);
    // UpdateCurrentStates: the stun ends, or another live buff's stun starts
    apply_stun_buffs(w, vic, None, true);
    if stage == 2 {
        // SetMovementMode(Falling) @3672
        enable_falling(w, vic);
        grapple_off(w, vic);
    }
    // @4055, placed or not
    clear_gravity_override(w, vic);
    if let (Some(g), Some(l)) = (src, lim) {
        let (gx, gy) = (w.fighter(g).body.x, actor_y(w, g));
        recall_move(w, vic, clamp(gx, l.left, l.right), clamp(gy, l.bottom, l.top));
    }
    w.fighter_mut(vic).capsule.capsule_off = false;
    remove_invisibility(w, vic);
}

/// The hooks of this file in Iron Giant's CharHooks (`mod.rs` character: worldTick allyTurretTick, timerTick
/// allyTurretTimers).
pub fn hooks(h: &mut CharHooks) {
    h.world_tick = Some(ally_turret_tick);
    h.timer_tick = Some(ally_turret_timers);
}

/// This file's registry entries: Buff_C017_Ally_Pilot_C, then C017_TenticalFastShot_C (a ParentProjectilePhysics
/// child with no graph of its own: components and defaults only).
// Unverified: the fast shot's lifetime and gravity are the parent's rules on the spawner's arguments.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        PILOT_BUFF,
        BuffEntry::new(
            crate::combat::buff_class::BuffClass::BuffC017AllyPilot,
            BuffHooks {
                begin: Some(pilot_begin),
                tick: Some(pilot_tick),
                end: Some(pilot_end),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.actors.enter(
        FAST_SHOT,
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::C017TenticalFastShot),
            ActorHooks::Projectile(ProjectileHooks::default()),
        ),
    )?;
    Ok(())
}
