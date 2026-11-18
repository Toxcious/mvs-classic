//! Montages, branches, root motion, the pose and the anim state machines (the fighter's fields,
//! written by the montage, branch and anim code of this module).

use crate::Real;
use crate::State;
use crate::combat::ActiveHit;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::input::{AttackBtn, Dir};
use crate::names::Name;
use crate::world::World;
use indexmap::{IndexMap, IndexSet};
use mvs_anim::montage::MontageProxy;
use mvs_data::fighter::AttackEntry;
use mvs_data::fighter::Branch;

pub mod blend_spaces;
pub mod box_pose;
pub mod branches;
pub mod grouped;
pub mod machines;
pub mod montages;
pub mod notify_sink;
pub mod overrides;
pub mod root_motion;
pub mod sequences;
pub mod slot_blend;
pub mod slot_mix;
pub mod taunts;
pub mod update;
pub mod visibility;

pub use branches::*;
pub use grouped::{
    active_grouped, active_instance, clear_grouped, grouped_done, grouped_late_ends, stop_active_grouped,
    stop_anim_montage, stop_grouped, stop_grouped_entry,
};
pub use montages::{
    COPY_INST, FOREIGN_INSTS, KEY_STRIDE, MontageKey, POLY_INST, active_of, advance_montage, any_active, any_notify,
    clear_fading, flush_proxy_events, has_active, hit_boxes_off_at, is_dash_anim, new_active_hit, notify_data,
    open_proxy_events, owner_of, play_intro_montage, play_plain_montage, proxy_clear, resolve, slot_name,
    start_montage, stop_dash_montage, stop_montage, stopped_owner, terminate_fading,
};
pub use notify_sink::{begin, end};
pub use root_motion::{consume_root, root_montage};

/// A playing montage: mvs-anim's player state, keyed by its montage and
/// its play's number ([`MontageKey`]). The player's calls run through `montages::with_player`, which pairs it with
/// the montage's data (`World::data`, by the fighter's character and the name) and the fighter's sink.
pub type MontageState = mvs_anim::montage::MontageState<MontageKey>;

/// `Montages.Fade`: a montage blending out with its hits.
#[derive(State, Debug)]
pub struct Fade {
    pub left: Real,
    /// The blend time (FAlphaBlend's float), compared with the next montage's BlendIn.
    pub time: Real,
    pub m: MontageState,
    pub hits: Option<Vec<ActiveHit>>,
    /// `tags`: the HitBoxTags of `hits`.
    pub tags: Option<Vec<String>>,
    /// `ends`: the fade's pending ends.
    pub ends: Option<Vec<NotifyRef>>,
}

impl Montages {
    /// A write to the active Hitframe at `key` (`activeHits.get(key)`), made to the fade's copy of it too where a
    /// stopped instance's fade keeps it lingering (`Fade.hits`: the fade and `activeHits` share the one hit;
    /// a lingering hit's key is its own, below 0).
    pub fn active_hit_write(&mut self, key: i32, mut op: impl FnMut(&mut ActiveHit)) {
        let Some(h) = self.active_hits.get_mut(&key) else { return };
        op(h);
        if key >= 0 {
            let n = h.n;
            for c in self.stop_keep.iter_mut().filter(|c| c.idx == key && c.n == n) {
                op(c);
            }
            return;
        }
        let n = h.n;
        for fd in &mut self.fading {
            for c in fd.hits.iter_mut().flatten() {
                if c.idx == key && c.n == n {
                    op(c);
                }
            }
        }
    }
}

/// `GroupedEntry`: a montage of a group play.
#[derive(State, Debug)]
pub struct GroupedEntry {
    pub m: MontageState,
    /// `fade`: left, of, time, w.
    pub fade: Option<[Real; 4]>,
    pub top: bool,
    pub inst: Option<i32>,
    pub charge: bool,
}

/// `QueuedPress`'s button: a `Press` or a taunt.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum QueuedBtn {
    Press(crate::input::Btn),
    Taunt,
}

/// `QueuedAction`: the newest press (a [`QueuedPress`]'s fields) and the windows that hold a
/// press.
#[derive(State, Debug)]
pub struct QueuedAction {
    pub press: QueuedBtn,
    pub dir: Dir,
    pub force: Option<ForcedPress>,
    /// `taunt`: GetTauntBranchStruct of the pressed slot.
    pub taunt: Option<Box<Branch>>,
    pub raw: Dir,
    /// The keys of the windows on at the press that have not ended.
    pub windows: Vec<i32>,
    /// `older`: the press a window the InputBufferCap left out kept, by window key.
    pub older: Option<IndexMap<i32, QueuedPress>>,
}

