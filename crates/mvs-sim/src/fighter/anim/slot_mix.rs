//! The montage's pose through a character AnimBP's slot nodes: C019_AnimBP's aim and move slots,
//! the slot graph of the data (`FighterData.slotHolds.graph`) with its update, the sub-mesh visibility the montages'
//! notifies switch, and the source's share under the Slot nodes.

use super::montages::{instance_data, notify_of, state_ref};
use super::slot_blend::{
    SeqInfo, SlotGraphIn, SlotPart, ZERO_W, has_players, has_timed_lists, slot_blend_step, slot_graph_parts,
    slot_machine_time_remaining, slot_player_pose, slot_player_time_from_end,
};
use super::update::{nm, shown_uid, slot_mix_of};
use super::visibility::sub_mesh_hidden;
use super::{Fade, SlotBlend, SlotPlayers, SlotPose};
use crate::Real;
use crate::consts;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::common::{truthy, warn_text};
use crate::math;
use crate::name;
use crate::names::Name;
use crate::registry::signals::FighterSignal;
use crate::world::World;

use mvs_data::Value;
use mvs_data::fighter::{Montage, SlotGraph};

/// `AIM_SLOTS`: C019_AnimBP's slot names per motion (center, up, down).
const AIM_STILL: [&str; 3] = ["AimCenterStill", "AimUpStill", "AimDownStill"];
const AIM_MOVE_F: [&str; 3] = ["AimCenterMoveF", "AimUpMoveF", "AimDownMoveF"];
const AIM_MOVE_B: [&str; 3] = ["AimCenterMoveB", "AimUpMoveB", "AimDownMoveB"];
const AIM: [&str; 3] = ["AimCenter", "AimUp", "AimDown"];

/// `blendSlots`: C019_AnimBP's slot graph over the montage instance `uid` (`mw`, `bw`: the
/// Morty state's slotMoveW and slotBackW), into the Morty state's slot mix.
pub fn blend_slots(w: &mut World, f: FighterId, uid: u32, mw: Real, bw: Real) {
    let d = w.data.clone();
    crate::chars::morty::morty_slot_mix(w, f).clear();
    let Some(m) = instance_data(&d, w, f, uid) else { return };
    let has = |n: &str| m.slots.iter().any(|s| s.slot == n);
    let ad = w.fighter(f).pose.aim_direction;
    let up = math::min(math::max(ad, 0.0), 1.0);
    let down = math::min(math::max(-ad, 0.0), 1.0);
    let aim = |s: &'static [&'static str; 3], w: Real| -> [(SlotPart<'static>, Real); 3] {
        [
            (SlotPart::Slot(s[0]), w * (1.0 - up) * (1.0 - down)),
            (SlotPart::Slot(s[1]), w * up * (1.0 - down)),
            (SlotPart::Slot(s[2]), w * down),
        ]
    };
    let bw = math::max(bw, 0.0);
    let mut parts = [(SlotPart::Loco, 0.0); 9];
    let n = if has("AimCenterStill") {
        parts[0..3].copy_from_slice(&aim(&AIM_STILL, 1.0 - mw));
        parts[3..6].copy_from_slice(&aim(&AIM_MOVE_F, mw * (1.0 - bw)));
        parts[6..9].copy_from_slice(&aim(&AIM_MOVE_B, mw * bw));
        9
    } else if has("StillBlend") {
        parts[0..3].copy_from_slice(&[
            (SlotPart::Slot("StillBlend"), 1.0 - mw),
            (SlotPart::Slot("MovingBlend"), mw * (1.0 - bw)),
            (SlotPart::Slot("MovingBackBlend"), mw * bw),
        ]);
        3
    } else if has("AimCenter") || has("AimUp") || has("AimDown") {
        parts[0..3].copy_from_slice(&aim(&AIM, 1.0));
        3
    } else {
        return;
    };
    let mut out = std::mem::take(crate::chars::morty::morty_slot_mix(w, f));
    mix_slots(w, f, Some(uid), &parts[..n], &mut out, None);
    *crate::chars::morty::morty_slot_mix(w, f) = out;
}

