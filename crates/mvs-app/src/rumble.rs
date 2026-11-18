//! Controller rumble: the sim's `rumble` / `rumbleStop` events (BaseFighter ControllerRumble /
//! RemoveControllerRumble) past the rest of their gates, each local human's PlayerController's list of active
//! ForceFeedbackEffects, and the XInput send.
//!
//! Gates (BaseFighter ControllerRumble, RemoveControllerRumble): IsVibrationEnabled (IsLocallyControlled, not a bot:
//! the sim's test; Get-Input-Vibration), then the controller's isGamePadKey. One list and one send per local human
//! with a pad, on that pad (ProcessForceFeedbackAndHaptics 142ccc380 sends to the PC's ControllerId).
//!
//! The list (APlayerController ClientPlayForceFeedback_Internal 142cbd1d0, ClientStopForceFeedback 142cbdfe0,
//! FActiveForceFeedbackEffect Update 142a66140 / GetValues 142a5b430): a play with a Tag other than None first
//! removes every entry of that Tag, then always appends; a stop of (null, None) empties the list, else removes every
//! entry with (the effect null or the same) and (Tag None or the same). Each frame every running entry adds the
//! frame's time to its PlayTime, is kept while PlayTime <= Duration (or it loops with Duration != 0), and writes each
//! channel's curve at PlayTime mod Duration as max(current, min(v, 1)) to its motors; then each value is
//! clamp(ForceFeedbackScale 1.0 x v, 0, 1). The XInput send (14162ddf0): low frequency max(LeftLarge, RightLarge),
//! high frequency max(LeftSmall, RightSmall), on a change over 1e-8 (Pads::rumble).

use mvs_data::{GameData, RumbleEffect};
use mvs_sim::Event;
use mvs_sim::names::{Name, Names};

use crate::input::Devices;

/// A curve of linear keys with constant extrapolation, at t.
pub fn curve_at(keys: &[(f64, f64)], t: f64) -> f64 {
    let Some(&(t0, v0)) = keys.first() else { return 0.0 };
    if t <= t0 {
        return v0;
    }
    let (tn, vn) = keys[keys.len() - 1];
    if t >= tn {
        return vn;
    }
    let mut i = 1;
    while keys[i].0 < t {
        i += 1;
    }
    let ((t0, v0), (t1, v1)) = (keys[i - 1], keys[i]);
    if t1 > t0 { v0 + (v1 - v0) * (t - t0) / (t1 - t0) } else { v1 }
}

const FORCE_FEEDBACK_SCALE: f64 = 1.0;

#[derive(Clone, Debug)]
struct Active {
    name: Name,
    fx: RumbleEffect,
    /// GetDuration's cached float.
    duration: f64,
    /// None: Tag None.
    tag: Option<Name>,
    looping: bool,
    while_paused: bool,
    /// PlayTime.
    t: f64,
}

/// One PlayerController's active effects and its four values.
#[derive(Clone, Debug, Default)]
pub struct RumbleMixer {
    active: Vec<Active>,
    /// LeftLarge, LeftSmall, RightLarge, RightSmall after the scale and the clamp.
    pub values: [f64; 4],
    /// max(LeftLarge, RightLarge).
    pub strong: f64,
    /// max(LeftSmall, RightSmall).
    pub weak: f64,
}

impl RumbleMixer {
    pub fn play(&mut self, name: Name, fx: &RumbleEffect, tag: Option<Name>, looping: bool, while_paused: bool) {
        if let Some(tag) = tag {
            self.active.retain(|a| a.tag != Some(tag));
        }
        let duration = f64::from(fx.duration as f32);
        self.active.push(Active { name, fx: fx.clone(), duration, tag, looping, while_paused, t: 0.0 });
    }

    pub fn stop(&mut self, name: Option<Name>, tag: Option<Name>) {
        if name.is_none() && tag.is_none() {
            self.active.clear();
            return;
        }
        self.active.retain(|a| !(name.is_none_or(|n| a.name == n) && tag.is_none_or(|t| a.tag == Some(t))));
    }

    /// ProcessForceFeedbackAndHaptics for a frame of `dt` seconds; `paused` the game paused.
    pub fn update(&mut self, dt: f64, paused: bool) {
        let v = &mut self.values;
        *v = [0.0; 4];
        let mut i = self.active.len();
        while i > 0 {
            i -= 1;
            let a = &mut self.active[i];
            if paused && !a.while_paused {
                continue;
            }
            a.t += dt;
            let d = a.duration;
            if a.t > d && (!a.looping || d == 0.0) {
                self.active.remove(i);
                continue;
            }
            let at = if d > 0.0 { a.t - d * (a.t / d).floor() } else { 0.0 };
            for c in &a.fx.channels {
                let x = curve_at(&c.keys, at).min(1.0);
                for (m, vm) in v.iter_mut().enumerate() {
                    if c.motors & (1 << m) != 0 && x > *vm {
                        *vm = x;
                    }
                }
            }
        }
        for vm in v.iter_mut() {
            *vm = (FORCE_FEEDBACK_SCALE * *vm).clamp(0.0, 1.0);
        }
        self.strong = v[0].max(v[2]);
        self.weak = v[1].max(v[3]);
    }

