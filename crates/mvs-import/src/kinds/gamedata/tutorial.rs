//! The tutorials' data: every TutorialActor class's class default object, its TutorialSteps and the actor's own
//! fields. The classes: the packages of the Tutorial
//! folder named TutorialActor* whose class default holds steps (TutorialActor and TutorialActor_Advanced, the
//! parents, hold none).
//!
//! Texts are kept as the asset's namespace, key and source string, not as the player's words: the game reads the
//! words from its `Game.locres` at run time (an earlier importer bound the strings of the locres at the import). A text
//! the asset has no key for (a culture-invariant one, a string table entry's) has an empty namespace and key, and
//! its source if it has one: what `localized` draws for it. A step's header is the one exception: its `source` is
//! ConceptHeaderTextEnglish on every step (the member Check For Attack Decay compares), not the asset's own source,
//! so a header with an empty key is drawn as nothing (an empty string: Intro steps 7, 13 and 15) and a keyed
//! header whose locres has no line falls back to that English.

use indexmap::IndexMap;
use mvs_data::tutorial::{TutorialActorData, TutorialInput, TutorialStep, TutorialTask, TutorialText};
use mvs_data::{Props, Tutorials, Value};

use super::shape::{EnumNames, defaults_or_properties, enum_names, fail, short_name};
use crate::fighter_port::CHARACTER_DATA;
use crate::kind::{Cx, KindError};

const FOLDER: &str = "/Game/Panda_Main/Blueprints/Tutorial";
const EVENT_TYPES: &str = "/Game/Panda_Main/Blueprints/Tutorial/TutorialEventTypes";
const INPUT_WIDGET_TYPES: &str = "/Game/Panda_Main/Blueprints/Tutorial/TutorialInputWidgetTypes";
const PGI: &str = "/Game/Panda_Main/Blueprints/PandaGameInstance/PGI_GlobalData.Default__PGI_GlobalData_C";

/// A class name without its `_C`.
fn no_c(c: &str) -> &str {
    c.strip_suffix("_C").unwrap_or(c)
}

/// One actor's reads, each failing with the actor and the place.
struct Reader<'a> {
    actor: &'a str,
    events: &'a EnumNames,
    widgets: &'a EnumNames,
    /// A pawn class's path (lower case) to its data key.
    key_of_pawn: &'a IndexMap<String, &'static str>,
}

