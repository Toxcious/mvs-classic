//! The anim graph's sequence notifies: the notifies of the clips the state machines play
//! (`CharDef.seqNotifies`), listed by each frame's update of the layer, and their states' dispatch.

use super::notify_sink::{begin, end, instant};
use super::update::{anim_len, nm, parts_of};
use crate::Real;
use crate::consts;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::tick::FrameMode;
use crate::names::Name;
use crate::world::World;

/// `seqFrameStart`: the marks of the pose and layer updates (`pose_at`, `layer_at`) are set and
/// read inside one tick only and dropped at the tick's start (from beginFrame), so they are no sim state between
/// ticks: a mark an update_pose after the mesh tick left (a hit) is gone before a tick runs again after a rollback's
/// restore.
pub fn seq_frame_start(w: &mut World, f: FighterId) {
    let an = &mut w.fighter_mut(f).anim;
    an.pose_at = None;
    an.layer_at = None;
}

/// `seqReset`: a new pawn (resetMatchState): the old anim instance's active states end at its
/// teardown (UAnimInstance::UninitializeAnimation, 4.26 source: NotifyEnd for each) and the new one starts with none
/// and no left player.
/// Unverified: the teardown's NotifyEnd is not read in the exe.
pub fn seq_reset(w: &mut World, f: FighterId) {
    seq_states_tick(w, f, &[]);
    w.fighter_mut(f).anim.fade_anim = Name::NONE;
}

/// `seqNotifyTick`: the anim graph's sequence notifies of this frame's update, then their
/// states' dispatch (seq_states_tick). The hitpause: the engine updates the players at the 0.005 dilation, so they go
/// on listing the states the time is in; the port holds the players there, so the states stay as they are, also when
/// a hit's pose update (update_pose from the hit) ran this tick with no advance after it. A tick with no graph update
/// at all keeps them too. The graph's own update of a tick the pawn is hit in (the movement's pose tick, before the
/// hit) is dispatched as any other.
/// Deviation: held, not dilated (a state whose end lies within the pause's dilated advance is not ended).
pub fn seq_notify_tick(w: &mut World, f: FighterId) {
    let fi = w.fighter_mut(f);
    let tick = fi.clock.tick;
    let updated = fi.anim.pose_at == Some(tick);
    let layer = updated && fi.anim.layer_at == Some(tick);
    // Consumed here: a tick run again after a rollback must not find the marks of its first run.
    fi.anim.pose_at = None;
    fi.anim.layer_at = None;
    let d = fi.anim.seq_delta;
    fi.anim.seq_delta = 0.0;
    let mode = crate::fighter::mode(w, f);
    let dead = mode == FrameMode::Dead;
    if if updated { mode == FrameMode::Paused && d <= 0.0 } else { !dead } {
        return;
    }
    let mut listed = std::mem::take(&mut w.fighter_mut(f).anim.seq_listed);
    listed.clear();
    // The character's own players above the layer (CharHooks.seqPlayers: Bugs's glide list), queued first.
    if let Some(h) = w.fighter(f).hooks.char.seq_players {
        h(w, f, if dead { None } else { Some(&mut listed) });
    }
    // A defeated pawn: its AnimInstance keeps updating in the build (AlwaysTickPoseAndRefreshBones), its machines
    // running on from the defeat; the port updates nothing while dead (meshTickMontages), so it lists nothing: every
    // state ends at the defeat and the left player is dropped.
    // Deviation: the nearest rule to a graph the port does not run; a state the build's machines would keep through
    // the defeat (a pawn defeated where it stood) ends here.
    if dead {
        w.fighter_mut(f).anim.fade_anim = Name::NONE;
    } else if layer {
        let data = w.data.clone();
        let p = parts_of(&data, w, f);
        let seqs = &p.cd.def.seq_notifies;
        let an = &w.fighter(f).anim;
        let (la, from, looping) = (an.loco_anim, an.seq_from, an.loco_loop);
        if d > 0.0 && seqs.contains_key(w.names.str(la)) {
            let len = anim_len(&p, w.names.str(la));
            let base = seq_base(w, f, la);
            seq_fire(w, f, la, from, d, looping, len, base, &mut listed);
        }
        if !w.fighter(f).anim.fade_anim.is_none() {
            // The left state is updated in every update of its crossfade, the one that completes it included (at
            // weight 0), and in no later one.
            let an = &mut w.fighter_mut(f).anim;
            an.fade_el += consts::DT;
            let done = an.fade_el >= (an.fade_dur - 1e-6);
            let fd = consts::DT * an.fade_rate;
            let (fa, ft, fl) = (an.fade_anim, an.fade_time, an.fade_loop);
            let len = anim_len(&p, w.names.str(fa));
            let base = seq_base(w, f, fa);
            seq_fire(w, f, fa, ft, fd, fl, len, base, &mut listed);
            let an = &mut w.fighter_mut(f).anim;
            if done {
                an.fade_anim = Name::NONE;
            } else {
                if an.fade_loop {
                    // the player's FAnimationRuntime::AdvanceTime (142816220): the Fmod wrap past the end
                    crate::math::ue::advance_time(true, fd, &mut an.fade_time, len);
                } else {
                    an.fade_time += fd;
                }
            }
        }
    }
    seq_states_tick(w, f, &listed);
    w.fighter_mut(f).anim.seq_listed = listed;
}

