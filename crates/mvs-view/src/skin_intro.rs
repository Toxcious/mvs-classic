//! A skin's own match intro as the match view draws it, over the default skin's intro the sim plays
//!.
//!
//! BaseFighter ReadyForIntro @84838 tests IsValidSoftObjectReference(AppliedSkin.IntroMontageOverride), LoadAsset
//! @25991, and its Loaded event sets IntroAnimation = that montage @25948; PlayIntroAnimation (@107265) plays
//! IntroAnimation. So in the build the skin's montage is the intro: its pose, its length, its blends and its notifies
//! (C021_S02_Emote_Intro_Montage, 2.0 s and no notify, in place of C021_Emote_Intro_Montage, 1.95 s with its fly-in
//! sounds and effects). The port's sim plays the default skin's intro whatever the skin (a player's skin is never sim
//! state), so the view draws the skin's montage over it:
//! - its pose at the sim's intro position (the same slot rule as the montage player's pose), at its own blend-out
//!   weight over the locomotion; once it is over, the locomotion while the sim's intro still runs;
//! - past the sim's intro end, on at its own clock while the fighter stands where the intro left it with no montage
//!   and the same locomotion clip (a skin's intro longer than the default's);
//! - its sound notifies posted at their times (presented here, not by the sim), its sub-mesh visibility states on the
//!   parts the sim keeps no tag of, and the default intro's sounds, effects and outline states not presented.
//!
//! The data: FighterData.skins[s].intro_montage, the montage in `montages` or the skin's own `montages`.

// Deviation: the sim keeps the default intro's timing (its end, the blend-out, its notifies' sim effects: the outline
// count, AnimNotify_Darkness, ResocketItem, SetSubmeshVisibility on a sim tag), so what the fighter can do and when is
// the default skin's; a skin intro longer than the default's is cut by any input, a shorter one is followed by the
// idle while the sim's intro goes on. A sound notify of the skin's is posted at the view's tick that passes its time
// (no queued / branching-point split); an Ak_SurfaceEffect's switch is the fighter's last.

use std::collections::BTreeSet;
use std::sync::Mutex;

use mvs_anim::montage::{MontageState, alpha_to_blend_option};
use mvs_data::Value;
use mvs_data::fighter::{FighterData, Montage, SubMesh};
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::truthy;
use mvs_sim::names::Name;
use mvs_sim::{Event, World};

use crate::skins::Skin;

const DT: f64 = 1.0 / 60.0;
/// The notify classes whose presentation this module draws for a skin's intro; another warns once.
const SOUNDS: [&str; 3] = ["Ak_OneShot_C", "Ak_VO_Effort_C", "Ak_SurfaceEffect_C"];
const PARTS: &str = "AnimNotify_SetSubmeshVisibility_C";
/// The default intro's notifies whose presentation the skin's intro replaces (the sim still runs them).
const SHOWN: [&str; 10] = [
    "Ak_OneShot_C",
    "Ak_VO_Effort_C",
    "Ak_SurfaceEffect_C",
    "Ak_OneShotWithCondition_C",
    "Ak_Continuous_C",
    "AnimNotify_PlayVFX_C",
    "AnimNotifyState_PlayTimedVFX",
    "AnimNotifyState_TimedParticleEffect",
    "AnimNotify_PlayParticleEffect",
    "AnimNotify_PlaySingleShotVFX",
];

/// `warnOnce`.
pub(crate) fn warn_once(msg: String) {
    static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    let mut seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !seen.contains(&msg) {
        eprintln!("warning: {msg}");
        seen.insert(msg);
    }
}

fn text<'a>(n: &'a mvs_data::fighter::Notify, k: &str) -> Option<&'a str> {
    n.p.get(k).and_then(Value::as_str)
}

fn flag(n: &mvs_data::fighter::Notify, k: &str) -> bool {
    n.p.get(k).is_some_and(truthy)
}

