//! The podium's toasts: the player's toast stock and the giving and receiving of a toast (PandaGameInstance BPI_Toast
//! @102206 and On Toast Received @141815 -> Send Toast Notification).
//!
//! The build's stock is the server's InventoryConsumables["match_toasts"]; the port's is the stats file's count (5 at
//! first, +1 per online match won with another human in it, with no cap). One service per page.
//! Deviation: the stats file and the host core's toast message are not in this crate: the service asks them through
//! `ToastBackend`, and `spend` answers at once. The owner calls `tick` every frame (the 60 s
//! ToastedRecently window runs on the view's clock) and `received` for each toast message of the host core.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use super::controller::{Cx, Event, Handler};
use super::manager::{Manager, NotificationParams};
use crate::slate::WidgetId;

/// What the service reads and writes outside the menu.
pub trait ToastBackend: std::fmt::Debug {
    /// The toasts left (the stats file's count).
    fn count(&self) -> i32;
    /// The stock - 1 (also for a bot): the count left, or None when the stats file refused it (none left).
    fn spend(&mut self) -> Option<i32>;
}

/// The stock the port keeps without a stats file: 5 toasts, one less
/// per spend.
/// Deviation: the stats file (+1 per online match won with another human in it, kept across runs) is not ported; the
/// stock starts at 5 in every run.
#[derive(Debug)]
pub struct LocalStock {
    pub count: i32,
}

impl ToastBackend for LocalStock {
    fn count(&self) -> i32 {
        self.count
    }
    fn spend(&mut self) -> Option<i32> {
        if self.count <= 0 {
            return None;
        }
        self.count -= 1;
        Some(self.count)
    }
}

/// Send Toast Notification's texts (empty namespace): "TOAST BACK" (buttonLabel1) and "BUY MORE" (buttonLabel2).
const TOAST_BACK: &str = "1AE9D5F64E8F04F61F93AF9DD4B9C9C4";
const BUY_MORE: &str = "9F73013048B0F6D401ACBE8CEA1A4ACB";
/// The notification's ExtraWidget (soft class UI_CustomNotificationToasts_C) and tag.
const TOAST_EXTRA: &str = "UI_CustomNotificationToasts";
const TOAST_TAG: &str = "toast";
/// BPI_Toast's Delay(60) @102267.
const RECENT_WINDOW: f64 = 60.0;
/// Send Toast Notification's screenTime @3140.
const SCREEN_TIME: f64 = 8.0;

/// Handler ids the owner of a toast notification (the service's `received`) gets back.
pub const TOAST_BACK_CLICKED: u32 = 0x7001;

#[derive(Debug)]
pub struct Toasts {
    /// The stock.
    pub backend: Rc<RefCell<dyn ToastBackend>>,
    /// OnToasted(id): a toast went to `id` (every podium button of it hides).
    pub on_toasted: Event<String>,
    /// OnToastsDecremented: the count went down by one.
    pub on_decremented: Event<()>,
    /// The count changed (a spend, a record's +1): the build's InventoryUpdated.
    pub on_changed: Event<i32>,
    /// ToastedRecently.
    recently: HashSet<String>,
    /// Seconds left of the Delay's latent action (< 0: none) and the toastee of the last BPI_Toast, which its end
    /// reads.
    window: f64,
    last_toastee: String,
    /// The names of the players who toasted this one, by id (the toast message's name): the notification's player
    /// row when the online lists do not have the toaster.
    senders: Vec<(String, String)>,
}

impl Default for Toasts {
    fn default() -> Toasts {
        Toasts {
            backend: Rc::new(RefCell::new(LocalStock { count: 5 })),
            on_toasted: Event::default(),
            on_decremented: Event::default(),
            on_changed: Event::default(),
            recently: HashSet::new(),
            window: -1.0,
            last_toastee: String::new(),
            senders: Vec::new(),
        }
    }
}

impl Toasts {
    /// The toasts left.
    pub fn count(&self) -> i32 {
        self.backend.borrow().count()
    }

    /// Whether `id` was toasted within the ToastedRecently window (TOAST BACK hides for it).
    pub fn toasted_recently(&self, id: &str) -> bool {
        self.recently.contains(id)
    }

    /// The name the toast message of `id` gave, if one came.
    pub fn sender_name(&self, id: &str) -> Option<&str> {
        self.senders.iter().find(|(s, _)| s == id).map(|(_, n)| n.as_str())
    }

    /// The Delay's clock.
    pub fn tick(&mut self, dt: f64) {
        if self.window < 0.0 {
            return;
        }
        self.window -= dt;
        if self.window <= 0.0 {
            self.window = -1.0;
            self.recently.remove(&self.last_toastee);
        }
    }

