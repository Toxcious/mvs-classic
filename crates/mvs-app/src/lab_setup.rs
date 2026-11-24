//! The Lab's set-up between its matches: what a restart carries over, and the config's
//! slots as a match's players.

use indexmap::IndexMap;
use mvs_settings::Settings;
use mvs_sim::MatchSetup;
use mvs_sim::setup::{MatchPlayer, TeamStyle, perk_list};
use mvs_sim::world::DEFAULT_CHARACTER;
use mvs_ui::bundle::{FighterInfo, RosterEntry, SkinInfo};
use mvs_ui::menu::online::NetPick;
use mvs_ui::menu::prefs::sim_input_prefs;
use mvs_ui::menu::training_config::{LabConfig, LabToggles};
use mvs_view::MatchCamera;

use crate::app::{MatchKind, MatchStart, Shell};
use crate::match_lab::LabMenu;

/// The match's music flow a restart goes on with (LabCarry.music).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabMusic {
    pub t: f64,
    pub state: String,
    pub timer: f64,
    pub match_point: bool,
    pub skin: String,
}

/// TrainGameMode.GetMatchPlayerDataInputs's roll: per slot the random fighter of a Random slot (None where the slot
/// has its own character), and the rolled fighter's skin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabRoll {
    pub characters: Vec<Option<String>>,
    pub skins: Vec<String>,
}

/// The Lab's Spawn All Players with other fighters, teams or bots than the running ones:
/// the match ended and started again on the config, the menu's config and toggles, the camera and the music going on.
// Deviation: the build replaces the pawns in the running level; here the world's clock, the fighters' HUD and the
// controllers start anew.
#[derive(Debug)]
pub struct LabCarry {
    pub config: LabConfig,
    pub toggles: LabToggles,
    pub camera: MatchCamera,
    pub music: LabMusic,
    /// The ?debug key B.
    pub bot_idle: bool,
    pub rolled: LabRoll,
    /// The session, the menu screen and the pause menu's widget (the level is not left in the build, where the menu
    /// widget lives on).
    pub menu: LabMenu,
    /// TrainingContainer OnMapLoadRequested: the config's map is loaded before the spawn; its camera and music
    /// begin anew.
    pub map_load: bool,
    /// The new match's parts a loader thread built for this config and roll (`crate::prepare`), its set-up with them.
    pub prepared: Option<Box<crate::prepare::Prepared>>,
}

/// A random bot's ring-out and banner, drawn from what a player can equip (less the icon and
/// badges, which no set-up reads).
#[derive(Clone, Debug, Default)]
pub struct BotPool {
    /// RingOutVfxData asset names.
    pub ring_outs: Vec<String>,
    /// BannerData slugs.
    pub banners: Vec<String>,
}

/// A bot's rolled loadout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BotLoadout {
    pub ring_out: String,
    pub banner: String,
}

/// What a Lab set-up reads besides the config: this machine's saved loadout, the fighters the menus offer (the ones
/// the port can draw), the bot pool, and the random source (Math.random: a roll is the menu's, never the sim's).
pub struct LabInputs<'a> {
    pub settings: &'a Settings,
    pub fighters: &'a IndexMap<String, FighterInfo>,
    pub pool: &'a BotPool,
    pub random: &'a mut dyn FnMut() -> f64,
    /// `?p1=` / `?p2=`: the first set-up's fighters (LAB_CHARACTERS).
    pub first: [Option<String>; 2],
}

impl<'a> LabInputs<'a> {
    /// The inputs from the shell's saved settings and the menus' fighter list.
    pub fn of(
        shell: &'a Shell,
        fighters: &'a IndexMap<String, FighterInfo>,
        pool: &'a BotPool,
        random: &'a mut dyn FnMut() -> f64,
        first: [Option<String>; 2],
    ) -> LabInputs<'a> {
        LabInputs { settings: &shell.settings, fighters, pool, random, first }
    }

    fn fighter(&self, character: Option<&str>) -> Option<&'a FighterInfo> {
        let fighters: &'a IndexMap<String, FighterInfo> = self.fighters;
        character.and_then(|c| fighters.get(c))
    }
}

// ---------- skins ----------