/// A part's own pose (a SequencePlayer's: its clip and time), `None` for the montage's or the locomotion's.
pub type OwnPose<'a> = &'a dyn Fn(SlotPart) -> Option<(Name, Real)>;

/// The pose of a part: the character's own (`own`), else the montage's track in the slot, else the locomotion. Each
/// part is its own entry, in the parts' order: the build's blend nodes weigh each child's pose (TwoWayBlend A x (1 -
/// alpha) + B x alpha, the Slot node its source), none merged with another at the same clip and time.
fn mix_poses(
    w: &World,
    f: FighterId,
    uid: Option<u32>,
    parts: &[(SlotPart, Real)],
    own: Option<OwnPose>,
    poses: &mut Vec<SlotPose>,
) {
    let a = &w.fighter(f).anim;
    let loco = if uid.is_some() { (a.loco_anim, a.loco_time) } else { (a.anim, a.anim_time) };
    let md = uid.and_then(|u| Some((instance_data(&w.data, w, f, u)?, state_ref(w, f, u)?)));
    for &(slot, wt) in parts {
        if wt <= 0.0 {
            continue;
        }
        let track = || {
            let SlotPart::Slot(s) = slot else { return None };
            let (m, st) = md?;
            let (si, gi, t) = st.pose_in(m, s)?;
            Some((nm(&w.names, &m.slots[si].segments[gi].anim), crate::math::narrow(t)))
        };
        let (anim, time) = own.and_then(|o| o(slot)).or_else(track).unwrap_or(loco);
        poses.push(SlotPose { anim, time, w: wt });
    }
}

/// The heaviest pose into `anim` / `animTime`, the others into `out` with their running weights.
/// The heaviest of `poses` into `anim` / `anim_time`, the others into `out` in order with their running weights;
/// `poses` is left empty.
fn mix_out(w: &mut World, f: FighterId, poses: &mut Vec<SlotPose>, out: &mut Vec<SlotPose>) {
    if poses.is_empty() {
        return;
    }
    let mut first = 0;
    for i in 1..poses.len() {
        if poses[i].w > poses[first].w {
            first = i;
        }
    }
    let head = poses[first];
    let an = &mut w.fighter_mut(f).anim;
    an.anim = head.anim;
    an.anim_time = head.time;
    let mut sum = head.w;
    for (i, p) in poses.iter().enumerate() {
        if i == first {
            continue;
        }
        sum += p.w;
        out.push(SlotPose { anim: p.anim, time: p.time, w: p.w / sum });
    }
    poses.clear();
}

/// An empty list of another element type over `v`'s buffer (an empty in-place collect keeps the allocation): the
/// World keeps the slot parts' buffer, whose entries borrow the match data, between two uses.
fn recycle<T, U>(mut v: Vec<T>) -> Vec<U> {
    v.clear();
    v.into_iter().map(|_| unreachable!("an empty list")).collect()
}

/// `mixSlots`: the poses of instance `uid`'s slot tracks under an AnimBP's blend nodes, as
/// one weighted sum (parts: a slot and its weight; the weights add up to 1): one entry per part, the heaviest becomes `anim` / `anim_time` and the others go to `out` in order with the running weights of
/// pose = lerp(pose, mix, w) (SlotMix). A slot without a track in the montage passes its source on (the locomotion
/// clip). C019_AnimBP's slot graph and WonderWoman_AnimBPV2's ShieldWalk lists. `own`: a part that is no slot (a
/// SequencePlayer of the slot graph, graph_slots). With no montage (`uid` `None`) the locomotion pose is the one
/// update_pose's tail wrote into `anim` / `anim_time`.
pub fn mix_slots(
    w: &mut World,
    f: FighterId,
    uid: Option<u32>,
    parts: &[(SlotPart, Real)],
    out: &mut Vec<SlotPose>,
    own: Option<OwnPose>,
) {
    out.clear();
    let mut poses = std::mem::take(&mut w.scratch.anim.poses);
    mix_poses(w, f, uid, parts, own, &mut poses);
    mix_out(w, f, &mut poses, out);
    w.scratch.anim.poses = poses;
}