/// The sim's intro montage of fighter data `d` (its default skin's IntroMontageOverride, else the pawn's
/// IntroAnimation), the skin's own and the two montages' data; None when the skin plays the default's intro or the data
/// holds no montage of one (warned).
fn montages<'d>(d: &'d FighterData, skin: &Skin) -> Option<(&'d str, &'d Montage, &'d Montage)> {
    let own = skin.intro.as_deref()?;
    let def = d.skin.intro_montage.as_deref().or_else(|| d.pawn.get("IntroAnimation").and_then(Value::as_str))?;
    let look = d.skins.as_ref().and_then(|m| m.get(&skin.name));
    let om = look.and_then(|l| l.montages.as_ref()).and_then(|m| m.get(own)).or_else(|| d.montages.get(own));
    let (Some(om), Some(dm)) = (om, d.montages.get(def)) else {
        warn_once(format!("skin intro {own} ({}): no montage in the data; the default intro is drawn", skin.name));
        return None;
    };
    Some((def, om, dm))
}

/// What of the default intro the skin's intro does not present (`mutedByIntro`'s part of a SkinIntro): the views', the
/// effects' and the sound's own copy, made from the fighter's data and skin.
#[derive(Clone, Debug)]
pub struct IntroMute {
    /// The sim's intro montage.
    def: String,
    /// The AkEvents of its sound notifies (the sim's sound events not presented).
    def_events: Vec<String>,
    /// Its notifies of the SHOWN classes, by index (the sim's effect events, by notify, not presented).
    def_notifies: Vec<usize>,
}

impl IntroMute {
    /// The skin's intro of fighter data `d` and skin `skin` (None: the default intro is the skin's).
    pub fn of(d: &FighterData, skin: &Skin) -> Option<IntroMute> {
        let (def, _, dm) = montages(d, skin)?;
        Some(IntroMute::new(def, dm))
    }

    fn new(def: &str, dm: &Montage) -> IntroMute {
        let shown = |n: &&mvs_data::fighter::Notify| SHOWN.contains(&n.cls.as_str());
        IntroMute {
            def: def.to_owned(),
            def_events: dm
                .notifies
                .iter()
                .filter(shown)
                .filter_map(|n| text(n, "AkEvent").filter(|s| !s.is_empty()).map(str::to_owned))
                .collect(),
            def_notifies: dm.notifies.iter().enumerate().filter(|(_, n)| shown(n)).map(|(k, _)| k).collect(),
        }
    }

    /// `simIntro`: the sim's intro shown this tick (PlayIntroAnimation's montage, the default's).
    fn sim_intro<'w>(&self, w: &'w World, f: FighterId) -> Option<&'w mvs_sim::fighter::anim::MontageState> {
        let fi = w.fighter(f);
        if mvs_sim::fighter::shown_montage(w, f) != Some(true) || !fi.anim.plain_is_intro {
            return None;
        }
        fi.montages.dash_montage.as_ref().filter(|sm| w.names.str(sm.montage.name) == self.def)
    }

    /// `mutedByIntro`: a sim event of fighter `f`'s default intro whose presentation the skin's intro replaces (not
    /// shown, not played).
    pub fn muted(&self, w: &World, f: FighterId, e: &Event) -> bool {
        match *e {
            Event::NotifyVfx { n, on, .. } | Event::ParticleNotify { n, on, .. } => {
                w.names.str(n.montage) == self.def
                    && usize::try_from(n.index).is_ok_and(|i| self.def_notifies.contains(&i))
                    && (!on || self.sim_intro(w, f).is_some())
            }
            Event::Sound { ev, .. } | Event::SoundStart { ev, .. } => {
                let ev = w.names.str(ev);
                self.def_events.iter().any(|d| d == ev) && self.sim_intro(w, f).is_some()
            }
            _ => false,
        }
    }
}

/// A SetSubmeshVisibility state of the skin's intro over parts with no sim tag.
#[derive(Clone, Debug)]
struct IntroPart {
    t: f64,
    dur: f64,
    visible: bool,
    /// The parts it sets, by index.
    keys: Vec<usize>,
}