/// Two SkinData names the same skin: FNames compare without case.
pub fn same_skin_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// UI_Functions GetDefaultSkinForCharacter: the IsDefaultSkin one.
pub fn default_skin(f: Option<&FighterInfo>) -> Option<&SkinInfo> {
    f?.skins.iter().find(|s| s.is_default)
}

/// UI_Functions GetSelectedSkinForCharacter @0-874: the skin whose slug the character's CharacterPreferences entry
/// holds, else the IsDefaultSkin one.
pub fn selected_skin<'a>(f: Option<&'a FighterInfo>, s: &Settings) -> Option<&'a SkinInfo> {
    let f = f?;
    let slug = s.hydra.character_skin_slug(&f.slug);
    (!slug.is_empty()).then(|| f.skins.iter().find(|k| k.slug == slug)).flatten().or_else(|| default_skin(Some(f)))
}

/// UI_Functions RandomSkin(Character, Only Unlocked false) @0-474: RandomIntegerInRange(0, Length), both ends
/// inclusive, so the index past the end draws a null skin (the default at the spawn) once in Length + 1.
pub fn random_skin<'a>(f: Option<&'a FighterInfo>, random: &mut dyn FnMut() -> f64) -> Option<&'a SkinInfo> {
    let l = f.map_or(&[][..], |f| f.skins.as_slice());
    // (`Math.min(Math.floor(random() * (l.length + 1)), l.length)`: the draw is in [0, 1), cast to an index)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let i = ((random() * (l.len() + 1) as f64).floor().max(0.0) as usize).min(l.len());
    l.get(i)
}

/// A pick's skin as the match takes it (MatchPlayer.skin): the SkinData name when the character has it and it is not
/// the default one, else "" (the default plays).
pub fn picked_skin(f: Option<&FighterInfo>, name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    match f.and_then(|f| f.skins.iter().find(|s| same_skin_name(&s.name, name))) {
        Some(s) if !s.is_default => s.name.clone(),
        _ => String::new(),
    }
}

/// The owner's skin in the first config: TrainingModePlayerState's default config @565-1352,
/// GetSelectedSkinForCharacter of the character. (the port's own) a fighter the queue's locked pick `mine` gave
/// (Wait in the Lab) takes that pick's skin, as the build's OverriddenTrainingSkin.
pub fn lab_owner_skin(i: &LabInputs<'_>, character: &str, mine: Option<&NetPick>) -> String {
    let f = i.fighter(Some(character));
    if let Some(m) = mine.filter(|m| m.character == character) {
        return picked_skin(f, &m.skin);
    }
    picked_skin(f, selected_skin(f, i.settings).map_or("", |s| s.name.as_str()))
}

/// A slot's skin as its pawn spawns with it (MatchPlayer.skin; "" the default): a Random slot's the roll's, another's
/// the config's.
pub fn lab_skin(i: &LabInputs<'_>, c: &LabConfig, rolled: &LabRoll, slot: usize) -> String {
    let p = &c.players[slot];
    match &p.character {
        None => rolled.skins.get(slot).cloned().unwrap_or_default(),
        Some(ch) => picked_skin(i.fighter(Some(ch)), &p.skin),
    }
}

// ---------- the loadout ----------

/// An asset's name as the sim keys it: the basename of its path ("/Game/.../ROV_Default.ROV_Default" ->
/// "ROV_Default").
pub fn asset_name(path: &str) -> &str {
    let tail = path.rsplit('/').next().unwrap_or(path);
    tail.split('.').next().unwrap_or(tail)
}

/// A character's taunt slots 0-3 from the prefs (asset names, "" an empty slot); `character` the port's name.
fn local_taunts(i: &LabInputs<'_>, character: &str) -> Vec<String> {
    match i.fighter(Some(character)) {
        Some(f) => i
            .settings
            .hydra
            .character_taunts(&f.slug)
            .iter()
            .map(|r| if r.slug.is_empty() { String::new() } else { asset_name(&r.path).to_owned() })
            .collect(),
        None => vec![String::new(); 4],
    }
}

/// GlobalEquippables.RingOutVfx's asset name ("" none).
pub fn local_ring_out(s: &Settings) -> String {
    let r = &s.hydra.ring_out_vfx;
    if r.slug.is_empty() { String::new() } else { asset_name(&r.path).to_owned() }
}

