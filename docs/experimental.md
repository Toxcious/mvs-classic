# Experimental settings

Rules of the port's own that the beta does not have, for custom games only. The custom lobby's
EXPERIMENTAL SETTINGS button (in the place of the beta's Reset To Defaults) opens them. Each is a lobby option
(`mvs-protocol` `lobby_rules::OptionKey`): off when a lobby is made, set by the leader, the same for every member
(online the host core keeps it). A lobby kept across its matches keeps its options. The page's rows are
`experimental_page` in `mvs-ui` `menu/widgets/custom_settings.rs`; their words are in `menu/custom_text.rs`.

The Lab has the same page with the Mutators row alone: its EXPERIMENTAL SETTINGS button under Move List in the
pause menu (`lab_experimental_page`, `mvs-ui` `menu/screens/lab.rs`). The Lab's config holds the set
(`LabConfig::mutators`); a change restarts the match at the unpause, as another roster does.

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

## Mutators (`mutators`)

Any number of mutators for the whole match, ticked in the row's list (`mvs-protocol` `lobby_rules::MutatorSet`, a
bit per entry of `LOBBY_MUTATORS`). The match set-up carries them as `mutators: [key, ...]` in the list's order
(`mvs-protocol` `setup.rs`), the same path the Silly queue uses, so the sim runs the build's own mutator code
(`mvs-sim` `world/mutators.rs`), which applies each in turn as the build's InitializeMutators does. Works in every
team style and with Stock Mode.

- The list: Silly (Random) is the build's `Mutator_RandomMutatorSelector` as the Silly queue plays it (Big Head or
  Grow for every fighter, random items, the final point shrink); then the fighter mutators one by one (Big Head,
  Grow, Giant, Miniature, Low Gravity, Instant Kill, Shields Up, Unlimited Jumps, Ability Frenzy, Heavy Hitters,
  Air Show, Double Damage, Ice Charge, Freeze Touch) and two world ones (Random Items, Final Point Shrink). Seven of
  them are the port's reworks of the build's (below), here as on the wheel. Mutators the port does not run yet (Chicken, Heart, Snowball Fight, Exploding Egg, Reindog Buddy,
  Fast Charge: no asset in the build) are not offered.
