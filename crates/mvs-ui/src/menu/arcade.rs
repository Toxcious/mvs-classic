//! Arcade (the build's "Arcade (Classic)"): one fighter against a ladder
//! of bots, run on this machine. The build's client builds the run (AXFunctions Create Arcade Run Round Data,
//! PGI_CoOpVsAIManager GetArcadeBotLoadouts) and its backend kept the round and the continues (the native
//! CoopVsAIManager fields are copied from backend messages only); the port keeps both here. Offline and solo only.
//!
//! The run lives in the menus' session (`Session::arcade`), so the screens and the program's match loop read the
//! same one; the local record is the stats file's (`StatsFile::arcade`).

use mvs_settings::HydraSettings;
use mvs_settings::stats::{StatsArcade, StatsFile};
use mvs_sim::ai::expert::EXPERT_LEVEL;
use mvs_sim::rng::Rng;
use mvs_sim::setup::{ArcadeRound, MatchPlayer, Rules, TeamStyle, perk_list};
use mvs_sim::{MatchSetup, Real};

use crate::bundle::{FighterInfo, MenuData};
use crate::menu::lobby::loadout::{
    bot_mastery_level, default_skin, local_banner, local_ring_out, picked_skin, ported_setup, random_skin,
    simulate_perk_unlock, with_local_loadout,
};
use crate::menu::lobby::{LobbyMatch, PreMatchPick};
use crate::menu::screens::pre_match::setup::{PreMatchPlayer, PreMatchSetup};

// ---------- the ladders (AXFunctions) ----------

/// EArcadeDifficulty: 1 Easy, 2 Medium (the cards' "Normal"), 3 Hard; [`ARCADE_EXPERT`] the port's own.
pub type ArcadeDifficulty = u8;

/// (the port's own) Expert, above Hard: the Hard run (its ladder, rounds, continues and the round buffs the match
/// gives) with every round's bot the Expert bot. The functions below that switch on the difficulty read it as Hard
/// (their last arm); `arcade_ladder` names the build's difficulty it stands for.
pub const ARCADE_EXPERT: ArcadeDifficulty = 4;

/// The build's EArcadeDifficulty a run plays the ladder of: Expert's is Hard's.
pub fn arcade_ladder(difficulty: ArcadeDifficulty) -> ArcadeDifficulty {
    if difficulty == ARCADE_EXPERT { 3 } else { difficulty }
}

/// ArcadeGetRoundType @0-531: the round types by 1-based round (0 Invalid); a round past the array is 0.
/// AXRoundType: 1 Regular, 2 Rivals, 3 Mini (in no ladder), 4 Reign Dog, 5 Giant Gizmo.
pub fn arcade_round_type(difficulty: ArcadeDifficulty, round: i32) -> u8 {
    let types: &[u8] = match difficulty {
        1 => &[0, 1, 1, 1],
        2 => &[0, 1, 1, 5, 2, 4],
        _ => &[0, 1, 1, 1, 5, 1, 1, 2, 4],
    };
    usize::try_from(round).ok().and_then(|r| types.get(r)).copied().unwrap_or(0)
}

/// GetNumArcadeLevels @119: Easy 3, Normal 5, Hard 8.
pub fn num_arcade_levels(difficulty: ArcadeDifficulty) -> i32 {
    match difficulty {
        1 => 3,
        2 => 5,
        _ => 8,
    }
}

/// GetNumArcadeContinues @119: Easy -1 (unlimited), Normal 3, Hard 1.
pub fn num_arcade_continues(difficulty: ArcadeDifficulty) -> i32 {
    match difficulty {
        1 => -1,
        2 => 3,
        _ => 1,
    }
}

/// IsArcadeRunFinished @41: Round >= GetNumArcadeLevels.
pub fn is_arcade_run_finished(difficulty: ArcadeDifficulty, round: i32) -> bool {
    round >= num_arcade_levels(difficulty)
}

/// ChooseArcadeBotDifficulty @0-911, an EBotBehavior (2 VeryEasy .. 5 Hard): round types 2-5 Hard (@891); Regular
/// and Invalid by the select @651: Easy VeryEasy; Normal Easy before round 2, Medium before 4, else Hard; Hard Medium
/// before round 2, else Hard.
pub fn choose_arcade_bot_difficulty(difficulty: ArcadeDifficulty, round: i32) -> i32 {
    if arcade_round_type(difficulty, round) >= 2 {
        return 5;
    }
    match difficulty {
        1 => 2,
        2 if round < 2 => 3,
        2 if round < 4 => 4,
        2 => 5,
        _ if round < 2 => 4,
        _ => 5,
    }
}

