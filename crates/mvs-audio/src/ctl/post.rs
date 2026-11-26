//! Posting an event: its actions in order with their delays (Play, PlayEvent, Stop, Pause / Resume, Mute,
//! UnMute, SetState), and the stops by key.

use mvs_data::sound::{ActionKind, ActionScope, Curve, Id, Obj};

use super::voice::{Opts, Who, bus_under, play_node, stop_where, voice_frame};
use super::{Ctl, Out, secs};
use crate::spec::{Fade, Param, Ramp, Target};
use crate::{Key, Note, Post};

/// The note of a Mute action on a bus when none was muted (its id the bus), and of an UnMute All that unmuted some
/// (its id how many): the mute is silent otherwise, and a session whose effects stay muted names it in the log.
pub const BUSES_MUTED: &str = "buses muted by a Mute action";
pub const BUSES_UNMUTED: &str = "buses unmuted by an UnMute All";

/// Posts an event at audio clock `now`.
// Unverified: a post of an event whose bank is not loaded is dropped with a warning. Here every bank a post can
// need is in a scope before it is posted.
pub fn post(ctl: &mut Ctl, p: &Post, now: u64, out: &mut Out) {
    let Some(acts) = ctl.banks.event(p.event).map(<[Id]>::to_vec) else {
        ctl.warn("event not loaded: post dropped", u64::from(p.event), out);
        return;
    };
    let opts = Opts {
        post: p.id,
        obj: p.obj,
        key: p.key,
        stop_key: p.stop_key,
        at: p.at,
        skip_delay: None,
        chain: None,
        sure: p.sure,
    };
    post_event(ctl, &acts, &opts, now, out);
}

/// An Event object's actions (a post; a PlayEvent action's target, which its bank holds).
fn post_event(ctl: &mut Ctl, acts: &[Id], opts: &Opts, now: u64, out: &mut Out) {
    let t = secs(now);
    let mut dirty = false;
    for &aid in acts {
        let Some(Obj::EventAction(a)) = ctl.banks.obj(aid).cloned() else { continue };
        let delay = a.p.get(15).unwrap_or(0.0) / 1000.0;
        let fade = a.p.get(16).unwrap_or(0.0) / 1000.0;
        let curve = a.fade_curve.unwrap_or(Curve::Linear);
        match a.kind {
            ActionKind::Play => {
                // CAkActionPlay::Execute (1800c5750): an action with the probability property (0x11, percent)
                // does nothing when it is 0 or under a roll of 0..100 (a sure post rolls and plays)
                if let Some(prob) = a.p.get(17)
                    && (prob == 0.0 || prob < ctl.draw(out) * 100.0)
                    && !opts.sure
                {
                    continue;
                }
                let Some(target) = a.target else { continue };
                if matches!(ctl.banks.obj(target), Some(Obj::MusicSwitchContainer(_))) {
                    super::music::play(ctl, target, opts.owner(), now, out);
                    continue;
                }
                play_node(ctl, target, t + delay, Fade { secs: fade, curve }, opts, &[], t, out);
            }
            ActionKind::PlayEvent => {
                let Some(target) = a.target else { continue };
                match ctl.banks.ev_obj(target).map(<[Id]>::to_vec) {
                    Some(sub) => post_event(ctl, &sub, opts, now, out),
                    None => ctl.warn("PlayEvent target missing", u64::from(target), out),
                }
            }
            ActionKind::Stop => {
                // the action's transition time and curve as a stop fade. A Stop with no target stops every voice
                // of the object it is posted on.
                // Unverified: CAkActionStop's way to CAkPBI::_Stop was not read; the targetless scope is read as
                // Wwise's Stop All on a game object.
                let (target, stop_key, obj) = (a.target, opts.stop_key, opts.obj);
                let pred = move |w: &Who| {
                    target.is_none_or(|x| w.chain.contains(&x))
                        && match stop_key {
                            Some(k) => w.key == Some(k),
                            None => obj.is_none_or(|o| w.owner == o),
                        }
                };
                stop_where(ctl, &pred, fade, delay, true, Some(curve), t, out);
            }
            ActionKind::SetState => {
                if let (Some(g), Some(v)) = (a.group, a.target) {
                    ctl.set_state(g, v, now, out);
                }
            }
            ActionKind::Pause => {
                if a.scope == ActionScope::All {
                    pause_all(ctl, &a.except, now, out);
                } else {
                    ctl.warn("Pause action scope not ported", u64::from(aid), out);
                }
                dirty = true;
            }
            ActionKind::Resume => {
                if (a.scope == ActionScope::All) && a.except.is_empty() {
                    resume_all(ctl, now, out);
                } else {
                    ctl.warn("Resume action scope not ported", u64::from(aid), out);
                }
                dirty = true;
            }
            ActionKind::Mute => {
                match a.target {
                    Some(b) if a.is_bus && fade == 0.0 => {
                        if !ctl.muted.contains(&b) {
                            // (each time, not once: a mute with no unmute after it in a pasted log names a session
                            // whose effects went silent)
                            if ctl.muted.is_empty() {
                                let _ = out.notes.push(Note::Warn { what: BUSES_MUTED, id: u64::from(b) });
                            }
                            ctl.muted.push(b);
                        }
                    }
                    _ => ctl.warn("Mute action not ported", u64::from(aid), out),
                }
                dirty = true;
            }
            ActionKind::UnMute => {
                if (a.scope == ActionScope::All) && a.except.is_empty() && fade == 0.0 {
                    if !ctl.muted.is_empty() {
                        let _ = out.notes.push(Note::Warn { what: BUSES_UNMUTED, id: ctl.muted.len() as u64 });
                    }
                    ctl.muted.clear();
                } else {
                    ctl.warn("UnMute action scope not ported", u64::from(aid), out);
                }
                dirty = true;
            }
            // Seek and the rest: not needed for one-shot playback
            _ => {}
        }
    }
    // (once, after the event's last action: a pause off resumes and then unmutes, and a voice is not judged
    // between the two)
    if dirty && !ctl.voices.list.is_empty() {
        voice_frame(ctl, true, t, out);
    }
}

