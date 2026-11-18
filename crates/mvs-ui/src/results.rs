//! The end-of-game data: what the match hands the results
//! screen, and the functions the end-of-game widgets compute from it (the stat sheets' rows and highlights, the
//! podium's order). The widgets themselves are `menu::widgets::end_of_game`, the screen `menu::screens::results`.

use mvs_sim::world::flow::PodiumEntry;

use crate::menu::arcade::ArcadeRun;

/// What the podium's records carry besides the PodiumData (the player's character, banner, skin,
/// id and bot), by player.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EogPlayer {
    /// The fighter's character (a roster key): the record's SelectedSkin stands behind it.
    pub character: String,
    /// The player's SkinData name (MatchPlayer.skin); "" the default.
    pub skin: String,
    /// The player's BannerData slug (MatchPlayer.banner: the podium banner's); None: none.
    pub banner: Option<String>,
    /// The player's online id ('' a bot, and every player of an offline match), the toast's target.
    pub id: String,
    /// A bot's slot.
    pub bot: bool,
    /// The name Local Play shows on the banner (localPlayUsername: the custom name, else the long player number);
    /// None: the username.
    pub local_name: Option<String>,
}

/// EndOfGameData: the match's PodiumData with what the widgets read besides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EndOfGame {
    /// The match config's team style (EGameMode as the widgets read it: 0 solo, 1 team, 2 free for all).
    pub team_style: u8,
    /// BPI_GetIsLocalPlay.
    pub local_play: bool,
    /// The experimental stock rule (the sheets show KOs in every mode).
    pub stocks: bool,
    /// The local player's index in `players` (YourMatchData).
    pub me: usize,
    /// `mvs_sim::world::flow::podium_data`, in MatchData order.
    pub players: Vec<PodiumEntry>,
    /// Each player's username (the record's User.Username), by player.
    pub names: Vec<String>,
    /// Each player's record besides the PodiumData, by player (empty: every player a default one).
    pub info: Vec<EogPlayer>,
    /// The arcade round the match was; None: no arcade run.
    pub arcade: Option<ArcadeRun>,
}

/// A podium record: the PodiumData of one player with the name the widgets read from
/// its User and its record besides.
#[derive(Clone, Debug, PartialEq)]
pub struct PodiumPlayer {
    /// The player's place in MatchData.
    pub index: usize,
    pub entry: PodiumEntry,
    pub username: String,
    pub info: EogPlayer,
}

impl EndOfGame {
    /// Player `i`'s podium record.
    pub fn player(&self, i: usize) -> PodiumPlayer {
        PodiumPlayer { index: i, entry: self.players[i], username: self.name(i).to_string(), info: self.info(i) }
    }

    /// Player `i`'s username.
    pub fn name(&self, i: usize) -> &str {
        self.names.get(i).map_or("", String::as_str)
    }

    /// Player `i`'s record besides the PodiumData.
    pub fn info(&self, i: usize) -> EogPlayer {
        self.info.get(i).cloned().unwrap_or_default()
    }
}

// The EX_TextConst keys of the Game target the stat sheets set (UI_EOG_StatSheet PopulateStats @589, @652, @1181).
pub const TEXT_SCORE: &str = "C8E6F4EF419208A09BA6B49517CC80FB";
pub const TEXT_KOS: &str = "CDAF30DF406FBD9BABE735A1DA7946A7";
pub const TEXT_DAMAGE: &str = "E2F856D74A29018E6BEF12B5865F95A0";