/// The port's bot level (MatchPlayer.difficulty 0..3 = EBotBehavior 2..5); an Expert run's every round the Expert
/// level.
pub fn arcade_bot_level(difficulty: ArcadeDifficulty, round: i32) -> i32 {
    if difficulty == ARCADE_EXPERT {
        return EXPERT_LEVEL;
    }
    choose_arcade_bot_difficulty(difficulty, round) - 2
}

// ---------- Create Arcade Run Round Data's constants ----------

/// L_BotFighterPool @47 (22 pawn classes), as the port's character keys, in the array's order: Pawn_Arya,
/// Pawn_BatmanV2, Pawn_BugsBunny, Pawn_Finn, Pawn_Garnet, Pawn_Harley, Pawn_Jake, Pawn_Reindog, Pawn_Shaggy,
/// Pawn_StevenUniverse, Pawn_Superman, Pawn_TomAndJerry, Pawn_Velma, Pawn_WonderWomanV3, Pawn_C015, Pawn_C020,
/// Pawn_C017, Pawn_C016, Pawn_C019, Pawn_C023A, Pawn_C023B, Pawn_C021.
pub const ARCADE_BOT_POOL: [&str; 22] = [
    "arya",
    "batman",
    "bugs",
    "finn",
    "garnet",
    "harley",
    "jake",
    "creature",
    "shaggy",
    "steven",
    "superman",
    "tomandjerry",
    "velma",
    "wonderwoman",
    "taz",
    "rick",
    "irongiant",
    "lebron",
    "morty",
    "gizmo",
    "stripe",
    "blackadam",
];
/// Pawn_Reindog (the Reign Dog and Mini rounds, the rival fallback @4874).
const REINDOG: &str = "creature";
/// Pawn_C023A (Giant Gizmo @7498).
const GIZMO: &str = "gizmo";
/// L_RandomMaps2v2 @1792 (MapData slugs). A solo run (NumBots 1) draws from this list and reads ArcadeMapSlug2v2
/// (@5158-5777: the select's true arm is the 2v2 one), as the bytecode has it. L_RandomMaps1v1 @1682
/// [map_trophy_room_large_platform, map_m009_1v1, map_m011_small, map_m008] is the two-bot run's and no solo run
/// reads it.
const RANDOM_MAPS_2V2: [&str; 4] = ["map_trophy_room_large_platform", "map_m009", "map_m011", "map_m008"];
/// The boss rounds' map (@3931, @4213, @7730).
const BOSS_MAP: &str = "map_trophy_room_large_platform";

// ---------- the run ----------

/// One round of a run, all drawn at the run's start (a retry replays the same plan).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArcadeRoundPlan {
    /// AXRoundType 1..5.
    pub kind: u8,
    /// The bot's port character key, its SkinData name and its perks (four slots, '' an empty one).
    pub bot_character: String,
    pub bot_skin: String,
    pub bot_perks: Vec<String>,
    /// The name the screens draw (GetArcadeBotLoadouts @4227-5216; the literals come from the player's import,
    /// MenuData::arcade_names).
    pub bot_name: String,
    /// The port's bot level 0..3 (MatchPlayer.difficulty: EBotBehavior - 2), or the Expert level.
    pub level: i32,
    /// The round's MapData asset name ('' none).
    pub map: String,
}

/// The run every arcade screen reads (it stands in for the build's context-2 multiqueue config and the backend's
/// CoopVsAIManager fields; AXFunctions IsArcadeModeActive, GetArcadeInfo).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArcadeRun {
    /// 1 Easy, 2 Normal, 3 Hard, ARCADE_EXPERT.
    pub difficulty: ArcadeDifficulty,
    /// The run's one fighter (ArcadeLockedCharacter): its port key and CharacterData slug; None until the first lock.
    pub character: Option<String>,
    pub character_slug: Option<String>,
    /// The locked fighter's ArcadeRivalSlug, set at the first lock.
    pub rival_slug: Option<String>,
    /// The seed the run's draws come from.
    pub seed: u32,
    pub rounds: Vec<ArcadeRoundPlan>,
    /// 1-based.
    pub round: i32,
    /// Continues left; -1 unlimited (GetNumArcadeContinues).
    pub continues: i32,
    /// The pre-match opens as a rematch: from round 2 on, and on a retry (MatchmakingHandler bIsRematch).
    pub rematch: bool,
    /// The last round's result (None before the first).
    pub last_won: Option<bool>,
    /// The run has ended (won or out of continues).
    pub over: bool,
}

