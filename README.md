# MVS Classic

A fan-made port of the 2023 MultiVersus open beta, rewritten from scratch as one native Windows program in Rust.
It plays like the beta did: the same characters, moves, stages and menus, read from your own copy of the beta.
It doesn't include any of the game's files.
> [!IMPORTANT]
> MVS Classic contains nothing from the original game. To play, you need your own copy of the 2023 MultiVersus
> open beta, which the launcher builds the game from. The [install guide](#install) shows how to download it.

## What's in it

- The open beta's roster, stages and match rules, with the beta's numbers and timings
- Local matches against friends or bots, the Lab, and the tutorials
- Online play with rollback netcode: quick play, custom lobbies and parties
- The beta's menus, HUD, effects and sound, drawn with Direct3D 11
- Built to run well on a wide range of PCs. The game keeps the beta's fixed 60 Hz tick rate, while the frame rate is
  uncapped and frames between ticks are interpolated, so high refresh rate displays get smooth motion

### Added by the port

The port also has a few features of its own. None of them change the characters or how a standard match plays.

- **Lab Tools.** A training panel in the Lab, opened with F1 or the right stick button. It shows collision shapes,
  projectiles, stage bounds and each character's current state, and changes the game speed. It can also set or lock
  a character's damage, make it invulnerable, give it unlimited air moves or hold it in place, and hide the HUD or
  visual effects.
- **Experimental settings.** Optional rules for custom lobbies, off by default and set by the lobby leader for
  everyone in it. Stock Mode gives each player a number of lives, and the last player or team with lives left wins.
  Mutators apply one of the beta's own mutators, such as Big Head or Low Gravity, to the whole match.
- **Text chat.** Messages in custom lobbies and online matches, sent to your team or to everyone.
- **Social.** A list of players who are online, invites, parties, and join links and short codes for
  custom lobbies.

## Screenshots

<p>
  <a href="docs/images/main-menu.jpg"><img src="docs/images/main-menu.jpg" alt="The main menu" width="49%"></a>
  <a href="docs/images/custom-games.jpg"><img src="docs/images/custom-games.jpg" alt="The custom games list" width="49%"></a>
</p>

<p>
  <a href="docs/images/pre-match.jpg"><img src="docs/images/pre-match.jpg" alt="The pre-match screen" width="24%"></a>
  <a href="docs/images/arcade.jpg"><img src="docs/images/arcade.jpg" alt="An Arcade match on Sky Arena" width="24%"></a>
  <a href="docs/images/character-select.jpg"><img src="docs/images/character-select.jpg" alt="The character select" width="24%"></a>
  <a href="docs/images/podium.jpg"><img src="docs/images/podium.jpg" alt="The podium after a match" width="24%"></a>
</p>
<p>
  <a href="docs/images/lab.jpg"><img src="docs/images/lab.jpg" alt="The Lab" width="24%"></a>
  <a href="docs/images/lab-display.jpg"><img src="docs/images/lab-display.jpg" alt="Lab Tools showing hitboxes, hurtboxes and stage bounds" width="24%"></a>
  <a href="docs/images/lab-settings.jpg"><img src="docs/images/lab-settings.jpg" alt="Lab Tools settings" width="24%"></a>
  <a href="docs/images/lab-fighters.jpg"><img src="docs/images/lab-fighters.jpg" alt="Lab Tools fighter options" width="24%"></a>
</p>
<p>
  <a href="docs/images/custom-lobby.jpg"><img src="docs/images/custom-lobby.jpg" alt="A custom game lobby" width="24%"></a>
  <a href="docs/images/launcher.jpg"><img src="docs/images/launcher.jpg" alt="The launcher" width="24%"></a>
  <a href="docs/images/launcher-setup.jpg"><img src="docs/images/launcher-setup.jpg" alt="Locating the open beta" width="24%"></a>
  <a href="docs/images/launcher-build.jpg"><img src="docs/images/launcher-build.jpg" alt="Building the game" width="24%"></a>
</p>

## What you need

- **Windows 10 or 11, 64-bit**, with a graphics card that supports Direct3D 11
- **MultiVersus in your Steam library.** The open beta is downloaded through Steam, which only works for accounts
  that have MultiVersus.
- **About 16 GB of disk space:** about 8 GB for the open beta and 8 GB for the game the launcher builds from it

## Install

1. **Download MVS Classic.** Download the zip from the latest release and unzip it into the folder where you want
   the game.
2. **Download the open beta.** It's still on Steam and is downloaded through the Steam console, about 8 GB. The
   launcher shows these steps too.
   1. Start Steam and sign in.
   2. Open the Steam console: enter `steam://open/console` in your browser's address bar or in the Windows Run
      dialog.
   3. Paste this into the console and press Enter:

      ```
      download_depot 1818750 1818751 4062948038835516814
      ```

   4. Wait for the message "Depot download complete", which names the folder it was saved to. The console shows no
      progress while it downloads.
3. **Set up the game.** Run `MVSC Launcher.exe` and follow its three steps:
   1. Choose where to install the game.
   2. Locate your open beta: press **Auto detect**, or **Choose folder** and pick the `depot_1818751` folder Steam
      created.
   3. Build the game. The launcher builds it from your copy of the beta. This happens once and needs about 8 GB of
      free space. Your copy of the beta is only read, never changed.
4. **Play.** Press Play in the launcher. Each time it opens, it checks this repository's releases for a newer
   version and offers to update.

## Online

### How it works

Online matches use rollback netcode. Each player's game runs the whole match and only the players' inputs are sent.
When an input arrives late, the game rewinds to that moment and replays the match with it, so a match never stops to
wait for a slow connection.

Players find each other through a relay server. It hosts custom lobbies, parties, invites and the quick play queue,
and passes a match's inputs between the players. It runs no game rules and holds no game files, and players never
see each other's IP addresses. By default the game uses a relay hosted for the project in Europe.

The launcher's Settings > Online has the other ways to connect:

- **Relay server:** the address of another relay, such as one a friend or a community runs.
- **Epic Online Services:** custom games and party invites through Epic, and matchmaking when no relay is
  available.
- **Local network (LAN):** host and join custom games with players on the same network.
- **Direct connections:** connect straight to friends in a custom game when both networks allow it. This can lower
  the lag. It's off by default.

### Host your own relay

Anyone can run a relay, on a PC at home or on a rented server:

1. Download the relay's zip from the same release: `mvs-relay.exe` for Windows and `mvs-relay` for Linux (x64).
2. Put it in a folder of its own and start it. On its first start it writes its config and its own key beside it;
   keep a backup of the key.
3. Open port 7777 for TCP and UDP: forward it on your router for a home PC, or open it in the server's firewall.
4. Give players the address. They enter it in the launcher under Settings > Online > Relay server, for example
   `relay.example.org` or `203.0.113.5:7777`.

Whoever runs a relay can read the traffic that passes through it and sees the addresses of the players who connect.
[docs/relay.md](docs/relay.md) has the full guide: running it as a Linux service, the config, backing up the key and
what players' games check.

## Build from source

You need Rust (stable) and the Visual Studio C++ build tools.

```
cargo build --release -p mvs-app
```

This builds `target\release\mvs-launcher.exe`, the launcher that builds the game from your copy of the beta, and
`target\release\mvs.exe`, the game. A release names them `MVSC Launcher.exe` and `game\MVS Classic.exe`.

## Notes

MVS Classic started out as a browser game written in TypeScript with three.js, later wrapped in Electron as a
desktop app. It got the matches playing, but a browser holds a game like this back: it ran heavier than it should,
the beta's own shaders had to be translated for WebGL, and the whole thing sat on a stack of web tools. So it was rewritten
as one native Rust program, piece by piece, with its own Direct3D 11 renderer, to run well across a wide range
of hardware, lower-end PCs included.

The TypeScript version is not published. While porting, it served as the reference: the Rust sim was checked
against its recorded matches tick by tick until they matched exactly. That's what code comments mean when they
mention "the TS" or "the oracle". [crates/README.md](crates/README.md) has more.

## About

MVS Classic is not affiliated with or endorsed by Warner Bros. Games or Player First Games. MultiVersus and its
characters are trademarks of their owners.

The code is under the [MIT licence](LICENSE). Third-party code keeps its own licence: see the `NOTICE` and `LICENSE`
files in the crates that hold it.
