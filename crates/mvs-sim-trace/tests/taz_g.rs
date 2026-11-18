//! Taz's TeamFightCloud with a teammate, a path no trace runs (the trace's teammate never presses in the cloud): a
//! fresh 2v2 World (Taz 0 with the perk, his teammate Taz 2, against two Shaggys). Taz plays his up special, the
//! teammate walks under him into the cloud and presses a neutral attack: Buff_C015AttachToDogpile carries him in the
//! cloud, which grows and lasts longer and gives Taz two armor packs. Then either he stays to the cloud's end (put at
//! Taz's height and thrown up) or leaves with a jump (the cloud back to its own size and time, the armor off). Each
//! tick writes one line; the lines asserted are the oracle's (a probe of the oracle's cloud, dogpile
//! and attach-ally code on the same setup and inputs).

use mvs_sim::actors::WorldActorState;
use mvs_sim::actors::item::ItemExt;
use mvs_sim::chars::taz::dogpile::world_scale;
use mvs_sim::combat::buffs::BuffExt;
use mvs_sim::events::NotifyRef;
use mvs_sim::fighter::match_side::{Foreign, SpawnRequest};
use mvs_sim::setup::{MatchPlayer, MatchSetup, TeamStyle};
use mvs_sim::{InputState, Real};
use mvs_sim_trace::harness::{self, Harness};

/// The probe's number: rounded to 3 places (Math.round: halves up) and written as JavaScript's number formatting writes
/// it.
fn r(v: Real) -> String {
    let k = (v * 1000.0 + 0.5).floor() / 1000.0;
    if k == 0.0 { "0".into() } else { format!("{k}") }
}

/// One tick's line: tick, phase; the perk buff's connected, ally, attach, labels, inputs and inside; the live cloud's
/// alpha, x, y, weak hits and scale; the teammate's x, y, invulnerability count, invisibility and carry buff; Taz's
/// armor packs, x and y; the enemies' damage.
fn line(h: &Harness, tag: &str) -> String {
    let w = &h.w;
    let (taz, mate) = (h.fighter(0), h.fighter(2));
    let cloud = w
        .fighter(taz)
        .buffs
        .buffs
        .iter()
        .find(|b| b.frames > 0.0 && w.names.get("Buff_C015_TeamFightCloud_C") == Some(b.cls));
    let c = match cloud.map(|b| &b.ext) {
        Some(BuffExt::TeamFightCloud(e)) => {
            let inside: Vec<String> = e.inside.iter().map(|&o| w.fighter(o).id.to_string()).collect();
            let inside = if inside.is_empty() { "-".to_owned() } else { inside.join(",") };
            [
                i32::from(e.connected).to_string(),
                e.ally.map_or(-1, |o| w.fighter(o).id).to_string(),
                i32::from(e.attach.is_some()).to_string(),
                e.labels.len().to_string(),
                e.inputs.len().to_string(),
                inside,
            ]
            .join(" ")
        }
        _ => "nocloud".to_owned(),
    };
    let d = w.actors.actors.iter().find_map(|(id, a)| match &a.state {
        WorldActorState::Dogpile(s) if !a.dead => Some((id, s)),
        _ => None,
    });
    let ds = d.map_or("-".to_owned(), |(id, s)| {
        format!("{},{},{},{},{}", r(s.alpha), r(s.x), r(s.y), s.weak_hits, r(world_scale(w, id)))
    });
    let fm = w.fighter(mate);
    let at = fm.buffs.buffs.iter().any(|b| b.frames > 0.0 && w.names.get("Buff_C015AttachToDogpile_C") == Some(b.cls));
    let ft = w.fighter(taz);
    [
        w.flow.tick.to_string(),
        tag.to_owned(),
        c,
        ds,
        r(fm.body.x),
        r(fm.body.y),
        fm.invuln.invuln_count.to_string(),
        i32::from(fm.vis.invisible).to_string(),
        i32::from(at).to_string(),
        ft.armor.armor_packs.len().to_string(),
        r(ft.body.x),
        r(ft.body.y),
        r(w.fighter(h.fighter(1)).stats.damage),
        r(w.fighter(h.fighter(3)).stats.damage),
    ]
    .join(" ")
}

