//! An online match's text chat (the port's own: the beta has no chat): the state of
//! the box, the log and the bubbles, with the gates a message goes through. Enter opens the box on team chat,
//! Shift+Enter on all chat (a match with no team mates: all chat either way); others' messages come as bubbles at
//! their scoreboard slot; the box is shown only while this player has it open. The program draws the bubbles at
//! `bubbles(now)` over the HUD's slot rectangles and the box from `open()`, feeds it the typed text and the keys, and
//! sends what `submit` hands back as `OnlineCmd::Chat` / the match client's chat message.
//!
//! Not here: the drawing and the text input of the native window (the program's HUD overlay owns them), the HUD's slot
//! rectangle (`hud.slotRect`) and the `chatInMatch` setting's storage (`UiSettings`, the program passes it in).

use std::collections::HashMap;

use mvs_protocol::chat::{CHAT_BUBBLE_MS, CHAT_BUBBLES, CHAT_FADE_MS, CHAT_LIMIT, ChatGate, chat_length, clean_chat};
use mvs_protocol::messages::ChatTo;
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

/// The lines a log keeps at the most.
pub const CHAT_KEPT: usize = 100;
/// A sender's receive budget is this many times the send budget; all senders together
/// RECEIVE_ALL_TIMES; the budgets of RECEIVE_SENDERS senders are kept, the oldest let go.
const RECEIVE_TIMES: f64 = 2.0;
const RECEIVE_ALL_TIMES: f64 = 4.0;
const RECEIVE_SENDERS: usize = 8;

/// A line of a chat log.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatLine {
    pub from: String,
    pub name: String,
    pub to: ChatTo,
    pub text: String,
    pub mine: bool,
}

/// What the box does with a key (the input's keydown).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxKey {
    Escape,
    Tab,
    Enter,
}

/// A bubble at a scoreboard slot: its text, channel and age.
#[derive(Clone, Debug, PartialEq)]
pub struct Bubble {
    pub slot: usize,
    pub text: String,
    pub all: bool,
    /// 1 fully shown, falling to 0 over CHAT_FADE_MS after CHAT_BUBBLE_MS.
    pub alpha: f64,
}

/// What `submit` decided.
#[derive(Clone, Debug, PartialEq)]
pub enum Submit {
    /// Nothing to send (an empty text): the box closes.
    Empty,
    /// Over the budget: the row shakes and the box stays.
    TooFast,
    /// Cleaned text to send (`all`: every team); the line is in the log and the box closes.
    Send { text: String, all: bool },
}

/// What a game sends and keeps. Times in ms.
/// Deviation: the budgets are not module-level (one per game for the lobby's chat and the match's alike); here
/// each match has its own (and each online lobby its own, mvs-ui lobby/net.rs), made fresh at its start. The two
/// chats are never open at once; only a burst sent across a lobby-to-match change gets a fresh budget.
#[derive(Debug)]
pub struct ChatBudget {
    send: ChatGate,
    senders: Vec<(String, ChatGate)>,
    all: ChatGate,
}

impl ChatBudget {
    pub fn new(now: f64) -> ChatBudget {
        ChatBudget { send: ChatGate::new(1.0, now), senders: Vec::new(), all: ChatGate::new(RECEIVE_ALL_TIMES, now) }
    }

    /// sendChat: the cleaned text, '' for nothing to send, None over the budget.
    pub fn send(&mut self, text: &str, now: f64) -> Option<String> {
        let t = clean_chat(&Value::String(text.to_string()));
        if t.is_empty() {
            return Some(String::new());
        }
        if !self.send.allow(now) {
            return None;
        }
        Some(t)
    }

    /// receiveChat: a message the host core passed on, as it is shown (None: empty, unnamed sender, or past the
    /// sender's or all senders' budget).
    pub fn receive(&mut self, from: &str, text: &str, now: f64) -> Option<String> {
        if from.is_empty() {
            return None;
        }
        let t = clean_chat(&Value::String(text.to_string()));
        if t.is_empty() {
            return None;
        }
        let at = match self.senders.iter().position(|(f, _)| f == from) {
            Some(i) => i,
            None => {
                if self.senders.len() >= RECEIVE_SENDERS {
                    self.senders.remove(0);
                }
                self.senders.push((from.to_string(), ChatGate::new(RECEIVE_TIMES, now)));
                self.senders.len() - 1
            }
        };
        // (both gates are asked: a sender over its own budget leaves the all-senders budget as it was)
        (self.senders[at].1.allow(now) && self.all.allow(now)).then_some(t)
    }
}

