# Experimental settings

Rules of the port's own that the beta does not have, for custom games only. The custom lobby's
EXPERIMENTAL SETTINGS button (in the place of the beta's Reset To Defaults) opens them. Each is a lobby option
(`mvs-protocol` `lobby_rules::OptionKey`): off when a lobby is made, set by the leader, the same for every member
(online the host core keeps it). A lobby kept across its matches keeps its options. The page's rows are
`experimental_page` in `mvs-ui` `menu/widgets/custom_settings.rs`; their words are in `menu/custom_text.rs`.

None of them changes a fighter's moves or feel (`AGENTS.md` "What the game is").

## Stock Mode (`stocks`)

Each player has the lobby's ring-out count as lives, in every team style (Free For All's drop is unlocked and the
drop reads Stocks). The match set-up carries `rules.stocks` (`mvs-protocol` `setup.rs`); the sim's part is
`mvs-sim` `world/flow.rs` (`start_stocks`, `stock_ring_out`, `stock_kill`).

- A ring-out takes a life. A player out of lives stays out and leaves the camera; team-mates play on.
- The last team with lives wins (in Free For All, the last player).
- A team's TeamKnockouts holds its lives left, so time up goes to the most lives, then the fewest falls or the
  lowest damage (`winners`); Free For All's catch-up buffs go to players with fewer lives than two others.
- Off under the rule: the attacker's point, Free For All's point steal, assist points, bonus objectives and double
  points. The match point warning plays when a team (1v1, 2v2) is down to its last life.
- HUD: each player's lives on the Free For All score badge of its portrait, in every mode; no score pips; a player
  out of lives is faded. The countdown card names the rule. The end-of-game sheets show KOs in Free For All.
- A lost teammate cannot take a life from a partner.
- The match record's plausibility check (`mvs-protocol` `record.rs`) runs without a count to win for the rule.

## Mutator (`mutator`)

One mutator for the whole match, picked from the row's list (`mvs-protocol` `lobby_rules::LOBBY_MUTATORS`, value 0
is Off). The match set-up carries it as `mutators: [key]` (`mvs-protocol` `setup.rs`), the same path the Silly queue
uses, so the sim runs the build's own mutator code (`mvs-sim` `world/mutators.rs`). Works in every team style and
with Stock Mode.

- The list: Silly (Random) is the build's `Mutator_RandomMutatorSelector` as the Silly queue plays it (Big Head or
  Grow for every fighter, random items, the final point shrink); then the fighter mutators one by one (Big Head,
  Grow, Giant, Miniature, Low Gravity, Instant Kill, Grey Health, Unlimited Jumps, Faster Cooldowns, Attacks Break
  Armor, Extra Air Special, Weaken on Hit, Charge Freeze, Ice Gauntlets) and two world ones (Random Items, Final
  Point Shrink). Mutators the port does not run yet (Chicken, Heart, Snowball Fight, Exploding Egg, Reindog Buddy,
  Fast Charge: no asset in the build) are not offered.
- One at a time: the row is a single choice.
- Final Point Shrink (and Silly's) under Stock Mode: the build's test reads the score, so under stocks the shrink
  comes when the match is one ring-out from its end (two teams left with lives, one on its last), checked at each
  life lost, a respawn or not.
- Random Items or Silly in FreeForAll: the mode's own item spawner runs alone (one centre safe, the mode's item
  list); the build would start both.
- The names are the port's own words (`mvs-ui` `menu/custom_text.rs`): the beta's Game.locres has no line for
  them.