/// The lines of a run: `leave` presses jump 20 ticks after the carry starts.
fn run(leave: bool) -> Option<Vec<String>> {
    let player = |c: &str, perks: &[&str]| MatchPlayer {
        character: Some(c.to_owned()),
        perks: perks.iter().map(|p| (*p).to_owned()).collect(),
        ..MatchPlayer::default()
    };
    let setup = MatchSetup {
        team_style: TeamStyle::TwoVsTwo,
        players: vec![
            player("taz", &["Perk_C015_TeamFightCloud"]),
            player("shaggy", &[]),
            player("taz", &[]),
            player("shaggy", &[]),
        ],
        ..MatchSetup::default()
    };
    let mut h = harness::fresh(&setup, 1)?;
    let i = InputState::default;
    let mut phases: Vec<(usize, &str, InputState, InputState)> = vec![
        (10, "idle", i(), i()),
        (1, "up", InputState { y: 1.0, special: true, ..i() }, InputState { x: 1.0, ..i() }),
        (11, "walk", i(), InputState { x: 1.0, ..i() }),
        (26, "stop", i(), i()),
        (1, "press", i(), InputState { attack: true, ..i() }),
    ];
    if leave {
        phases.extend([
            (20, "held", i(), i()),
            (1, "leave", i(), InputState { jump: true, ..i() }),
            (200, "after", i(), i()),
        ]);
    } else {
        phases.push((480, "after", i(), i()));
    }
    let mut out = Vec::new();
    for (n, tag, a, b) in phases {
        for _ in 0..n {
            h.step(&[a, i(), b, i()]);
            out.push(line(&h, tag));
        }
    }
    if let Ok(dir) = std::env::var("MVS_CLOUD_DUMP") {
        let name = if leave { "rs_leave.txt" } else { "rs_stay.txt" };
        std::fs::write(format!("{dir}/{name}"), out.join("\n") + "\n").expect("the dump");
    }
    Some(out)
}

/// Each oracle line is the run's line of its tick and phase.
fn check(lines: &[String], want: &[&str]) {
    for w in want {
        let key: Vec<&str> = w.split(' ').take(2).collect();
        let got = lines.iter().find(|l| l.split(' ').take(2).eq(key.iter().copied()));
        assert_eq!(got.map(String::as_str), Some(*w), "tick {} {}", key[0], key[1]);
    }
}

// (The oracle agrees on every tick of both runs as written; these are its turns.)

/// The cloud's spawn with the teammate inside (his label and input bound), the carry (the buff, his place under Taz,
/// invulnerable and unseen, the grown cloud, Taz's two armor packs), the cloud's growth to its end at the new time,
/// and the end: the teammate put at Taz's height and thrown up.
#[test]
fn the_teammate_rides_the_cloud_to_its_end_as_the_oracle() {
    let Some(lines) = run(false) else { return };
    check(
        &lines,
        &[
            "45 stop 0 2 0 1 1 0,2 0.006,-732.232,527.846,0,1.008 -757.326 180.412 0 0 0 0 -732.232 544.346 0 0",
            "46 stop 0 2 0 1 1 0,2 0.011,-732.232,529.455,0,1.017 -757.326 180.412 0 0 0 0 -732.232 545.955 0 0",
            "49 press 0 2 1 0 0 - 0.025,-732.232,530.915,0,1.537 -732.232 403.415 2 1 1 2 -732.232 547.415 0 0",
            "50 after 0 2 1 0 0 - 0.027,-732.232,530.856,0,1.54 -732.232 403.356 2 1 1 2 -732.232 547.356 0 0",
            "60 after 0 2 1 0 0 - 0.051,-732.232,515.304,0,1.576 -732.232 387.804 2 1 1 2 -732.232 531.804 0 0",
            "100 after 0 2 1 0 0 - 0.146,-732.232,180.873,0,1.719 -732.232 53.373 2 1 1 2 -732.232 197.373 0 0",
            "102 after 0 2 1 0 0 - 0.151,-732.232,163.912,0,1.726 -732.232 36.412 2 1 1 2 -732.232 180.412 0 0",
            "459 after 0 2 1 0 0 - 1,-732.232,163.912,0,3 -732.232 36.412 2 1 1 2 -732.232 180.412 0 0",
            "461 after 0 -1 1 0 0 - - -732.232 180.412 0 0 0 0 -732.232 180.412 0 0",
            "462 after 0 -1 1 0 0 - - -732.232 221.364 0 0 0 0 -732.232 180.412 0 0",
            "466 after 0 -1 1 0 0 - - -732.232 370.881 0 0 0 0 -732.232 193.072 0 0",
        ],
    );
}

/// The teammate leaves with a jump: the carry buff's end (its own press), the cloud back to its own size and time
/// (OnAttachBuffDestroyed's UpgradeDogpileMulticast with the originals), the armor off, and the cloud's earlier end.
#[test]
fn the_teammate_leaves_the_cloud_as_the_oracle() {
    let Some(lines) = run(true) else { return };
    check(
        &lines,
        &[
            "69 held 0 2 1 0 0 - 0.072,-732.232,478.032,0,1.608 -732.232 350.532 2 1 1 2 -732.232 494.532 0 0",
            "70 leave 0 2 1 0 0 - 0.078,-732.232,472.53,0,1.117 -732.232 493.818 0 0 0 0 -732.232 489.03 0 0",
            "71 after 0 2 1 0 0 - 0.083,-732.232,466.755,0,1.125 -732.232 491.674 0 0 0 0 -732.232 483.255 0 0",
            "72 after 0 2 1 0 0 - 0.089,-732.232,460.708,0,1.133 -732.232 545.959 0 0 0 0 -732.232 477.208 0 0",
            "100 after 0 2 1 0 0 - 0.244,-732.232,180.873,0,1.367 -732.232 632.934 0 0 0 0 -732.232 197.373 0 0",
            "236 after 0 2 1 0 0 - 1,-732.232,163.912,0,2.5 -732.232 180.412 0 0 0 0 -732.232 180.412 0 0",
            "238 after 0 -1 1 0 0 - - -732.232 180.412 0 0 0 0 -732.232 180.412 0 0",
            "240 after 0 -1 1 0 0 - - -732.232 180.412 0 0 0 0 -732.232 183.781 0 0",
        ],
    );
}