/// The match chat's state.
#[derive(Debug)]
pub struct MatchChat {
    me: usize,
    peers: Vec<String>,
    names: Vec<String>,
    teams: Vec<u32>,
    mates: bool,
    log: Vec<ChatLine>,
    budget: ChatBudget,
    bubbles: HashMap<usize, Vec<(ChatLine, f64)>>,
    /// The box is open, on all chat or team chat.
    open: bool,
    all: bool,
    pub input: String,
}

impl MatchChat {
    /// `peers`: each slot's player id ('' a bot); `names`, `teams` by slot; `me` this player's slot.
    pub fn new(me: usize, peers: Vec<String>, names: Vec<String>, teams: Vec<u32>, now: f64) -> MatchChat {
        let mates = teams.iter().enumerate().any(|(i, t)| i != me && !peers[i].is_empty() && Some(t) == teams.get(me));
        MatchChat {
            me,
            peers,
            names,
            teams,
            mates,
            log: Vec::new(),
            budget: ChatBudget::new(now),
            bubbles: HashMap::new(),
            open: false,
            all: false,
            input: String::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The box's channel is all chat.
    pub fn to_all(&self) -> bool {
        self.all
    }

    pub fn log(&self) -> &[ChatLine] {
        &self.log
    }

    /// Enter / Shift+Enter (`shift`): not while the setting is off, the pause menu is up, or the box is open. The
    /// program lets the held keys go when this returns true (releaseKeys).
    pub fn open_box(&mut self, shift: bool, enabled: bool, paused: bool) -> bool {
        if self.open || !enabled || paused {
            return false;
        }
        self.all = shift || !self.mates;
        self.input.clear();
        self.open = true;
        true
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input.clear();
    }

    /// The pause menu came up, or the setting went off mid-match: the box closes; off also clears the bubbles.
    pub fn settings_changed(&mut self, enabled: bool, paused: bool) {
        if paused || !enabled {
            self.close();
        }
        if !enabled {
            self.bubbles.clear();
        }
    }

    /// A text typed: cut where typed, past the limit (the count is in graphemes, an emoji one).
    pub fn set_input(&mut self, text: &str) {
        self.input = if chat_length(text) > CHAT_LIMIT {
            text.graphemes(true).take(CHAT_LIMIT).collect()
        } else {
            text.to_string()
        };
    }

    /// A key of the open box. Escape closes, Tab switches the channel (a match with mates), Enter submits.
    pub fn key(&mut self, key: BoxKey, enabled: bool, now: f64) -> Option<Submit> {
        if !self.open {
            return None;
        }
        match key {
            BoxKey::Escape => {
                self.close();
                None
            }
            BoxKey::Tab => {
                if self.mates {
                    self.all = !self.all;
                }
                None
            }
            BoxKey::Enter => {
                let text = std::mem::take(&mut self.input);
                match self.budget.send(&text, now) {
                    None => {
                        self.input = text;
                        Some(Submit::TooFast)
                    }
                    Some(t) if t.is_empty() => {
                        self.close();
                        Some(Submit::Empty)
                    }
                    Some(t) => {
                        let all = self.all;
                        let line = ChatLine {
                            from: String::new(),
                            name: self.names.get(self.me).cloned().unwrap_or_default(),
                            to: if all { ChatTo::All } else { ChatTo::Team },
                            text: t.clone(),
                            mine: true,
                        };
                        self.add(line, enabled, now);
                        self.close();
                        Some(Submit::Send { text: t, all })
                    }
                }
            }
        }
    }

    /// A chat message of the match client (`m.from`, `m.to`, `m.text`). Only the match's channels are shown.
    pub fn received(&mut self, from: &str, to: ChatTo, text: &str, enabled: bool, now: f64) {
        let Some(slot) = self.peers.iter().position(|p| p == from) else { return };
        if !matches!(to, ChatTo::Team | ChatTo::All) {
            return;
        }
        if let Some(t) = self.budget.receive(from, text, now) {
            let name = self.names.get(slot).cloned().unwrap_or_default();
            self.add(ChatLine { from: from.to_string(), name, to, text: t, mine: false }, enabled, now);
        }
    }

    fn add(&mut self, line: ChatLine, enabled: bool, now: f64) {
        self.log.push(line.clone());
        if self.log.len() > CHAT_KEPT {
            let over = self.log.len() - CHAT_KEPT;
            self.log.drain(..over);
        }
        let slot = if line.mine { Some(self.me) } else { self.peers.iter().position(|p| *p == line.from) };
        if let Some(slot) = slot.filter(|_| enabled) {
            let s = self.bubbles.entry(slot).or_default();
            s.insert(0, (line, now));
            s.truncate(CHAT_BUBBLES);
        }
    }

    /// A log line's side for its colour: mine, an ally's or an enemy's.
    pub fn side(&self, line: &ChatLine) -> Side {
        if line.mine {
            Side::Me
        } else if self.peers.iter().position(|p| *p == line.from).and_then(|i| self.teams.get(i))
            == self.teams.get(self.me)
        {
            Side::Ally
        } else {
            Side::Enemy
        }
    }

    /// The bubbles to draw at `now` (ms), newest first per slot; the ones past their fade go.
    pub fn bubbles(&mut self, now: f64) -> Vec<Bubble> {
        let mut out = Vec::new();
        for (slot, list) in &mut self.bubbles {
            list.retain(|(_, at)| now - at < CHAT_BUBBLE_MS + CHAT_FADE_MS);
            for (line, at) in list.iter() {
                let age = now - at;
                let alpha = if age <= CHAT_BUBBLE_MS { 1.0 } else { 1.0 - (age - CHAT_BUBBLE_MS) / CHAT_FADE_MS };
                out.push(Bubble { slot: *slot, text: line.text.clone(), all: line.to == ChatTo::All, alpha });
            }
        }
        out.sort_by_key(|b| b.slot);
        out
    }
}

/// Whose line it is (the log's name colour).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Me,
    Ally,
    Enemy,
}

// ---------- the overlay ----------

use mvs_ui::element::{BoxEl, DrawEffects, Element, ElementList, PaintGeometry, TextEl, Zone};
use mvs_ui::menu::custom_text as words;
use mvs_ui::text::{FontInfo, Fonts};

/// A bubble's widest text, u (one u is a pixel of a 1280 x 720 screen).
const BUBBLE_MAX_U: f64 = 260.0;
/// The corner points a rounded corner is drawn with.
const CORNER_STEPS: usize = 4;

type Rgba = [f64; 4];

fn rgb(hex: u32, a: f64) -> Rgba {
    [f64::from((hex >> 16) & 255) / 255.0, f64::from((hex >> 8) & 255) / 255.0, f64::from(hex & 255) / 255.0, a]
}

/// What the overlay needs of the frame: the HUD canvas size (device pixels), the layer, the scoreboard's position
/// and a scoreboard portrait's rectangle (`Hud::slot_rect`).
pub struct Overlay<'a> {
    pub size: [u32; 2],
    pub layer: i32,
    pub bottom: bool,
    pub slot_rect: &'a dyn Fn(usize) -> Option<[f64; 4]>,
    /// The caret shows (blinks with real time).
    pub caret: bool,
}