impl ArcadeRun {
    /// The current round's plan (None: no rounds before the first lock).
    pub fn plan(&self) -> Option<&ArcadeRoundPlan> {
        usize::try_from(self.round - 1).ok().and_then(|i| self.rounds.get(i))
    }
}

/// The account id of the run's two players on the pre-match screen.
pub const ME_ID: &str = "local";
pub const BOT_ID: &str = "arcade-bot";

/// (the port's own) An arcade round's rules: 3 ring-outs to win, 420 s.
/// Deviation: the build's arcade match takes NumRingoutsForWin and MatchDuration from the backend's match_config
/// alone (CreateMatchConfigObject leaves them at the constructor's -1 and no client path sets them for context 2), so
/// the build has no value for them.
pub const ARCADE_RULES: Rules = Rules { knockouts_to_win: 3.0, duration: 420.0, stocks: false };

/// The run and what the screens keep of a round.
#[derive(Debug, Default)]
pub struct ArcadeState {
    pub run: Option<ArcadeRun>,
    /// The run as the pre-match started its match (round and continues before the round's result): the end of game's
    /// widgets read GetArcadeInfo before the backend advances the run (UI_ArcadeEOGContinue Set Button Text And
    /// Visibility @0, Update Retries Text @75, UI_AXProgressStage UpdateRoundNumber), and the program applies the
    /// result at the match's end. None: none since the last pre-match opened.
    pub played: Option<ArcadeRun>,
    /// The difficulty of a new run asked for (the end of a run's PLAY AGAIN: arcadeStart(difficulty)); taken by the
    /// program at the results' end.
    pub restart: Option<ArcadeDifficulty>,
    /// The local player's skin and perks as the pre-match locked them (arcadeLock; arcadePicks).
    locked_skin: String,
    human_perks: Vec<Option<String>>,
}

impl ArcadeState {
    /// SetupArcadeRun (PGI_CoOpVsAIManager @6693-6900): the difficulty, round 1, GetNumArcadeContinues. A new run:
    /// not a rematch, no fighter yet; the rounds are drawn at the first lock (they need the fighter's rival), from
    /// `seed` (the caller's random u32).
    pub fn start(&mut self, difficulty: ArcadeDifficulty, seed: u32) {
        self.run = Some(ArcadeRun {
            difficulty,
            seed,
            round: 1,
            continues: num_arcade_continues(difficulty),
            ..ArcadeRun::default()
        });
        self.human_perks.clear();
    }

