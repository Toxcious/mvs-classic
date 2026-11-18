//! UI_InputLabel: a key or button of an action, with the Slots and
//! key names that it reads.

use serde_json::Value;

use super::{find, prop, prop_bool, wd};
use crate::bindings::is_gamepad_key;
use crate::bundle::Rgba;
use crate::menu::controller::{Controller, Cx};
use crate::menu::manager::{is_ui_action, ui_action_key};
use crate::menu::widgets::common::{enum_index, rgba, text};
use crate::slate::Vis;

pub use crate::bindings::LAB_TOOLS_ACTION;

/// EInputAction -> the action mapping's name (UI_InputLabel CreateKey). The UI actions have no mapping name.
pub const ACTION_NAMES: &[(&str, &str)] = &[
    ("GameAttack", "Attack"),
    ("GameSpecial", "Special"),
    ("GameEvade", "Evade"),
    ("GameJump", "Jump"),
    ("GameTaunt1", "Taunt1"),
    ("GameTaunt2", "Taunt2"),
    ("GameTaunt3", "Taunt3"),
    ("GameTaunt4", "Taunt4"),
    ("GameNeutralAttack", "Neutral Attack"),
    ("GameNeutralSpecial", "Neutral Special"),
    ("GamePickupItem", "Pickup Item"),
    ("GameNeutralEvade", "Neutral Evade"),
    ("Up", "Up"),
    ("Down", "Down"),
    ("Left", "Left"),
    ("Right", "Right"),
    ("GameRightAttack", "Right Attack"),
    ("GameLeftAttack", "Left Attack"),
    ("GameUpAttack", "Up Attack"),
    ("GameDownAttack", "Down Attack"),
    ("GameRightSpecial", "Right Special"),
    ("GameLeftSpecial", "Left Special"),
    ("GameUpSpecial", "Up Special"),
    ("GameDownSpecial", "Down Special"),
    ("LabTools", LAB_TOOLS_ACTION),
];

/// The mapping name of an action (None: not one the label drives).
pub fn action_name(action: &str) -> Option<&'static str> {
    ACTION_NAMES.iter().find(|(k, _)| *k == action).map(|(_, v)| *v)
}

/// The row titles (UI_InputBindingSet AddBinding's label switch).
pub fn action_label(mapping: &str) -> &str {
    match mapping {
        "Evade" => "Dodge",
        "Taunt1" => "Taunt 1",
        "Taunt2" => "Taunt 2",
        "Taunt3" => "Taunt 3",
        "Taunt4" => "Taunt 4",
        other => other,
    }
}

/// UI_InputLabel's modifier texts by EInputActionModifier (the switch @768-2268, its copy @2313-3642): the
/// MENU_TEXTS keys; and the Format "{modifier} + " (@2097) they go through before TextToUpper.
const MODIFIER_TEXTS: &[(&str, &str)] = &[
    ("Neutral", "0DF6CDB94285A1F8D39A3A83C742B63A"),
    ("Side", "06C47FDD407B9D582B75D4A69CF513F3"),
    ("Up", "2E2FF13E415A47A93B81CD9A70CCA05E"),
    ("Down", "9FDC516D47362F6A29B7B4A7165B093D"),
    ("SideAir", "0724A6ED4241C15CCF062F94DDBFA6F4"),
    ("Hold", "1DF69A0840FE482E9FD75A8C6C02AA1B"),
    ("Left", "48DC34EA450122F1251A369932A1CDA2"),
    ("Right", "389237734851535742511C887B34D7E0"),
    ("Direction", "676F2E1A422DFBC8A41E1B84A960129B"),
    ("NeutralAir", "537228CE4C1DDEA791EF8BB18355165B"),
    ("UpAir", "909001934351BF1B928EAA904C33B0BB"),
    ("DownAir", "5FF9EC7A4A0FEEC1942EBA912E1AA555"),
];
const MODIFIER_FORMAT: &str = "7F1D1BC742809C58145B67995339C8CA";
const DIRECTIONS: [&str; 4] = ["Up", "Down", "Left", "Right"];

/// An enumerator's name without its enum (`E::Name` -> `Name`); '' for none.
pub fn enum_name(v: Option<&Value>) -> String {
    let s = match v {
        Some(Value::String(s)) => s.as_str(),
        _ => "",
    };
    s.rsplit("::").next().unwrap_or_default().to_string()
}

/// A struct member by its name without the editor's suffix.
pub(crate) fn field<'a>(o: Option<&'a Value>, stem: &str) -> Option<&'a Value> {
    let Value::Object(m) = o? else { return None };
    let pre = format!("{stem}_");
    m.iter().find(|(k, _)| k.as_str() == stem || k.starts_with(&pre)).map(|(_, v)| v)
}

/// UI_Functions BreakApartMappings: the key slots of a mapping (keyboard 1 / 2, gamepad 1 / 2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slots {
    pub k1: Option<String>,
    pub k2: Option<String>,
    pub g1: Option<String>,
    pub g2: Option<String>,
}