impl Reader<'_> {
    fn err<T>(&self, m: &str) -> Result<T, KindError> {
        fail(format!("tutorial data: {} {m}", self.actor))
    }

    fn str<'v>(&self, v: Option<&'v Value>, m: &str) -> Result<&'v str, KindError> {
        match v {
            Some(Value::Str(s)) => Ok(s),
            _ => self.err(&format!("{m} is not a string")),
        }
    }

    fn num(&self, v: Option<&Value>, m: &str) -> Result<f64, KindError> {
        match v {
            Some(Value::Num(n)) => Ok(*n),
            _ => self.err(&format!("{m} is not a number")),
        }
    }

    fn bool(&self, v: Option<&Value>, m: &str) -> Result<bool, KindError> {
        match v {
            Some(Value::Bool(b)) => Ok(*b),
            _ => self.err(&format!("{m} is not a bool")),
        }
    }

    /// A class reference as the build's class name without the `_C`, `None` for none.
    fn cls(&self, v: Option<&Value>, m: &str) -> Result<Option<String>, KindError> {
        match v {
            None | Some(Value::Null) => Ok(None),
            v => Ok(Some(no_c(short_name(self.str(v, m)?)).to_owned())),
        }
    }

    /// An FName, `None` for None.
    fn name(&self, v: Option<&Value>, m: &str) -> Result<Option<String>, KindError> {
        let s = self.str(v, m)?;
        Ok((s != "None").then(|| s.to_owned()))
    }

    /// The byte of an enum asset's value.
    fn byte(&self, table: &EnumNames, v: Option<&Value>, m: &str) -> Result<u8, KindError> {
        let s = self.str(v, m)?;
        match table.get(s) {
            Some(&b) => u8::try_from(b as i64).or_else(|_| self.err(&format!("{m} {s} is no byte"))),
            None => self.err(&format!("{m} {s} is no value of its enum asset")),
        }
    }

    /// A native enum's value without its prefix, `""` for None.
    fn native(&self, v: Option<&Value>, m: &str) -> Result<String, KindError> {
        let s = self.str(v, m)?;
        // A value with no `::` is kept whole (no value of the build lacks the prefix).
        let n = s.split_once("::").map_or(s, |(_, n)| n);
        Ok(if n == "None" { String::new() } else { n.to_owned() })
    }

    /// A text as its locres key and source (`localized`'s inputs); a text with no key keeps its source alone.
    fn text(&self, v: Option<&Value>) -> TutorialText {
        let Some(Value::Obj(t)) = v else { return TutorialText::default() };
        let s = |k: &str| t.get(k).and_then(Value::as_str);
        // (localized looks a text up only when it has both)
        let (namespace, key) = s("namespace").zip(s("key")).unwrap_or_default();
        TutorialText {
            namespace: namespace.to_owned(),
            key: key.to_owned(),
            source: s("source").unwrap_or_default().to_owned(),
        }
    }

    /// A pawn class's data key (its CharacterData as the port names it), `None` for a class the port has none for.
    fn char_key(&self, pawn: Option<&Value>) -> Option<String> {
        let Some(Value::Str(p)) = pawn else { return None };
        self.key_of_pawn.get(&p.to_lowercase()).map(|k| (*k).to_owned())
    }

    fn list<'v>(&self, v: Option<&'v Value>, m: &str) -> Result<&'v [Value], KindError> {
        match v {
            None | Some(Value::Null) => Ok(&[]),
            Some(Value::Arr(a)) => Ok(a),
            _ => self.err(&format!("{m} is not an array")),
        }
    }

    fn obj<'v>(&self, v: &'v Value, m: &str) -> Result<&'v Props, KindError> {
        v.as_obj().map_or_else(|| self.err(&format!("{m} is not a struct")), Ok)
    }

    fn input(&self, x: &Value, at: &str) -> Result<TutorialInput, KindError> {
        let x = self.obj(x, at)?;
        let w = self.byte(self.widgets, x.get("InputWidgetClass"), &format!("{at} InputWidgetClass"))?;
        if w > 1 {
            return self.err(&format!("{at} InputWidgetClass byte {w}"));
        }
        let none = Props::new();
        let params = match x.get("UIInputLabelParams") {
            None | Some(Value::Null) => &none,
            Some(p) => self.obj(p, &format!("{at} UIInputLabelParams"))?,
        };
        let texture = match x.get("InputButtonWidgetTexture") {
            None | Some(Value::Null) => String::new(),
            Some(t) if t.as_str() == Some("") => String::new(),
            t => short_name(self.str(t, &format!("{at} InputButtonWidgetTexture"))?).to_owned(),
        };
        Ok(TutorialInput {
            widget: w,
            texture,
            action: self.native(params.get("Action"), &format!("{at} Action"))?,
            modifier: self.native(params.get("Modifier"), &format!("{at} Modifier"))?,
            label: self.text(params.get("Label")),
        })
    }

    fn task(&self, t: &Value, at: &str) -> Result<TutorialTask, KindError> {
        let t = self.obj(t, at)?;
        let inputs = self
            .list(t.get("TaskControllerInputs"), &format!("{at} TaskControllerInputs"))?
            .iter()
            .enumerate()
            .map(|(xi, x)| self.input(x, &format!("{at} input {xi}")))
            .collect::<Result<_, _>>()?;
        let mut custom_actors = Vec::new();
        for (ci, c) in
            self.list(t.get("CustomTutorialActors"), &format!("{at} CustomTutorialActors"))?.iter().enumerate()
        {
            custom_actors.extend(self.cls(Some(c), &format!("{at} CustomTutorialActors {ci}"))?);
        }
        let req = self.num(t.get("TaskRepetitionReq"), &format!("{at} TaskRepetitionReq"))?;
        Ok(TutorialTask {
            text: self.text(t.get("TaskText")),
            event: self.byte(self.events, t.get("TaskCompletionEvent"), &format!("{at} TaskCompletionEvent"))?,
            req: i32::try_from(req as i64).or_else(|_| self.err(&format!("{at} TaskRepetitionReq {req}")))?,
            counter: self.bool(t.get("HasTaskCounter"), &format!("{at} HasTaskCounter"))?,
            custom_actors,
            minigame_counter: self.bool(t.get("HasMinigameCounter"), &format!("{at} HasMinigameCounter"))?,
            save_score: self.bool(t.get("SaveMinigameScore"), &format!("{at} SaveMinigameScore"))?,
            minigame_text: self.text(t.get("MinigameCounterText")),
            inputs,
        })
    }

    fn step(&self, s: &Value, si: usize) -> Result<TutorialStep, KindError> {
        let at = format!("step {si}");
        let s = self.obj(s, &at)?;
        let get = |k: &str| s.get(k);
        let tasks = self
            .list(get("TutorialTasks"), &format!("{at} TutorialTasks"))?
            .iter()
            .enumerate()
            .map(|(ti, t)| self.task(t, &format!("{at} task {ti}")))
            .collect::<Result<_, _>>()?;
        // An empty PlayerClass path passes `cls` here (no step of the build has one).
        let Some(player) = self.cls(get("PlayerClass"), &format!("{at} PlayerClass"))? else {
            return self.err(&format!("{at} has no PlayerClass"));
        };
        // (the member Check For Attack Decay compares: a culture-invariant header has its English here alone)
        let mut header = self.text(get("ConceptHeaderText"));
        header.source =
            self.str(get("ConceptHeaderTextEnglish"), &format!("{at} ConceptHeaderTextEnglish"))?.to_owned();
        Ok(TutorialStep {
            new_concept: self.bool(get("StartsNewConcept"), &format!("{at} StartsNewConcept"))?,
            header,
            text: self.text(get("ConceptText")),
            start_button_disabled_time: self
                .num(get("StartButtonDisabledTime"), &format!("{at} StartButtonDisabledTime"))?
                as f32,
            player,
            character: self.char_key(get("PlayerClass")),
            player_tag: self.str(get("PlayerSpawnTag"), &format!("{at} PlayerSpawnTag"))?.to_owned(),
            dummy: self.cls(get("DummyClass"), &format!("{at} DummyClass"))?,
            dummy_character: self.char_key(get("DummyClass")),
            spawn_target_dummy: self.bool(get("SpawnTargetDummy"), &format!("{at} SpawnTargetDummy"))?,
            dummy_controller: self.cls(get("TargetDummyController"), &format!("{at} TargetDummyController"))?,
            bag_percent: self.num(get("PunchingBagPercent"), &format!("{at} PunchingBagPercent"))? as f32,
            bag_tag: self.str(get("PunchBagSpawnTag"), &format!("{at} PunchBagSpawnTag"))?.to_owned(),
            disable_cooldowns: self.bool(get("DisablePlayerCooldowns"), &format!("{at} DisablePlayerCooldowns"))?,
            demo: self.cls(get("DemoAIController"), &format!("{at} DemoAIController"))?,
            vo: self.name(get("VOSoundAlias"), &format!("{at} VOSoundAlias"))?,
            tasks,
        })
    }
}

