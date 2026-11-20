//! The sim's events under rollback. A tick that is simulated again queues its events again;
//! the presenter must show each one once. An event's key is its tick, its variant, the actors and names in it, and its
//! count among equal ones in that tick.
//!
//! The key of an event is a typed key per [`Event`] variant holding the same fields (a data reference by its id in the
//! data).

use indexmap::IndexMap;
use mvs_sim::Event;
use mvs_sim::events::{ElectricKind, NotifyRef, SoundKey, UiAbilityOp, UiStateOp};
use mvs_sim::names::Name;

/// `baseKey`: what tells two events of one tick apart, without their count.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct EventKey {
    tag: std::mem::Discriminant<Event>,
    parts: Vec<u64>,
}

/// The fields of a key: the event's key fields, then the uiState op's and an item UI event's `actor`.
#[derive(Clone, Copy)]
enum F {
    Fighter,
    Attacker,
    Victim,
    Item,
    Team,
    Ev,
    Key,
    Cls,
    Name,
    Group,
    Kind,
    Phase,
    On,
    Hide,
    N,
    Add,
    Remove,
    Actor,
}

/// A `null` (a field that is there with no value: `rumbleStop`'s `name`), apart from every value.
const NULL: u64 = u64::MAX;

/// A key's parts: each field as its tag and its value.
struct Parts(Vec<u64>);

impl Parts {
    fn put(&mut self, f: F, v: u64) -> &mut Self {
        self.0.push(f as u64);
        self.0.push(v);
        self
    }
    fn int(&mut self, f: F, v: i32) -> &mut Self {
        self.put(f, u64::from(v.cast_unsigned()))
    }
    fn name(&mut self, f: F, v: Name) -> &mut Self {
        self.put(f, u64::from(v.0))
    }
    fn bool(&mut self, f: F, v: bool) -> &mut Self {
        self.put(f, u64::from(v))
    }
    /// A notify of the loaded data: the same on every simulation of the tick.
    fn notify(&mut self, f: F, n: &NotifyRef) -> &mut Self {
        self.put(f, (u64::from(n.montage.0) << 32) | u64::from(n.index))
    }
    /// A sound key (the string `'proj' + id`): its tag and its number.
    fn sound_key(&mut self, f: F, k: &SoundKey) -> &mut Self {
        self.put(f, (u64::from(k.tag.0) << 32) | u64::from(k.n.cast_unsigned()))
    }
    /// An optional field: left out where it is undefined.
    fn opt<T>(&mut self, v: Option<T>, put: impl FnOnce(&mut Self, T) -> &mut Self) -> &mut Self {
        if let Some(v) = v {
            put(self, v);
        }
        self
    }
    /// An item UI event of a spawned actor that is no item: its id is a projectile's, which meets the items' ids.
    fn actor(&mut self, actor: bool) -> &mut Self {
        if actor {
            self.put(F::Actor, 1);
        }
        self
    }
}