impl QueuedAction {
    /// The newest press alone.
    pub fn newest(&self) -> QueuedPress {
        QueuedPress { press: self.press, dir: self.dir, force: self.force, taunt: self.taunt.clone(), raw: self.raw }
    }

    /// `{ ...p, windows }`.
    pub fn of(p: QueuedPress, windows: Vec<i32>) -> Self {
        QueuedAction { press: p.press, dir: p.dir, force: p.force, taunt: p.taunt, raw: p.raw, windows, older: None }
    }
}

/// `OverridePack`: Struct_AbilityOverridePack.
#[derive(State, Debug, Default)]
pub struct OverridePack {
    pub priority: Real,
    pub overrides: Vec<OverrideRow>,
    /// The pack is an object of the loaded game data held by reference (the trace writes it as `{static: "object"}`).
    pub from_data: bool,
}

/// `OverrideRow`: Struct_AbilityOverride.
#[derive(State, Debug)]
pub struct OverrideRow {
    pub aerial_move: bool,
    pub attack: AttackEntry,
}

/// `ForeignWindow`.
#[derive(State, Debug)]
pub struct ForeignWindow {
    pub n: NotifyRef,
    pub key: i32,
    pub cur: Name,
}

/// `lateBranch`.
#[derive(State, Debug)]
pub struct LateBranch {
    pub ws: Vec<(NotifyRef, i32)>,
    pub q: Option<QueuedAction>,
    pub cur: Name,
}

/// `lateAuto`.
#[derive(State, Debug)]
pub struct LateAuto {
    pub ws: Vec<NotifyRef>,
    pub cur: Name,
}

/// `chargeRamp`.
#[derive(State, Debug, Default)]
pub struct ChargeRamp {
    pub n: i32,
    pub at: crate::Clock,
    pub span: Real,
    pub pending: bool,
}

/// `dispatchOwed`: a montage step whose notifies are owed (`{ m, h: Advanced }`): the instance by its number and the
/// steps (`Advanced`).
#[derive(State, Debug)]
pub struct DispatchOwed {
    pub m: u32,
    pub ranges: Vec<[f64; 4]>,
    pub gone: bool,
}

/// The montage slots and their bookkeeping.
#[derive(State, Debug, Default)]
pub struct Montages {
    pub montage: Option<MontageState>,
    pub montage_hit: bool,
    /// `activeHits`, by notify index.
    pub active_hits: IndexMap<i32, ActiveHit>,
    pub fading: Vec<Fade>,
    pub linger_seq: i32,
    /// The hits a stop keeps for its fade (`stopMontage`'s `keep`), while the stop's end handlers run: a write to
    /// one reaches its copy here, as it does for the one shared hit, even after its end took it from `activeHits`.
    /// Empty outside a stop.
    pub stop_keep: Vec<ActiveHit>,
    pub dash_montage: Option<MontageState>,
    /// `advancing`: the instance whose Advance runs, by its number.
    pub advancing: Option<u32>,
    pub grouped: Option<(Vec<GroupedEntry>, bool)>,
    pub fade_ends: Option<Vec<NotifyRef>>,
    /// `proxyOwed`: the owed handlers' proxies (proxyClear reads the instance's proxy alone).
    pub proxy_owed: Option<Vec<MontageProxy>>,
    pub montage_abort: bool,
    pub played_in_advance: bool,
    pub charge_sound_seq: i32,
    pub charge_sound_key: Name,
    pub charge_ramp: Option<ChargeRamp>,
    pub charge_value: Real,
    pub charge_ramp_delay: Real,
    /// `montageMeshes`: mesh, anim, loop, socket, age.
    pub montage_meshes: Vec<MontageMesh>,
    /// The last number a play was given (`MontageKey::uid`).
    pub play_seq: u32,
    /// Bumped when `grouped` is made anew (an identity test of the grouped instances).
    pub grouped_gen: u32,
    /// The instances a player runs, by number, and those of them a handler dropped from their place: empty
    /// between two calls, so off the state.
    #[state(skip)]
    pub running: Vec<u32>,
    #[state(skip)]
    pub limbo: Vec<MontageState>,
}