/// A fighter's skin intro (`SkinIntro`): its montage, the default's, and the view's clock.
#[derive(Debug)]
pub struct SkinIntro {
    mute: IntroMute,
    /// The sim's intro name (the view's crossfade key while the skin's is drawn); None: never interned, so never
    /// played.
    def_name: Option<Name>,
    /// The default intro's tracks' sequences (the slot mix entries the skin's pose replaces).
    def_anims: Vec<String>,
    /// The default intro's AnimNotify_TurnOffOutline states, by index in its notifies.
    def_outline: Vec<usize>,
    own: Montage,
    /// The skin's montage, positioned by the view for its pose.
    player: MontageState<()>,
    /// The sound notifies: time and AkEvent.
    sounds: Vec<(f64, String)>,
    parts: Vec<IntroPart>,
    // the view's clock: the position drawn last tick and the fighter's tick, place and locomotion clip there
    pos: f64,
    tick: i32,
    x: f64,
    y: f64,
    loco: Name,
    /// The skin's intro was drawn last tick.
    on: bool,
    /// The position up to which its sounds are posted (-1: none yet).
    heard: f64,
}

/// What the view draws for a fighter with a skin intro this tick (`IntroPose`): the skin montage's clip (by slot and
/// segment), its time and weight (0: its blend-out is over, the locomotion is drawn while the sim's intro goes on).
#[derive(Clone, Copy, Debug)]
pub struct IntroPose {
    slot: usize,
    seg: usize,
    pub time: f64,
    pub w: f64,
    pub blend_in: f64,
    pub blend_out: f64,
    /// The default intro's TurnOffOutline states the sim has open (the skin's intro has none of its own).
    pub outline_fix: i32,
}

impl SkinIntro {
    /// `make`: fighter data `d`'s skin intro for the view of skin `skin` (None: the skin plays the default's intro).
    pub fn new(w: &World, d: &FighterData, skin: &Skin) -> Option<SkinIntro> {
        let (def, own, dm) = montages(d, skin)?;
        let name = &own.name;
        if !own.auto_blend_out {
            warn_once(format!("skin intro {name}: no auto blend-out; drawn as if it had one"));
        }
        // the look's sub-meshes and their tags (a part the look does not list, a default part kept, has none)
        let look = d.skins.as_ref().and_then(|m| m.get(&skin.name));
        let subs: &[SubMesh] = look.map_or(&d.skin.sub_meshes, |l| &l.sub_meshes);
        let mut sounds = Vec::new();
        let mut parts = Vec::new();
        for n in &own.notifies {
            if SOUNDS.contains(&n.cls.as_str()) {
                if let Some(ev) = text(n, "AkEvent").filter(|s| !s.is_empty()) {
                    sounds.push((n.t, ev.to_owned()));
                }
            } else if n.cls == PARTS {
                let tag = text(n, "SubmeshTag").unwrap_or("").to_lowercase();
                // (a part the sim keeps: the default intro's notifies set it in the sim)
                let keys: Vec<usize> = (0..skin.parts.len())
                    .filter(|&k| {
                        skin.part_tags[k].is_none()
                            && subs
                                .iter()
                                .find(|m| m.key == skin.parts[k].key)
                                .is_some_and(|m| m.tags.iter().any(|t| t.to_lowercase() == tag))
                    })
                    .collect();
                if keys.is_empty() {
                    continue;
                }
                if !n.state || !flag(n, "ReverseAtNotifyEnd") {
                    warn_once(format!(
                        "skin intro {name}: SetSubmeshVisibility {} without ReverseAtNotifyEnd not presented",
                        text(n, "SubmeshTag").unwrap_or("")
                    ));
                } else {
                    parts.push(IntroPart { t: n.t, dur: n.dur, visible: flag(n, "Visible"), keys });
                }
            } else {
                warn_once(format!("skin intro {name}: notify {} not presented", n.cls));
            }
        }
        let def_outline = dm
            .notifies
            .iter()
            .enumerate()
            .filter(|(_, n)| n.cls == "AnimNotify_TurnOffOutline_C")
            .map(|(k, _)| k)
            .collect();
        let mut def_anims: Vec<String> = Vec::new();
        for g in dm.slots.iter().flat_map(|s| &s.segments) {
            if !def_anims.contains(&g.anim) {
                def_anims.push(g.anim.clone());
            }
        }
        Some(SkinIntro {
            mute: IntroMute::new(def, dm),
            def_name: w.names.get(def),
            def_anims,
            def_outline,
            own: own.clone(),
            player: MontageState::new((), true),
            sounds,
            parts,
            pos: 0.0,
            tick: 0,
            x: 0.0,
            y: 0.0,
            loco: Name::NONE,
            on: false,
            heard: -1.0,
        })
    }