impl EventKey {
    /// The key of `e`: its variant and its `KEY_FIELDS` (`fighter`, `attacker`, `victim`, `item`, `team`, `ev`, `key`,
    /// `cls`, `name`, `group`, `kind`, `phase`, `on`, `hide`, `n`; a uiState's added id or removed name; an item UI
    /// event's `actor`).
    pub fn of(e: &Event) -> EventKey {
        use F::*;
        let mut p = Parts(Vec::with_capacity(8));
        match e {
            Event::Sound { fighter, ev, key, .. } => {
                p.int(Fighter, *fighter).name(Ev, *ev).opt(key.as_ref(), |p, k| p.sound_key(Key, k));
            }
            Event::SpawnedSound { instigator, ev, .. } => {
                p.int(Fighter, *instigator).name(Ev, *ev);
            }
            Event::SoundStart { fighter, ev, key } => {
                p.int(Fighter, *fighter).name(Ev, *ev).sound_key(Key, key);
            }
            Event::SoundStop { fighter, key, ev, .. } => {
                p.int(Fighter, *fighter).opt(*ev, |p, v| p.name(Ev, v)).sound_key(Key, key);
            }
            Event::Rtpc { fighter, name, .. }
            | Event::Rumble { fighter, name, .. }
            | Event::MatScalar { fighter, name, .. } => {
                p.int(Fighter, *fighter).name(Name, *name);
            }
            Event::RumbleStop { fighter, name, .. } => {
                p.int(Fighter, *fighter).put(Name, name.map_or(NULL, |n| u64::from(n.0)));
            }
            Event::Switch { fighter, group, .. } => {
                p.int(Fighter, *fighter).name(Group, *group);
            }
            Event::Hit { attacker, victim, .. } => {
                p.int(Attacker, *attacker).int(Victim, *victim);
            }
            Event::ItemFx { cls, item, fighter, .. } => {
                p.int(Fighter, *fighter).int(Item, *item).name(Cls, *cls);
            }
            Event::ProjectileDeath { cls, fighter, .. } => {
                p.int(Fighter, *fighter).name(Cls, *cls);
            }
            Event::ShockwaveVfx { fighter, cls, n, .. } => {
                p.int(Fighter, *fighter).name(Cls, *cls).int(N, *n);
            }
            Event::SlotActive { on, .. } | Event::CameraMovement { on } => {
                p.bool(On, *on);
            }
            Event::CameraShake { cls } | Event::MutatorPopText { cls, .. } => {
                p.name(Cls, *cls);
            }
            Event::WheelNext { index } | Event::WheelChange { index } => {
                p.int(N, *index);
            }
            Event::SmokeTrail { fighter, on }
            | Event::RageVfx { fighter, on }
            | Event::SpawnFx { fighter, on }
            | Event::RageGlow { fighter, on, .. }
            | Event::SwordGlow { fighter, on }
            | Event::EatUi { fighter, on, .. } => {
                p.int(Fighter, *fighter).bool(On, *on);
            }
            Event::DiVisual { fighter, attacker, .. } => {
                p.int(Fighter, *fighter).int(Attacker, *attacker);
            }
            Event::Backstab { fighter, victim } => {
                p.int(Fighter, *fighter).int(Victim, *victim);
            }
            Event::AryaKnife { fighter, kind, victim, .. } => {
                p.int(Fighter, *fighter).int(Victim, *victim).put(Kind, *kind as u64);
            }
            Event::GauntletHit { fighter, cls, victim } => {
                p.int(Fighter, *fighter).int(Victim, *victim).name(Cls, *cls);
            }
            Event::ReflectFx { item, .. } => {
                p.bool(Item, *item);
            }
            Event::PopText { fighter, kind, .. } => {
                p.int(Fighter, *fighter).put(Kind, *kind as u64);
            }
            Event::ItemUiCreate { item, fighter, actor, .. } => {
                p.int(Fighter, *fighter).int(Item, *item).actor(*actor);
            }
            Event::ItemUi { item, fighter, hide, actor, .. } => {
                p.int(Fighter, *fighter).int(Item, *item).bool(Hide, *hide).actor(*actor);
            }
            Event::ItemUiExit { item, fighter, actor } => {
                p.opt(*fighter, |p, f| p.int(Fighter, f)).int(Item, *item).actor(*actor);
            }
            Event::ElectricFx { fighter, kind, on, .. } => {
                p.int(Fighter, *fighter).put(Kind, *kind as u64).opt(*on, |p, v| p.bool(On, v));
            }
            Event::TazHealFx { fighter, kind } => {
                p.int(Fighter, *fighter).put(Kind, *kind as u64);
            }
            Event::MoveFx { fighter, kind, .. } => {
                p.int(Fighter, *fighter).put(Kind, *kind as u64);
            }
            Event::DisguiseFx { fighter, kind } => {
                p.int(Fighter, *fighter).put(Kind, *kind as u64);
            }
            Event::TeamScore { team, .. }
            | Event::MatchPoint { team }
            | Event::MatchPointShow { team }
            | Event::MatchOver { team } => {
                p.int(Team, *team);
            }
            Event::MatchEnd { phase, team } => {
                p.int(Team, *team).put(Phase, *phase as u64);
            }
            Event::UiState { fighter, op } => {
                p.int(Fighter, *fighter);
                match op {
                    UiStateOp::Add(a) => p.name(Add, a.id),
                    UiStateOp::Remove(r) => p.name(Remove, *r),
                };
            }
            Event::CloudLabel { fighter, on, hide, .. } => {
                p.int(Fighter, *fighter).bool(On, *on).opt(*hide, |p, v| p.bool(Hide, v));
            }
            Event::UiAbility { fighter, n, op } => {
                let kind = match op {
                    UiAbilityOp::Exit => 0,
                    UiAbilityOp::Error => 1,
                    UiAbilityOp::Update { .. } => 2,
                };
                p.int(Fighter, *fighter).put(Kind, kind).int(N, *n);
            }
            Event::NotifyVfx { fighter, n, on, .. }
            | Event::EmoteVfx { fighter, n, on, .. }
            | Event::ActorVfx { fighter, n, on, .. }
            | Event::ParticleNotify { fighter, n, on, .. } => {
                p.int(Fighter, *fighter).bool(On, *on).notify(N, n);
            }
            Event::HitVfx { victim, .. } => {
                p.int(Victim, *victim);
            }
            Event::ComboCounts { fighter, .. }
            | Event::CombatInput { fighter }
            | Event::MarkerBurst { fighter }
            | Event::MortarBoom { fighter, .. }
            | Event::AcceleratorBuff { fighter, .. }
            | Event::AcceleratorEnd { fighter, .. }
            | Event::Ko { fighter }
            | Event::ArmorHit { fighter, .. }
            | Event::ArmorBreak { fighter }
            | Event::Respawn { fighter }
            | Event::SlotBound { fighter, .. }
            | Event::PriorityAdd { fighter }
            | Event::KoVfx { fighter, .. }
            | Event::BlastBoxSound { fighter, .. }
            | Event::EvadeFx { fighter }
            | Event::RageResidual { fighter, .. }
            | Event::SizeScale { fighter, .. }
            | Event::EnlargeFx { fighter, .. }
            | Event::GrowFx { fighter }
            | Event::PolymorphFx { fighter, .. }
            | Event::TjPoof { fighter, .. }
            | Event::Stocks { fighter, .. }
            | Event::Abduct { fighter }
            | Event::IntroFx { fighter }
            | Event::IntroReveal { fighter }
            | Event::StruggleUi { fighter, .. }
            | Event::AbilityAmmo { fighter, .. }
            | Event::SubMeshAnim { fighter, .. }
            | Event::PulseHit { fighter, .. }
            | Event::AbilityUiCreate { fighter, .. } => {
                p.int(Fighter, *fighter);
            }
            Event::TutorialUi { .. } | Event::BounceVfx { .. } | Event::DoublePoints | Event::TimeUp => {}
        }
        EventKey { tag: std::mem::discriminant(e), parts: p.0 }
    }
}

