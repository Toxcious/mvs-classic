//! The values the menuscene bind needs from Blueprint bytecode, which the object kind does not hold (it keeps a
//! Blueprint's properties, not its Kismet script): tables, each row naming the bytecode it is read from.

use mvs_material::menuscene::{EventArg, MirrorWhen};

use super::util::fail;
use crate::kind::KindError;

/// An argument of a director call as the table writes it.
#[derive(Clone, Copy, Debug)]
pub enum Arg {
    N(f64),
    B(bool),
    S(&'static str),
}

impl Arg {
    pub fn value(self) -> EventArg {
        match self {
            Arg::N(n) => EventArg::Num(n),
            Arg::B(b) => EventArg::Bool(b),
            Arg::S(s) => EventArg::Str(s.to_owned()),
        }
    }
}

/// A sequence director entry point (`SequenceDirector_C:SequenceEvent__ENTRYPOINTSequenceDirector_<n>`): its one
/// EX_LocalVirtualFunction call and the arguments it passes (`target`: its first is the event's bound "Target"
/// object). Each entry point is that call then EX_Return.
#[derive(Clone, Copy, Debug)]
pub struct DirectorCall {
    pub call: &'static str,
    pub args: &'static [Arg],
    pub target: bool,
}

const fn call(call: &'static str, args: &'static [Arg]) -> DirectorCall {
    DirectorCall { call, args, target: true }
}

use Arg::{B, N, S};

/// Entry point n of each sequence's director, from the json-bytecode dumps of the sequences (all with a Target first
/// parameter but SequenceUI_Event's). Seq_Clash and Seq_Clash_FFA_New have no event track.
pub const DIRECTOR_CALLS: &[(&str, &[DirectorCall])] = &[
    // Panda_Main/Maps/Map_PodiumStage/Seq_WinnerIntro_2v2, SequenceDirector_0..16
    (
        "Seq_WinnerIntro_2v2",
        &[
            call("ShowFighter", &[N(0.0), B(true)]),
            call("ShowFighter", &[N(1.0), B(true)]),
            call("ShowNametag", &[N(0.0), B(true)]),
            call("ShowNametag", &[N(1.0), B(true)]),
            call("PodiumEvent", &[S("StartCelebration")]),
            call("ShowFighter", &[N(0.0), B(false)]),
            call("ShowFighter", &[N(1.0), B(false)]),
            call("AnimBannerIn", &[N(0.0), B(false)]),
            call("PodiumEvent", &[S("DetermineWinningTeam")]),
            call("ZoomToWInner", &[]),
            call("AnimBannerIn", &[N(0.0), B(true)]),
            call("AnimBannerIn", &[N(1.0), B(true)]),
            call("ShowNametag", &[N(0.0), B(false)]),
            call("ShowNametag", &[N(1.0), B(false)]),
            call("AnimBannerIn", &[N(1.0), B(false)]),
            call("PodiumEvent", &[S("ShowUI")]),
            call("PodiumEvent", &[S("ShowStats")]),
        ],
    ),
    // Panda_Main/Maps/Map_PodiumStage/Seq_WinnerIntro_FFA, SequenceDirector_0..16
    (
        "Seq_WinnerIntro_FFA",
        &[
            call("ShowFighter", &[N(0.0), B(true)]),
            call("ShowNametag", &[N(0.0), B(true)]),
            call("PodiumEvent", &[S("StartCelebration")]),
            call("ShowFighter", &[N(0.0), B(false)]),
            call("ShowFighter", &[N(1.0), B(false)]),
            call("ShowFighter", &[N(2.0), B(false)]),
            call("AnimBannerIn", &[N(0.0), B(false)]),
            call("PodiumEvent", &[S("DetermineWinningTeam")]),
            call("ZoomToWInner", &[]),
            call("AnimBannerIn", &[N(0.0), B(true)]),
            call("ShowNametag", &[N(0.0), B(false)]),
            call("ShowNametag", &[N(1.0), B(false)]),
            call("ShowNametag", &[N(2.0), B(false)]),
            call("AnimBannerIn", &[N(1.0), B(false)]),
            call("PodiumEvent", &[S("ShowUI")]),
            call("AnimBannerIn", &[N(2.0), B(false)]),
            call("PodiumEvent", &[S("ShowStats")]),
        ],
    ),
    // Panda_Main/PreMatch/Seq_AnimInRed, SequenceDirector_0..3
    (
        "Seq_AnimInRed",
        &[
            call("ShowPlayer", &[N(2.0), B(true)]),
            call("ShowPlayer", &[N(1.0), B(false)]),
            DirectorCall { call: "SequenceUI_Event", args: &[S("ShowVS")], target: false },
            call("ShowPlayer", &[N(3.0), B(false)]),
        ],
    ),
    // Panda_Main/PreMatch/Seq_AnimInFFA_New, SequenceDirector_0..3
    (
        "Seq_AnimInFFA_New",
        &[
            call("ShowPlayer", &[N(2.0), B(false)]),
            call("ShowPlayer", &[N(1.0), B(false)]),
            DirectorCall { call: "SequenceUI_Event", args: &[S("ShowFFA")], target: false },
            call("ShowPlayer", &[N(3.0), B(false)]),
        ],
    ),
];

/// A director entry point's call by its function's object path (`...SequenceEvent__ENTRYPOINTSequenceDirector_<n>`).
pub fn director_call(seq: &str, f: &str) -> Result<DirectorCall, KindError> {
    const AT: &str = ":SequenceEvent__ENTRYPOINTSequenceDirector_";
    let n = f
        .rfind(AT)
        .map(|i| &f[i + AT.len()..])
        .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|d| d.parse::<usize>().ok());
    let rows = DIRECTOR_CALLS.iter().find(|(s, _)| *s == seq).map(|(_, r)| *r);
    n.and_then(|n| rows.and_then(|r| r.get(n)))
        .copied()
        .ok_or_else(|| fail(format!("{seq}: no director entry for {f} (menuscene tables.rs DIRECTOR_CALLS)")))
}

