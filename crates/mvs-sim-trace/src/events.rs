//! The step's events as the record writes them (`EventRecord`: the oracle event's `type` and its other keys, an object
//! by value and a notify by its identity), and back. A key the oracle type marks optional (`x?:`, `light?: true`) is
//! left out where the Rust value is unset (`~`); the others are always written, `null` for a Rust `None` (`=`).

use mvs_sim::events::{
    DisguiseFxKind, ElectricKind, EndPhase, Event, KnifeKind, MoveFxKind, PopKind, PopParams, SoundHit, SoundKey,
    TazHealKind, UiAbilityOp, UiStateOp, UiStateParams,
};
use mvs_trace::{EventRecord, Obj, Value};

use crate::conv::{Plain, Unset, as_obj, check_keys, named_enum, obj, plain_struct, text};
use crate::cx::{Lx, Wx};

named_enum!(PopKind, "pop text kind", [
    PopKind::WallFatigue => "wallFatigue", PopKind::AttackDecay => "attackDecay", PopKind::BuffDecay => "buffDecay",
    PopKind::Healing => "healing", PopKind::TurnAttack => "turnAttack", PopKind::FfaRingout => "ffaRingout",
    PopKind::FfaAssist => "ffaAssist", PopKind::FfaRungout => "ffaRungout", PopKind::Dodged => "dodged",
    PopKind::DodgedOff => "dodgedOff", PopKind::LastStand => "lastStand",
    PopKind::SpecialsRefreshed => "specialsRefreshed", PopKind::AirRefreshed => "airRefreshed",
    PopKind::ReflectProjectiles => "reflectProjectiles", PopKind::Caught => "caught",
    PopKind::JumpsReset => "jumpsReset", PopKind::FinnTreasure => "finnTreasure",
    PopKind::FinnHighFive => "finnHighFive", PopKind::FinnBoots => "finnBoots", PopKind::FinnSale => "finnSale",
    PopKind::NotifyText => "notifyText", PopKind::TjCheese => "tjCheese",
    PopKind::GizmoForceDance => "gizmoForceDance", PopKind::GizmoAttach => "gizmoAttach",
    PopKind::Transferred => "transferred",
]);
named_enum!(KnifeKind, "knife kind", [KnifeKind::Stick => "stick", KnifeKind::Ground => "ground", KnifeKind::Dash => "dash"]);
named_enum!(ElectricKind, "electric kind", [
    ElectricKind::Loop => "loop", ElectricKind::Burst => "burst", ElectricKind::Beam => "beam",
]);
named_enum!(TazHealKind, "heal kind", [TazHealKind::Lick => "lick", TazHealKind::Pickup => "pickup"]);
named_enum!(MoveFxKind, "move fx kind", [
    MoveFxKind::Jump => "jump", MoveFxKind::DoubleJump => "doubleJump", MoveFxKind::Land => "land",
    MoveFxKind::FastFall => "fastFall", MoveFxKind::WallOn => "wallOn", MoveFxKind::WallOff => "wallOff",
]);
named_enum!(DisguiseFxKind, "disguise fx kind", [
    DisguiseFxKind::Activate => "activate", DisguiseFxKind::Destroy => "destroy",
]);
named_enum!(EndPhase, "end phase", [
    EndPhase::GameEnd => "gameEnd", EndPhase::PreTravel => "preTravel", EndPhase::Cleanup => "cleanup",
    EndPhase::Travel => "travel",
]);