/// The anvil's line: tick, phase; the anvil's x, y, vx, vy, platforms on, dead, durability, flying and thrown; Taz's
/// and Shaggy's places; Shaggy's damage.
fn anvil_line(h: &Harness, tag: &str) -> String {
    let w = &h.w;
    let cls = w.names.get("C015Anvil_C");
    let a = w.actors.items.iter().find(|(_, x)| Some(x.cls) == cls).map(|(_, x)| x);
    let s = a.map_or("-".to_owned(), |a| {
        let on = matches!(&a.ext, ItemExt::Anvil(s) if s.platforms_on);
        [
            r(a.x),
            r(a.y),
            r(a.vx),
            r(a.vy),
            i32::from(on).to_string(),
            i32::from(a.dead).to_string(),
            r(a.durability),
            i32::from(a.flying).to_string(),
            i32::from(a.thrown).to_string(),
        ]
        .join(",")
    });
    let (t, sh) = (w.fighter(h.fighter(0)), w.fighter(h.fighter(1)));
    [
        w.flow.tick.to_string(),
        tag.to_owned(),
        s,
        r(t.body.x),
        r(t.body.y),
        r(sh.body.x),
        r(sh.body.y),
        r(sh.stats.damage),
    ]
    .join(" ")
}

/// C015Anvil_C, which no trace match spawns (the perk's eat of a large projectile, then the spit): a fresh 1v1 (Taz
/// with EatProjectiles against Shaggy), the spit montage's anvil notify's request pushed as the burp pushes it with no
/// stored speeds. The anvil flies through the stage's pass-through platforms, hits the stage (ItemThrowHit: a
/// durability off, the platforms on from then), bounces back and comes to rest on a platform, as the oracle.
#[test]
fn the_anvil_flies_hits_and_rests_as_the_oracle() {
    let player = |c: &str, perks: &[&str]| MatchPlayer {
        character: Some(c.to_owned()),
        perks: perks.iter().map(|p| (*p).to_owned()).collect(),
        ..MatchPlayer::default()
    };
    let setup = MatchSetup {
        players: vec![player("taz", &["Perk_C015_EatProjectiles"]), player("shaggy", &[])],
        ..MatchSetup::default()
    };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let none = [InputState::default(); 2];
    let mut out = Vec::new();
    for (n, tag) in [(10, "idle"), (6, "stop")] {
        for _ in 0..n {
            h.step(&none);
            out.push(anvil_line(&h, tag));
        }
    }
    let f = h.fighter(0);
    let montage = h.w.names.get("C015_Special_SpitProjectile_Ground_Montage").expect("the spit montage's name");
    let r = SpawnRequest {
        cls: h.w.names.get("C015Anvil_C").expect("the anvil's name"),
        n: NotifyRef { montage, index: 8 },
        attack_id: h.w.fighter(f).decay.attack_count,
        foreign: Foreign::None,
        copy: None,
        charge: None,
        aim: None,
        props: None,
        instigator: None,
        origin: None,
    };
    h.w.fighter_mut(f).requests.spawn_requests.push(r);
    for _ in 0..200 {
        h.step(&none);
        out.push(anvil_line(&h, "after"));
    }
    if let Ok(dir) = std::env::var("MVS_CLOUD_DUMP") {
        std::fs::write(format!("{dir}/rs_anvil.txt"), out.join("\n") + "\n").expect("the dump");
    }
    check(
        &out,
        &[
            "17 after -629.167,190.42,1250,576,0,0,3,1,0 -800 180.412 800 210.412 0",
            "50 after 45.393,70.809,1061.481,-909.056,0,0,3,1,0 -800 180.412 800 210.412 0",
            "53 after 53.527,72.178,-249.917,467.175,1,0,2,0,0 -800 180.412 800 210.412 0",
            "110 after -182.75,65.344,-247.554,-463.757,1,0,2,0,0 -800 180.412 800 210.412 0",
            "116 after -191,63.531,0,83.473,1,0,2,0,0 -800 180.412 800 210.412 0",
            "134 after -191,56.332,0,0,1,0,2,0,0 -800 180.412 800 210.412 0",
            "216 after -191,56.332,0,0,1,0,2,0,0 -800 180.412 800 210.412 0",
        ],
    );
}