/// `montageHasTrack`: a non-empty, non-additive track in the slot.
fn montage_has_track(m: &Montage, slot: &str) -> bool {
    m.slots.iter().any(|s| s.slot == slot && !s.segments.is_empty() && s.additive != Some(true))
}

/// `graphIn`: the slot graph walk's view of the fighter's AnimBP.
struct GraphIn<'w> {
    w: &'w World,
    f: FighterId,
    input: fn(&World, FighterId, Name) -> Real,
    nodes: usize,
}

impl SlotGraphIn for GraphIn<'_> {
    fn who(&self) -> &str {
        self.w.names.str(self.w.fighter(self.f).ch)
    }

    fn warn(&self, msg: &str) {
        warn_text(self.w, msg);
    }

    fn live_players(&self, p: &SlotPlayers) {
        let mut l = self.w.scratch.live_players.borrow_mut();
        l.0 = true;
        super::slot_blend::slot_players_copy(&mut l.1, p);
    }

    fn input(&self, name: &str) -> Real {
        let v = (self.input)(self.w, self.f, nm(&self.w.names, name));
        if !v.is_finite() {
            panic!("{}: CharHooks.slotInput gives no value for {name} ({} graph nodes)", self.who(), self.nodes);
        }
        v
    }

    fn layered(&self, node: &str, kind: &str) -> bool {
        (kind == "AnimNode_LayeredBoneBlend" || kind == "AnimNode_ApplyAdditive")
            && (self.input)(self.w, self.f, nm(&self.w.names, node)) == 0.0
    }

    fn source(&self, node: &str, kind: &str) -> bool {
        kind == "AnimNode_StateMachine" && (self.input)(self.w, self.f, nm(&self.w.names, node)) == 0.0
    }

    fn layer(&self, node: &str) -> Option<Real> {
        let l = self.w.fighter(self.f).hooks.char.slot_layer?;
        Some(l(self.w, self.f, nm(&self.w.names, node)))
    }

    fn anim(&self, anim: &str) -> Option<SeqInfo> {
        graph_seq(self.w, self.f, anim)
    }
}

fn graph_in<'w>(w: &'w World, f: FighterId, input: fn(&World, FighterId, Name) -> Real, g: &SlotGraph) -> GraphIn<'w> {
    GraphIn { w, f, input, nodes: g.nodes.len() }
}

/// `graphSeq`: a SequencePlayer's sequence of the slot graph: its length and RateScale from
/// the box index; one missing there warns (`None`: the player holds its time), never a made-up length.
fn graph_seq(w: &World, f: FighterId, a: &str) -> Option<SeqInfo> {
    let cd = super::montages::char_data(&w.data, &w.names, w.fighter(f).ch)?;
    let x = cd.boxes.anims.get(a);
    if x.is_none() {
        let who = w.names.str(w.fighter(f).ch);
        warn_text(w, &format!("{who}: the slot graph sequence {a} is not in the box index"));
    }
    x.map(|x| SeqInfo { length: crate::math::narrow(x.length), rate_scale: crate::math::narrow(x.rate_scale) })
}

fn graph_seq_len(w: &World, f: FighterId, a: &str) -> Real {
    graph_seq(w, f, a).map_or(0.0, |s| s.length)
}

/// `slotBlendOf`.
fn slot_blend_of(w: &World, f: FighterId) -> &SlotBlend {
    let s = w.fighter(f).hooks.char.slot_mix.and_then(|h| h(w, f)).and_then(|m| m.slot_blend.as_ref());
    s.unwrap_or_else(|| {
        panic!(
            "{}: its slot graph has a BlendListByBool with a blend time: CharHooks.slotMix needs a slotBlend",
            w.names.str(w.fighter(f).ch)
        )
    })
}

/// `slotPlayersOf`.
fn slot_players_of(w: &World, f: FighterId) -> &SlotPlayers {
    let p = w.fighter(f).hooks.char.slot_mix.and_then(|h| h(w, f)).and_then(|m| m.slot_players.as_ref());
    p.unwrap_or_else(|| {
        panic!(
            "{}: its slot graph has a SequencePlayer, a state machine or a list that resets its child: CharHooks.slotMix needs a slotPlayers",
            w.names.str(w.fighter(f).ch)
        )
    })
}