/// `montageMeshes` entry.
#[derive(State, Debug, Default)]
pub struct MontageMesh {
    pub mesh: Name,
    pub anim: Name,
    pub looping: bool,
    pub socket: Name,
    pub age: Real,
}

/// The branch manager.
#[derive(State, Debug, Default)]
pub struct Branching {
    pub queued: Option<QueuedAction>,
    /// `winEnds`: BranchEndTime by window key (float32 seconds).
    pub win_ends: Option<IndexMap<i32, Real>>,
    pub button_states: Option<i32>,
    pub br_axis_x: Real,
    pub br_axis_y: Real,
    pub br_stick_x: Real,
    pub defer_queued_ends: bool,
    pub pending_move_ends: Vec<i32>,
    pub pending_ends: Vec<(NotifyRef, i32)>,
    pub late_branch: Option<LateBranch>,
    pub late_branch_due: bool,
    pub late_auto: Option<LateAuto>,
    pub win_cleared: Option<Vec<i32>>,
    pub branch_montage: Option<Name>,
    pub same_attack_blocked: bool,
    pub override_packs: Option<Vec<OverridePack>>,
    pub foreign_wins: Option<Vec<ForeignWindow>>,
    pub charge_btn: Option<ChargeBtn>,
}

/// `chargeBtn`: the button a charge holds.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum ChargeBtn {
    Attack(AttackBtn),
    Jump,
    Dodge,
}

/// Root motion and the step's dispatch flags.
#[derive(State, Debug, Default)]
pub struct RootMotion {
    pub root_dx: Real,
    pub root_dy: Real,
    pub root_has: bool,
    pub pose_ticked: bool,
    pub dispatch_owed: Option<DispatchOwed>,
    pub anim_updated: bool,
    /// `extracting`: the instance whose Advance extracts, set around the call.
    #[state(skip)]
    pub extracting: Option<u32>,
    /// `rootVel`: what the last consume handed the movement component.
    #[state(skip)]
    pub root_vel: (Real, Real),
}

/// The notify sinks (`plainSink`, `dashSink`): which one a montage step
/// dispatches into, matched where the step runs.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum SinkKind {
    /// `plainSink`: begin / end / instant do nothing.
    #[default]
    Plain,
    /// `dashSink`: the dash montage's.
    Dash,
    /// The fighter itself (`Fighter implements NotifySink`).
    Fighter,
    /// `groupedSink`: a grouped instance that is not an attack instance.
    Grouped,
}

/// The sequence players, the loco anim and the hit-react anim.
#[derive(State, Debug, Default)]
pub struct Anim {
    pub anim: Name,
    pub anim_time: Real,
    pub loco_anim: Name,
    pub loco_time: Real,
    pub loco_rate: Real,
    pub loco_loop: bool,
    /// `locoBlend`: dur, hermite.
    pub loco_blend: (Real, bool),
    pub loco_gen: i32,
    pub anim_key: AnimKey,
    pub seq_on: IndexSet<i32>,
    pub seq_from: Real,
    pub seq_delta: Real,
    pub fade_anim: Name,
    pub fade_time: Real,
    pub fade_rate: Real,
    pub fade_loop: bool,
    pub fade_el: Real,
    pub fade_dur: Real,
    pub plain_is_intro: bool,
    pub plain_sink: SinkKind,
    pub dash_sink: SinkKind,
    /// `nav`: the navigation set, swapped at run time by a pawn's own graph.
    pub nav: SetRef,
    /// `SLOTS`: the taunt slots' montages, made at the first read.
    pub taunt_slots: Option<Vec<Name>>,
    /// `poseAt` / `layerAt`: the tick of this frame's last pose update and of the last one that
    /// updated the BaseMachine layer. Set and read inside one tick (dropped at its start by `seq_frame_start`), so off
    /// the state as a map keyed by the fighter would.
    #[state(skip)]
    pub pose_at: Option<i32>,
    #[state(skip)]
    pub layer_at: Option<i32>,
    /// `SEQ_LISTED`: the keys of the states listed by this frame's update, emptied at each use.
    #[state(skip)]
    pub seq_listed: Vec<i32>,
    /// `holdPack`: the held item's montage overrides (`ch.nav.hold`).
    pub hold_pack: Vec<HoldOverride>,
    /// Base_AnimBP's IsBeingCarried with its CarryInt and the carried player's time (`update::set_being_carried`).
    pub carried: Option<Carried>,
}