    /// BPI_Toast for the podium player `id` ('' a bot or an offline player): the ToastedRecently window, the spend,
    /// the send when `valid` (a human of the last online match), then OnToastsDecremented and OnToasted.
    pub fn toast(cx: &mut Cx<'_>, id: &str, valid: bool) {
        let backend = cx.view.session.toasts.backend.clone();
        let s = &mut cx.view.session.toasts;
        // @102216 Set_Add, then the Delay: one 60 s window from the first toast; a toast inside it starts none (a Delay
        // whose action exists ignores the call) and its end removes only the latest toastee (the event parameter it
        // reads), every other toastee of the window staying for the program's run.
        s.recently.insert(id.to_string());
        s.last_toastee = id.to_string();
        if s.window < 0.0 {
            s.window = RECENT_WINDOW;
        }
        // @102499-102654 the stock - 1, also for a bot. Deviation: before the send (@102345), so that a toast the stats
        // file refused (none left: the podium's buttons are hidden then) is not sent.
        if backend.borrow_mut().spend().is_none() {
            return;
        }
        // @102345 SendToast (IsValidToastee): the port's toast message, when the toastee is a human of an online match.
        if valid && !id.is_empty() {
            cx.online(super::online::OnlineCmd::Toast { to: id.to_string() });
        }
        // @102757, @102889 (the handlers copied first: they read the service again)
        let (decremented, toasted, changed) = {
            let s = &cx.view.session.toasts;
            (s.on_decremented.clone(), s.on_toasted.clone(), s.on_changed.clone())
        };
        decremented.fire(cx, &());
        toasted.fire(cx, &id.to_string());
        let count = backend.borrow().count();
        changed.fire(cx, &count);
        // @104608-104900: the stock gone, every toast notification's TOAST BACK is disabled; else the toastee's own
        // (@16697-17038: tag "toast", AccountID the toastee, the toastee in ToastedRecently).
        let none = count <= 0;
        for n in cx.view.manager.notifications_shown().to_vec() {
            let Some(note) = cx.view.notification(n) else { continue };
            let p = note.params();
            if p.tag != TOAST_TAG {
                continue;
            }
            let own = p.account_id == id && cx.view.session.toasts.recently.contains(id);
            if none || own {
                cx.visit_dyn(n, |c, cx| {
                    if let Some(note) = c.as_notification() {
                        note.disable_action1(cx);
                    }
                });
            }
        }
    }

    /// On Toast Received: the host's toast message, shown as the toast notification (Send Toast Notification
    /// @2482-3819, the toaster not a spectator: the port's host core has none). The rewards loop (@4922-5327) has
    /// nothing to show (the port's toasts reward nothing): the title is empty, and the toaster shows as the player
    /// row. TOAST BACK while the toaster is not in ToastedRecently and a toast is left; BUY MORE when none is
    /// (BuyToasts @115493 opens the shop: the port has none, the button does nothing).
    /// Shown as it comes on the menus and on the results screen (the podium, where a toast is given).
    /// Deviation: a toast that comes during a match is shown at the next menu or results screen (the build's
    /// UI_Global shows it over anything).
    /// The build has no sound of its own for a received toast: the notification's Construct posts
    /// ui_menu_notification (UI_Notification @5105), as every notification (audio null @3192).
    /// The notification's widget: its TOAST BACK (OnAction1) is the owner's to bind to `toast(from, true)`
    /// (ToastBack @92175-92525: BPI_Toast(the notification's AccountID, a human)): the notification hears its own
    /// `handle(TOAST_BACK_CLICKED, ())`; the toaster is the `account_id` of the notification it shows, `name` the
    /// toaster's name the message gave (its player row when the online lists do not have it).
    pub fn received(cx: &mut Cx<'_>, count: i32, from: &str, name: &str) -> Option<WidgetId> {
        if from.is_empty() {
            return None;
        }
        let senders = &mut cx.view.session.toasts.senders;
        match senders.iter_mut().find(|(s, _)| s == from) {
            Some(s) => s.1 = name.to_string(),
            None => senders.push((from.to_string(), name.to_string())),
        }
        let params = NotificationParams {
            screen_time: SCREEN_TIME,
            allow_dismissal: true,
            audio: None,
            tag: TOAST_TAG.into(),
            button_label1: if !cx.view.session.toasts.recently.contains(from) && count > 0 {
                cx.view.game_text(TOAST_BACK).to_string()
            } else {
                String::new()
            },
            button_label2: if count > 0 { String::new() } else { cx.view.game_text(BUY_MORE).to_string() },
            account_id: from.to_string(),
            show_timer: false,
            timing: 0,
            extra_widget: TOAST_EXTRA.into(),
            ..NotificationParams::default()
        };
        let n = Manager::show_notification(cx, params)?;
        if let Some(note) = cx.view.notification(n) {
            note.on_action1().add(Handler { owner: n, id: TOAST_BACK_CLICKED });
        }
        Some(n)
    }
}