/// What a tick queued for the first time (`fresh`) and what was shown for that tick before but did not come again
/// (`gone`: the prediction it came from was wrong).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Taken {
    pub fresh: Vec<Event>,
    pub gone: Vec<Event>,
}

/// `EventLog`: by tick, the events shown for it, by key and count.
#[derive(Clone, Debug, Default)]
pub struct EventLog {
    shown: IndexMap<u32, IndexMap<(EventKey, u32), Event>>,
}

impl EventLog {
    pub fn new() -> EventLog {
        EventLog::default()
    }

    /// `take`: the tick's events as simulated now, against what was shown for it.
    pub fn take(&mut self, tick: u32, events: &[Event]) -> Taken {
        let before = self.shown.get(&tick);
        let mut now = IndexMap::with_capacity(events.len());
        let mut counts: IndexMap<EventKey, u32> = IndexMap::new();
        let mut fresh = Vec::new();
        for e in events {
            let b = EventKey::of(e);
            let n = counts.entry(b.clone()).or_insert(0);
            let k = (b, *n);
            *n += 1;
            if !before.is_some_and(|m| m.contains_key(&k)) {
                fresh.push(e.clone());
            }
            now.insert(k, e.clone());
        }
        let gone = before.map_or_else(Vec::new, |b| {
            b.iter().filter(|(k, _)| !now.contains_key(*k)).map(|(_, e)| e.clone()).collect()
        });
        self.shown.insert(tick, now);
        Taken { fresh, gone }
    }

    /// `forget`: ticks up to this one are final (confirmed on every peer): they are not simulated again.
    pub fn forget(&mut self, up_to: u32) {
        self.shown.retain(|&t, _| t > up_to);
    }
}

