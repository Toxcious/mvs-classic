//! The bot log of a trace's match (`sim-trace --bot-log`): the World stepped from the header as a plain run steps
//! it, each bot slot on its controller's think, with the sim's bot log on (`mvs_sim::ai::log`), and the lines kept
//! with the frame they were made in: `<frame> <line>`. The oracle's replay (its bot-log option)
//! writes the same lines; `bot-log-diff` compares the two. The log only reads: the World steps as in a
//! plain run.

use std::fmt::Write as _;

use mvs_sim::ai::log;
use mvs_trace::Trace;

use crate::run::{caught, step_caught};
use crate::{bots_for, world_for};

/// A match's log.
pub struct BotLog {
    /// The lines, each ended by a newline.
    pub text: String,
    /// The frames stepped.
    pub ticks: usize,
    /// The thinks logged.
    pub thinks: usize,
    /// The frame whose step (or think) panicked, and the panic: the log ends there.
    pub panic: Option<(usize, String)>,
}

/// Turns the thread's log off however the run ends.
struct Off;

impl Drop for Off {
    fn drop(&mut self) {
        log::stop();
    }
}

/// The bot log of `trace`'s match, up to frame `until`.
pub fn bot_log(trace: &Trace, store: &mvs_store::Store, until: Option<usize>) -> Result<BotLog, String> {
    let n = until.map_or(trace.len(), |u| u.min(trace.len()));
    let mut out = BotLog { text: String::new(), ticks: 0, thinks: 0, panic: None };
    log::start();
    let _off = Off;
    let made = caught(|| {
        let (w, book) = world_for(&trace.header, store)?;
        let bots = bots_for(&trace.header.spec, &w);
        Ok::<_, String>((w, book, bots))
    });
    let (mut w, book, mut bots) = match made {
        Ok(r) => r?,
        Err(e) => {
            out.panic = Some((0, e));
            return Ok(out);
        }
    };
    for i in 0..n {
        let frame = trace.frame(i).map_err(|e| e.to_string())?;
        let mut inputs: Vec<_> = frame.inputs.iter().map(|r| crate::input_of(r, &w, &book)).collect();
        let mut stepped = caught(|| crate::page_calls(&mut w, &trace.header, i));
        for (slot, bot) in bots.iter_mut().enumerate() {
            if stepped.is_err() {
                break;
            }
            if let Some(bot) = bot {
                let me = w.players[slot];
                stepped = caught(|| bot.think(&mut w, me)).map(|input| inputs[slot] = input);
            }
        }
        let stepped = stepped.and_then(|()| step_caught(&mut w, &inputs));
        w.events.clear();
        for l in log::take() {
            out.thinks += usize::from(l.starts_with("think "));
            let _ = writeln!(out.text, "{i} {l}");
        }
        out.ticks = i + 1;
        if let Err(e) = stepped {
            out.panic = Some((i, e));
            break;
        }
    }
    Ok(out)
}