- The row is a checklist: a click ticks or unticks a mutator and the list stays up for the next; Off clears it.
  A ticked row is white with a bullet (the menu's faces have no tick mark). The row reads Off, the one mutator's
  name, or the count.
- Mutators that act on the same thing (Grow and Miniature, Big Head and Silly) are not kept apart: each applies as
  the build's code would with both in the list.
- Final Point Shrink (and Silly's) under Stock Mode: the build's test reads the score, so under stocks the shrink
  comes when the match is one ring-out from its end (two teams left with lives, one on its last), checked at each
  life lost, a respawn or not.
- Random Items or Silly in FreeForAll: the mode's own item spawner runs alone (one centre safe, the mode's item
  list); the build would start both.
- The names are the port's own words (`mvs-ui` `menu/custom_text.rs`): the beta's Game.locres has no line for
  them.
- The reworks: seven of the build's mutators did too little to be seen in a window of the wheel, so the port
  changes them wherever they are picked (`mvs-sim` `combat/mutator_buffs.rs`, each with its `// Deviation:`):
  Shields Up (Grey Health: 50 grey health, given again whole after 4 seconds without a hit; the build's 25 once),
  Ability Frenzy (Faster Cooldowns: cooldowns x0.1; the build's x0.6), Heavy Hitters (Attacks Break Armor: it
  also launches x1.4), Double Damage (Weaken on Hit: hits deal x2; the build's gave the hit one a Weakened stack),
  Ice Charge (Charge Freeze: charges fill twice as fast and a hit charged past half freezes; the build's freezes on
  a full charge), Freeze Touch (Ice Gauntlets: every third hit a fighter lands freezes; the build's gave Superman's
  ally gauntlet, whose first hit of a life froze) and Air Show (Extra Air Special: air specials never run out in
  the air; the build's gave one more).

## Chaos Wheel (`wheel`)

One mutator at a time for every fighter, changed every 20 seconds: the match starts plain, the first mutator
comes on 20 seconds after the match starts, and each one ends as the next comes on, until the match is over. The
match set-up carries the rule as `wheel: { pool, seconds, warn }` (`mvs-protocol` `setup.rs`, filled by
`lobby_rules::wheel_rule`): the mutator keys that may be rolled, the window's length and how far ahead the next
one is drawn. The sim's part is `mvs-sim` `world/wheel.rs`. Works in every team style, with Stock Mode and
beside the Mutators row.

- The pool (`mvs-mutators`, each with its tier): Mild (sky blue): Giant, Miniature, Low Gravity, Air Show,
  Shields Up; Wild (violet): Big Head, Unlimited Jumps, Ability Frenzy, Heavy Hitters, Double Damage, Ice Charge,
  Freeze Touch, Random Items. The reel and the card show a mutator in its tier's colour. Not on the wheel: Grow (it grows with damage dealt, which 20 seconds barely shows), Instant Kill
  (the 200 damage would stay after its window), Silly and Final Point Shrink (whole-match rules).
- The next mutator is drawn 3 seconds before the change from a random stream of the rule's own (made from the
  match seed, so it moves no other draw of the match). The draw never gives the one that is on, one ticked in the
  Mutators row (it is on all match), or one the match cannot run: Random Items on a stage with no item area, or
  in Free For All, whose own spawner runs all match.
- On, every fighter alive gets the mutator's buff as the build's Apply Mutators To Fighter gives it; a fighter
  that respawns inside a window gets it at the respawn. Off, the buff is removed, and the rule takes back what
  two of them leave behind in the build (a match's mutator never ends there): Low Gravity's override and Air
  Show's count. Shields Up's grey health stays until it is hit off; an Ice stack a window gave runs out by itself;
  a projectile thrown as a Giant stays big.
- Random Items' window runs the stage's item spawner at a rate of the rule's own (the first item 2 seconds in,
  one every 3 seconds after, no centre safe): the build's rate would give one item in 20 seconds. Items out when
  the window ends stay.
- A cooldown multiplier's end (Ability Frenzy, Big Head) multiplies each cooldown's length by new / old, as the
  build's buffs end: some lengths come back one float step off (14 seconds as 14.000001).
- The rule's saved state is by player slot: a mode with more pawns than players (Waves) does not take it.
- The page's row is Chaos Wheel, Off or On; the Lab's page does not have it. The option's value is a bool, and
  the pool, the window and the warning are the rule's own constants (`WHEEL_MUTATORS`, `WHEEL_SECONDS`,
  `WHEEL_WARN`), not settings. Offline the menus fill the set-up from the lobby's options; online the host core
  does (`mvs-host`), so every member's match has the same rule.
- HUD (`mvs-ui` `hud/wheel.rs`): a card under the scoreboard's clock (above it with the HUD at the bottom), at
  the scoreboard's scale and opacity, in its fonts: a label, the mutator's name and a bar that runs down over the
  window. The first window reads GET READY over the rule's name. When the next mutator is drawn the label reads
  NEXT and the name spins through the pool, landing on the drawn one by half the warning; the spin is a function
  of the ticks left, so a step simulated again shows the same. At the change the mutator's name shows once as a
  screen banner, with the parameters of the Silly selector's pick. The countdown card names the rule on a second
  line.
- Sounds: one of the build's UI events at the draw and one at the change (`ROLL_SOUND`, `CHANGE_SOUND`), both
  from the banks every match keeps loaded. The beta has no sound for the rule; the two are picked by name.
- A window of Random Items needs the items' sounds, so a match with the rule loads them with the match's own.