/// Conv_IntToText / Conv_Int64ToText(value, false, true, 1, 324): grouping on (en-US).
pub fn int_text(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// UI_EOG_StatSheet DetermineHighScores (KOs winner, damage winner): 1v1 against the other player and 2v2
/// against the teammate, a value that is not 0 and not lower wins; free for all needs a value above all three
/// others.
pub fn high_scores(eog: &EndOfGame, me: usize) -> (bool, bool) {
    let p = &eog.players[me];
    let others: Vec<&PodiumEntry> = eog.players.iter().enumerate().filter(|&(i, _)| i != me).map(|(_, o)| o).collect();
    match eog.team_style {
        0 | 1 => {
            let o = if eog.team_style == 0 {
                others.first().copied()
            } else {
                others.iter().copied().find(|o| o.team_index == p.team_index)
            };
            match o {
                Some(o) => (
                    p.ring_outs != 0 && p.ring_outs >= o.ring_outs,
                    p.damage_dealt != 0.0 && p.damage_dealt >= o.damage_dealt,
                ),
                None => (false, false),
            }
        }
        2 if others.len() == 3 => {
            let of = |x: &PodiumEntry| if eog.stocks { f64::from(x.ring_outs) } else { f64::from(x.score) };
            (others.iter().all(|o| of(p) > of(o)), others.iter().all(|o| p.damage_dealt > o.damage_dealt))
        }
        _ => (false, false),
    }
}

/// A row of a stat sheet (UI_EOG_StatInfoStruct).
#[derive(Clone, Debug, PartialEq)]
pub struct StatRow {
    /// The title's Game target key.
    pub title: &'static str,
    pub value: String,
    /// The highlight: PaletteForIndex(IsYourTeam ? 0 : 1)'s index when the row is the best, else None
    /// (transparent).
    pub highlight: Option<i32>,
}

/// UI_EOG_StatSheet PopulateStats' two rows of player `i`: KOs (Score in free for all without stocks) and Damage.
pub fn stat_rows(eog: &EndOfGame, i: usize) -> [StatRow; 2] {
    let p = &eog.players[i];
    let your_team = p.team_index == eog.players[eog.me].team_index;
    let ffa = eog.team_style == 2 && !eog.stocks;
    let (kos, damage) = high_scores(eog, i);
    // PaletteForIndex(IsYourTeam ? 0 : SelectInt(TeamIndex, 1, SpectateColors)).base; SpectateColors is false
    let colour = if your_team { 0 } else { 1 };
    // Round64 of DamageDealt: half away from zero (round half up, the same on the non-negative damage)
    #[allow(clippy::cast_possible_truncation)]
    let damage_value = p.damage_dealt.round() as i64;
    #[allow(clippy::cast_possible_truncation)]
    let first_value = if ffa { p.score as i64 } else { i64::from(p.ring_outs) };
    [
        StatRow {
            title: if ffa { TEXT_SCORE } else { TEXT_KOS },
            value: int_text(first_value),
            highlight: kos.then_some(colour),
        },
        StatRow { title: TEXT_DAMAGE, value: int_text(damage_value), highlight: damage.then_some(colour) },
    ]
}

/// The players in the podium's order (SortPodiumDataResults): the winners, then the others, each list with your
/// own record first and the rest in MatchData order.
pub fn podium_order(eog: &EndOfGame) -> Vec<usize> {
    let mut out = Vec::with_capacity(eog.players.len());
    for winners in [true, false] {
        let list = (0..eog.players.len()).filter(|&i| eog.players[i].is_winner == winners);
        let (mine, rest): (Vec<usize>, Vec<usize>) = list.partition(|&i| i == eog.me);
        out.extend(mine);
        out.extend(rest);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(i: i32, team: i32, ring_outs: i32, damage: f64, winner: bool) -> PodiumEntry {
        PodiumEntry {
            player_index: i,
            team_index: team,
            ring_outs,
            damage_dealt: mvs_sim::math::narrow(damage),
            falls: 0,
            score: mvs_sim::math::narrow(f64::from(ring_outs)),
            is_winner: winner,
            assists: 0,
            off_platform_ring_outs: 0,
            assisted: 0,
            doubles: 0,
            dodges: 0,
        }
    }

    #[test]
    fn int_text_groups() {
        assert_eq!(int_text(0), "0");
        assert_eq!(int_text(999), "999");
        assert_eq!(int_text(1000), "1,000");
        assert_eq!(int_text(-1234567), "-1,234,567");
    }

    #[test]
    fn high_scores_one_v_one() {
        let eog = EndOfGame {
            players: vec![entry(0, 0, 2, 120.4, true), entry(1, 1, 2, 80.0, false)],
            ..EndOfGame::default()
        };
        assert_eq!(high_scores(&eog, 0), (true, true));
        assert_eq!(high_scores(&eog, 1), (true, false));
        let zero = EndOfGame { players: vec![entry(0, 0, 0, 0.0, false), entry(1, 1, 0, 0.0, false)], ..eog };
        assert_eq!(high_scores(&zero, 0), (false, false));
    }

    #[test]
    fn podium_order_winners_mine_first() {
        let eog = EndOfGame {
            team_style: 2,
            me: 2,
            players: vec![
                entry(0, 0, 0, 0.0, false),
                entry(1, 1, 0, 0.0, true),
                entry(2, 2, 0, 0.0, false),
                entry(3, 3, 0, 0.0, false),
            ],
            ..EndOfGame::default()
        };
        assert_eq!(podium_order(&eog), vec![1, 2, 0, 3]);
    }
}