/// A clip rect, pixels: x0, y0, x1, y1.
type Clip = Option<[f64; 4]>;

struct Painter<'a> {
    list: &'a mut ElementList,
    fonts: &'a mut Fonts,
    layer: i32,
}

impl Painter<'_> {
    fn box_el(r: [f64; 4], tint: Rgba) -> Element {
        let geometry = PaintGeometry { size: [r[2], r[3]], transform: [1.0, 0.0, 0.0, 1.0, r[0], r[1]] };
        Element::Box(BoxEl { geometry, tint, uv: [0.0, 0.0, 1.0, 1.0], ..BoxEl::default() })
    }

    fn rect(&mut self, r: [f64; 4], tint: Rgba) {
        self.list.push(self.layer, None, DrawEffects::NONE, Painter::box_el(r, tint));
    }

    /// A rect with corners rounded at `radius` (chat.css border-radius), cut by a zone around it.
    fn rounded(&mut self, r: [f64; 4], radius: f64, tint: Rgba) {
        let radius = radius.min(r[2] / 2.0).min(r[3] / 2.0);
        let [x0, y0, x1, y1] = [r[0], r[1], r[0] + r[2], r[1] + r[3]];
        // corner centres clockwise from the top left, each with its arc's start angle
        let corners = [
            (x0 + radius, y0 + radius, std::f64::consts::PI),
            (x1 - radius, y0 + radius, 1.5 * std::f64::consts::PI),
            (x1 - radius, y1 - radius, 0.0),
            (x0 + radius, y1 - radius, 0.5 * std::f64::consts::PI),
        ];
        let mut pts = Vec::with_capacity(4 * (CORNER_STEPS + 1));
        for (cx, cy, a0) in corners {
            for k in 0..=CORNER_STEPS {
                let a = a0 + std::f64::consts::FRAC_PI_2 * k as f64 / CORNER_STEPS as f64;
                pts.push([cx + radius * a.cos(), cy + radius * a.sin()]);
            }
        }
        let zone: Zone = pts.into();
        self.list.push_zoned(self.layer, None, Some(zone), DrawEffects::NONE, Painter::box_el(r, tint));
    }

    /// A bubble's tail (chat.css `.chat-bubble:first-child::before`: a square of `side` turned 45 degrees on the
    /// middle of the bubble's edge at `edge_y`, under it): the half outside the bubble, pointing up (`up`) or down.
    fn tail(&mut self, cx: f64, edge_y: f64, side: f64, up: bool, tint: Rgba) {
        let half = side * std::f64::consts::FRAC_1_SQRT_2;
        let tip = if up { edge_y - half } else { edge_y + half };
        let zone: Zone = vec![[cx - half, edge_y], [cx + half, edge_y], [cx, tip]].into();
        let r = [cx - half, edge_y.min(tip), 2.0 * half, half];
        self.list.push_zoned(self.layer, None, Some(zone), DrawEffects::NONE, Painter::box_el(r, tint));
    }

    /// Text at (x, y) top left, cut to `clip`; -> its width.
    fn text_in(&mut self, x: f64, y: f64, size: f64, text: &str, color: Rgba, clip: Clip) -> f64 {
        let info = FontInfo { size, ..FontInfo::default() };
        let m = self.fonts.measure(&info, text, 1.0);
        let run = self.fonts.shape(&info, text, 1.0);
        let geometry = PaintGeometry { size: [m.width, m.height], transform: [1.0, 0.0, 0.0, 1.0, x, y] };
        let el = TextEl { geometry, run, color, outline_color: [0.0, 0.0, 0.0, 0.0], material: None };
        self.list.push(self.layer + 1, clip, DrawEffects::NONE, Element::Text(el));
        m.width
    }

    fn text(&mut self, x: f64, y: f64, size: f64, text: &str, color: Rgba) -> f64 {
        self.text_in(x, y, size, text, color, None)
    }

    fn width(&mut self, size: f64, text: &str) -> f64 {
        self.fonts.measure(&FontInfo { size, ..FontInfo::default() }, text, 1.0).width
    }

    /// `text` broken at characters so no line passes `max`, the first `first` (overflow-wrap: anywhere).
    fn wrap(&mut self, size: f64, text: &str, first: f64, max: f64) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for g in text.graphemes(true) {
            let mut t = cur.clone();
            t.push_str(g);
            let room = if lines.is_empty() { first } else { max };
            if !cur.is_empty() && self.width(size, &t) > room {
                lines.push(std::mem::take(&mut cur));
                cur.push_str(g);
            } else if lines.is_empty() && cur.is_empty() && first < max && self.width(size, &t) > room {
                // a first line with no room for its first character starts under it
                lines.push(String::new());
                cur = t;
            } else {
                cur = t;
            }
        }
        if !cur.is_empty() || lines.is_empty() {
            lines.push(cur);
        }
        lines
    }
}