/// `undo`: the event that takes back one that was shown from a wrong prediction, where one exists: what was switched
/// on goes off and the other way round. Single shots (a sound, a burst of particles) are left to play out.
pub fn undo(e: &Event) -> Option<Event> {
    let mut u = e.clone();
    match &mut u {
        Event::SoundStart { fighter, key, .. } => {
            return Some(Event::SoundStop {
                fighter: *fighter,
                key: *key,
                fade: None,
                ev: None,
                curve: None,
                cut: false,
            });
        }
        // (a taunt's sticker over the head, AnimNotify_PlayEmoteVFX: removed at its NotifyEnd; Arya's sword: a glow
        // lit by a montage the prediction got wrong stays lit until her next Off otherwise; Taz's Chew / Spit labels:
        // a wrong prediction's are taken off, every one of them)
        Event::SmokeTrail { on, .. }
        | Event::RageVfx { on, .. }
        | Event::SpawnFx { on, .. }
        | Event::RageGlow { on, .. }
        | Event::NotifyVfx { on, .. }
        | Event::EmoteVfx { on, .. }
        | Event::ActorVfx { on, .. }
        | Event::ParticleNotify { on, .. }
        | Event::SwordGlow { on, .. }
        | Event::EatUi { on, .. }
        | Event::CameraMovement { on } => *on = !*on,
        // (the TeamFightCloud's NEUTRAL + attack label likewise, a wrong one taken off at once)
        Event::CloudLabel { on, hide, .. } => {
            *on = !*on;
            *hide = Some(true);
        }
        Event::ElectricFx { kind, on, .. } => {
            if *kind != ElectricKind::Loop {
                return None;
            }
            *on = Some(!on.unwrap_or(false));
        }
        Event::ItemUiCreate { item, fighter, actor, .. } => {
            return Some(Event::ItemUiExit { item: *item, fighter: Some(*fighter), actor: *actor });
        }
        Event::ItemUi { hide, .. } => *hide = !*hide,
        Event::UiState { fighter, op } => {
            let UiStateOp::Add(a) = op else { return None };
            return Some(Event::UiState { fighter: *fighter, op: UiStateOp::Remove(a.id) });
        }
        // (the struggle bar a wrong prediction showed is hidden; a hide it made is left: the next call sets the bar)
        Event::StruggleUi { time_left, hide_input, .. } => {
            // (NaN too: the test is `timeLeft > 0`)
            if (*time_left).partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                return None;
            }
            *time_left = 0.0;
            *hide_input = false;
        }
        _ => return None,
    }
    Some(u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_sim::events::Fid;

    fn sound(fighter: Fid, ev: u32) -> Event {
        Event::Sound { fighter, ev: Name(ev), x: None, y: None, hit: None, pooled: false, key: None }
    }

    #[test]
    fn a_tick_simulated_again_shows_only_what_is_new_and_what_went() {
        let mut log = EventLog::new();
        let a = sound(0, 1);
        assert_eq!(log.take(5, &[a.clone(), a.clone()]).fresh.len(), 2);
        // the second of the two gone, another in its place
        let b = Event::Ko { fighter: 1 };
        let t = log.take(5, &[a.clone(), b.clone()]);
        assert_eq!(t.fresh, vec![b]);
        assert_eq!(t.gone, vec![a.clone()]);
        // a value outside the key (x) does not make it another event
        let mut moved = sound(0, 1);
        if let Event::Sound { x, .. } = &mut moved {
            *x = Some(3.0);
        }
        assert_eq!(log.take(5, &[moved, Event::Ko { fighter: 1 }]), Taken::default());
        log.forget(5);
        assert_eq!(log.take(5, std::slice::from_ref(&a)).fresh, vec![a]);
    }

    #[test]
    fn a_null_name_and_an_undefined_field_are_kept_apart() {
        let stop = |name| Event::RumbleStop { fighter: 0, name, tag: Name(1) };
        assert_ne!(EventKey::of(&stop(None)), EventKey::of(&stop(Some(Name(0)))));
        let e = |on| Event::ElectricFx { fighter: 0, kind: ElectricKind::Loop, on, target: None, x: None, y: None };
        assert_ne!(EventKey::of(&e(None)), EventKey::of(&e(Some(false))));
    }

    #[test]
    fn undo_switches_back() {
        let e = Event::ElectricFx { fighter: 0, kind: ElectricKind::Loop, on: None, target: None, x: None, y: None };
        assert!(matches!(undo(&e), Some(Event::ElectricFx { on: Some(true), .. })));
        let burst =
            Event::ElectricFx { fighter: 0, kind: ElectricKind::Burst, on: None, target: None, x: None, y: None };
        assert_eq!(undo(&burst), None);
        let c = Event::ItemUiCreate { item: 3, fighter: 1, action: 0, on_ground: true, actor: true };
        assert_eq!(undo(&c), Some(Event::ItemUiExit { item: 3, fighter: Some(1), actor: true }));
        let s = Event::StruggleUi { fighter: 0, time_left: 1.0, total: 2.0, hide_input: true };
        assert!(matches!(undo(&s), Some(Event::StruggleUi { hide_input: false, .. })));
        assert_eq!(undo(&sound(0, 1)), None);
    }
}