/// A bot's random ring-out and banner.
// Deviation: the build's offline modes give a bot none (ROV_Default); the port gives each a random one. With no
// cosmetics catalogue yet the pool is empty, which is the build's way.
pub fn roll_bot_loadout(pool: &BotPool, random: &mut dyn FnMut() -> f64) -> BotLoadout {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let mut at = |n: usize| ((random() * n as f64).floor().max(0.0) as usize).min(n - 1);
    BotLoadout {
        ring_out: if pool.ring_outs.is_empty() {
            String::new()
        } else {
            pool.ring_outs[at(pool.ring_outs.len())].clone()
        },
        banner: if pool.banners.is_empty() { String::new() } else { pool.banners[at(pool.banners.len())].clone() },
    }
}

/// The taunts when any is set, the ring-out and banner when not empty.
fn with_cosmetics(p: &mut MatchPlayer, taunts: Vec<String>, ring_out: &str, banner: &str) {
    if taunts.iter().any(|t| !t.is_empty()) {
        p.taunts = (0..4).map(|n| taunts.get(n).cloned().unwrap_or_default()).collect();
    }
    if !ring_out.is_empty() {
        p.ring_out = Some(ring_out.to_owned());
    }
    if !banner.is_empty() {
        p.banner = Some(banner.to_owned());
    }
}

// ---------- perks ----------

/// A Lab slot's perks: the config's Perks, which a Random slot (bIsRandom @639) replaces by every recommended perk
/// the offline mode has (@864), uncapped.
// Unverified: Set_ToArray's order and the offline GetCharacterSavedPerks; the offline list is the character's
// recommended perks as the menu data holds them.
pub fn lab_perks(i: &LabInputs<'_>, character: &str, random: bool, perks: &[Option<String>]) -> Vec<String> {
    let slots: Vec<String> = if random {
        i.fighter(Some(character)).and_then(|f| f.perks.as_ref()).map_or_else(Vec::new, |p| p.recommended.clone())
    } else {
        perks.iter().map(|p| p.clone().unwrap_or_default()).collect()
    };
    perk_list(&slots)
}

/// The local user's saved loadout for the character, its four slots (so an unedited Lab's match has the
/// perks it saved).
pub fn lab_default_perks(s: &Settings, character: &str) -> Vec<Option<String>> {
    let saved = s.perks.get(character);
    (0..4).map(|n| saved.and_then(|v| v.get(n)).cloned().flatten()).collect()
}

// ---------- the setups ----------

/// The config's occupied slots as the match's players.
pub fn lab_slots(c: &LabConfig) -> Vec<usize> {
    (0..c.players.len()).filter(|&n| !c.players[n].account_id.is_empty()).collect()
}

/// The Lab's first match when it opens from the menus: the owner is the fighter locked on
/// the queue's pre-match screen (`mine`, Wait in the Lab: Build Default Player Training Config's
/// GetLastSelectedCharacter reads the "PreviewCharacter" save state 11 wrote; the port's queue session holds that
/// pick), else `?p1`'s (character absent: the default), in its owner skin with the owner's saved loadout; the dummy
/// is `?p2`'s in its default skin with none. `roll_bots`: a bot's ring-out rolled, else none.
pub fn lab_first_setup(i: &mut LabInputs<'_>, roll_bots: bool, mine: Option<&NetPick>) -> MatchSetup {
    let mut players =
        vec![MatchPlayer { bot: false, ..MatchPlayer::default() }, MatchPlayer { bot: true, ..MatchPlayer::default() }];
    for (n, p) in players.iter_mut().enumerate() {
        let character = if n == 0 { mine.map(|m| m.character.clone()) } else { None }.or_else(|| i.first[n].clone());
        let c = character.clone().unwrap_or_else(|| DEFAULT_CHARACTER.to_owned());
        p.character = character;
        let skin = if n == 0 { lab_owner_skin(i, &c, mine) } else { String::new() };
        if !skin.is_empty() {
            p.skin = Some(skin);
        }
        let saved = if p.bot { Vec::new() } else { lab_default_perks(i.settings, &c) };
        p.perks = lab_perks(i, &c, false, &saved);
        let ring_out = if n == 0 {
            local_ring_out(i.settings)
        } else if p.bot && roll_bots {
            roll_bot_loadout(i.pool, i.random).ring_out
        } else {
            String::new()
        };
        let taunts = local_taunts(i, &c);
        // Deviation: the owner takes local player 0's input preferences, where labFirstSetup passes none (the
        // defaults); the build's fighters read PM's preferences whatever the match.
        if n == 0 {
            p.input = sim_input_prefs(&i.settings.input, 0);
        }
        with_cosmetics(p, taunts, &ring_out, "");
    }
    MatchSetup { team_style: TeamStyle::OneVsOne, players, ..MatchSetup::default() }
}