/// The list is walked from its last entry to the first, invalid / None keys skipped; a key that is no gamepad key
/// fills Keyboard 1, then Keyboard 2, a gamepad key Gamepad 1, then Gamepad 2. `keys`: the mapping's keys as
/// GetActionMappingByName gives them (the last entry first), so the walk is the stored order.
pub fn break_apart_mappings<'a>(keys: impl DoubleEndedIterator<Item = &'a str>) -> Slots {
    let mut s = Slots::default();
    for k in keys.rev() {
        if k.is_empty() || k == "None" {
            continue;
        }
        let slot = if is_gamepad_key(k) {
            if s.g1.is_none() { &mut s.g1 } else { &mut s.g2 }
        } else if s.k1.is_none() {
            &mut s.k1
        } else {
            &mut s.k2
        };
        if slot.is_none() {
            *slot = Some(k.to_string());
        }
    }
    s
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputLabelParams {
    /// EInputAction's enumerator name.
    pub action: String,
    pub modifier: String,
    pub label: String,
    pub player_index: i32,
    pub automatic_input_type: bool,
    /// 0 gamepad, 1 keyboard, 2 mouse.
    pub fixed_input_type: i32,
    pub tint: Rgba,
    pub text_size: f64,
    pub key_size: f64,
    pub icon_size: [f64; 2],
    pub custom_icon: Option<String>,
}

/// UI_InputLabel.
#[derive(Debug)]
pub struct InputLabel {
    pub params: InputLabelParams,
    /// The 'Auto Refresh' property (true unless it is false): the owner writes it.
    pub auto_refresh: bool,
    current_input_type: i32,
}

impl InputLabel {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(InputLabel {
            params: InputLabelParams {
                action: "UISubmit".into(),
                modifier: "None".into(),
                label: String::new(),
                player_index: 0,
                automatic_input_type: true,
                fixed_input_type: 0,
                tint: [1.0; 4],
                text_size: 18.0,
                key_size: 18.0,
                icon_size: [32.0, 32.0],
                custom_icon: None,
            },
            auto_refresh: true,
            current_input_type: 0,
        })
    }

    fn driven(&self) -> bool {
        action_name(&self.params.action).is_some() || is_ui_action(&self.params.action)
    }

    fn resolve_type(&self, cx: &Cx<'_>) -> i32 {
        let q = &self.params;
        if q.automatic_input_type {
            cx.view.manager.last_interaction_type[q.player_index.clamp(0, 3) as usize]
        } else {
            q.fixed_input_type
        }
    }

    /// RefreshLoop / Refresh: a change of the input type redraws the label.
    pub fn refresh(&mut self, cx: &mut Cx<'_>) {
        let t = self.resolve_type(cx);
        if t == self.current_input_type {
            return;
        }
        self.current_input_type = t;
        self.force_refresh(cx);
    }

    pub fn update_params(&mut self, cx: &mut Cx<'_>, params: InputLabelParams) {
        self.params = params;
        self.current_input_type = self.resolve_type(cx);
        self.force_refresh(cx);
    }

    pub fn force_refresh(&mut self, cx: &mut Cx<'_>) {
        if !self.driven() {
            return;
        }
        let q = self.params.clone();
        let hide_text = prop_bool(cx, "ForceHideText");
        let label = find(cx, "textLabel");
        let modifier = find(cx, "modifierLabel");
        let bx = find(cx, "keyboardContainer");
        let icon = find(cx, "buttonIcon");
        let shown = |on: bool| if on { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        let mtext = self.modifier_text(cx, &q.modifier);
        {
            let l = wd(cx, label);
            l.text = q.label.clone();
            l.vis = shown(!q.label.is_empty() && !hide_text);
        }
        // @3805: ForceHideText hides the modifier as it does the label
        {
            let m = wd(cx, modifier);
            m.vis = shown(!mtext.is_empty() && !hide_text);
            m.text = mtext;
        }
        wd(cx, bx).vis = Vis::Collapsed;
        wd(cx, icon).vis = Vis::Collapsed;
        // The UI-bound actions read the manager's key lists; the others the stored mapping's slots.
        let ui = is_ui_action(&q.action);
        let slots = if ui {
            Slots {
                g1: ui_action_key(&q.action, true).map(str::to_string),
                g2: None,
                k1: ui_action_key(&q.action, false).map(str::to_string),
                k2: None,
            }
        } else {
            let name = action_name(&q.action).unwrap_or_default();
            break_apart_mappings(cx.bindings.action_mapping(name, q.player_index.max(0) as usize))
        };
        let binding_slot = prop(cx, "Binding Slot");
        let second = !ui && binding_slot.as_f64().unwrap_or(1.0) == 2.0;
        if let Some(c) = &q.custom_icon {
            let i = wd(cx, icon);
            i.tex = Some(c.clone());
            i.vis = Vis::SelfHitTestInvisible;
        } else if self.current_input_type == 0 {
            // ButtonValue: the generic USB controller's buttons have no glyph
            let analog =
                cx.view.tree().props(cx.w).get("Force Analog Directions").and_then(Value::as_bool) != Some(false);
            let key = if analog && DIRECTIONS.contains(&q.action.as_str()) {
                Some(format!("Gamepad_LeftStick_{}", q.action))
            } else if second {
                slots.g2.clone()
            } else {
                slots.g1.clone()
            };
            let tex = key
                .filter(|k| {
                    !(k.starts_with("GenericUSBController_Button")
                        && k.len() == 28
                        && matches!(k.as_bytes()[27], b'1'..=b'4'))
                })
                .and_then(|k| cx.view.input_icon(&k).map(str::to_string));
            if let Some(t) = tex {
                let i = wd(cx, icon);
                i.tex = Some(t);
                i.vis = Vis::SelfHitTestInvisible;
            }
        } else {
            // KeyboardValue
            let key = if second { slots.k2 } else { slots.k1 };
            if let Some(k) = key {
                let kl = find(cx, "keyLabel");
                let name = cx.view.text.key_names().name(&k);
                wd(cx, kl).text = name;
                wd(cx, bx).vis = Vis::SelfHitTestInvisible;
            }
        }
        self.update_colors(cx);
    }

    /// The switch on Params.Modifier (@768): None (and a value the switch has no text for) the empty text, else
    /// Format("{modifier} + ") of its text, upper-cased.
    fn modifier_text(&self, cx: &Cx<'_>, m: &str) -> String {
        let Some((_, key)) = MODIFIER_TEXTS.iter().find(|(k, _)| *k == m) else { return String::new() };
        cx.view.game_text(MODIFIER_FORMAT).replace("{modifier}", cx.view.game_text(key)).to_uppercase()
    }

    /// UI_Functions SetFontSize keeps the typeface and clears the letter spacing.
    fn update_colors(&self, cx: &mut Cx<'_>) {
        let q = &self.params;
        let dark = prop_bool(cx, "Is Dark");
        let dark_color = prop(cx, "Dark Color");
        let c = if dark && !dark_color.is_null() { rgba(&dark_color) } else { q.tint };
        let key_text = prop(cx, "KeyTextColor");
        let key = find(cx, "keyLabel");
        let label = find(cx, "textLabel");
        let icon = find(cx, "buttonIcon");
        let modifier = find(cx, "modifierLabel");
        let bx = find(cx, "keyboardContainer");
        let k = wd(cx, key);
        k.font_size = q.key_size;
        k.letter_spacing = 0.0;
        k.color = if dark {
            q.tint
        } else if !key_text.is_null() {
            rgba(&key_text)
        } else {
            [0.0, 0.003922, 0.015686, 1.0]
        };
        let l = wd(cx, label);
        l.font_size = q.text_size;
        l.letter_spacing = 0.0;
        l.color = c;
        wd(cx, modifier).color = c;
        wd(cx, bx).brush_color = c;
        let i = wd(cx, icon);
        i.brush_w = q.icon_size[0];
        i.brush_h = q.icon_size[1];
        i.color = c;
    }
}

impl Controller for InputLabel {
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let props = prop(cx, "Params");
        let p = if props.is_null() { None } else { Some(&props) };
        let size = field(p, "SizeIconSize");
        let f = |v: Option<&Value>, k: &str, d: f64| v.and_then(|v| v.get(k)).and_then(Value::as_f64).unwrap_or(d);
        self.params = InputLabelParams {
            action: match field(p, "Action") {
                Some(a) => enum_name(Some(a)),
                None => "UISubmit".into(),
            },
            modifier: match field(p, "Modifier") {
                Some(a) => enum_name(Some(a)),
                None => "None".into(),
            },
            label: text(field(p, "Label")),
            player_index: field(p, "PlayerIndex").and_then(Value::as_i64).unwrap_or(0) as i32,
            automatic_input_type: field(p, "AutomaticInputType").and_then(Value::as_bool).unwrap_or(true),
            fixed_input_type: enum_index(field(p, "FixedInputType")),
            tint: field(p, "Tint").map_or([1.0; 4], rgba),
            text_size: f(field(p, "TextFontInfo"), "Size", 18.0),
            key_size: f(field(p, "KeyFontInfo"), "Size", 18.0),
            icon_size: [f(size, "X", 32.0), f(size, "Y", 32.0)],
            custom_icon: field(p, "CustomActionIcon").and_then(Value::as_str).map(str::to_string),
        };
        self.auto_refresh = cx.view.tree().props(cx.w).get("Auto Refresh").and_then(Value::as_bool) != Some(false);
    }

    fn construct(&mut self, cx: &mut Cx<'_>) {
        if !self.driven() {
            return;
        }
        self.current_input_type = self.resolve_type(cx);
        self.force_refresh(cx);
    }

    fn tick(&mut self, cx: &mut Cx<'_>, _dt: f64) {
        if self.driven() && self.auto_refresh {
            self.refresh(cx);
        }
    }
}