/// Base_AnimBP's carried pose (OnCarried @0-56: IsBeingCarried = IsCarried, CarryInt = BoolToInt(IsAbove)): the layer
/// root BlendListByBool_16 on IsBeingCarried shows BlendListByInt_2 on CarryInt over the anim set's CarriedBelow (0)
/// and CarriedAbove (1), looped, in place of the movement machine.
#[derive(State, Copy, PartialEq, Debug)]
pub struct Carried {
    /// CarryInt 1: CarriedAbove.
    pub above: bool,
    /// The sequence player's time.
    pub time: Real,
}

/// `animKey`: what the shown pose is a play of, the string `'m' + anim` for a
/// montage's clip and `'g' + locoGen` for the machines' player; `''` before the first update.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum AnimKey {
    #[default]
    None,
    Montage(Name),
    Gen(i32),
}

/// `HoldOverride`.
#[derive(State, Debug, Default)]
pub struct HoldOverride {
    pub air: bool,
    pub dir: i32,
    pub montage: Name,
}

/// The pose's inputs.
#[derive(State, Debug, Default)]
pub struct Pose {
    pub aim_direction: Real,
    /// BPI-written: the head's scale.
    pub head_scale: Option<Real>,
    /// `equipped`: the equipped meshes, by tag.
    pub equipped: Vec<Name>,
    pub sub_hidden: IndexSet<Name>,
    pub blend_out_subs: IndexMap<Name, Name>,
    /// `bouncePose` and `bounceTimes`.
    pub bounce_pose: Real,
    pub bounce_times: IndexMap<i32, Real>,
}

/// `GroundState`.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum GroundState {
    Fall,
    Jump1,
    Jump2,
    FastFall,
    Wall,
    HitStunned,
    Recovery,
    Bounce,
    HitWobble,
    A2g,
    Entry,
    Run,
    RunStop,
    #[default]
    Idle,
    Crouch,
    AltEnter,
    AltLoop,
    IdleReset,
    RunStop2,
    CrouchIn,
    CrouchOut,
    HoverIn,
    HoverLoop,
    CrouchRun,
    CrouchRunIn,
    CrouchIntoCrouchRun,
    CrouchRunIntoCrouch,
}

/// `RunSubState`.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum RunSubState {
    Run,
    CrouchRun,
    CrouchRunIn,
    CrouchIntoCrouchRun,
}

/// A state machine's running blend (`smActive` value).
#[derive(State, Debug, Default)]
pub struct SmBlend {
    pub prev: Name,
    pub next: Name,
    pub el: Real,
    pub dur: Real,
    pub hermite: bool,
    pub alpha: Real,
}

/// The anim state machines.
#[derive(State, Debug, Default)]
pub struct StateMachines {
    pub gs: GroundState,
    pub run_sub: Option<RunSubState>,
    pub gs_time: Real,
    /// `smActive`: the active transitions of each machine, oldest first.
    pub sm_active: IndexMap<Name, Vec<SmBlend>>,
    pub gs_rel: Real,
    pub sm_times: IndexMap<Name, Real>,
    pub a2g_times: IndexMap<Name, Real>,
    pub edge_times: Option<IndexMap<Name, Real>>,
    pub dash_stop_blend: bool,
    pub dash_stop_delay: Real,
    /// `gsKept`: the last `set_gs` entered a state without initialising it; read by
    /// `air_machine` inside its one call, so off the state.
    #[state(skip)]
    pub gs_kept: bool,
}

/// Visibility and the camera.
#[derive(State, Debug, Default)]
pub struct Visibility {
    pub invisible: bool,
    pub on_camera: bool,
    pub invis_more: Option<Real>,
    pub always_visible: Option<IndexSet<Name>>,
    pub invis_shown: Option<IndexSet<Name>>,
    pub hit_mirror: Option<bool>,
}

/// `NavSet` and `AnimSet` are the character definition's (`mvs_data::def`): static data a fighter names, never
/// holds (a reference, hashed as a static id).
pub use mvs_data::def::{AnimSet, NavSet};