/// The config's slots as the match's players: TrainGameMode.GetTeamStyleFromTrainingConfiguration
/// (more than 2 team indexes in use: free for all, else a team of 2 or more: 2v2, else 1v1).
pub fn lab_setup(i: &mut LabInputs<'_>, c: &LabConfig, rolled: &LabRoll) -> MatchSetup {
    let slots = lab_slots(c);
    let mut sizes: IndexMap<i32, usize> = IndexMap::new();
    for &n in &slots {
        *sizes.entry(c.players[n].team_index).or_insert(0) += 1;
    }
    let team_style = if sizes.len() > 2 {
        TeamStyle::FreeForAll
    } else if sizes.values().copied().max().unwrap_or(0) >= 2 {
        TeamStyle::TwoVsTwo
    } else {
        TeamStyle::OneVsOne
    };
    let mut players = Vec::new();
    for &n in &slots {
        let lp = &c.players[n];
        let character = lp
            .character
            .clone()
            .or_else(|| rolled.characters.get(n).cloned().flatten())
            .unwrap_or_else(|| DEFAULT_CHARACTER.to_owned());
        let skin = lab_skin(i, c, rolled, n);
        players.push(MatchPlayer {
            bot: lp.bot_difficulty != 0,
            perks: lab_perks(i, &character, lp.character.is_none(), &lp.perks),
            character: Some(character),
            team: Some(u8::try_from(lp.team_index).unwrap_or(0)),
            start: Some(u8::try_from(n).unwrap_or(0)),
            starting_damage: Some(mvs_sim::math::narrow(lp.starting_damage)),
            skin: (!skin.is_empty()).then_some(skin),
            ..MatchPlayer::default()
        });
    }
    // (a bot's random ring-out and banner, drawn at each set-up; Deviation: TrainGameMode gives its bots none)
    let bots: Vec<Option<BotLoadout>> =
        players.iter().map(|p| p.bot.then(|| roll_bot_loadout(i.pool, i.random))).collect();
    for (k, p) in players.iter_mut().enumerate() {
        let local = c.players[slots[k]].account_id == "local";
        let character = p.character.clone().unwrap_or_else(|| DEFAULT_CHARACTER.to_owned());
        let ring_out = if local {
            local_ring_out(i.settings)
        } else {
            bots[k].as_ref().map_or_else(String::new, |b| b.ring_out.clone())
        };
        let banner = bots[k].as_ref().map_or_else(String::new, |b| b.banner.clone());
        let taunts = local_taunts(i, &character);
        // (the local account's: local player 0's input preferences, a bot's the defaults)
        if local {
            p.input = sim_input_prefs(&i.settings.input, 0);
        }
        with_cosmetics(p, taunts, &ring_out, &banner);
    }
    let wheel = mvs_protocol::lobby_rules::wheel_rule(c.wheel).as_ref().map(mvs_ui::menu::lobby::sim_wheel);
    MatchSetup { team_style, players, mutators: c.mutators.names(), wheel, ..MatchSetup::default() }
}

/// The config's random fighters rolled: a Random slot of an occupied account takes a random
/// fighter of the ones the port can draw, and that fighter's random skin.
pub fn lab_roll(i: &mut LabInputs<'_>, c: &LabConfig) -> LabRoll {
    let drawn: Vec<String> = i.fighters.keys().cloned().collect();
    let mut roll = LabRoll::default();
    for p in &c.players {
        if !p.account_id.is_empty() && p.character.is_none() && !drawn.is_empty() {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
            let k = (((i.random)() * drawn.len() as f64).floor().max(0.0) as usize).min(drawn.len() - 1);
            let ch = drawn[k].clone();
            let f = i.fighters.get(&ch);
            let skin = picked_skin(f, random_skin(f, i.random).map_or("", |s| s.name.as_str()));
            roll.characters.push(Some(ch));
            roll.skins.push(skin);
        } else {
            roll.characters.push(None);
            roll.skins.push(String::new());
        }
    }
    roll
}