    pub fn idle(&self) -> bool {
        self.active.is_empty() && self.strong == 0.0 && self.weak == 0.0
    }
}

/// The match's rumble: a mixer per local human with a device that can have a pad.
#[derive(Debug, Default)]
pub struct MatchRumble {
    /// (player slot, its mixer).
    pads: Vec<(usize, RumbleMixer)>,
    warned: Vec<Name>,
}

impl MatchRumble {
    pub fn new(devices: &Devices) -> MatchRumble {
        let pads = devices.slots().map(|s| (s, RumbleMixer::default())).collect();
        MatchRumble { pads, warned: Vec::new() }
    }

    /// The gates past the sim's IsBot test: Get-Input-Vibration, then the controller's isGamePadKey.
    fn gates(devices: &mut Devices, slot: usize) -> bool {
        devices.device(slot).is_some_and(|d| d.prefs.vibration && d.is_game_pad_key)
    }

    /// A sim event: a `rumble` or `rumbleStop` goes to its fighter's mixer.
    pub fn event(&mut self, e: &Event, data: &GameData, names: &Names, devices: &mut Devices) {
        let none = |n: Name| {
            let s = names.str(n);
            if s.is_empty() || s == "None" { None } else { Some(n) }
        };
        let (fighter, play) = match *e {
            Event::Rumble { fighter, .. } | Event::RumbleStop { fighter, .. } => {
                (fighter, matches!(e, Event::Rumble { .. }))
            }
            _ => return,
        };
        let Ok(slot) = usize::try_from(fighter) else { return };
        if devices.pad_slot(slot).is_none() || !Self::gates(devices, slot) {
            return;
        }
        let Some((_, mixer)) = self.pads.iter_mut().find(|(s, _)| *s == slot) else { return };
        match *e {
            Event::Rumble { name, looping, tag, play_while_paused, .. } if play => {
                let Some(fx) = data.rumble(names.str(name)) else {
                    if !self.warned.contains(&name) {
                        self.warned.push(name);
                        eprintln!("rumble: no ForceFeedbackEffect {}", names.str(name));
                    }
                    return;
                };
                mixer.play(name, fx, none(tag), looping, play_while_paused);
            }
            Event::RumbleStop { name, tag, .. } => mixer.stop(name.and_then(none), none(tag)),
            _ => {}
        }
    }

    /// Once per drawn frame, after the frame's ticks: `dt` the frame's real time, s.
    pub fn frame(&mut self, dt: f64, devices: &mut Devices) {
        for (slot, mixer) in &mut self.pads {
            if mixer.idle() {
                continue;
            }
            mixer.update(dt, false);
            if let Some(k) = devices.pad_slot(*slot) {
                devices.pads_mut().rumble(k, mixer.strong, mixer.weak);
            }
        }
    }

    /// Every effect stopped and the motors off (the window lost focus, the match ended).
    pub fn stop_all(&mut self, devices: &mut Devices) {
        for (slot, mixer) in &mut self.pads {
            mixer.stop(None, None);
            mixer.update(0.0, false);
            if let Some(k) = devices.pad_slot(*slot) {
                devices.pads_mut().rumble(k, 0.0, 0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_data::RumbleChannel;

    fn fx() -> RumbleEffect {
        RumbleEffect {
            duration: 0.5,
            channels: vec![RumbleChannel { motors: 1 | 8, keys: vec![(0.0, 1.0), (0.5, 0.0)] }],
        }
    }

    #[test]
    fn an_effect_ramps_and_ends() {
        let mut m = RumbleMixer::default();
        m.play(Name(1), &fx(), None, false, false);
        m.update(0.25, false);
        assert_eq!((m.strong, m.weak), (0.5, 0.5));
        m.update(0.3, false);
        assert!(m.idle() || m.strong == 0.0);
        assert!(m.active.is_empty());
    }

    #[test]
    fn a_tagged_play_replaces_its_tag() {
        let mut m = RumbleMixer::default();
        m.play(Name(1), &fx(), Some(Name(5)), true, false);
        m.play(Name(2), &fx(), Some(Name(5)), true, false);
        assert_eq!(m.active.len(), 1);
        m.stop(None, Some(Name(5)));
        assert!(m.active.is_empty());
    }
}