    /// The pre-match's lock (UI_PreMatch state 4: ArcadeSetLockedCharacter @43743, SetupArcadeRivals @43311 with the
    /// fighter's ArcadeRivalSlug), each round: the first sets the run's fighter, its slug and rival and draws the
    /// rounds (PGI_MatchmakingHandler RecreateArcadeRun -> SetupArcadeRun, UI_PreMatch @35470; a rematch keeps the
    /// run as drawn); every lock sets the skin.
    pub fn lock(&mut self, menu: &MenuData, character: &str, skin: &str) {
        let Some(run) = self.run.as_mut() else { return };
        self.locked_skin = skin.to_string();
        if run.character.is_some() {
            return;
        }
        let slug =
            menu.fighters.get(character).map(|f| f.slug.clone()).or_else(|| {
                menu.roster.iter().find(|r| r.fighter.as_deref() == Some(character)).map(|r| r.slug.clone())
            });
        run.character = Some(character.to_string());
        run.rival_slug = slug
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| fighter_of_slug(menu, s))
            .map(|f| f.arcade.rival.clone())
            .filter(|r| !r.is_empty());
        run.character_slug = slug;
        if run.rounds.is_empty() {
            run.rounds =
                generate_arcade_rounds(run.difficulty, run.rival_slug.as_deref().unwrap_or(""), run.seed, menu);
        }
    }

    /// The round's end (the port's advance rule, the stand-in for the backend's StartNextArcadeMatch): a win goes to
    /// the next round, or completes the run on the last (IsArcadeRunFinished); a loss replays the same round and
    /// spends a continue unless they are unlimited (-1), and with none left ends the run. UI_ArcadeEOGContinue's
    /// Series Ended (@128-486) is `over` after this. A run won saves the record (the build's backend wrote it:
    /// Comp_Actor_EndOfGameHandler HandleRewards @18309-20089, arcade_completion_1p and stat tracker 17 on Hard).
    pub fn round_result(&mut self, won: bool, stats: &mut StatsFile) {
        let Some(run) = self.run.as_mut().filter(|r| !r.over) else { return };
        run.last_won = Some(won);
        if won {
            if is_arcade_run_finished(run.difficulty, run.round) {
                run.over = true;
                save_completion(run, stats);
            } else {
                run.round += 1;
            }
        } else if run.continues == 0 {
            run.over = true;
        } else if run.continues > 0 {
            run.continues -= 1;
        }
        run.rematch = !run.over;
    }

    /// Leaving the run (the pre-match's Leave Arcade, UI_PreMatch OnArcadeQuitConfirmed @121252; End Series; a match
    /// left): nothing is saved. A new run asked for and not yet started goes too (End Series starts it
    /// at once, so leaving after it drops that run).
    pub fn leave(&mut self) {
        self.run = None;
        self.restart = None;
        self.human_perks.clear();
    }

    /// The pre-match screen's picks at its start (BPI_CleanUpAndStartMatch): the local player's perks and skin for
    /// the round's match.
    pub fn picks(&mut self, menu: &MenuData, picks: &[PreMatchPick]) {
        let Some(mine) = picks.iter().find(|p| p.account_id == ME_ID) else { return };
        self.human_perks.clone_from(&mine.perks);
        self.locked_skin.clone_from(&mine.skin);
        if self.run.as_ref().is_some_and(|r| r.character.is_none()) {
            self.lock(menu, &mine.character, &mine.skin);
        }
    }

    /// The pre-match screen's set-up for the run's next round: the local player (`username`), and the round's bot
    /// once the rounds are drawn (a rematch; on the first entry the bot is known after the lock: the screen reads the
    /// session's run then). The map and level are the round's (MapData.Level), None / '' before the rounds are
    /// drawn. None: no run in progress.
    pub fn pre_match_setup(&self, menu: &MenuData, username: &str) -> Option<PreMatchSetup> {
        let run = self.run.as_ref().filter(|r| !r.over)?;
        let plan = run.plan();
        let mut players = vec![PreMatchPlayer {
            account_id: ME_ID.to_string(),
            username: username.to_string(),
            team: 0,
            is_bot: false,
            character: None,
            skin: String::new(),
            cosmetics: None,
            net_cosmetics: None,
        }];
        if let Some(p) = plan {
            players.push(PreMatchPlayer {
                account_id: BOT_ID.to_string(),
                username: p.bot_name.clone(),
                team: 1,
                is_bot: true,
                character: Some(p.bot_character.clone()),
                skin: p.bot_skin.clone(),
                cosmetics: None,
                net_cosmetics: None,
            });
        }
        let map = plan.map(|p| p.map.clone()).filter(|m| !m.is_empty());
        let level = map.as_ref().and_then(|m| menu.maps.get(m)).map(|m| m.level.clone()).unwrap_or_default();
        Some(PreMatchSetup {
            players,
            me: 0,
            team_style: 0,
            leader: true,
            map,
            level,
            set_score: None,
            arcade: Some(run.clone()),
            net: None,
            solo: None,
        })
    }

    /// The current round's match (a local one, as the Lab's and Local Play's): a 1v1, the local player on team 0
    /// with the run's fighter, the round's bot on team 1 (its level, character, skin and perks), the round's map,
    /// ARCADE_RULES and MatchSetup.arcade {difficulty, stage}. The names are the run's: the bot keeps its plan's.
    /// `drawable`: a character the port can draw. None: no round to play.
    pub fn arcade_match(
        &self,
        menu: &MenuData,
        hydra: &HydraSettings,
        input: &mvs_settings::InputSettings,
        drawable: &dyn Fn(&str) -> bool,
        username: &str,
    ) -> Option<LobbyMatch> {
        let run = self.run.as_ref()?;
        let plan = run.plan()?;
        let fighter = |k: &str| menu.fighters.get(k);
        let skin_of = |k: &str, name: &str| Some(picked_skin(fighter(k), name)).filter(|s| !s.is_empty());
        let human_perks: Vec<String> = self.human_perks.iter().map(|p| p.clone().unwrap_or_default()).collect();
        let human = MatchPlayer {
            bot: false,
            team: Some(0),
            character: run.character.clone(),
            skin: run.character.as_deref().and_then(|c| skin_of(c, &self.locked_skin)),
            perks: perk_list(&human_perks),
            ..MatchPlayer::default()
        };
        let bot = MatchPlayer {
            bot: true,
            team: Some(1),
            difficulty: Some(mvs_sim::real(plan.level)),
            character: Some(plan.bot_character.clone()),
            skin: skin_of(&plan.bot_character, &plan.bot_skin),
            perks: perk_list(&plan.bot_perks),
            ..MatchPlayer::default()
        };
        let mut setup = MatchSetup {
            team_style: TeamStyle::OneVsOne,
            players: vec![human, bot],
            rules: Some(ARCADE_RULES),
            flow: true,
            // (the match's round buffs are the ladder's: an Expert run's Hard's)
            arcade: Some(ArcadeRound {
                difficulty: arcade_ladder(run.difficulty),
                stage: u8::try_from(run.round).unwrap_or(u8::MAX),
            }),
            ..MatchSetup::default()
        };
        // (the local player's taunts, ring-out and banner; the bot the character's taunts and no ring-out or banner:
        // the build's offline modes give a bot none)
        ported_setup(&mut setup, drawable);
        let (ring_out, banner) = (local_ring_out(hydra), local_banner(hydra));
        with_local_loadout(
            &mut setup,
            hydra,
            &menu.roster,
            &mut |p, _| if p.bot { String::new() } else { ring_out.clone() },
            &mut |p, _| if p.bot { String::new() } else { banner.clone() },
            &mut |p, _| (!p.bot).then(|| crate::menu::prefs::sim_input_prefs(input, 0)),
        );
        Some(LobbyMatch {
            setup,
            me: 0,
            usernames: vec![username.to_string(), plan.bot_name.clone()],
            map: Some(plan.map.clone()).filter(|m| !m.is_empty()),
            humans: None,
            lobby: None,
        })
    }

    // ---------- the round on the screens ----------

    /// keepArcadeRound: the pre-match starts the round's match.
    pub fn keep_round(&mut self) {
        self.played.clone_from(&self.run);
    }

    /// dropArcadeRound: a pre-match of the run opens, the screens read the run itself again.
    pub fn drop_round(&mut self) {
        self.played = None;
    }

    /// arcadeRoundPlayed: the round played while a run is on (None: none, or the run was left since).
    pub fn round_played(&self) -> Option<&ArcadeRun> {
        if self.run.is_some() { self.played.as_ref() } else { None }
    }

    /// arcadeInfo (AXFunctions GetArcadeInfo: the round, the continues, the difficulty) as a screen reads it: the
    /// round played, from the match's start to the next pre-match, else the run.
    pub fn info(&self) -> Option<&ArcadeRun> {
        self.round_played().or(self.run.as_ref())
    }

    /// A run in progress (IsArcadeModeActive with a run not over).
    pub fn live(&self) -> Option<&ArcadeRun> {
        self.run.as_ref().filter(|r| !r.over)
    }
}