/// When a lighting rig scales its Lights component Y by -1 (BP_CharacterPreview SpawnLighting @36087 -> @31002 calls
/// the rig's AssignLightingChannels): Left when FacingLeft is true, Right when it is false. The rig's ubergraph runs
/// Lights.SetRelativeScale3D(MakeVector(1, select(FacingLeft){false: a, true: b}, 1)) with a and b float constants:
/// BP_CharacterLighting @402-558 has b = -1, the three others @402 / @425 a = -1. A rig class takes the nearest class
/// of its chain (Super) that is here (the nearest that defines the event).
pub const RIG_MIRROR: &[(&str, MirrorWhen)] = &[
    ("/Game/Panda_Main/PreMatch/CharacterLighting/BP_CharacterLighting.BP_CharacterLighting_C", MirrorWhen::Left),
    (
        "/Game/Panda_Main/PreMatch/CharacterLighting/BL_Morty/BP_CharLights_Morty.BP_CharLights_Morty_C",
        MirrorWhen::Right,
    ),
    (
        "/Game/Panda_Main/PreMatch/CharacterLighting/BL_WonderWoman/BP_CharLights_WonderWoman.BP_CharLights_WonderWoman_C",
        MirrorWhen::Right,
    ),
    (
        "/Game/Panda_Main/PreMatch/CharacterLighting/BL_WonderWoman/BP_CharLights_WonderWoman_Adjusted.BP_CharLights_WonderWoman_Adjusted_C",
        MirrorWhen::Right,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn director() {
        let p = "/Game/X.Seq_AnimInRed_DirectorBP_C:SequenceEvent__ENTRYPOINTSequenceDirector_2";
        let c = director_call("Seq_AnimInRed", p).unwrap();
        assert_eq!((c.call, c.target), ("SequenceUI_Event", false));
        assert!(director_call("Seq_AnimInRed", &p.replace("_2", "_9")).is_err());
        assert!(director_call("Seq_Clash", p).is_err());
    }
}