/// Which of the character definition's sets a fighter reads: `Base` (`CharDef.nav` / `CharDef.anim`) or one of
/// `navSets` / `animSets` by its key.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum SetRef {
    #[default]
    Base,
    Named(Name),
}

/// `SlotMix`: CharHooks.slotMix's poses mixed into the fighter's pose in order
/// (pose = lerp(pose, mix, w)) and the timed nodes of its slot graph. Sim state of the character's state record.
#[derive(State, Debug, Default)]
pub struct SlotMix {
    pub slot_mix: Vec<SlotPose>,
    /// `slotBlend`: the timed BlendListByBool nodes' state.
    pub slot_blend: Option<SlotBlend>,
    /// `slotPlayers`: the SequencePlayers' and state machines' state.
    pub slot_players: Option<SlotPlayers>,
}

/// A `slotMix` entry.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct SlotPose {
    pub anim: Name,
    pub time: Real,
    pub w: Real,
}

pub use slot_blend::{SlotBlend, SlotPlayers, SlotScratch};

/// A fighter's montage player, named across the tracks (a montage player is handed around by reference): the
/// fighter's own fields of [`Montages`] by their place.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum MontageSlot {
    /// `montage`: the main slot's.
    Main,
    /// `dashMontage`.
    Dash,
    /// `fading[i]`: a montage blending out.
    Fading(u32),
    /// `grouped[i]`: a grouped montage.
    Grouped(u32),
    /// The second mesh's (a disguise's copy, `foreign`).
    Foreign,
    /// A polymorph's mesh's (`Fighter.polymorph.play`).
    Poly,
}

/// `OpenPose`: the pose a Hitframe opened in.
#[derive(State, Copy, PartialEq, Debug)]
pub struct OpenPose {
    pub anim: Name,
    pub time: Real,
}

/// `ForcedPress`: an absolute InputDirection byte.
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub struct ForcedPress {
    pub dir: i32,
}

/// `QueuedPress`: the input the buffered branch windows hold.
#[derive(State, Debug)]
pub struct QueuedPress {
    /// BranchInfo.BranchInputButton.
    pub press: QueuedBtn,
    /// BranchInfo.BranchInputDirection.
    pub dir: Dir,
    pub force: Option<ForcedPress>,
    /// GetTauntBranchStruct of the pressed slot.
    pub taunt: Option<Box<Branch>>,
    /// The press's direction before the diagonals are folded (what the override check compares).
    pub raw: Dir,
}

// ---- the call surface of visibility

/// `addInvisibility`.
pub fn add_invisibility(w: &mut World, f: FighterId) {
    visibility::add_invisibility(w, f)
}

/// `removeInvisibility`.
pub fn remove_invisibility(w: &mut World, f: FighterId) {
    visibility::remove_invisibility(w, f)
}

/// `setSubmeshAlwaysVisible`.
pub fn set_submesh_always_visible(w: &mut World, f: FighterId, tag: Name, on: bool) {
    visibility::set_submesh_always_visible(w, f, tag, on)
}

/// `setupInitialVisibility`.
pub fn setup_initial_visibility(w: &mut World, f: FighterId) {
    visibility::setup_initial_visibility(w, f)
}

// ---- the call surface of taunts

pub use taunts::{set_taunt_slots, taunt_branch, taunt_info_montage, taunt_slot_entry, taunt_slot_name};

// ---- the call surface of overrides

pub use overrides::{
    Override, add_override_pack, clear_override_packs, override_attack, override_window_branch, pack_answers_no_press,
    remove_override_pack, row_branch,
};

// ---- anim

pub use machines::{
    AIR_STATES, GsOpt, HitMachine, a2g_pose, air_machine, anim_bounce_type, anim_bouncing, bounce_anim,
    crouch_run_play_rate, enter_bounce, enter_hit_stunned, ground_machine, hit_machine, hit_pose, hover_machine,
    run_play_rate, set_gs, sm_advance, sm_transition, wobble_pose,
};
pub use sequences::{seq_base, seq_fade_start, seq_fire, seq_frame_start, seq_notify_tick, seq_reset, seq_states_tick};
pub use slot_mix::{
    blend_slots, fade_weight, graph_player_time_from_end, graph_relevant_anim_time_remaining, graph_slots, mix_slots,
    set_sub_mesh, slot_graph_tick, slot_source_share, submesh_notify,
};
pub use update::{
    FALL_RATE, FollowedMove, anim_direction, anim_direction_tick, anim_fast_falling, anim_len_of, anim_set_switched,
    f_interp_to, fall_lean_tick, followed_move, idle_anim, layer_frozen, loco_clip, set_being_carried,
    set_hit_react_info, set_loco, source_held, state_name, update_pose,
};