/// `graphSlots`: the shown montage's pose (instance `uid`; `None`: no montage) through the
/// AnimBP's slot graph as the data states it, for a character whose hooks give the graph's inputs
/// (CharHooks.slotInput): the montage's tracks are weighed by the nodes over them and mixed by mix_slots into `anim`
/// / `anim_time` and the hook's slot mix. The node rules are those of `slot_blend`.
/// Deviation: the shapes and the drawn pose mix the posed tracks linearly (the shortcut of Morty's, Batman's and
/// Wonder Woman's slot mixes); an additive track counts as none (the slot passes its source on); the bones the
/// passed-through nodes change (ModifyBone HeadScale, the mirror, AnimDynamics) are not the sim's.
pub fn graph_slots(w: &mut World, f: FighterId, uid: Option<u32>, input: fn(&World, FighterId, Name) -> Real) {
    let d = w.data.clone();
    let cd = super::montages::char_data(&d, &w.names, w.fighter(f).ch).expect("the fighter's character data");
    let g = &cd.fighter.slot_holds.graph;
    let m = uid.and_then(|u| instance_data(&d, w, f, u));
    let mut parts: Vec<(SlotPart, Real)> = recycle(std::mem::take(&mut w.scratch.anim.parts));
    let mut poses = std::mem::take(&mut w.scratch.anim.poses);
    {
        let wr: &World = w;
        let s = if has_timed_lists(g) { Some(slot_blend_of(wr, f)) } else { None };
        let p = if has_players(g) { Some(slot_players_of(wr, f)) } else { None };
        let gi = graph_in(wr, f, input, g);
        slot_graph_parts(g, s, &gi, &mut |slot| m.is_some_and(|m| montage_has_track(m, slot)), &mut parts, p);
        if uid.is_none() && parts.iter().all(|(k, _)| *k == SlotPart::Loco) {
            // the locomotion alone: update_pose's own pose
            w.scratch.anim.parts = recycle(parts);
            w.scratch.anim.poses = poses;
            return;
        }
        let own = |k: SlotPart| match k {
            SlotPart::Seq(id) => slot_player_pose(g, p, id).map(|(a, t)| (nm(&wr.names, a), t)),
            _ => None,
        };
        let own: Option<OwnPose> = if p.is_some() { Some(&own) } else { None };
        mix_poses(wr, f, uid, &parts, own, &mut poses);
    }
    w.scratch.anim.parts = recycle(parts);
    let mut out = std::mem::take(&mut slot_mix_of(w, f).slot_mix);
    out.clear();
    mix_out(w, f, &mut poses, &mut out);
    slot_mix_of(w, f).slot_mix = out;
    w.scratch.anim.poses = poses;
}

/// `graphPlayerTimeFromEnd`: GetInstanceAssetPlayerTimeFromEnd(index) of the fighter's AnimBP
/// for a player of its slot graph (a transition rule's bound function, read through CharHooks.slotInput): the length
/// minus the player's time, whatever its weight; FLT_MAX with no such player (read in the exe).
pub fn graph_player_time_from_end(w: &World, f: FighterId, index: i32) -> Real {
    let cd = super::montages::char_data(&w.data, &w.names, w.fighter(f).ch).expect("the fighter's character data");
    let g = &cd.fighter.slot_holds.graph;
    let live = w.scratch.live_players.borrow();
    let p = if has_players(g) { Some(if live.0 { &live.1 } else { slot_players_of(w, f) }) } else { None };
    slot_player_time_from_end(g, p, index, &|a| graph_seq_len(w, f, a))
}

