//! The online server's choices turned into the game's: the server draws ids, picks
//! and seeds and holds no game data; each game resolves them from the tables it binds from its own build. Every game
//! of a match has the same tables because the server matched only games of one compatibility id.

use std::collections::BTreeMap;

use mvs_protocol::online::{BadgeRef, CosmeticRef, PlayerCosmetics, QueueFoundMsg, QueueMode, QueuePlayer};
use mvs_protocol::queue_rules::{QUEUE_MODES, QueueFound};
use mvs_protocol::setup::MatchSetupWire;
use mvs_ui::bundle::MenuData;
use mvs_ui::menu::lobby::loadout::{BotLoadout, BotPool, random_bot_loadout, simulate_perk_unlock};

/// The rotation of each queue mode (the EMapRotation names as the menu data keeps
/// them): the unranked ones, and Silly's the Casual rotation (the Casual card's queue type 3).
pub fn queue_rotation(mode: QueueMode) -> &'static str {
    match mode {
        QueueMode::OneVOne => "UnrankedSolos",
        QueueMode::TwoVTwo => "UnrankedDuos",
        QueueMode::Ffa => "UnrankedFreeForAll",
        QueueMode::Silly => "Casual",
    }
}

/// A queue bot's level. Deviation: the port's queue bots take the full unlock
/// (the build rolls a level from the owner's mastery: 1 or 2, no perk or one).
pub const QUEUE_BOT_LEVEL: i32 = 8;

/// The tables of this game's build.
#[derive(Clone, Debug, Default)]
pub struct Tables {
    /// The queue's map pools by mode: the MapData names whose rotations hold the mode's and that have a stage.
    pub maps: BTreeMap<QueueMode, Vec<String>>,
    pub pool: BotPool,
    /// A queue bot's perks by character (queueBotPerks; a lobby bot's are the same: lobbyBotPerks).
    pub perks: BTreeMap<String, Vec<String>>,
}

impl Tables {
    /// The tables of the menu data.
    /// `has_stage`: a MapData a stage of the build opens.
    /// Unverified: the build's set and order are native PGI_GlobalData GetMapsForRotation; read
    /// as the MapData packages in the menu data's order (sorted by path). The order only names which pick is which
    /// map.
    pub fn of(menu: &MenuData, pool: BotPool, has_stage: &dyn Fn(&str) -> bool) -> Tables {
        let mut maps = BTreeMap::new();
        for mode in QUEUE_MODES {
            let rot = queue_rotation(mode);
            let pool: Vec<String> = menu
                .maps
                .iter()
                .filter(|(name, m)| m.rotations.iter().any(|r| r == rot) && has_stage(name))
                .map(|(name, _)| name.clone())
                .collect();
            if pool.is_empty() {
                eprintln!("online: no map of the {} rotation has a stage", mode.id());
            }
            maps.insert(mode, pool);
        }
        let perks = mvs_protocol::roster::CHARACTERS
            .iter()
            .map(|c| ((*c).to_owned(), simulate_perk_unlock(menu, c, QUEUE_BOT_LEVEL)))
            .collect();
        Tables { maps, pool, perks }
    }

    /// The queue map of a pick: the mode's pool entry `pick % length` ('' with an empty pool).
    pub fn queue_map(&self, mode: QueueMode, pick: u32) -> String {
        let pool = self.maps.get(&mode).map_or(&[][..], Vec::as_slice);
        if pool.is_empty() { String::new() } else { pool[pick as usize % pool.len()].clone() }
    }

    /// A bot's perks (queueBotPerksOf, lobbyBotPerksOf).
    pub fn bot_perks(&self, character: &str) -> Vec<String> {
        self.perks.get(character).cloned().unwrap_or_else(|| vec![String::new(); 4])
    }

    /// A bot's cosmetics and ring-out from the seed the server drew.
    pub fn bot_loadout_of(&self, seed: u32) -> BotLoadout {
        let mut r = seeded_random(seed);
        random_bot_loadout(&self.pool, &mut r)
    }

    /// A player as the game keeps it: a bot with a seed gets its drawn cosmetics and ring-out, and its perks with
    /// `perks` (a queue bot's). A human, or a bot without a seed, as it came.
    pub fn with_bot_draw(&self, p: &QueuePlayer, perks: bool) -> QueuePlayer {
        let Some(seed) = p.seed.filter(|_| p.bot) else { return p.clone() };
        let l = self.bot_loadout_of(seed);
        let mut out = p.clone();
        out.cosmetics = Some(cosmetics_of_loadout(&l));
        out.ring_out = Some(l.ring_out);
        if perks && !p.character.is_empty() {
            out.perks = Some(self.bot_perks(&p.character));
        }
        out
    }

    /// queue.found as the game takes it: the map of the pick, each bot drawn (a queue bot's perks with it).
    pub fn resolve_found(&self, m: &QueueFoundMsg) -> QueueFound {
        QueueFound {
            match_id: m.match_id.clone(),
            mode: m.mode,
            map: self.queue_map(m.mode, m.map_pick),
            players: m.players.iter().map(|p| self.with_bot_draw(p, true)).collect(),
            me: m.me,
        }
    }