// Unverified: the oracle builds a sound key as a string (`f.id + ':wind'`, `f.id + ':' + idx`, `'proj' + pr.id`,
// `'smoke' + z.id`); the Rust key is its fixed part and its number. A tag that starts with `:` follows its number
// (`1:wind` is n 1, tag `:wind`), any other precedes it (`proj0` is tag `proj`, n 0): the sim's keys keep that form.
// A key of two numbers (`0:vo3`, `1:charge2`, `1:6`) is its tag (`:vo`, `:charge`, `:`) and the second number, the
// event's fighter written ahead (`fighter_key_put`). A tag with a `#` takes its number there (`proj#d`: `proj7d`).
impl Plain for SoundKey {
    fn to_value(&self, x: &Wx) -> Value {
        let tag = x.str(self.tag);
        Value::Str(if self.n == i32::MIN {
            // a key the oracle holds as a bare string (the Court's `m010ShotClock`): its tag alone
            tag.to_owned()
        } else if let Some((pre, post)) = tag.split_once('#') {
            format!("{pre}{}{post}", self.n)
        } else if tag.starts_with(':') {
            format!("{}{tag}", self.n)
        } else {
            format!("{tag}{}", self.n)
        })
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(k) = Self::from_value(v, l) {
            *self = k;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let s = text(v, l)?.to_owned();
        if s == "m010ShotClock" {
            return Some(SoundKey { tag: l.name(&s), n: i32::MIN });
        }
        if let Some((n, tag)) = fighter_key_parse(&s) {
            return Some(SoundKey { tag: l.name(tag), n });
        }
        let colon = s.find(':').filter(|&i| i > 0 && s[..i].parse::<i32>().is_ok());
        let parsed = match colon {
            Some(i) => s[..i].parse::<i32>().ok().map(|n| (n, s[i..].to_owned())),
            None => {
                let at = s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
                if at == s.len() {
                    // (a number with a letter after it: `proj7d`)
                    let word = s.trim_end_matches(|c: char| c.is_ascii_alphabetic()).len();
                    let start = s[..word].trim_end_matches(|c: char| c.is_ascii_digit()).len();
                    (start > 0 && start < word && word < s.len())
                        .then(|| (s[start..word].parse::<i32>().ok(), format!("{}#{}", &s[..start], &s[word..])))
                        .and_then(|(n, t)| Some((n?, t)))
                } else {
                    (at > 0).then(|| (s[at..].parse::<i32>().ok(), s[..at].to_owned())).and_then(|(n, t)| Some((n?, t)))
                }
            }
        };
        match parsed {
            Some((n, tag)) => Some(SoundKey { tag: l.name(&tag), n }),
            None => {
                l.miss(format_args!("the sound key {s:?} is neither <n>:<tag> nor <tag><n>"));
                None
            }
        }
    }
}

plain_struct!(SoundHit {
    "victim" = victim, "zFloor" = z_floor, "charge" = charge, "surface" ~ surface, "instigator" ~ instigator,
} make |v, l| {
    let mut h = SoundHit { victim: -1, z_floor: None, charge: None, surface: None, instigator: None };
    h.load_value(v, l);
    Some(h)
});

plain_struct!(UiStateParams {
    "id" = id, "stateType" = state_type, "displayType" = display_type, "currentValue" = current_value,
    "maxValue" = max_value, "timer" = timer, "iconMaterial" = icon_material, "active" = active,
    "popText" = pop_text, "autoRemoveTime" = auto_remove_time, "subicon" = subicon,
} make |v, l| {
    let mut p = UiStateParams {
        id: mvs_sim::names::Name::NONE,
        state_type: 0,
        display_type: 0,
        current_value: 0.0,
        max_value: 0.0,
        timer: 0.0,
        icon_material: mvs_sim::names::Name::NONE,
        active: false,
        pop_text: mvs_sim::names::Name::NONE,
        auto_remove_time: 0.0,
        subicon: None,
    };
    p.load_value(v, l);
    Some(p)
});

/// `UiStateOp`: `{add: params}` or `{remove: id}`.
impl Plain for UiStateOp {
    fn to_value(&self, x: &Wx) -> Value {
        match self {
            UiStateOp::Add(p) => obj(vec![("add", p.to_value(x))]),
            UiStateOp::Remove(id) => obj(vec![("remove", id.to_value(x))]),
        }
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(o) = Self::from_value(v, l) {
            *self = o;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        check_keys(o, &["add", "remove"], l);
        if let Some(p) = o.get("add") {
            return UiStateParams::from_value(p, l).map(UiStateOp::Add);
        }
        match o.get("remove") {
            Some(id) => mvs_sim::names::Name::from_value(id, l).map(UiStateOp::Remove),
            None => {
                l.miss("a ui state op of neither add nor remove");
                None
            }
        }
    }
}

// Unverified: `popText.params` is a notify's own record or a caller's (`Record<string, any>`); a graph's text is held
// by the item and mutator traces. A notify is written as its properties (its identity where the data has none), a
// position as `{x, y}`, the values as an object of their names.
impl Plain for PopParams {
    fn to_value(&self, x: &Wx) -> Value {
        match self {
            // The oracle passes the notify's own properties: written whole, as the data holds them.
            PopParams::Notify(n) => match crate::fighter::notify_props_any(x, *n) {
                Some(p) => crate::parts::data_put(&mvs_data::value::Value::Obj(p.clone())),
                None => n.to_value(x),
            },
            PopParams::At { x: px, y } => {
                obj(vec![("x", Value::Num(f64::from(*px))), ("y", Value::Num(f64::from(*y)))])
            }
            PopParams::Values(v) => {
                Value::Obj(Obj::new(v.iter().map(|(k, n)| (x.str(*k).to_owned(), Value::Num(f64::from(*n)))).collect()))
            }
            PopParams::Graph(g) => graph_pop_text(g, x),
        }
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(p) = Self::from_value(v, l) {
            *self = p;
        }
    }
    // A notify's own properties (the first AnimNotify_PopText of the data written the same: two alike pop the same
    // text), a graph's text (its `Text` key), a position (`x` and `y` alone) or the values (numbers by name).
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        let book = l.book;
        if let Some((m, i)) = book.pop_texts.iter().find(|(_, p)| crate::parts::same_value(p, v)).map(|(k, _)| k) {
            return Some(PopParams::Notify(mvs_sim::events::NotifyRef { montage: l.name(m), index: *i }));
        }
        if o.get("Text").is_some() {
            return graph_pop_text_load(o, l).map(|g| PopParams::Graph(Box::new(g)));
        }
        let mut vals = Vec::new();
        for (k, n) in o.entries() {
            let Value::Num(n) = n else {
                l.miss("a pop text's params (a notify's own properties have no load rule)");
                return None;
            };
            vals.push((l.name(k), mvs_sim::math::narrow(*n)));
        }
        match (o.get("x"), o.get("y")) {
            (Some(Value::Num(px)), Some(Value::Num(y))) if o.len() == 2 => {
                Some(PopParams::At { x: mvs_sim::math::narrow(*px), y: mvs_sim::math::narrow(*y) })
            }
            _ => Some(PopParams::Values(vals)),
        }
    }
}

/// A string value interned.
fn name_of(v: &Value, l: &mut Lx) -> Option<mvs_sim::names::Name> {
    let s = text(v, l)?;
    Some(l.name(s))
}

/// [`graph_pop_text`]'s inverse.
fn graph_pop_text_load(o: &Obj, l: &mut Lx) -> Option<mvs_sim::combat::pop_text::GraphPopText> {
    use crate::conv::{int, num};
    let mut g = mvs_sim::combat::pop_text::GraphPopText::default();
    let t = l.at("Text", |l| as_obj(o.get("Text")?, l))?;
    check_keys(t, &["Namespace", "Key", "SourceString", "LocalizedString"], l);
    g.text_key = name_of(t.get("Key")?, l)?;
    if let Some(v) = t.get("SourceString") {
        g.source_string = Some(name_of(v, l)?);
    }
    if let Some(v) = t.get("LocalizedString") {
        g.localized_string = Some(name_of(v, l)?);
    }
    let n = |k: &str, l: &mut Lx| num(o.get(k)?, l);
    g.lifetime = mvs_sim::math::narrow(l.at("Lifetime", |l| n("Lifetime", l))?);
    g.scale = mvs_sim::math::narrow(l.at("Scale", |l| n("Scale", l))?);
    g.delay = mvs_sim::math::narrow(l.at("Delay", |l| n("Delay", l))?);
    for (key, to) in [("Color", &mut g.color[..]), ("Location", &mut g.location[..])] {
        let c = l.at(key, |l| as_obj(o.get(key)?, l))?;
        let keys: &[&str] = if key == "Color" { &["R", "G", "B", "A"] } else { &["X", "Y", "Z"] };
        check_keys(c, keys, l);
        for (slot, k) in to.iter_mut().zip(keys) {
            *slot = mvs_sim::math::narrow(l.at(key, |l| num(c.get(k)?, l))?);
        }
    }
    let space = l.at("Space", |l| n("Space", l).and_then(|v| int(v, l)))?;
    g.space = u8::try_from(space).ok()?;
    let just = l.at("Justification", |l| n("Justification", l).and_then(|v| int(v, l)))?;
    g.justification = u8::try_from(just).ok()?;
    if let Some(v) = o.get("Audio") {
        g.audio = Some(name_of(v, l)?);
    }
    g.queued = matches!(o.get("Queued"), Some(Value::Bool(true)));
    g.local_only = matches!(o.get("LocalOnly"), Some(Value::Bool(true)));
    if let Some(a) = o.get("Args") {
        for (k, v) in l.at("Args", |l| as_obj(a, l))?.entries() {
            g.args.push((l.name(k), mvs_sim::math::narrow(num(v, l)?)));
        }
    }
    let known = [
        "Text",
        "Lifetime",
        "Color",
        "Location",
        "Space",
        "Scale",
        "Justification",
        "Delay",
        "Audio",
        "Queued",
        "LocalOnly",
        "Args",
    ];
    check_keys(o, &known, l);
    Some(g)
}

/// A graph's `GraphPopText` by its oracle keys, the optional ones left out where unset.
fn graph_pop_text(g: &mvs_sim::combat::pop_text::GraphPopText, x: &Wx) -> Value {
    let mut t = vec![("Key", Value::Str(x.str(g.text_key).to_owned()))];
    // Deviation: a text taken from a class's data carries its Namespace (always "" in the build's data) with its
    // SourceString; GraphPopText holds no Namespace field.
    if g.source_string.is_some() {
        t.insert(0, ("Namespace", Value::Str(String::new())));
    }
    if let Some(n) = g.source_string {
        t.push(("SourceString", Value::Str(x.str(n).to_owned())));
    }
    if let Some(n) = g.localized_string {
        t.push(("LocalizedString", Value::Str(x.str(n).to_owned())));
    }
    let [r, gr, b, a] = g.color;
    let [lx, ly, lz] = g.location;
    let mut o = vec![
        ("Text", obj(t)),
        ("Lifetime", Value::Num(f64::from(g.lifetime))),
        (
            "Color",
            obj(vec![
                ("R", Value::Num(f64::from(r))),
                ("G", Value::Num(f64::from(gr))),
                ("B", Value::Num(f64::from(b))),
                ("A", Value::Num(f64::from(a))),
            ]),
        ),
        (
            "Location",
            obj(vec![
                ("X", Value::Num(f64::from(lx))),
                ("Y", Value::Num(f64::from(ly))),
                ("Z", Value::Num(f64::from(lz))),
            ]),
        ),
        ("Space", Value::Num(f64::from(g.space))),
        ("Scale", Value::Num(f64::from(g.scale))),
        ("Justification", Value::Num(f64::from(g.justification))),
        ("Delay", Value::Num(f64::from(g.delay))),
    ];
    if let Some(n) = g.audio {
        o.push(("Audio", Value::Str(x.str(n).to_owned())));
    }
    if g.queued {
        o.push(("Queued", Value::Bool(true)));
    }
    if g.local_only {
        o.push(("LocalOnly", Value::Bool(true)));
    }
    if !g.args.is_empty() {
        let args = g.args.iter().map(|(k, n)| (x.str(*k).to_owned(), Value::Num(f64::from(*n)))).collect();
        o.push(("Args", Value::Obj(Obj::new(args))));
    }
    obj(o)
}

/// One event's keys: written from the variant's fields, or the variant made from them.
macro_rules! key_put {
    ($o:ident, $x:ident, $k:literal, =, $f:expr) => {
        $o.push(($k.to_owned(), Plain::to_value($f, $x)))
    };
    ($o:ident, $x:ident, $k:literal, ~, $f:expr) => {
        if !Unset::unset($f) {
            $o.push(($k.to_owned(), Plain::to_value($f, $x)))
        }
    };
}

macro_rules! key_make {
    ($ob:ident, $l:ident, $k:literal, =) => {
        match $ob.get($k) {
            Some(v) => $l.at($k, |l| Plain::from_value(v, l))?,
            None => {
                $l.miss(concat!("key ", $k, " absent"));
                return None;
            }
        }
    };
    ($ob:ident, $l:ident, $k:literal, ~) => {
        match $ob.get($k) {
            Some(v) => $l.at($k, |l| Plain::from_value(v, l))?,
            None => Default::default(),
        }
    };
}

/// The events whose keys are their fields: `"type" Variant { "key" mode field, .. }`.
macro_rules! events {
    ($($ts:literal $v:ident { $($k:literal $m:tt $f:ident),* $(,)? })*) => {
        fn fields_put(e: &Event, x: &Wx) -> Option<Vec<(String, Value)>> {
            #[allow(unused_mut)]
            match e {
                $(Event::$v { $($f),* } => {
                    let mut o: Vec<(String, Value)> = Vec::new();
                    $(key_put!(o, x, $k, $m, $f);)*
                    Some(o)
                })*
                _ => None,
            }
        }

        fn fields_make(kind: &str, ob: &Obj, l: &mut Lx) -> Option<Option<Event>> {
            match kind {
                $($ts => Some((|| {
                    check_keys(ob, &[$($k),*], l);
                    Some(Event::$v { $($f: key_make!(ob, l, $k, $m)),* })
                })()),)*
                _ => None,
            }
        }
    };
}

events! {
    "comboCounts" ComboCounts {
        "fighter" = fighter, "juggler" = juggler, "juggleCount" = juggle_count, "juggleDamage" = juggle_damage,
        "hitstunCount" = hitstun_count, "hitstunDamage" = hitstun_damage,
    }
    "sound" Sound { "fighter" = fighter, "ev" = ev, "x" ~ x, "y" ~ y, "hit" ~ hit, "pooled" ~ pooled, "key" ~ key }
    "spawnedSound" SpawnedSound {
        "instigator" = instigator, "ev" = ev, "x" = x, "y" = y, "rtpc" = rtpc, "value" = value,
    }
    "soundStart" SoundStart { "fighter" = fighter, "ev" = ev, "key" = key }
    "soundStop" SoundStop {
        "fighter" = fighter, "key" = key, "fade" ~ fade, "ev" ~ ev, "curve" ~ curve, "cut" ~ cut,
    }
    "rtpc" Rtpc { "fighter" = fighter, "name" = name, "value" = value, "interpMs" = interp_ms }
    "switch" Switch { "fighter" = fighter, "group" = group, "value" = value }
    "combatInput" CombatInput { "fighter" = fighter }
    "hit" Hit {
        "attacker" = attacker, "victim" = victim, "damage" = damage, "painDamage" = pain_damage, "speed" = speed,
        "hitpause" = hitpause, "lockout" = lockout, "x" = x, "y" = y, "light" ~ light,
    }
    "markerBurst" MarkerBurst { "fighter" = fighter }
    "itemFx" ItemFx {
        "cls" = cls, "item" = item, "fighter" = fighter, "vfx" = vfx, "x" = x, "y" = y, "scale" = scale,
    }
    "mortarBoom" MortarBoom { "fighter" = fighter, "x" = x, "y" = y, "ax" = ax, "ay" = ay, "facing" = facing }
    "acceleratorBuff" AcceleratorBuff { "fighter" = fighter, "x" = x, "y" = y }
    "acceleratorEnd" AcceleratorEnd { "fighter" = fighter, "x" = x, "y" = y }
    "projectileDeath" ProjectileDeath {
        "cls" = cls, "fighter" = fighter, "vfx" = vfx, "x" = x, "y" = y, "ox" = ox, "rot" = rot, "scale" = scale,
        "tint" ~ tint, "to" ~ to,
    }
    "shockwaveVfx" ShockwaveVfx {
        "fighter" = fighter, "cls" = cls, "n" = n, "x" = x, "y" = y, "facing" = facing, "hitPause" = hit_pause,
    }
    "ko" Ko { "fighter" = fighter }
    "armorHit" ArmorHit { "fighter" = fighter, "x" = x, "y" = y }
    "armorBreak" ArmorBreak { "fighter" = fighter }
    "respawn" Respawn { "fighter" = fighter }
    "slotBound" SlotBound { "fighter" = fighter, "slot" = slot }
    "slotActive" SlotActive { "slot" = slot, "on" = on }
    "priorityAdd" PriorityAdd { "fighter" = fighter }
    "cameraShake" CameraShake { "cls" = cls }
    "rumble" Rumble {
        "fighter" = fighter, "name" = name, "looping" = looping, "tag" = tag,
        "ignoreTimeDilation" = ignore_time_dilation, "playWhilePaused" = play_while_paused,
    }
    "rumbleStop" RumbleStop { "fighter" = fighter, "name" = name, "tag" = tag }
    "bounceVfx" BounceVfx { "x" = x, "y" = y, "roll" = roll }
    "blastBoxSound" BlastBoxSound { "fighter" = fighter, "p" = p }
    "koVfx" KoVfx {
        "fighter" = fighter, "asset" = asset, "hold" = hold, "p" = p, "tl" = tl, "tr" = tr, "bl" = bl,
        "center" = center,
    }
    "smokeTrail" SmokeTrail { "fighter" = fighter, "on" = on }
    "diVisual" DiVisual { "fighter" = fighter, "attacker" = attacker, "vx" = vx, "vy" = vy, "x" = x, "y" = y }
    "rageVfx" RageVfx { "fighter" = fighter, "on" = on }
    "spawnFx" SpawnFx { "fighter" = fighter, "on" = on }
    "rageGlow" RageGlow { "fighter" = fighter, "on" = on, "shirt" = shirt }
    "swordGlow" SwordGlow { "fighter" = fighter, "on" = on }
    "backstab" Backstab { "fighter" = fighter, "victim" = victim }
    "aryaKnife" AryaKnife {
        "fighter" = fighter, "knife" = knife, "kind" = kind, "victim" = victim, "x" = x, "y" = y,
    }
    "matScalar" MatScalar { "fighter" = fighter, "name" = name, "value" = value }
    "evadeFx" EvadeFx { "fighter" = fighter }
    "reflectFx" ReflectFx { "actor" = actor, "item" = item, "owner" = owner }
    "popText" PopText { "fighter" = fighter, "kind" = kind, "stacks" = stacks, "gain" ~ gain, "params" ~ params }
    "itemUiCreate" ItemUiCreate {
        "item" = item, "fighter" = fighter, "action" = action, "onGround" = on_ground, "actor" ~ actor,
    }
    "itemUi" ItemUi { "item" = item, "fighter" = fighter, "hide" = hide, "onGround" = on_ground, "actor" ~ actor }
    "itemUiExit" ItemUiExit { "item" = item, "fighter" ~ fighter, "actor" ~ actor }
    "rageResidual" RageResidual { "fighter" = fighter, "scale" = scale }
    "sizeScale" SizeScale { "fighter" = fighter, "scale" = scale }
    "enlargeFx" EnlargeFx { "fighter" = fighter, "begin" = begin }
    "growFx" GrowFx { "fighter" = fighter }
    "polymorphFx" PolymorphFx { "fighter" = fighter, "begin" = begin, "vfx" = vfx }
    "electricFx" ElectricFx {
        "fighter" = fighter, "kind" = kind, "on" ~ on, "target" ~ target, "x" ~ x, "y" ~ y,
    }
    "gauntletHit" GauntletHit { "fighter" = fighter, "cls" = cls, "victim" = victim }
    "tazHealFx" TazHealFx { "fighter" = fighter, "kind" = kind }
    "tjPoof" TjPoof { "fighter" = fighter, "x" = x, "y" = y, "scale" = scale }
    "teamScore" TeamScore { "team" = team, "score" = score, "isLeader" = is_leader, "tied" = tied }
    "stocks" Stocks { "fighter" = fighter, "stocks" = stocks }
    "matchPoint" MatchPoint { "team" = team }
    "matchPointShow" MatchPointShow { "team" = team }
    "mutatorPopText" MutatorPopText { "cls" = cls, "field" = field, "text" ~ text }
    "wheelNext" WheelNext { "index" = index }
    "wheelChange" WheelChange { "index" = index }
    "matchOver" MatchOver { "team" = team }
    "abduct" Abduct { "fighter" = fighter }
    "cameraMovement" CameraMovement { "on" = on }
    "introFx" IntroFx { "fighter" = fighter }
    "introReveal" IntroReveal { "fighter" = fighter }
    "matchEnd" MatchEnd { "phase" = phase, "team" = team }
    "uiState" UiState { "fighter" = fighter, "op" = op }
    "eatUi" EatUi { "fighter" = fighter, "on" = on, "onGround" = on_ground }
    "cloudLabel" CloudLabel { "fighter" = fighter, "owner" = owner, "on" = on, "hide" ~ hide }
    "struggleUi" StruggleUi { "fighter" = fighter, "timeLeft" = time_left, "total" = total, "hideInput" = hide_input }
    "abilityAmmo" AbilityAmmo { "fighter" = fighter, "slot" = slot, "ammo" = ammo, "increase" = increase }
    "moveFx" MoveFx { "fighter" = fighter, "kind" = kind, "x" = x, "y" = y }
    "notifyVfx" NotifyVfx {
        "fighter" = fighter, "n" = n, "on" = on, "left" = left, "copy" ~ copy, "poly" ~ poly,
    }
    "emoteVfx" EmoteVfx { "fighter" = fighter, "n" = n, "on" = on, "ps" = ps, "scale" = scale }
    "actorVfx" ActorVfx { "fighter" = fighter, "actor" = actor, "n" = n, "on" = on, "x" = x, "y" = y }
    "particleNotify" ParticleNotify {
        "fighter" = fighter, "n" = n, "on" = on, "left" = left, "copy" ~ copy, "poly" ~ poly,
    }
    "subMeshAnim" SubMeshAnim { "fighter" = fighter, "tag" = tag, "anim" = anim, "loop" = looping }
    "pulseHit" PulseHit { "fighter" = fighter, "painDamage" = pain_damage }
    "abilityUiCreate" AbilityUiCreate { "fighter" = fighter, "slot" = slot, "icon" = icon, "maxCount" = max_count }
    "disguiseFx" DisguiseFx { "fighter" = fighter, "kind" = kind }
    "hitVfx" HitVfx {
        "victim" = victim, "x" = x, "y" = y, "zFloor" = z_floor, "vx" = vx, "vy" = vy, "dist" = dist,
        "decayStacks" = decay_stacks, "override" = override_vfx, "forceSmall" = force_small,
    }
}

/// `uiAbility`: the op's keys spread into the event (`{type, fighter, n} & UiAbilityOp`, `kind` its tag).
fn ui_ability_put(fighter: i32, n: i32, op: &UiAbilityOp) -> Vec<(String, Value)> {
    let mut o =
        vec![("fighter".to_owned(), Value::Num(f64::from(fighter))), ("n".to_owned(), Value::Num(f64::from(n)))];
    let kind = match op {
        UiAbilityOp::Exit => "exit",
        UiAbilityOp::Error => "error",
        UiAbilityOp::Update { ammo, cd, cd_max, ammo_cd, ammo_max, pulse, pulse_ammo } => {
            for (k, v) in [
                ("ammo", Value::Num(f64::from(*ammo))),
                ("cd", Value::Num(f64::from(*cd))),
                ("cdMax", Value::Num(f64::from(*cd_max))),
                ("ammoCd", Value::Num(f64::from(*ammo_cd))),
                ("ammoMax", Value::Num(f64::from(*ammo_max))),
                ("pulse", Value::Bool(*pulse)),
                ("pulseAmmo", Value::Bool(*pulse_ammo)),
            ] {
                o.push((k.to_owned(), v));
            }
            "update"
        }
    };
    o.push(("kind".to_owned(), Value::Str(kind.to_owned())));
    o
}

/// The keys the oracle builds with two numbers, `{fighter}:vo{n}`, `{fighter}:charge{n}`
/// and `{fighter}:{n}`: the Rust key holds the tag and the second
/// number, the fighter is the event's.
const FIGHTER_TAGS: &[&str] = &[":vo", ":charge", ":"];

fn fighter_key_put(e: &Event, o: &mut [(String, Value)], x: &Wx) {
    let (fighter, key) = match e {
        Event::Sound { fighter, key: Some(key), .. } => (*fighter, *key),
        Event::SoundStart { fighter, key, .. } | Event::SoundStop { fighter, key, .. } => (*fighter, *key),
        _ => return,
    };
    let tag = x.str(key.tag);
    if !FIGHTER_TAGS.contains(&tag) {
        return;
    }
    if let Some((_, v)) = o.iter_mut().find(|(k, _)| k == "key") {
        *v = Value::Str(format!("{fighter}{tag}{}", key.n));
    }
}

/// A key of `FIGHTER_TAGS` as it writes it: the tag and `n`.
pub fn fighter_key_parse(s: &str) -> Option<(i32, &'static str)> {
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    let i = s.find(':')?;
    let (id, rest) = (&s[..i], &s[i + 1..]);
    if !digits(id) {
        return None;
    }
    FIGHTER_TAGS.iter().find_map(|tag| {
        let n = rest.strip_prefix(&tag[1..]).filter(|n| digits(n))?;
        n.parse().ok().map(|n| (n, *tag))
    })
}

/// An event as the record writes it.
pub fn put(e: &Event, x: &Wx) -> EventRecord {
    let fields = match e {
        Event::DoublePoints | Event::TimeUp => Vec::new(),
        Event::UiAbility { fighter, n, op } => ui_ability_put(*fighter, *n, op),
        Event::TutorialUi { call } => vec![("call".to_owned(), tutorial_ui_put(call, x))],
        e => {
            let mut o = fields_put(e, x).unwrap_or_else(|| {
                // (a variant added since the table was written)
                x.unmapped(&format!("a {} event (no rule to write it)", e.type_name()));
                Vec::new()
            });
            fighter_key_put(e, &mut o, x);
            o
        }
    };
    EventRecord { kind: e.type_name().to_owned(), fields: Obj::new(fields) }
}

/// A tutorialUi event's call as the oracle writes it: a plain object, `op` and the variant's fields.
fn tutorial_ui_put(c: &mvs_sim::events::TutorialUiCall, x: &Wx) -> Value {
    use mvs_sim::events::TutorialUiCall as C;
    let n = |v: i32| Value::Num(f64::from(v));
    let (op, mut o): (&str, Vec<(&str, Value)>) = match c {
        C::Intro => ("intro", vec![]),
        C::Widget => ("widget", vec![]),
        C::Setup { step, index, count } => {
            ("setup", vec![("step", n(*step)), ("index", n(*index)), ("count", n(*count))])
        }
        C::TaskCount { task, count, index, total } => {
            ("taskCount", vec![("task", n(*task)), ("count", n(*count)), ("index", n(*index)), ("total", n(*total))])
        }
        C::Jiggle { task } => ("jiggle", vec![("task", n(*task))]),
        C::FadeOut { task } => ("fadeOut", vec![("task", n(*task))]),
        C::FadeIn => ("fadeIn", vec![]),
        C::Concept { step, start_disabled } => {
            ("concept", vec![("step", n(*step)), ("startDisabled", start_disabled.to_value(x))])
        }
        C::Focus { on } => ("focus", vec![("on", Value::Bool(*on))]),
        C::Cleanup => ("cleanup", vec![]),
        C::RemoveWidget => ("removeWidget", vec![]),
        C::Congrats { text } => ("congrats", vec![("text", n(*text))]),
        C::Finished { actor } => ("finished", vec![("actor", actor.to_value(x))]),
        C::EndScreen { has_next } => ("endScreen", vec![("hasNext", Value::Bool(*has_next))]),
        C::Scoreboard { visible } => ("scoreboard", vec![("visible", Value::Bool(*visible))]),
        C::MinigameCounter { task, text, visible } => (
            "minigameCounter",
            vec![("task", n(*task)), ("text", text.to_value(x)), ("visible", Value::Bool(*visible))],
        ),
        C::MinigameCount { task, count } => ("minigameCount", vec![("task", n(*task)), ("count", n(*count))]),
        C::RewardPop { fighter } => ("rewardPop", vec![("fighter", n(*fighter))]),
    };
    o.push(("op", Value::Str(op.to_owned())));
    obj(o)
}

/// The inverse of [`tutorial_ui_put`].
fn tutorial_ui_make(ob: &Obj, l: &mut Lx) -> Option<Event> {
    use mvs_sim::events::TutorialUiCall as C;
    check_keys(ob, &["call"], l);
    let Some(c) = ob.get("call") else {
        l.miss("key call absent");
        return None;
    };
    let c = as_obj(c, l)?;
    let get = |k: &str, l: &mut Lx| {
        let v = c.get(k);
        if v.is_none() {
            l.miss(format_args!("key call.{k} absent"));
        }
        v
    };
    let i = |k: &str, l: &mut Lx| get(k, l).and_then(|v| crate::conv::num(v, l)).and_then(|v| crate::conv::int(v, l));
    let b = |k: &str, l: &mut Lx| match get(k, l) {
        Some(Value::Bool(v)) => Some(*v),
        Some(v) => {
            l.miss(format_args!("key call.{k}: {v:?} is not a boolean"));
            None
        }
        None => None,
    };
    let name = |k: &str, l: &mut Lx| get(k, l).and_then(|v| mvs_sim::names::Name::from_value(v, l));
    let op = text(get("op", l)?, l)?.to_owned();
    let (call, keys): (C, &[&str]) = match op.as_str() {
        "intro" => (C::Intro, &[]),
        "widget" => (C::Widget, &[]),
        "setup" => (
            C::Setup { step: i("step", l)?, index: i("index", l)?, count: i("count", l)? },
            &["step", "index", "count"],
        ),
        "taskCount" => (
            C::TaskCount { task: i("task", l)?, count: i("count", l)?, index: i("index", l)?, total: i("total", l)? },
            &["task", "count", "index", "total"],
        ),
        "jiggle" => (C::Jiggle { task: i("task", l)? }, &["task"]),
        "fadeOut" => (C::FadeOut { task: i("task", l)? }, &["task"]),
        "fadeIn" => (C::FadeIn, &[]),
        "concept" => (
            C::Concept {
                step: i("step", l)?,
                start_disabled: get("startDisabled", l).and_then(|v| mvs_sim::Real::from_value(v, l))?,
            },
            &["step", "startDisabled"],
        ),
        "focus" => (C::Focus { on: b("on", l)? }, &["on"]),
        "cleanup" => (C::Cleanup, &[]),
        "removeWidget" => (C::RemoveWidget, &[]),
        "congrats" => (C::Congrats { text: i("text", l)? }, &["text"]),
        "finished" => (C::Finished { actor: name("actor", l)? }, &["actor"]),
        "endScreen" => (C::EndScreen { has_next: b("hasNext", l)? }, &["hasNext"]),
        "scoreboard" => (C::Scoreboard { visible: b("visible", l)? }, &["visible"]),
        "minigameCounter" => (
            C::MinigameCounter { task: i("task", l)?, text: name("text", l)?, visible: b("visible", l)? },
            &["task", "text", "visible"],
        ),
        "minigameCount" => (C::MinigameCount { task: i("task", l)?, count: i("count", l)? }, &["task", "count"]),
        "rewardPop" => (C::RewardPop { fighter: i("fighter", l)? }, &["fighter"]),
        k => {
            l.miss(format_args!("{k:?} is not a tutorialUi op"));
            return None;
        }
    };
    for (k, _) in c.entries() {
        if k != "op" && !keys.contains(&k.as_str()) {
            l.miss(format_args!("key call.{k} has no Rust field"));
        }
    }
    Some(Event::TutorialUi { call })
}

/// The inverse of [`ui_ability_put`].
fn ui_ability_make(ob: &Obj, l: &mut Lx) -> Option<Event> {
    let n = |k: &str, l: &mut Lx| match ob.get(k) {
        Some(v) => crate::conv::num(v, l),
        None => {
            l.miss(format_args!("key {k} absent"));
            None
        }
    };
    let fighter = crate::conv::int(n("fighter", l)?, l)?;
    let slot = crate::conv::int(n("n", l)?, l)?;
    let kind = text(ob.get("kind").unwrap_or(&Value::Null), l)?.to_owned();
    let (op, keys): (UiAbilityOp, &[&str]) = match kind.as_str() {
        "exit" => (UiAbilityOp::Exit, &["fighter", "n", "kind"]),
        "error" => (UiAbilityOp::Error, &["fighter", "n", "kind"]),
        "update" => {
            let b = |k: &str, l: &mut Lx| match ob.get(k) {
                Some(Value::Bool(b)) => Some(*b),
                v => {
                    l.miss(format_args!("key {k}: {v:?} is not a boolean"));
                    None
                }
            };
            let op = UiAbilityOp::Update {
                ammo: mvs_sim::math::narrow(n("ammo", l)?),
                cd: mvs_sim::math::narrow(n("cd", l)?),
                cd_max: mvs_sim::math::narrow(n("cdMax", l)?),
                ammo_cd: mvs_sim::math::narrow(n("ammoCd", l)?),
                ammo_max: mvs_sim::math::narrow(n("ammoMax", l)?),
                pulse: b("pulse", l)?,
                pulse_ammo: b("pulseAmmo", l)?,
            };
            (op, &["fighter", "n", "kind", "ammo", "cd", "cdMax", "ammoCd", "ammoMax", "pulse", "pulseAmmo"])
        }
        k => {
            l.miss(format_args!("{k:?} is not a uiAbility kind"));
            return None;
        }
    };
    check_keys(ob, keys, l);
    Some(Event::UiAbility { fighter, n: slot, op })
}

/// The event a record holds, or `None` (named) where it has no Rust form.
pub fn make(r: &EventRecord, l: &mut Lx) -> Option<Event> {
    let ob = &r.fields;
    match r.kind.as_str() {
        "doublePoints" | "timeUp" => {
            check_keys(ob, &[], l);
            Some(if r.kind == "timeUp" { Event::TimeUp } else { Event::DoublePoints })
        }
        "uiAbility" => ui_ability_make(ob, l),
        "tutorialUi" => tutorial_ui_make(ob, l),
        k => match fields_make(k, ob, l) {
            Some(e) => e,
            None => {
                l.miss(format_args!("an event {k} (no rule to load it)"));
                None
            }
        },
    }
}

#[cfg(test)]
mod fighter_key_tests {
    use super::fighter_key_parse as vo_key_parse;

    #[test]
    fn a_vo_key_is_its_fighter_then_its_line() {
        assert_eq!(vo_key_parse("0:vo3"), Some((3, ":vo")));
        assert_eq!(vo_key_parse("12:vo40"), Some((40, ":vo")));
        assert_eq!(vo_key_parse("1:wind"), None);
        assert_eq!(vo_key_parse(":vo3"), None);
        assert_eq!(vo_key_parse("0:vo"), None);
        assert_eq!(vo_key_parse("1:6"), Some((6, ":")));
        assert_eq!(vo_key_parse("1:charge1"), Some((1, ":charge")));
    }
}