/// Pause All except the buses `except` and everything routed into them (CAkBus::ExecuteMasterBusAction 18005c710):
/// a paused voice stands still and is silent, its start moves by the pause; the state transitions are held
/// (CAkTransitionManager::PauseAllStateTransitions 18003f140).
// Unverified: not listened to in the build. A fade-in under way is not stretched by the pause; a paused voice with
// the kill behaviour is not judged against the threshold while paused; a muted bus's voices lose their aux sends
// too.
fn pause_all(ctl: &mut Ctl, except: &[Id], now: u64, out: &mut Out) {
    let t = secs(now);
    for i in 0..ctl.voices.list.len() {
        let v = &ctl.voices.list[i];
        if v.paused.is_some() || bus_under(ctl, v.bus, except) {
            continue;
        }
        let v = &mut ctl.voices.list[i];
        v.paused = Some(t);
        let id = v.id;
        Ctl::change(
            out,
            crate::spec::ParamChange {
                at: super::samples(t),
                target: Target::Voice(id),
                param: Param::Pause,
                value: 1.0,
                ramp: Ramp::Set,
            },
        );
    }
    if ctl.syncs.hold_states(now) {
        let Ctl { buses, syncs, .. } = ctl;
        buses.states_moved(0.0, true, now, syncs, out);
    }
}

/// Resume All: the paused voices go on (one paused before its start at its moved start), the state transitions
/// take the time they were held with.
fn resume_all(ctl: &mut Ctl, now: u64, out: &mut Out) {
    let t = secs(now);
    for i in 0..ctl.voices.list.len() {
        let Some(p) = ctl.voices.list[i].paused else { continue };
        let (_, cents) = ctl.syncs.rtpc_of(&ctl.voices.list[i].plan.rtpc, ctl.voices.list[i].obj);
        let v = &mut ctl.voices.list[i];
        v.start += t - p;
        v.paused = None;
        let at = super::samples(t.max(v.start));
        let id = Target::Voice(v.id);
        Ctl::change(out, crate::spec::ParamChange { at, target: id, param: Param::Pause, value: 0.0, ramp: Ramp::Set });
        if !v.streamed {
            v.rate = f64::from(crate::math::pow(2.0, (v.cents + cents) / 1200.0) as f32);
            let value = v.cents + cents;
            Ctl::change(out, crate::spec::ParamChange { at, target: id, param: Param::Cents, value, ramp: Ramp::Set });
        }
    }
    if let Some(ms) = ctl.syncs.release_states(now) {
        let Ctl { buses, syncs, .. } = ctl;
        buses.states_moved(ms, false, now, syncs, out);
    }
}

/// Stops the sounds of a key over `fade` s: along the sound engine's stop curve when one is given, else a line.
pub fn stop_key(ctl: &mut Ctl, key: Key, fade: f64, curve: Option<Curve>, now: u64, out: &mut Out) {
    stop_where(ctl, &|w: &Who| w.key == Some(key), fade, 0.0, true, curve, secs(now), out);
}

/// Stops every sound but those of the key or group hashed `keep` (a key `<keep>:<name>` is in the group).
pub fn stop_all(ctl: &mut Ctl, fade: f64, keep: Option<u64>, now: u64, out: &mut Out) {
    let pred = |w: &Who| match (keep, w.key) {
        (None, _) | (Some(_), None) => true,
        (Some(k), Some(key)) => key.id != k && key.under != k,
    };
    stop_where(ctl, &pred, fade, 0.0, true, None, secs(now), out);
}