/// Every TutorialActor class's data, by class name, in the order the folder lists the packages.
pub fn tutorials(cx: &Cx<'_>) -> Result<Tutorials, KindError> {
    let events = enum_names(&cx.object(EVENT_TYPES)?)?;
    let widgets = enum_names(&cx.object(INPUT_WIDGET_TYPES)?)?;
    // a pawn class's data key: its CharacterData (CharacterDataByFighter) as the port's CHARACTER_DATA names it
    let pgi = defaults_or_properties(&cx.object(PGI)?)?;
    let mut key_of_pawn = IndexMap::new();
    for pair in pgi.get("CharacterDataByFighter").and_then(Value::as_arr).unwrap_or_default() {
        let Some([Value::Str(pawn), Value::Str(data)]) = pair.as_arr() else { continue };
        // A path with no `.` is kept whole (no CharacterData path of the build lacks the object name).
        let pkg = data.rfind('.').map_or(data.as_str(), |i| &data[..i]).to_lowercase();
        if let Some((k, _)) = CHARACTER_DATA.iter().find(|(_, p)| p.to_lowercase() == pkg) {
            key_of_pawn.insert(pawn.to_lowercase(), *k);
        }
    }
    let mut out = Tutorials::new();
    for pkg in cx.catalog.list_packages(FOLDER, false, None) {
        let name = &pkg[pkg.rfind('/').map_or(0, |i| i + 1)..];
        if !name.starts_with("TutorialActor") || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            continue;
        }
        let cdo = cx.object(&format!("{pkg}.Default__{name}_C"))?;
        let p = defaults_or_properties(&cdo)?;
        // (a class without steps: a parent)
        let Some(steps) = p.get("TutorialSteps").and_then(Value::as_arr).filter(|s| !s.is_empty()) else { continue };
        let actor = no_c(short_name(cdo.class())).to_owned();
        let r = Reader { actor: &actor, events: &events, widgets: &widgets, key_of_pawn: &key_of_pawn };
        let steps = steps.iter().enumerate().map(|(si, s)| r.step(s, si)).collect::<Result<Vec<_>, _>>()?;
        let congratulations = r
            .list(p.get("RandomCongratulationText"), "RandomCongratulationText")?
            .iter()
            .map(|t| r.text(Some(t)))
            .collect();
        let data = TutorialActorData {
            actor: actor.clone(),
            name: r.text(p.get("TutorialName")),
            intro_text: r.text(p.get("IntroText")),
            intro_vo: r.name(p.get("IntroVOSoundAlias"), "IntroVOSoundAlias")?,
            next: r.cls(p.get("NextTutorial"), "NextTutorial")?,
            complete_key: match p.get("tutorialCompleteKey") {
                Some(Value::Str(k)) if !k.is_empty() => Some(k.clone()),
                _ => None,
            },
            congratulations,
            steps,
        };
        out.insert(actor, data);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(members: &[(&str, &str)]) -> Value {
        Value::Obj(members.iter().map(|(k, v)| ((*k).to_owned(), Value::Str((*v).to_owned()))).collect())
    }

    #[test]
    fn texts_keep_their_locres_key() {
        let (events, widgets, keys) = (EnumNames::new(), EnumNames::new(), IndexMap::new());
        let r = Reader { actor: "A", events: &events, widgets: &widgets, key_of_pawn: &keys };
        let t = r.text(Some(&obj(&[("source", "Hi"), ("namespace", ""), ("key", "K1")])));
        assert_eq!((t.namespace.as_str(), t.key.as_str(), t.source.as_str()), ("", "K1", "Hi"));
        // a string table entry and an invariant text have no key; none is empty
        let t = r.text(Some(&obj(&[("table", "T"), ("key", "K2")])));
        assert_eq!(t, TutorialText::default());
        assert_eq!(r.text(Some(&obj(&[("source", "Plain")]))).source, "Plain");
        assert_eq!(r.text(None), TutorialText::default());
        assert_eq!(r.native(Some(&Value::Str("EInputModifier::Hold".into())), "m").unwrap(), "Hold");
        assert_eq!(r.native(Some(&Value::Str("EInputModifier::None".into())), "m").unwrap(), "");
        assert_eq!(no_c("Pawn_Shaggy_C"), "Pawn_Shaggy");
    }
}