/// What a spawn cannot change in place: who is in which slot, as what, on which team, with
/// which perks; equal rosters spawn in place, others restart.
// Deviation: a perk change takes the restart a character change takes (the build spawns new pawns, whose
// PerkManager.Initialize takes the config's perks, TrainGameMode SpawnPlayerPawn @247); the slots in order, trailing
// empties dropped.
pub fn lab_roster(c: &LabConfig, rolled: &LabRoll) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (n, p) in c.players.iter().enumerate() {
        if p.account_id.is_empty() {
            out.push_str("-;");
            continue;
        }
        let ch = p.character.clone().or_else(|| rolled.characters.get(n).cloned().flatten()).unwrap_or_default();
        let mut perks: Vec<&str> = p.perks.iter().map(|x| x.as_deref().unwrap_or("")).collect();
        while perks.last() == Some(&"") {
            perks.pop();
        }
        let _ = write!(out, "{ch},{},{},{};", p.bot_difficulty != 0, p.team_index, perks.join("|"));
    }
    out
}

/// The roster with each slot's skin appended (matchLabMenu's rosterOf): a skin change takes the restart a character
/// change takes.
pub fn lab_roster_skins(i: &LabInputs<'_>, c: &LabConfig, rolled: &LabRoll) -> String {
    let mut s = lab_roster(c, rolled);
    for (n, p) in c.players.iter().enumerate() {
        if !p.account_id.is_empty() {
            s.push_str(&lab_skin(i, c, rolled, n));
        }
        s.push('/');
    }
    // (the port's own) the mutators: another set restarts the match as another roster does
    if !c.mutators.is_empty() {
        s.push_str(&format!("m{}", c.mutators.0));
    }
    if c.wheel.on {
        let w = c.wheel;
        s.push_str(&format!("w{}.{}.{}.{}.{}", w.seconds, w.warn, w.chaos, u8::from(w.two), w.off));
    }
    s
}

/// The menus' fighter list, by character slug.
pub type Fighters = IndexMap<String, FighterInfo>;

/// The Lab's first match from the menus (with the bots' ring-outs rolled); `mine` the
/// queue's locked pick when the Lab opens from its pre-match screen, else the owner is `preview_character`'s.
pub fn first_setup(
    settings: &Settings,
    fighters: &Fighters,
    roster: &[RosterEntry],
    mine: Option<&NetPick>,
) -> MatchSetup {
    let pool = BotPool::default();
    let mut rng = random_source();
    let first = [preview_character(settings, fighters, roster), None];
    let mut i = LabInputs { settings, fighters, pool: &pool, random: &mut rng, first };
    lab_first_setup(&mut i, true, mine)
}

/// Build Default Player Training Config's GetLastSelectedCharacter (TrainingPlayerSettings @1348): the
/// "PreviewCharacter" save (Save Chosen Characters PM@35893, the last pick: `profile.last_character`, a roster slug)
/// as a fighter the menus offer; None (the default character) before any pick or for one this store lacks.
pub fn preview_character(settings: &Settings, fighters: &Fighters, roster: &[RosterEntry]) -> Option<String> {
    let last = &settings.profile.last_character;
    if last.is_empty() {
        return None;
    }
    roster.iter().find(|r| r.slug == *last)?.fighter.clone().filter(|k| fighters.contains_key(k))
}

/// The restart's match. A
/// carry with prepared parts takes their set-up: the one they were built for on the same config and roll (its bots'
/// loadouts drawn then).
pub fn restart(shell: &Shell, mut carry: LabCarry) -> MatchStart {
    let prepared = carry.prepared.take();
    let mut start = start_of(shell, &carry.config, &carry.rolled);
    if let Some(p) = &prepared {
        start.setup = p.key.setup.clone();
    }
    start.kind = MatchKind::Lab { carry: Some(Box::new(carry)) };
    start.prepared = prepared;
    start
}