/// `graphRelevantAnimTimeRemaining`: GetRelevantAnimTimeRemaining(MachineIndex, StateIndex)
/// of the fighter's AnimBP for a state machine of its slot graph (C023A_AnimBP's TransitionResult_2 @1409 and _3
/// @3931: (82, 1)): the state's most relevant player's time left; FLT_MAX with none.
pub fn graph_relevant_anim_time_remaining(w: &World, f: FighterId, machine: i32, state: i32) -> Real {
    let cd = super::montages::char_data(&w.data, &w.names, w.fighter(f).ch).expect("the fighter's character data");
    let g = &cd.fighter.slot_holds.graph;
    let live = w.scratch.live_players.borrow();
    let p = if has_players(g) { Some(if live.0 { &live.1 } else { slot_players_of(w, f) }) } else { None };
    let who = w.names.str(w.fighter(f).ch);
    slot_machine_time_remaining(g, p, machine, state, who, &|a| graph_seq_len(w, f, a))
}

/// `slotGraphTick`: the anim instance's update of its slot graph's timed BlendListByBool
/// nodes, SequencePlayers and state machines, for a character that enters
/// CharHooks.slotInput and whose graph has one: after the montages' advance and before the pose is read (update_pose),
/// in every update of the instance, by the instance's delta (DeltaSeconds 0.0166668 x dilation, in float32). A Slot
/// node's source counts as updated when the shown montage has no track in its slot, the node is
/// bAlwaysUpdateSourcePose (slotHolds.nodes), or the montage's weight is below 1 - 1e-5.
/// Deviation: the shown montage's weight stands for the slot's (GetSlotWeight sums every instance of the slot's group,
/// a stopped one blending out among them), as graph_slots takes the shown montage alone.
pub fn slot_graph_tick(w: &mut World, f: FighterId, dilation: Real) {
    let Some(input) = w.fighter(f).hooks.char.slot_input else { return };
    let d = w.data.clone();
    let cd = super::montages::char_data(&d, &w.names, w.fighter(f).ch).expect("the fighter's character data");
    let g = &cd.fighter.slot_holds.graph;
    let (timed, players) = (has_timed_lists(g), has_players(g));
    if !timed && !players {
        return;
    }
    if timed {
        slot_blend_of(w, f);
    }
    if players {
        slot_players_of(w, f);
    }
    let sm = shown_uid(w, f).and_then(|u| {
        let m = instance_data(&d, w, f, u)?;
        Some((m, state_ref(w, f, u)?.weight(m)))
    });
    let holds = &cd.fighter.slot_holds.nodes;
    // the slot mix's own state, taken out for the update (the walk reads the World)
    let mix = slot_mix_of(w, f);
    let mut s = if timed { mix.slot_blend.take() } else { None };
    let mut p = if players { mix.slot_players.take() } else { None };
    let mut sc = std::mem::take(&mut w.scratch.slot_walk);
    {
        let wr: &World = w;
        let gi = graph_in(wr, f, input, g);
        let who = wr.names.str(wr.fighter(f).ch);
        let mut updated = |node: &str, slot: &str| {
            let Some((m, weight)) = sm else { return true };
            if !montage_has_track(m, slot) {
                return true;
            }
            let h = holds
                .iter()
                .find(|x| x.node == node)
                .unwrap_or_else(|| panic!("{who}: {node} is not in slotHolds.nodes"));
            h.always || 1.0 - crate::math::narrow(weight) > ZERO_W
        };
        // the build's DeltaSeconds x the hit pause's dilation, a float32 (fall_lean_tick's delta)
        let dt = consts::DELTA_SECONDS * dilation;
        slot_blend_step(g, s.as_mut(), dt, &gi, &mut updated, p.as_mut(), &mut sc);
    }
    w.scratch.slot_walk = sc;
    w.scratch.live_players.borrow_mut().0 = false;
    let mix = slot_mix_of(w, f);
    if timed {
        mix.slot_blend = s;
    }
    if players {
        mix.slot_players = p;
    }
}