/// A run won: the fighter's best difficulty (arcade_completion_1p, kept as a maximum: IsMax) and, on Hard, one more
/// TotalArcadeHardCompletions. (A record that cannot be read is not written over: `set_arcade` keeps the file's.)
/// An Expert run is recorded as the Hard run it plays (the record holds the build's difficulties only).
fn save_completion(run: &ArcadeRun, stats: &mut StatsFile) {
    let mut record: StatsArcade = stats.arcade();
    let difficulty = arcade_ladder(run.difficulty);
    if let Some(slug) = run.character_slug.as_deref().filter(|s| !s.is_empty()) {
        let best = record.stars.entry(slug.to_string()).or_insert(0);
        *best = (*best).max(difficulty);
    }
    if difficulty == 3 {
        record.hard_completions += 1;
    }
    if !stats.set_arcade(record) {
        eprintln!("mvs: arcade record: not saved");
    }
}

// ---------- the sources ----------

/// The fighter of a CharacterData slug, compared without case.
fn fighter_of_slug<'a>(menu: &'a MenuData, slug: &str) -> Option<&'a FighterInfo> {
    menu.fighters.values().find(|f| f.slug.eq_ignore_ascii_case(slug))
}

/// A CharacterData slug's character key, compared without case (GlobalCharacterData's FString key): the fighters'
/// slugs, then the roster's.
fn key_of_slug(menu: &MenuData, slug: &str) -> Option<String> {
    if slug.is_empty() {
        return None;
    }
    let by_fighter = menu.fighters.iter().find(|(_, f)| !f.slug.is_empty() && f.slug.eq_ignore_ascii_case(slug));
    by_fighter.map(|(k, _)| k.clone()).or_else(|| {
        menu.roster
            .iter()
            .find(|r| !r.slug.is_empty() && r.slug.eq_ignore_ascii_case(slug))
            .and_then(|r| r.fighter.clone())
    })
}