    /// match.start's set-up and perks with what the server left to the games: each bot slot's ring-out and banner
    /// from its drawn loadout (`players`: the pre-match's as resolved, with_bot_draw) and its perks. The order of the
    /// fields is the same on every game (the match's start hash reads the set-up whole).
    pub fn with_bot_tables(
        &self,
        setup: &MatchSetupWire,
        perks: &[Vec<String>],
        players: &[QueuePlayer],
    ) -> (MatchSetupWire, Vec<Vec<String>>) {
        let mut out = setup.clone();
        let mut all = Vec::new();
        for (slot, p) in out.players.iter_mut().enumerate() {
            if !p.bot {
                all.push(perks.get(slot).cloned().unwrap_or_default());
                continue;
            }
            if let Some(q) = players.get(slot).filter(|q| q.bot) {
                let banner = q.cosmetics.as_ref().map(|c| c.banner.slug.as_str());
                *p = mvs_protocol::loadout::with_cosmetics(p, None, q.ring_out.as_deref(), banner, None, None);
            }
            all.push(p.character.as_deref().map(|c| self.bot_perks(c)).unwrap_or_default());
        }
        (out, all)
    }
}

/// (the port's own) A draw in [0, 1) from a seed (mulberry32): the same sequence on
/// every machine.
pub fn seeded_random(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6d2b_79f5);
        let mut t = a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// A drawn loadout as the pre-match carries it. Deviation: the asset names are '' (the bot pool keeps slugs: the
/// screens read the slugs).
pub fn cosmetics_of_loadout(l: &BotLoadout) -> PlayerCosmetics {
    let r = |slug: &str| CosmeticRef { slug: slug.to_owned(), asset: String::new() };
    PlayerCosmetics {
        banner: r(&l.banner),
        // (the counts are whole and small)
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        badges: l
            .badges
            .iter()
            .map(|b| BadgeRef { slug: b.slug.clone(), asset: String::new(), count: b.count as u32 })
            .collect(),
        icon: r(&l.icon),
    }
}

/// A pre-match's cosmetics as the screen takes them (a ring-out beside them, '' none).
pub fn loadout_of_cosmetics(c: &PlayerCosmetics, ring_out: &str) -> BotLoadout {
    BotLoadout {
        ring_out: ring_out.to_owned(),
        banner: c.banner.slug.clone(),
        icon: c.icon.slug.clone(),
        badges: c
            .badges
            .iter()
            .map(|b| mvs_ui::menu::lobby::loadout::BadgeRef { slug: b.slug.clone(), count: f64::from(b.count) })
            .collect(),
    }
}

/// (the port's own) The mutators of a queue mode's match (MatchSetup.mutators): the Silly queue's is the build's one
/// selection rule, Mutator_RandomMutatorSelector. Deviation: the build's 2v2casual configs named their mutators from
/// the backend (UMatchConfig GetMutatorData), which no file of the build holds.
pub fn queue_mutators(mode: QueueMode) -> Option<&'static [&'static str]> {
    match mode {
        QueueMode::Silly => Some(&["Mutator_RandomMutatorSelector"]),
        _ => None,
    }
}

/// A queue match's set-up with its mode's mutators, whatever the server sent.
pub fn with_mode_mutators(setup: &MatchSetupWire, mode: QueueMode) -> MatchSetupWire {
    let mut out = setup.clone();
    out.mutators = queue_mutators(mode).map(|l| l.iter().map(|s| (*s).to_owned()).collect());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mulberry32_draws_as_the_page() {
        // (seeded_random(1) and seeded_random(0): numbers an oracle run gave)
        let mut r = seeded_random(1);
        assert_eq!([r(), r()], [0.6270739405881613, 0.002735721180215478]);
        let mut z = seeded_random(0);
        assert_eq!(z(), 0.26642920868471265);
    }

    #[test]
    fn maps_and_bots() {
        let mut t = Tables::default();
        t.maps.insert(QueueMode::OneVOne, vec!["a".into(), "b".into()]);
        assert!(t.queue_map(QueueMode::OneVOne, 3) == "b" && t.queue_map(QueueMode::Ffa, 3).is_empty());
        t.perks.insert("shaggy".into(), vec!["s".into(), "g".into(), String::new(), String::new()]);
        let bot = QueuePlayer {
            id: "bot1".into(),
            bot: true,
            character: "shaggy".into(),
            seed: Some(7),
            ..Default::default()
        };
        let d = t.with_bot_draw(&bot, true);
        assert!(d.cosmetics.is_some() && d.ring_out.is_some() && d.perks.as_ref().unwrap()[0] == "s");
        let human = QueuePlayer { id: "h".into(), ..Default::default() };
        assert_eq!(t.with_bot_draw(&human, true), human);
        let setup = MatchSetupWire {
            players: vec![
                mvs_protocol::setup::MatchPlayerWire { bot: false, ..Default::default() },
                mvs_protocol::setup::MatchPlayerWire {
                    bot: true,
                    character: Some("shaggy".into()),
                    ..Default::default()
                },
            ],
            mutators: Some(vec!["x".into()]),
            ..Default::default()
        };
        let (s, perks) = t.with_bot_tables(&setup, &[vec!["p".into()], vec![]], &[human, d]);
        assert!(perks[0] == ["p"] && perks[1][0] == "s");
        assert_eq!(s.players[0], setup.players[0]);
        assert!(with_mode_mutators(&setup, QueueMode::OneVOne).mutators.is_none());
        assert_eq!(with_mode_mutators(&setup, QueueMode::Silly).mutators.unwrap(), ["Mutator_RandomMutatorSelector"]);
    }
}