    /// The sim's intro name.
    pub fn def_name(&self) -> Option<Name> {
        self.def_name
    }

    /// Whether `anim` is a sequence of the default intro's tracks (pickIntro: a slot mix entry the skin's pose
    /// replaces).
    pub fn def_anim(&self, anim: &str) -> bool {
        self.def_anims.iter().any(|a| a == anim)
    }

    /// The clip a pose names.
    pub fn anim(&self, p: &IntroPose) -> &str {
        &self.own.slots[p.slot].segments[p.seg].anim
    }

    /// `introPose`: what the view draws for fighter `f` this tick (once a tick, before its sounds): None when the
    /// skin's intro is not drawn.
    pub fn pose(&mut self, w: &World, f: FighterId) -> Option<IntroPose> {
        let fi = w.fighter(f);
        let sm = self.mute.sim_intro(w, f);
        let pos = if let Some(sm) = sm {
            sm.pos
        } else if self.on
            && mvs_sim::fighter::shown_montage(w, f).is_none()
            && fi.life.dead == 0.0
            && mvs_sim::fighter::grounded(w, f)
            && f64::from(fi.body.x) == self.x
            && f64::from(fi.body.y) == self.y
            && fi.anim.loco_anim == self.loco
        {
            self.pos + f64::from(fi.clock.tick - self.tick) * DT
        } else {
            self.on = false;
            self.heard = -1.0;
            return None;
        };
        // its own automatic blend-out (the montage player's: from max(BlendOutTriggerTime, 1e-4) before the end, or
        // BlendOut before it, run for the whole BlendOut with the end pose held: C005_S02_Emote_Intro_Montage,
        // trigger 0)
        let (bo, start, end) = {
            let own = &self.own;
            let bo = if own.blend_out > 0.0 { own.blend_out } else { 0.0 };
            let start = if own.blend_out_trigger_time >= 0.0 {
                own.length - own.blend_out_trigger_time.max(1e-4)
            } else {
                own.length - bo
            };
            (bo, start, own.length.max(start + bo))
        };
        if pos >= end {
            self.on = false;
            sm?;
        } else {
            self.on = true;
            self.pos = pos;
            self.tick = fi.clock.tick;
            self.x = f64::from(fi.body.x);
            self.y = f64::from(fi.body.y);
            self.loco = fi.anim.loco_anim;
        }
        // (a stop of the sim's intro is the view's crossfade under the default intro's name: pose.rs place_view)
        let own = &self.own;
        let wt = if pos >= end {
            0.0
        } else if pos <= start || !(bo > 0.0) {
            1.0
        } else {
            let warn = |s: &str| warn_once(s.to_owned());
            1.0 - alpha_to_blend_option(own.blend_out_option.as_deref(), ((pos - start) / bo).min(1.0), &warn)
        };
        self.player.pos = pos.min(own.length);
        let (slot, seg, time) = self.player.pose(own, None)?;
        // the default intro's TurnOffOutline states the sim has open
        let open = sm.map_or(0, |sm| self.def_outline.iter().filter(|&&k| sm.has(k)).count());
        Some(IntroPose {
            slot,
            seg,
            time,
            w: wt,
            blend_in: own.blend_in,
            blend_out: bo,
            outline_fix: i32::try_from(open).unwrap_or(i32::MAX),
        })
    }

    /// `introParts`: the parts the skin intro's SetSubmeshVisibility states show or hide this tick, by part index
    /// (-1: not set).
    pub fn parts(&self, out: &mut [i8]) {
        out.fill(-1);
        if !self.on {
            return;
        }
        for p in &self.parts {
            if self.pos >= p.t && self.pos < p.t + p.dur {
                for &k in &p.keys {
                    out[k] = i8::from(p.visible);
                }
            }
        }
    }

    /// `introSounds`: the skin intro's sounds whose times the view's position passed since the last call (AkEvent
    /// names).
    pub fn sounds(&mut self, out: &mut Vec<String>) {
        out.clear();
        if !self.on {
            return;
        }
        for (t, ev) in &self.sounds {
            if (self.heard < 0.0 || *t > self.heard) && *t <= self.pos + 1e-6 {
                out.push(ev.clone());
            }
        }
        self.heard = self.pos + 1e-6;
    }
}