/// Whether the AnimBP's update reaches the locomotion under montage `m` at `weight`, for a character whose hooks
/// give the slot graph's inputs (CharHooks.slotInput; `None` without): a Slot node passes the update to its source
/// when the montage has no track in its slot, with bAlwaysUpdateSourcePose, or at a source weight above 0.00001
/// (FAnimNode_Slot::Update_AnyThread 1424945f0), the blend nodes as their weights stand
/// (`slot_blend::slot_graph_source_updated`).
pub fn graph_source_updated(w: &World, f: FighterId, m: &Montage, weight: Real) -> Option<bool> {
    let input = w.fighter(f).hooks.char.slot_input?;
    let cd = super::montages::char_data(&w.data, &w.names, w.fighter(f).ch).expect("the fighter's character data");
    let g = &cd.fighter.slot_holds.graph;
    let holds = &cd.fighter.slot_holds.nodes;
    let s = if has_timed_lists(g) { Some(slot_blend_of(w, f)) } else { None };
    let p = if has_players(g) { Some(slot_players_of(w, f)) } else { None };
    let gi = graph_in(w, f, input, g);
    let who = w.names.str(w.fighter(f).ch);
    let mut updated = |node: &str, slot: &str| {
        if !montage_has_track(m, slot) {
            return true;
        }
        let h =
            holds.iter().find(|x| x.node == node).unwrap_or_else(|| panic!("{who}: {node} is not in slotHolds.nodes"));
        h.always || 1.0 - weight > ZERO_W
    };
    Some(super::slot_blend::slot_graph_source_updated(g, s, &gi, &mut updated, p))
}

/// `setSubMesh`: Comp_FighterVisibilityManager.SetSubMeshVisibility: MeshManager.GetMesh(tag)
/// (no such mesh: nothing); visible takes it out of HiddenMeshes and shows it, hidden adds it (and hides it when it
/// was not in the set).
pub fn set_sub_mesh(w: &mut World, f: FighterId, tag: Name, visible: bool) {
    let lower = w.names.str(tag).to_lowercase();
    let t = nm(&w.names, &lower);
    if w.fighter(f).shapes.disguise.is_some() {
        // a Disguise's clone meshes carry the victim's tags
        crate::combat::disguise::clone_sub_mesh(w, f, t, visible);
    }
    let d = w.data.clone();
    let cd = super::montages::char_data(&d, &w.names, w.fighter(f).ch).expect("the fighter's character data");
    if !cd.def.sub_meshes.contains(&lower) {
        return;
    }
    let pose = &mut w.fighter_mut(f).pose;
    let was = pose.sub_hidden.contains(&t);
    if visible {
        pose.sub_hidden.shift_remove(&t);
    } else {
        pose.sub_hidden.insert(t);
        sub_mesh_hidden(w, f, t);
    }
    // broadcast UpdatedSubMesh(ComponentTag, Visible): shown @388 at every call, hidden @1474 only when it was not
    // hidden yet (the pawn's binding, then the actors': FighterSignal)
    if visible || !was {
        if let Some(h) = w.fighter(f).hooks.char.updated_sub_mesh {
            h(w, f, tag, visible);
        }
        crate::fighter::signal_bound(w, f, FighterSignal::UpdatedSubMesh, visible, 0.0, tag, None);
    }
}

fn prop_str(v: Option<&Value>) -> Option<&str> {
    match v {
        Some(Value::Str(s)) => Some(s),
        _ => None,
    }
}

/// `submeshNotify`: AnimNotify_SetSubmeshVisibility (state). The tag is MirrorSubmeshTag when
/// the fighter faces left and it is set, read again at the end. Begin: with ReverseWhenAnimBlendsOut the notify is
/// filed in BlendOutDependentNotifies (SetSubMeshVisibilityWithBlendOutTrigger, AddNotify), then
/// SetSubMeshVisibility(tag, Visible). End: with ReverseWhenAnimBlendsOut the reverse only if the notify was still
/// filed (Map_Remove), else with ReverseAtNotifyEnd the reverse, else nothing. PropagateToChildren reaches no drawn
/// child here. The file's key is the notify's `key` property (no notify has one: every filed
/// notify shares the one key, `Name::NONE`).
pub fn submesh_notify(w: &mut World, f: FighterId, n: NotifyRef, begin: bool) {
    let Some(nd) = notify_of(w, f, n) else { return };
    let p = &nd.p;
    let mirror = prop_str(p.get("MirrorSubmeshTag"));
    let tag = match mirror {
        Some(m) if w.fighter(f).body.facing_dir < 0.0 && !m.is_empty() && m != "None" => nm(&w.names, m),
        _ => prop_str(p.get("SubmeshTag")).map_or(name!("None"), |s| nm(&w.names, s)),
    };
    let vis = p.get("Visible").is_some_and(truthy);
    let reverse_blend_out = p.get("ReverseWhenAnimBlendsOut").is_some_and(truthy);
    let reverse_end = p.get("ReverseAtNotifyEnd").is_some_and(truthy);
    let key = prop_str(p.get("key")).map_or(Name::NONE, |k| nm(&w.names, k));
    if begin {
        if reverse_blend_out {
            w.fighter_mut(f).pose.blend_out_subs.insert(key, tag);
        }
        set_sub_mesh(w, f, tag, vis);
    } else if reverse_blend_out {
        if w.fighter_mut(f).pose.blend_out_subs.shift_remove(&key).is_some() {
            set_sub_mesh(w, f, tag, !vis);
        }
    } else if reverse_end {
        set_sub_mesh(w, f, tag, !vis);
    }
}

