//! The loading screen before a match: the screen drawn
//! each frame while the match's World, views, warm-up, sound scopes and HUD bundle load on the loader's threads
//! (`crate::prepare`), then the match, which installs them.
//!
//! The screen is the build's SLoadingScreen (`mvs_ui::menu::screens::loading`): a random background image, the tip
//! bar and the throbber, which turns as the frames go.

use std::time::Instant;

use mvs_ui::menu::screens::loading::LoadingScreen;

use crate::app::{Frame, Go, MatchKind, MatchStart, Shell};
use crate::lab_host::LabAhead;
use crate::prepare::{Pending, PrepareJob};
use crate::{Error, Result};

/// A match loading behind the loading screen.
#[derive(Debug)]
pub struct Loading {
    start: Option<MatchStart>,
    /// The match's load on the loader (None: a replay, no store, or taken).
    pending: Option<Pending>,
    /// When the first frame was drawn (None before it).
    shown: Option<Instant>,
    /// The screen's settings (None: no UI, or no loading entry in the store; the frame is black).
    screen: Option<LoadingScreen>,
    /// A new Lab's menu is to be built on the next frame, while the loader works (`LabAhead`).
    lab_due: bool,
    lab_ahead: Option<Box<LabAhead>>,
}

impl Loading {
    pub fn new(shell: &mut Shell, start: MatchStart) -> Result<Loading> {
        // FMath::RandRange over the images
        let pick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| f64::from(d.subsec_nanos() % 1_000_000) / 1_000_000.0);
        let screen = match (shell.ui.as_ref(), shell.store()) {
            (Some(_), Some(store)) => match LoadingScreen::load(store, pick) {
                Ok(s) => Some(s),
                Err(e) => {
                    eprintln!("mvs: loading screen: {e}");
                    None
                }
            },
            _ => None,
        };
        let pending = PrepareJob::of(shell, &start, false).map(Pending::start);
        let lab_due = pending.is_some() && shell.ui.is_some() && matches!(start.kind, MatchKind::Lab { carry: None });
        Ok(Loading { start: Some(start), pending, shown: None, screen, lab_due, lab_ahead: None })
    }

    /// One frame of the screen; `Go::Match` once the match's load has ended. A scripted run waits for it on its
    /// second frame, so its frames do not depend on the load's time.
    pub fn frame(&mut self, shell: &mut Shell, _f: &Frame) -> Result<Go> {
        let Some(shown) = self.shown else {
            self.shown = Some(Instant::now());
            self.draw(shell, 0.0)?;
            return Ok(Go::Stay);
        };
        if std::mem::take(&mut self.lab_due)
            && let Some(start) = &self.start
        {
            let name = start.usernames.first().map_or("Player", String::as_str);
            // (a Lab's World starts with SameAttackPenalty on: World::new's; a match whose is not makes its menu anew)
            match LabAhead::build(shell, &start.setup, name, true) {
                Ok(a) => self.lab_ahead = Some(Box::new(a)),
                Err(e) => eprintln!("mvs: the Lab's menu ahead of its match: {e}"),
            }
        }
        let ready = if self.pending.is_none() {
            Some(None)
        } else if shell.waits_loads() {
            self.pending.take().map(Pending::wait)
        } else {
            self.pending.as_mut().and_then(Pending::take)
        };
        let Some(prepared) = ready else {
            self.draw(shell, shown.elapsed().as_secs_f64())?;
            return Ok(Go::Stay);
        };
        let mut start = self.start.take().ok_or_else(|| Error::Match("the loading screen ran twice".into()))?;
        start.prepared = prepared;
        start.lab_ahead = self.lab_ahead.take();
        Ok(Go::Match(Box::new(start)))
    }

    /// The screen at `time` seconds after its first frame, presented.
    fn draw(&self, shell: &mut Shell, time: f64) -> Result<()> {
        let renderer = shell.renderer.as_ref().ok_or_else(|| Error::Match("no store is open".into()))?;
        let back = shell.swap.back_buffer();
        if let Some(rtv) = &back.rtv {
            // SAFETY: a render target view of the swap chain's back buffer, on this device's context.
            unsafe { renderer.gpu().ctx.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]) };
        }
        if let Some(u) = shell.ui.as_mut() {
            u.list.clear();
            if let Some(s) = &self.screen {
                let image = s.image().and_then(|p| match renderer.assets().texture(p) {
                    Ok(t) => Some([f64::from(t.desc.size[0]), f64::from(t.desc.size[1])]),
                    Err(e) => {
                        eprintln!("mvs: loading screen: image {p}: {e}");
                        None
                    }
                });
                s.paint(shell.swap.size, image, time, &mut u.list);
            }
            // `stat fps`'s line (Show Framerate) and the ping line of an online session, as on every screen
            let fps = crate::fps::line(shell.settings.graphics.show_fps, shell.perf.readout.as_ref());
            let ping = crate::fps::ping_line(shell.online.as_ref().and_then(|h| h.borrow().ping_ms()));
            let size = shell.swap.size;
            crate::fps::draw(&mut u.list, &mut u.menu_fonts, size, crate::fps::LAYER, fps.as_ref(), ping.as_ref());
            if !u.list.elements.is_empty() {
                u.ui.draw(renderer.gpu(), renderer.assets(), &mut u.menu_fonts, &u.list, back)?;
            }
            u.list.clear();
        }
        shell.script_frame(false)?;
        shell.swap.present(true)?;
        Ok(())
    }
}