// ---- the call surface of box pose

/// `layerPitch`.
pub fn layer_pitch(w: &World, f: FighterId) -> Option<Real> {
    box_pose::layer_pitch(w, f)
}

/// `hitPitch`.
pub fn hit_pitch(w: &World, f: FighterId) -> Option<Real> {
    box_pose::hit_pitch(w, f)
}

/// `sampleBoxes` into `out`, `sampled_boxes` into a buffer of the World's pool
/// (`put_pose` hands it back); `poseBoxes` the same.
pub use box_pose::{
    SkelWork, StruckPose, drop_struck_poses, pose_boxes, posed_boxes, put_pose, put_skel_work, sample_boxes,
    sampled_boxes, struck_boxes, take_pose, take_skel_work, take_struck_poses,
};

// ---- the call surface of blend spaces

/// `fallAnim`.
pub fn fall_anim(w: &mut World, f: FighterId) -> Name {
    blend_spaces::fall_anim(w, f)
}

/// `fallRate`.
pub fn fall_rate(w: &mut World, f: FighterId) -> Real {
    blend_spaces::fall_rate(w, f)
}

// ---- the call surface's argument and return types

/// What `resolveUnpressed` takes: a window.
pub type ResolveUnpressedW = OpenWindowsOpenWindows;

/// What `newActiveHit` takes or gives (a plain record).
#[derive(Clone, Debug)]
pub struct NewActiveHitOpenPose {
    pub anim: Name,
    pub time: Real,
}

/// The anim code's buffers, reused from call to call (the World's `scratch.anim`): each is taken out while it is
/// filled and read, and put back empty or holding what its next user clears.
#[derive(Clone, Debug, Default)]
pub struct AnimScratch {
    /// `activeOf`'s list of an instance's NotifyTicks (`montages::notify_ticks`).
    pub active: Vec<ActiveOfActiveOf>,
    /// The poses a slot mix weighs (`slot_mix::mix_slots`, `graph_slots`).
    pub poses: Vec<SlotPose>,
    /// The slot graph's parts (`graph_slots`): empty between two uses (its entries borrow the match data).
    pub parts: Vec<(slot_blend::SlotPart<'static>, Real)>,
}

/// What `activeOf` takes or gives (a plain record).
#[derive(Clone, Copy, Debug)]
pub struct ActiveOfActiveOf {
    pub n: NotifyRef,
    pub i: i32,
}

/// What `openWindows` gives: a window, its notify and its key (`{ n, i }`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpenWindowsOpenWindows {
    pub n: NotifyRef,
    pub i: i32,
}

/// What `rowOk` takes or gives (a plain record).
#[derive(Clone, Debug)]
pub struct RowOkR {
    pub conditions: Option<Vec<Name>>,
    pub ability_slot: Option<i32>,
}

/// What `resolve` takes or gives (a plain record).
#[derive(Clone, Debug)]
pub struct ResolveOut {
    pub m: MontageSlot,
    pub n: NotifyRef,
    pub i: i32,
}

/// What `resolveBuffered` takes: a window.
pub type ResolveBufferedW = OpenWindowsOpenWindows;

/// `resolveBuffered`'s 'branch' | 'jump' | 'dodge'.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolveBufferedKind {
    Branch,
    Jump,
    Dodge,
}

/// What `resolveBuffered` takes or gives (a plain record).
#[derive(Clone, Debug)]
pub struct ResolveBufferedOut {
    pub kind: ResolveBufferedKind,
    pub br: Option<Branch>,
}

/// What `setLoco` takes (a plain record): `looping` `None` takes the anim set's.
#[derive(Clone, Copy, Debug, Default)]
pub struct SetLocoOpt {
    pub restart: bool,
    pub linear: bool,
    pub time: Option<Real>,
    pub looping: Option<bool>,
}

// ---- the set lookups and the montage events of anim

