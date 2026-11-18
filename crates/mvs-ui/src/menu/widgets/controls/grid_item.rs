//! UI_GridItemBase: what UI_GridItemList uses of a tile.
//!
//! A tile controller holds a `GridItemState` and forwards `resume` to `GridItemState::resume`; the list reads the
//! tile through the `GridItem` trait (a controller's `as_grid_item`, owed by the track that ports the list).

use super::wd;
use crate::menu::controller::{Cx, Event, Handler};
use crate::slate::WidgetId;

/// The delay's end (ReadyToEnter @3811) and the wait on the EnterAnim: the ids a tile's `resume` forwards.
pub const GRID_READY: u32 = 0xF001;
pub const GRID_WAIT: u32 = 0xF002;

/// (the tile, the player).
pub type GridEvent = Event<(WidgetId, i32)>;

#[derive(Debug, Default)]
pub struct GridItemState {
    pub focused_event: GridEvent,
    pub selected_event: GridEvent,
    pub index: i32,
    pub selected: bool,
    pub force_unlocked: bool,
    /// Toggle Selected State: a click on a selected tile deselects it.
    pub toggle_selected_state: bool,
    pub is_entering: bool,
    /// The EnterAnim of the Enter in progress (GetAnimations), none for a class without one.
    enter_anim: Option<String>,
}

/// What a tile class gives the list: its interactors, its lock, its data and its look. The data type is the
/// class's own.
pub trait GridItem {
    fn state(&mut self) -> &mut GridItemState;
    /// The port's own lock (what the port does not have), not the build's GetLocked.
    fn locked(&self) -> bool;
    fn set_size(&mut self, cx: &mut Cx<'_>, x: f64, y: f64);
    /// GetAnimations: the class's EnterAnim.
    fn enter_anim(&self) -> Option<&'static str> {
        None
    }
    fn update_appearance(&mut self, cx: &mut Cx<'_>);
}

impl GridItemState {
    /// Focus @4542 (audio and animations: per class).
    pub fn focus(&self, cx: &mut Cx<'_>, player: i32) {
        let me = cx.w;
        self.focused_event.fire(cx, &(me, player));
    }

    /// Select @5670.
    pub fn select(tile: &mut dyn GridItem, cx: &mut Cx<'_>, player: i32) {
        let s = tile.state();
        if s.is_entering {
            return;
        }
        s.selected = if s.toggle_selected_state { !s.selected } else { true };
        tile.update_appearance(cx);
        let me = cx.w;
        tile.state().selected_event.fire(cx, &(me, player));
    }

    /// Deselect @7456.
    pub fn deselect(tile: &mut dyn GridItem, cx: &mut Cx<'_>) {
        tile.state().selected = false;
        tile.update_appearance(cx);
    }

    /// Enter(Delay, Index) @8464: unseen and not selectable until the delay's UI timer (ReadyToEnter @3811), then the
    /// class's EnterAnim (GetAnimations), whose end clears IsEntering. A class without one: @4518.
    pub fn enter(tile: &mut dyn GridItem, cx: &mut Cx<'_>, delay: f64) {
        let anim = tile.enter_anim().map(str::to_string);
        let s = tile.state();
        s.is_entering = true;
        s.enter_anim = anim;
        let me = cx.w;
        wd(cx, me).opacity = 0.0;
        if delay > 0.0 {
            cx.view.delay(delay, Handler { owner: me, id: GRID_READY });
        } else {
            Self::resume(tile, cx, GRID_READY);
        }
    }

    /// The tile's `resume` for `GRID_READY` and `GRID_WAIT`.
    pub fn resume(tile: &mut dyn GridItem, cx: &mut Cx<'_>, id: u32) {
        let me = cx.w;
        let anim = tile.state().enter_anim.clone();
        match id {
            GRID_READY => {
                wd(cx, me).opacity = 1.0;
                tile.update_appearance(cx);
                let Some(a) = anim else {
                    tile.state().is_entering = false;
                    return;
                };
                cx.view.play(me, &a, false, 1.0);
                Self::resume(tile, cx, GRID_WAIT);
            }
            GRID_WAIT => {
                let Some(a) = anim else { return };
                if cx.view.is_animation_playing(me, &a) {
                    cx.view.delay(0.0, Handler { owner: me, id: GRID_WAIT });
                } else {
                    tile.state().is_entering = false;
                }
            }
            _ => {}
        }
    }
}