/// `fadeWeight`: a stopped instance's weight in its blend-out (`fading`).
pub fn fade_weight(fd: &Fade) -> Real {
    let n = mvs_anim::montage::blend_ticks(fd.time);
    if n > 0.0 { math::min(math::max(fd.left / crate::math::narrow(n), 0.0), 1.0) } else { 0.0 }
}

/// `slotCount`: an instance's weight into the stage's totals (`SLOT_TOTAL`, `SLOT_FULL`).
fn slot_count(m: &Montage, st: &[&str], wt: Real, total: &mut Real, full: &mut Real) {
    if wt.is_nan() || wt <= ZERO_W {
        return;
    }
    let (mut any, mut is_full) = (false, false);
    for t in &m.slots {
        if t.segments.is_empty() || !st.contains(&t.slot.as_str()) {
            continue;
        }
        any = true;
        if t.additive != Some(true) {
            is_full = true;
        }
    }
    if !any {
        return;
    }
    *total += wt;
    if is_full {
        *full += wt;
    }
}

/// `slotSourceShare`: the source's share of the finished pose under an AnimBP's Slot nodes
/// above it: the product of each stage's SourceWeight (a stage: one Slot node, or a set of slots counted as one). A
/// Slot node's (FAnimInstanceProxy::GetSlotWeight FUN_142856c40): Total = the weights of every montage instance with
/// a non-empty track in the slot, NonAdditive = those of the full-pose tracks, NonAdditive / Total when Total > 1,
/// SourceWeight = 1 - NonAdditive. The instances: the attack montage and the dash montage (their weight) and every
/// stopped one still blending out (`fading`), an instance above 0.00001 alone (UpdateMontageEvaluationData). Used by
/// Batman's glide stack and Bugs's glide list (CharHooks.pose / slotSource).
/// Deviation: linear both ways (a Cubic blend-out such as Batman_Nav_EvadeF_v3_Montage's is not taken), and a stopped
/// instance's blend-out starts at 1, not at the weight it had at the stop.
pub fn slot_source_share(w: &World, f: FighterId, stages: &[&[&str]]) -> Real {
    let ms = &w.fighter(f).montages;
    if ms.montage.is_none() && ms.dash_montage.is_none() && ms.fading.is_empty() {
        return 1.0;
    }
    let inst = |s: &super::MontageState| {
        let u = s.montage.uid;
        let m = instance_data(&w.data, w, f, u).expect("the instance's montage data");
        (m, s.weight(m))
    };
    let mut share = 1.0;
    for st in stages {
        let (mut total, mut full) = (0.0, 0.0);
        for s in [&ms.montage, &ms.dash_montage].into_iter().flatten() {
            let (m, wt) = inst(s);
            slot_count(m, st, crate::math::narrow(wt), &mut total, &mut full);
        }
        for fd in &ms.fading {
            let m = instance_data(&w.data, w, f, fd.m.montage.uid).expect("the instance's montage data");
            slot_count(m, st, fade_weight(fd), &mut total, &mut full);
        }
        let na = if total > 1.0 { full / total } else { full };
        share *= 1.0 - math::min(math::max(na, 0.0), 1.0);
    }
    share
}