/// `f.nav`: the navigation set the fighter reads.
pub fn nav_of(w: &World, f: FighterId) -> &NavSet {
    let fi = w.fighter(f);
    let def = &montages::char_data(&w.data, &w.names, fi.ch).expect("the fighter's character data").def;
    match fi.anim.nav {
        SetRef::Base => &def.nav,
        SetRef::Named(k) => def.nav_sets.get(w.names.str(k)).unwrap_or(&def.nav),
    }
}

/// `f.data.montages[name]` (`montages::montage_data`).
pub use montages::montage_data;

/// `f.nav.hold`: the HoldingItemOverride pack of the navigation set the fighter plays.
pub fn nav_hold(w: &World, f: FighterId) -> Vec<HoldOverride> {
    nav_of(w, f)
        .hold
        .iter()
        .map(|h| HoldOverride {
            air: h.air,
            dir: h.dir,
            montage: h.montage.as_deref().map_or(Name::NONE, |m| crate::fighter::common::lit(w, m)),
        })
        .collect()
}

/// `animMontageStart`.
pub fn anim_montage_start(w: &mut World, f: FighterId, dash: bool) {
    let sm = &mut w.fighter_mut(f).sm;
    if dash {
        sm.dash_stop_delay = 0.05;
    } else {
        sm.dash_stop_blend = false;
    }
}

/// `montageBlendingOut`: instance `uid`'s OnMontageBlendingOut, once.
pub fn montage_blending_out(w: &mut World, f: FighterId, uid: u32) {
    let Some(s) = montages::state_mut(w, f, uid) else { return };
    if s.blend_out_fired {
        return;
    }
    s.blend_out_fired = true;
    let name = s.montage.name;
    if w.fighter(f).shapes.disguise.is_some() {
        crate::combat::disguise::owner_montage_event(w, f, name);
    }
    let subs: Vec<Name> = w.fighter(f).pose.blend_out_subs.values().copied().collect();
    for tag in subs {
        let hidden = {
            let l = w.names.str(tag).to_lowercase();
            w.fighter(f).pose.sub_hidden.iter().any(|&h| w.names.str(h) == l)
        };
        set_sub_mesh(w, f, tag, hidden);
    }
    if let Some(h) = w.fighter(f).hooks.char.montage_blending_out {
        h(w, f, name);
    }
    crate::fighter::moves::clear_attack_cancelled(w, f);
    branches::branch_blending_out(w, f, name);
}

/// `montage.blendingOut` of `montage` or the dash montage (isMontagePlaying).
pub fn blending_out(w: &World, f: FighterId, dash: bool) -> bool {
    let m = &w.fighter(f).montages;
    let s = if dash { &m.dash_montage } else { &m.montage };
    s.as_ref().is_some_and(|s| s.blending_out())
}

/// `f.shownMontage?.pose()` (pausedPose): the shown montage's current anim and its time, `None`
/// without a shown montage or a pose.
pub fn shown_pose(w: &World, f: FighterId) -> Option<(Name, Real)> {
    let uid = update::shown_uid(w, f)?;
    update::inst_pose(&w.data, w, f, uid, None)
}

/// An instance's montage and its current section's name (`m.name`,
/// `m.m.sections[m.sectionIndexAt(Math.max(m.pos, 0))]?.name`); `Name::NONE` for no section
/// there.
pub fn instance_section(w: &World, f: FighterId, m: MontageSlot) -> Option<(Name, Name)> {
    let uid = montages::slot_uid(w, f, m)?;
    let s = montages::state_ref(w, f, uid)?;
    let md = montages::instance_data(&w.data, w, f, uid)?;
    let i = s.section_index_at(md, crate::math::dbl::max(s.pos, 0.0));
    let sec = md.sections.get(i).map_or(Name::NONE, |x| update::nm(&w.names, &x.name));
    Some((s.montage.name, sec))
}

/// `m.blendingOut` of a fighter's montage instance; false for an empty
/// slot.
pub fn slot_blending_out(w: &World, f: FighterId, m: MontageSlot) -> bool {
    crate::fighter::control::slot_state(w.fighter(f), m).is_some_and(|s| s.blending_out())
}

/// `m.jumpToSection(section)` on a fighter's montage instance; false
/// for an empty slot or a montage the data does not have.
pub fn slot_jump_to_section(w: &mut World, f: FighterId, m: MontageSlot, section: Name) -> bool {
    let s = w.names.str(section).to_owned();
    crate::fighter::control::with_player(w, f, m, |p| p.jump_to_section(&s)).unwrap_or(false)
}