/// The Lab match of `config` and `rolled`, with no carry.
pub fn start_of(shell: &Shell, config: &LabConfig, rolled: &LabRoll) -> MatchStart {
    let (fighters, pool) = (fighters_of(shell), BotPool::default());
    let mut rng = random_source();
    let mut i = LabInputs {
        settings: &shell.settings,
        fighters: &fighters,
        pool: &pool,
        random: &mut rng,
        first: [None, None],
    };
    let setup = lab_setup(&mut i, config, rolled);
    let slots = lab_slots(config);
    let me = slots.iter().position(|&s| config.players[s].account_id == "local").unwrap_or(0);
    // TrainGameMode.PopulateMatchPlayerData @709: a bot of a Random slot is "{Character} (Bot)" of the fighter
    // rolled for it, every other slot its PlayerName.
    let usernames = slots
        .iter()
        .map(|&s| {
            let p = &config.players[s];
            match rolled.characters.get(s).cloned().flatten() {
                Some(ch) if p.bot_difficulty != 0 => bot_name(&fighters, &ch),
                _ => p.player_name.clone(),
            }
        })
        .collect();
    let map = (!config.selected_map.is_empty()).then(|| config.selected_map.clone());
    let mut start = MatchStart::new(setup, MatchKind::Lab { carry: None }, 0);
    start.map = map;
    start.me = me;
    start.usernames = usernames;
    start
}

/// The usernames of the Lab's first match (`first_setup`'s): the owner's name (else 'Player'),
/// the dummy "{Character} (Bot)" as `first_config` names it.
pub fn first_usernames(settings: &Settings, fighters: &Fighters, setup: &MatchSetup) -> Vec<String> {
    let name = &settings.profile.name;
    let dummy = setup.players.get(1).and_then(|p| p.character.clone()).unwrap_or_else(|| DEFAULT_CHARACTER.to_owned());
    vec![if name.is_empty() { "Player".to_owned() } else { name.clone() }, bot_name(fighters, &dummy)]
}

/// GenerateDisplayName of a bot: "{DisplayName} (Bot)" of its fighter (the slug where the menus lack it).
fn bot_name(fighters: &Fighters, character: &str) -> String {
    let display = fighters.get(character).map_or(character, |f| f.display_name.as_str());
    format!("{display} (Bot)")
}

/// The fighters the menus offer (the menu bundle's, drawable ones).
pub fn fighters_of(shell: &Shell) -> IndexMap<String, FighterInfo> {
    shell.ui.as_ref().and_then(|u| u.menu.menu.as_ref()).map(|m| m.fighters.clone()).unwrap_or_default()
}

/// Math.random: a time-seeded xorshift draw in [0, 1) (the menus' rolls; never the sim's).
pub fn random_source() -> impl FnMut() -> f64 {
    let mut x = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9E37_79B9_7F4A_7C15, |d| u64::try_from(d.as_nanos() & u128::from(u64::MAX)).unwrap_or(1))
        | 1;
    move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        #[allow(clippy::cast_precision_loss)]
        let v = (x >> 11) as f64 / (1u64 << 53) as f64;
        v
    }
}

/// The Lab's first config: the owner's slot and Bot1 on team 1, the other
/// two empty; `setup` is the first set-up the match runs, `name` the owner's name.
// Deviation: the map is map_training for every Lab (the port's stage option is not a Lab setting).
pub fn first_config(
    fighters: &Fighters,
    settings: &Settings,
    setup: &MatchSetup,
    name: &str,
    same_attack: bool,
) -> LabConfig {
    use mvs_ui::menu::training_config::{LabPlayerConfig, empty_lab_player};
    let character = |n: usize| {
        setup.players.get(n).and_then(|p| p.character.clone()).unwrap_or_else(|| DEFAULT_CHARACTER.to_owned())
    };
    let (mine, dummy) = (character(0), character(1));
    LabConfig {
        players: vec![
            LabPlayerConfig {
                account_id: "local".into(),
                player_name: name.to_owned(),
                character: Some(mine.clone()),
                skin: setup.players.first().and_then(|p| p.skin.clone()).unwrap_or_default(),
                perks: lab_default_perks(settings, &mine),
                ..empty_lab_player()
            },
            LabPlayerConfig {
                account_id: "Bot1".into(),
                player_name: bot_name(fighters, &dummy),
                character: Some(dummy),
                bot_difficulty: 1,
                team_index: 1,
                ..empty_lab_player()
            },
            empty_lab_player(),
            empty_lab_player(),
        ],
        leader_account_id: "local".into(),
        selected_map: "map_training".into(),
        instant_cooldowns: false,
        same_attack_penalty: same_attack,
        mutators: setup_mutators(setup),
        wheel: mvs_protocol::lobby_rules::WheelOptions::default(),
    }
}