/// `seqFire`: one player's update of the sequence `anim` (its list in
/// `CharDef.seqNotifies`): GetAnimNotifies (exe 1428717c0) from `from` by `delta`, in pieces at the clip's end for a
/// looping player; GetAnimNotifiesFromDeltaPositions (exe 1428719e0): nothing when a piece is empty (previous ==
/// current: a non-looping clip held at its end), else a notify with TriggerTime <= current and EndTriggerTime >
/// previous. Instants fire here; a state's key goes into `listed` (base + index). A non-looping player's time is taken
/// clamped to the clip (AdvanceTime 142816220 clamps it; the locomotion time is not).
#[allow(clippy::too_many_arguments, reason = "the call's arguments")]
pub fn seq_fire(
    w: &mut World,
    f: FighterId,
    anim: Name,
    from: Real,
    delta: Real,
    looping: bool,
    len: Real,
    base: i32,
    listed: &mut Vec<i32>,
) {
    let data = w.data.clone();
    let p = parts_of(&data, w, f);
    let Some(list) = p.cd.def.seq_notifies.get(w.names.str(anim)) else { return };
    let mut prev = crate::math::min(from, len);
    let mut left = delta;
    let mut pass = 0;
    while pass < 4 && left > 0.0 {
        let end = prev + left > len;
        let cur = if end { len } else { prev + left };
        if cur > prev {
            for (i, n) in list.iter().enumerate() {
                let i = i32::try_from(i).expect("a notify index");
                // TriggerTime, and GetEndTriggerTime: TriggerTime + Duration in float
                let t = crate::math::narrow(n.t);
                if !n.state {
                    if t <= cur && t > prev {
                        instant(w, f, seq_ref(anim, i));
                    }
                } else if t <= cur && t + crate::math::narrow(n.dur) > prev && !listed.contains(&(base + i)) {
                    listed.push(base + i);
                }
            }
        }
        if !end || !looping {
            return;
        }
        left -= cur - prev;
        prev = 0.0;
        pass += 1;
    }
}

/// A sequence notify as the sinks take it: the sequence's name and the notify's index in its list
/// (`montages::notify_of` finds it there).
fn seq_ref(anim: Name, i: i32) -> NotifyRef {
    NotifyRef { montage: anim, index: i.cast_unsigned() }
}

/// `seqBase`: the key of the sequence's first notify: the sequences in `CharDef.seqNotifies`
/// order, each notify one key (its FAnimNotifyEvent: one per notify of a sequence, not per player); 0 for a sequence
/// with none.
pub fn seq_base(w: &World, f: FighterId, anim: Name) -> i32 {
    let p = parts_of(&w.data, w, f);
    let a = w.names.str(anim);
    let mut k = 0;
    for (name, l) in &p.cd.def.seq_notifies {
        if name == a {
            return k;
        }
        k += i32::try_from(l.len()).expect("a notify count");
    }
    0
}

/// The notify of key `k` (`seqKeys(f).refs[k]`): its sequence and its index there.
fn seq_key(w: &World, f: FighterId, k: i32) -> Option<NotifyRef> {
    let p = parts_of(&w.data, w, f);
    let mut base = 0;
    for (name, l) in &p.cd.def.seq_notifies {
        let n = i32::try_from(l.len()).expect("a notify count");
        if k < base + n {
            return Some(seq_ref(nm(&w.names, name), k - base));
        }
        base += n;
    }
    None
}

/// `seqIdx`: the index a sequence state is begun and ended with (the sinks' key: Ak_Continuous's sound, PlayVFX's
/// effect): below 0, apart from every montage instance's notify indices.
fn seq_idx(key: i32) -> i32 {
    -1 - key
}

/// `seqStatesTick`: UAnimInstance::TriggerAnimNotifies (exe 142845f30) for the sequence states:
/// an active state the frame's queue lists again stays (no End, no Begin; a looping clip's state across the wrap, a
/// state the entered and the left player of one sequence both list), the others get NotifyEnd, then the new ones
/// NotifyBegin in queue order (the current player's, then the left one's). The queue is the frame's update of the
/// layer: none when the layer is held under a montage (layer_frozen), so its states end there. The class is the
/// registry's (begin / end: one nobody entered warns there, no default).
/// Deviation: the engine has one queue for the montages' notifies and the graph's: their ends come before every Begin
/// of the frame. Here the montages' dispatch (advanceMontage) runs first, its begins included, then this one.
/// Deviation: NotifyTick is not dispatched (no handler of the registry has one).
pub fn seq_states_tick(w: &mut World, f: FighterId, listed: &[i32]) {
    if w.fighter(f).anim.seq_on.is_empty() && listed.is_empty() {
        return;
    }
    let mut i = 0;
    while let Some(&k) = w.fighter(f).anim.seq_on.get_index(i) {
        if listed.contains(&k) {
            i += 1;
            continue;
        }
        w.fighter_mut(f).anim.seq_on.shift_remove_index(i);
        let r = seq_key(w, f, k).expect("a sequence notify's key");
        end(w, f, r, seq_idx(k));
    }
    for &k in listed {
        if w.fighter(f).anim.seq_on.contains(&k) {
            continue;
        }
        w.fighter_mut(f).anim.seq_on.insert(k);
        let r = seq_key(w, f, k).expect("a sequence notify's key");
        begin(w, f, r, seq_idx(k));
    }
}

/// `seqFadeStart`: the state the machines left this frame: its player goes on while the
/// transition's crossfade runs, and is updated in the update that completes it (a 0 s crossfade: this frame's, one
/// window of seq_notify_tick).
pub fn seq_fade_start(w: &mut World, f: FighterId, anim: Name, time: Real, rate: Real, looping: bool) {
    let has = parts_of(&w.data, w, f).cd.def.seq_notifies.contains_key(w.names.str(anim));
    let an = &mut w.fighter_mut(f).anim;
    if !has {
        an.fade_anim = Name::NONE;
        return;
    }
    an.fade_anim = anim;
    an.fade_time = time;
    an.fade_rate = rate;
    an.fade_loop = looping;
    an.fade_el = 0.0;
    an.fade_dur = an.loco_blend.0;
}