impl MatchChat {
    /// Draws the bubbles at the scoreboard slots and, while open, the box (chat.css): the bubbles stand under the
    /// portrait with the scoreboard at the top and above it at the bottom, the newest nearest with a tail toward it;
    /// the box is opposite the scoreboard.
    pub fn draw(&mut self, o: &Overlay<'_>, now: f64, fonts: &mut Fonts, list: &mut ElementList) {
        let u = (f64::from(o.size[1]) / 720.0).min(f64::from(o.size[0]) / 1280.0);
        let mut p = Painter { list, fonts, layer: o.layer };
        let bubbles = self.bubbles(now);
        let slots: Vec<usize> = {
            let mut s: Vec<usize> = bubbles.iter().map(|b| b.slot).collect();
            s.dedup();
            s
        };
        for slot in slots {
            let Some(at) = (o.slot_rect)(slot) else { continue };
            let cx = at[0] + at[2] / 2.0;
            // newest first; the stack grows away from the portrait
            let mut y = if o.bottom { at[1] - 10.0 * u } else { at[1] + at[3] + 10.0 * u };
            for (i, b) in bubbles.iter().filter(|b| b.slot == slot).enumerate() {
                let size = 15.0 * u;
                let max = BUBBLE_MAX_U * u;
                let lines = p.wrap(size, &b.text, max, max);
                let widest = lines.iter().map(|l| p.width(size, l)).fold(0.0, f64::max);
                let lh = size * 1.3;
                let (w, h) = (widest + 22.0 * u, lines.len() as f64 * lh + 10.0 * u);
                // the older bubbles are .55 opaque
                let a = b.alpha * if i == 0 { 1.0 } else { 0.55 };
                let top = if o.bottom { y - h } else { y };
                let tint = if b.all { rgb(0xffd5cf, a) } else { rgb(0xfdfaf4, a) };
                p.rounded([cx - w / 2.0, top, w, h], 6.0 * u, tint);
                if i == 0 {
                    let edge = if o.bottom { top + h } else { top };
                    p.tail(cx, edge, 12.0 * u, !o.bottom, tint);
                }
                for (k, l) in lines.iter().enumerate() {
                    let lw = p.width(size, l);
                    p.text(cx - lw / 2.0, top + 5.0 * u + k as f64 * lh, size, l, rgb(0x1b1f3a, a));
                }
                y = if o.bottom { top - 5.0 * u } else { top + h + 5.0 * u };
            }
        }
        if !self.open {
            return;
        }
        let w = 330.0 * u;
        let x = f64::from(o.size[0]) / 2.0 - w / 2.0;
        let (log_h, row_h) = (158.0 * u, 34.0 * u);
        let total = log_h + 6.0 * u + row_h;
        let top = if o.bottom { 20.0 * u } else { f64::from(o.size[1]) - 56.0 * u - total };
        p.rounded([x, top, w, log_h], 4.0 * u, rgb(0x0b0e1f, 0.2));
        // The log (chat.css .chat-log): the newest line at the bottom, each wrapped in the box's width (the first
        // row after its tag and name), the older ones up to the box's top and cut there; the top 70 u fades out.
        let size = 14.0 * u;
        let row = size * 1.3;
        let (left, right) = (x + 10.0 * u, x + w - 10.0 * u);
        let log_clip = Some([x, top, x + w, top + log_h]);
        let fade = |y: f64| ((y - top) / (70.0 * u)).clamp(0.0, 1.0);
        let mut bottom = top + log_h - 6.0 * u;
        for line in self.log.iter().rev() {
            if bottom <= top {
                break;
            }
            let (tag, tag_c) = match line.to {
                ChatTo::All => (words::CHAT_TAG_ALL, rgb(0xc9d2ee, 1.0)),
                _ => (words::CHAT_TAG_TEAM, rgb(0x4db8ff, 1.0)),
            };
            let name_c = match self.side(line) {
                Side::Me => rgb(0xffd200, 1.0),
                Side::Ally => rgb(0x4db8ff, 1.0),
                Side::Enemy => rgb(0xff5c7a, 1.0),
            };
            let name = format!("{}:", line.name);
            let lead = p.width(11.0 * u, tag) + 5.0 * u + p.width(size, &name) + 5.0 * u;
            let rows = p.wrap(size, &line.text, right - left - lead, right - left);
            let y = bottom - rows.len() as f64 * row;
            let mut x1 = left;
            let a = fade(y + row / 2.0);
            x1 += p.text_in(x1, y, 11.0 * u, tag, [tag_c[0], tag_c[1], tag_c[2], a], log_clip) + 5.0 * u;
            x1 += p.text_in(x1, y, size, &name, [name_c[0], name_c[1], name_c[2], a], log_clip) + 5.0 * u;
            for (k, r) in rows.iter().enumerate() {
                let ry = y + k as f64 * row;
                let a = fade(ry + row / 2.0);
                p.text_in(if k == 0 { x1 } else { left }, ry, size, r, rgb(0xffffff, a), log_clip);
            }
            bottom = y - 3.0 * u;
        }
        let ry = top + log_h + 6.0 * u;
        p.rounded([x, ry, w, row_h], 4.0 * u, rgb(0x0b0e1f, 0.55));
        let (chan, c) =
            if self.all { (words::CHAT_TO_ALL, rgb(0xc9d2ee, 1.0)) } else { (words::CHAT_TO_TEAM, rgb(0x4db8ff, 1.0)) };
        let mut x1 = x + 10.0 * u;
        x1 += p.text(x1, ry + 8.0 * u, 13.0 * u, chan, c) + 8.0 * u;
        // The input (chat.css .chat-input, flex 1 to the row's padding): cut to its room and scrolled left so the
        // caret, at the end of the text, stays in it.
        let typed = self.input.clone();
        let size = 16.0 * u;
        let caret_w = 2.5 * u;
        let room = right - x1;
        let tw = p.width(size, &typed);
        let scroll = (tw + caret_w - room).max(0.0);
        let clip = Some([x1, ry, right, ry + row_h]);
        p.text_in(x1 - scroll, ry + 7.0 * u, size, &typed, rgb(0xffffff, 1.0), clip);
        if o.caret {
            p.rect([x1 - scroll + tw + 1.0 * u, ry + 8.0 * u, 1.5 * u, 18.0 * u], rgb(0xffffff, 1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat() -> MatchChat {
        MatchChat::new(
            0,
            vec!["a".into(), "b".into(), "c".into()],
            vec!["Ann".into(), "Bob".into(), "Cy".into()],
            vec![0, 0, 1],
            0.0,
        )
    }

    #[test]
    fn open_channels() {
        let mut c = chat();
        assert!(c.open_box(false, true, false));
        assert!(!c.to_all());
        c.close();
        assert!(c.open_box(true, true, false));
        assert!(c.to_all());
        c.close();
        assert!(!c.open_box(false, false, false));
        assert!(!c.open_box(false, true, true));
    }

    #[test]
    fn no_mates_is_all_chat() {
        let mut c = MatchChat::new(0, vec!["a".into(), "".into()], vec![String::new(); 2], vec![0, 0], 0.0);
        assert!(c.open_box(false, true, false));
        assert!(c.to_all());
    }

    #[test]
    fn send_logs_and_bubbles() {
        let mut c = chat();
        c.open_box(false, true, false);
        c.set_input("hi there");
        let r = c.key(BoxKey::Enter, true, 0.0);
        assert_eq!(r, Some(Submit::Send { text: "hi there".into(), all: false }));
        assert!(!c.is_open());
        assert_eq!(c.log().len(), 1);
        let b = c.bubbles(1000.0);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].slot, 0);
        assert!(c.bubbles(CHAT_BUBBLE_MS + CHAT_FADE_MS + 1.0).is_empty());
    }

    #[test]
    fn send_budget_shakes() {
        let mut c = chat();
        for _ in 0..3 {
            c.open_box(false, true, false);
            c.set_input("x");
            assert!(matches!(c.key(BoxKey::Enter, true, 0.0), Some(Submit::Send { .. })));
        }
        c.open_box(false, true, false);
        c.set_input("x");
        assert_eq!(c.key(BoxKey::Enter, true, 0.0), Some(Submit::TooFast));
        assert!(c.is_open());
    }

    #[test]
    fn received_slot_and_side() {
        let mut c = chat();
        c.received("b", ChatTo::Team, "yo", true, 0.0);
        c.received("zz", ChatTo::Team, "who", true, 0.0);
        c.received("c", ChatTo::Lobby, "lobby", true, 0.0);
        assert_eq!(c.log().len(), 1);
        let l = c.log()[0].clone();
        assert_eq!(c.side(&l), Side::Ally);
        c.received("c", ChatTo::All, "gg", true, 0.0);
        let l = c.log()[1].clone();
        assert_eq!(c.side(&l), Side::Enemy);
        assert_eq!(c.bubbles(0.0).len(), 2);
    }

    #[test]
    fn setting_off_clears_bubbles() {
        let mut c = chat();
        c.received("b", ChatTo::All, "yo", true, 0.0);
        c.settings_changed(false, false);
        assert!(c.bubbles(0.0).is_empty());
    }

    #[test]
    fn receive_budget_per_sender() {
        let mut c = chat();
        for _ in 0..8 {
            c.received("b", ChatTo::All, "m", true, 0.0);
        }
        // a sender holds 2 x the burst (6); all senders 4 x (12)
        assert_eq!(c.log().len(), 6);
    }

    #[test]
    fn input_cut_at_limit() {
        let mut c = chat();
        c.set_input(&"a".repeat(100));
        assert_eq!(chat_length(&c.input), CHAT_LIMIT);
    }
}