/// The first set-up's mutators as the config's set (none for a Lab opened from the menus).
fn setup_mutators(setup: &MatchSetup) -> mvs_protocol::lobby_rules::MutatorSet {
    use mvs_protocol::lobby_rules::{LOBBY_MUTATORS, MutatorSet};
    setup
        .mutators
        .iter()
        .fold(MutatorSet::default(), |s, m| LOBBY_MUTATORS.iter().position(|x| x == m).map_or(s, |n| s.with(n, true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_config_holds_the_owner_and_bot1() {
        let settings = Settings::default();
        let fighters = Fighters::new();
        let mut rng = || 0.5;
        let pool = BotPool::default();
        let mut i =
            LabInputs { settings: &settings, fighters: &fighters, pool: &pool, random: &mut rng, first: [None, None] };
        let setup = lab_first_setup(&mut i, false, None);
        let c = first_config(&fighters, &settings, &setup, "Me", true);
        assert_eq!(c.players.len(), 4);
        assert_eq!((c.players[0].account_id.as_str(), c.players[0].player_name.as_str()), ("local", "Me"));
        assert_eq!(
            (c.players[1].account_id.as_str(), c.players[1].bot_difficulty, c.players[1].team_index),
            ("Bot1", 1, 1)
        );
        assert_eq!(lab_slots(&c), vec![0, 1]);
        assert!(c.same_attack_penalty && !c.instant_cooldowns);
        assert_eq!(c.selected_map, "map_training");
    }

    #[test]
    fn the_first_lab_names_its_owner_and_dummy() {
        let mut settings = Settings::default();
        settings.profile.name = "Me".into();
        let mut fighters = Fighters::new();
        fighters.insert("shaggy".into(), FighterInfo { display_name: "Shaggy".into(), ..FighterInfo::default() });
        let mut rng = || 0.5;
        let pool = BotPool::default();
        let mut i =
            LabInputs { settings: &settings, fighters: &fighters, pool: &pool, random: &mut rng, first: [None, None] };
        let setup = lab_first_setup(&mut i, false, None);
        assert_eq!(first_usernames(&settings, &fighters, &setup), ["Me", "Shaggy (Bot)"]);
        let c = first_config(&fighters, &settings, &setup, "Me", true);
        assert_eq!(c.players[1].player_name, "Shaggy (Bot)");
    }

    /// A Lab opened from the menus takes the last pick (the "PreviewCharacter" save) as its owner's fighter, the
    /// default before any pick or for a fighter this store lacks; the queue's locked pick wins over it.
    #[test]
    fn the_first_lab_takes_the_last_pick() {
        let mut settings = Settings::default();
        let mut fighters = Fighters::new();
        fighters.insert("wonderwoman".into(), FighterInfo::default());
        let roster = [RosterEntry {
            slug: "character_wonder_woman".into(),
            fighter: Some("wonderwoman".into()),
            ..RosterEntry::default()
        }];
        let owner = |settings: &Settings, mine: Option<&NetPick>| {
            first_setup(settings, &fighters, &roster, mine).players[0].character.clone()
        };
        assert_eq!(owner(&settings, None), None);
        settings.profile.last_character = "character_wonder_woman".into();
        assert_eq!(owner(&settings, None).as_deref(), Some("wonderwoman"));
        let mine = NetPick { character: "shaggy".into(), ..NetPick::default() };
        assert_eq!(owner(&settings, Some(&mine)).as_deref(), Some("shaggy"));
        settings.profile.last_character = "character_unknown".into();
        assert_eq!(owner(&settings, None), None);
    }
}
