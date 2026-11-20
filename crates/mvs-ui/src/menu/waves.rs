//! (the port's own) The Waves mode's menus state (`docs/waves-plan.md`): the variant and stage the player picked, the
//! run the program started, and the last run's result for the result page. Its words are in `custom_text.rs`.

use mvs_settings::stats::StatsFile;
use mvs_sim::modes::waves::WavesStatus;
use mvs_sim::setup::WavesVariant;

/// The mode's state across screens (`Session::waves`).
#[derive(Debug, Default)]
pub struct WavesState {
    /// The variant the player picked; None: the mode's card was not opened since the last run.
    pub variant: Option<WavesVariant>,
    /// The stage the player picked (a map's key, as the Lab's map pick gives it).
    pub stage: Option<String>,
    /// The last run's result, for the result page; taken by the page.
    pub last: Option<WavesResult>,
}

/// A finished run as the result page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct WavesResult {
    pub variant: WavesVariant,
    /// The player's character key.
    pub character: String,
    pub ring_outs: u32,
    /// The run's clock at its end.
    pub seconds: f64,
    /// 100 Ring-Outs reached its target.
    pub cleared: bool,
    /// The run beat the fighter's best for the variant (and was saved).
    pub new_record: bool,
    /// The fighter's best for the variant after this run: ring-outs (Endless) or seconds (100 Ring-Outs), or None.
    pub best: Option<f64>,
}

impl WavesState {
    /// The end of a run: its result, the record saved to `stats` when it is a new best, and `last` set.
    pub fn finish(&mut self, stats: &mut StatsFile, character: &str, status: &WavesStatus) -> WavesResult {
        let _ = (stats, character, status);
        unimplemented!("WavesState::finish: the menus track")
    }
}