/// A MapData slug's MapData asset name (MapData.Slug, as LocalPlayFacilitator's GetMapsFromSlugs reads it).
fn map_of_slug(menu: &MenuData, slug: &str) -> Option<String> {
    menu.maps.iter().find(|(_, m)| m.slug.eq_ignore_ascii_case(slug)).map(|(k, _)| k.clone())
}

// ---------- the generator ----------

/// `rng.int(n)` as an index.
fn index(rng: &mut Rng, n: usize) -> usize {
    // (a draw in [0, n) of a small count)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let i = rng.int(n as Real) as usize;
    i
}

/// One round as Create Arcade Run Round Data leaves it (AXRoundInfo): its type, its bots' characters and its map
/// slug.
struct RoundInfo {
    kind: u8,
    bots: Vec<String>,
    map_slug: String,
}

/// AXFunctions Create Arcade Run Round Data (@0-7855) and PGI_CoOpVsAIManager GetArcadeBotLoadouts (@768-5216) for
/// every round (the PGI_CoOpVsAIManager matchmaking loop @1055-2698), solo: one bot a round (ChooseArcadeGameMode
/// gives 0 with one party member, and GameMode 0 is one bot, @7393). `rival`: the locked fighter's ArcadeRivalSlug
/// ('' unknown).
/// Deviation: the build draws from FMath's global stream; the port from one Rng of the run's seed, in the build's
/// order: for rounds 1..N the Regular pick, the map draw and the character draw (Create Arcade Run Round Data @6021,
/// @5245, @5348), then for rounds 1..N each bot's mastery and skin (GetArcadeBotLoadouts @768
/// GetRandomMasteryLevelForBot, @1791 UI_Functions RandomSkin, called for every round type).
pub fn generate_arcade_rounds(
    difficulty: ArcadeDifficulty,
    rival: &str,
    seed: u32,
    menu: &MenuData,
) -> Vec<ArcadeRoundPlan> {
    // (the seed is a uint32, read as its i32 bits)
    let mut rng = Rng::new(seed.cast_signed());
    // L_AvailableBots = L_BotFighterPool (@1897) is an array copy made before the pool's removals (the Easy exclusion
    // @1975, which passes the set as the list to remove from; each rival @2845; Reindog and C023A @3089 / @3238), so
    // the Regular picks draw from all 22. The pool is read again only by the refill (@6016) when the copy is empty,
    // which a solo run's at most 6 Regular picks never reach.
    let mut available: Vec<&str> = ARCADE_BOT_POOL.to_vec();
    let mut infos = Vec::new();
    for round in 1..=num_arcade_levels(difficulty) {
        let kind = arcade_round_type(difficulty, round);
        let mut bots: Vec<String> = Vec::new();
        let mut map_slug = String::new();
        match kind {
            3 | 4 => {
                bots.push(REINDOG.to_string());
                map_slug = BOSS_MAP.to_string();
            }
            5 => {
                bots.push(GIZMO.to_string());
                map_slug = BOSS_MAP.to_string();
            }
            1 | 2 => {
                if kind == 1 {
                    // Array_Random(L_AvailableBots) and RemoveItem (@6021-6107): a Regular opponent never repeats. A
                    // pick not in CharacterDataByFighter gives Pawn_Shaggy (@6453-6586); every class of the pool is
                    // a character the port has.
                    let i = index(&mut rng, available.len());
                    bots.push(available.remove(i).to_string());
                } else {
                    // GetCharacterDataFromSlug(rival).BaseCharacter, or Reindog when it is not valid (@4586-4961)
                    bots.push(key_of_slug(menu, rival).unwrap_or_else(|| REINDOG.to_string()));
                }
                // the map (@5104-5777): m from the random list (drawn even when the character's field is used), c a
                // bot of the round, and c's ArcadeMapSlug2v2 when it is not empty, else m
                let m = RANDOM_MAPS_2V2[index(&mut rng, RANDOM_MAPS_2V2.len())];
                let c = &bots[index(&mut rng, bots.len())];
                let own = menu.fighters.get(c).filter(|f| !f.slug.is_empty()).map(|f| f.arcade.map2v2.as_str());
                map_slug = own.filter(|s| !s.is_empty()).unwrap_or(m).to_string();
            }
            _ => {}
        }
        infos.push(RoundInfo { kind, bots, map_slug });
    }
    let names = &menu.arcade_names;
    infos
        .iter()
        .zip(1..)
        .map(|(info, round)| {
            let mut chosen: Vec<String> = Vec::new();
            let plans: Vec<(String, String, Vec<String>, String)> = info
                .bots
                .iter()
                .map(|key| {
                    let fi = menu.fighters.get(key);
                    // GetRandomMasteryLevelForBot, then SimulatePerkUnlockSystem over GetRecommendedPerks
                    // (GetArcadeBotLoadouts @768-1260)
                    let mastery = bot_mastery_level(&mut |n| {
                        // (a small count)
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
                        let i = index(&mut rng, n as usize) as i32;
                        i
                    });
                    let perks = simulate_perk_unlock(menu, key, mastery);
                    // the skin (GetArcadeBotLoadouts @3289): RandomSkin (@1791, drawn for every round type), or the
                    // default when it is not valid; the boss rounds take the CDO's ReindogBossSkin / GizmoBossSkin
                    // instead.
                    // Unverified: before the names are bound the boss plays its default skin.
                    let drawn = random_skin(fi, &mut || f64::from(rng.next())).map(|s| s.name.clone());
                    let skin = match info.kind {
                        4 => names.reindog_boss_skin.clone(),
                        5 => names.gizmo_boss_skin.clone(),
                        _ => drawn.or_else(|| default_skin(fi).map(|s| s.name.clone())).unwrap_or_default(),
                    };
                    // the name (GetArcadeBotLoadouts @4227-5216): DisplayName (the "Bot" text when it is empty),
                    // "Nemesis " + it for a rival, the literal of a boss round; a name taken this round gets " 2", or
                    // " 3" when " 2" is taken too (@4498-4793)
                    let display = fi.map(|f| f.display_name.as_str()).filter(|d| !d.is_empty());
                    let display = display.unwrap_or(&names.empty_name).to_string();
                    let mut name = match info.kind {
                        2 => format!("{}{display}", names.nemesis_prefix),
                        3 => names.mini.clone(),
                        4 => names.reindog.clone(),
                        5 => names.giant_gizmo.clone(),
                        _ => display,
                    };
                    if chosen.contains(&name) {
                        let two = format!("{name} 2");
                        name = if chosen.contains(&two) { format!("{name} 3") } else { two };
                    }
                    chosen.push(name.clone());
                    (key.clone(), skin, perks, name)
                })
                .collect();
            let bot = plans.into_iter().next();
            ArcadeRoundPlan {
                kind: if info.kind == 0 { 1 } else { info.kind },
                bot_character: bot.as_ref().map_or_else(|| REINDOG.to_string(), |b| b.0.clone()),
                bot_skin: bot.as_ref().map(|b| b.1.clone()).unwrap_or_default(),
                bot_perks: bot.as_ref().map_or_else(|| vec![String::new(); 4], |b| b.2.clone()),
                bot_name: bot.as_ref().map(|b| b.3.clone()).unwrap_or_default(),
                level: arcade_bot_level(difficulty, round),
                map: map_of_slug(menu, &info.map_slug).unwrap_or_default(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dev store's menu data (None when it is not there).
    fn menu() -> Option<MenuData> {
        let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
        crate::Bundle::load(&store, "menu").ok()?.menu
    }

    #[test]
    fn the_menu_data_has_the_catalogue_and_the_arcade_names() {
        let Some(m) = menu() else { return };
        assert!(!m.cosmetics.stat_tracker.is_empty(), "no stat trackers in the cosmetics catalogue");
        assert!(!m.cosmetics.ringout.is_empty() && !m.cosmetics.banner.is_empty());
        assert!(!m.account_levels.is_empty(), "no account levels");
        let n = &m.arcade_names;
        assert!(!n.nemesis_prefix.is_empty() && !n.reindog.is_empty() && !n.giant_gizmo.is_empty(), "{n:?}");
    }

    #[test]
    fn an_expert_run_is_the_hard_ladder_with_the_expert_bot() {
        assert_eq!(arcade_ladder(ARCADE_EXPERT), 3);
        assert_eq!(arcade_ladder(2), 2);
        assert_eq!(num_arcade_levels(ARCADE_EXPERT), num_arcade_levels(3));
        assert_eq!(num_arcade_continues(ARCADE_EXPERT), num_arcade_continues(3));
        for round in 1..=8 {
            assert_eq!(arcade_round_type(ARCADE_EXPERT, round), arcade_round_type(3, round));
            assert_eq!(arcade_bot_level(ARCADE_EXPERT, round), EXPERT_LEVEL);
            assert_eq!(arcade_bot_level(3, round), choose_arcade_bot_difficulty(3, round) - 2);
        }
    }

    #[test]
    fn a_hard_run_has_its_ladder() {
        let Some(m) = menu() else { return };
        let rounds = generate_arcade_rounds(3, "", 12345, &m);
        assert_eq!(rounds.len(), 8);
        let kinds: Vec<u8> = rounds.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [1, 1, 1, 5, 1, 1, 2, 4]);
        assert_eq!(rounds[3].bot_character, GIZMO);
        assert_eq!(rounds[7].bot_character, REINDOG);
        // (no rival: Reindog)
        assert_eq!(rounds[6].bot_character, REINDOG);
        assert_eq!(rounds[7].bot_name, m.arcade_names.reindog);
        assert_eq!(rounds[7].bot_skin, m.arcade_names.reindog_boss_skin);
        assert!(rounds.iter().all(|r| !r.map.is_empty()), "{rounds:?}");
        let levels: Vec<i32> = rounds.iter().map(|r| r.level).collect();
        assert_eq!(levels, [2, 3, 3, 3, 3, 3, 3, 3]);
        // the Regular picks never repeat
        let regular: Vec<&str> = rounds.iter().filter(|r| r.kind == 1).map(|r| r.bot_character.as_str()).collect();
        let mut sorted = regular.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), regular.len());
        // the same seed, the same run
        assert_eq!(generate_arcade_rounds(3, "", 12345, &m), rounds);
    }

    #[test]
    fn a_round_lost_spends_a_continue_and_the_last_won_ends_the_run() {
        let mut s = ArcadeState::default();
        let mut stats = StatsFile::memory();
        s.start(2, 1);
        assert_eq!(s.run.as_ref().map(|r| r.continues), Some(3));
        s.round_result(false, &mut stats);
        let r = s.run.as_ref().unwrap();
        assert_eq!((r.round, r.continues, r.rematch, r.over), (1, 2, true, false));
        s.round_result(true, &mut stats);
        assert_eq!(s.run.as_ref().map(|r| r.round), Some(2));
        // Hard: one continue, then out
        s.start(3, 1);
        s.round_result(false, &mut stats);
        s.round_result(false, &mut stats);
        let r = s.run.as_ref().unwrap();
        assert!(r.over && !r.rematch);
        assert!(s.live().is_none());
        // Easy: unlimited
        s.start(1, 1);
        for _ in 0..5 {
            s.round_result(false, &mut stats);
        }
        assert_eq!(s.run.as_ref().map(|r| (r.continues, r.over)), Some((-1, false)));
    }

    #[test]
    fn the_screens_read_the_round_played_until_the_next_pre_match() {
        let mut s = ArcadeState::default();
        s.start(1, 1);
        s.keep_round();
        s.run.as_mut().unwrap().round = 2;
        assert_eq!(s.info().map(|r| r.round), Some(1));
        s.drop_round();
        assert_eq!(s.info().map(|r| r.round), Some(2));
        s.keep_round();
        s.leave();
        assert!(s.info().is_none() && s.round_played().is_none());
    }
}
