//! The stage-B class: the fields of the record the build's float32 and math routines must not move (the event
//! list, the state machines' states, montage names, sections and notify ticks, counts, ids, ticks, the inputs, the
//! HUD's damage), as a table of path patterns and rules. A compare that keeps only these differences checks the
//! sim against the oracle's f64 traces once its floats are the build's own.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::field::{Difference, general_path, parse_shown_f64};

/// How the class reads a field's difference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Every difference counts.
    Keep,
    /// Every difference counts; the number is an integer (a count, an id, a tick, an enum held as a number), which
    /// the class's test checks on the oracle's traces.
    Int,
    /// No difference counts.
    Skip,
    /// Two numbers are not compared (a float); any other difference counts (a string, a bool, a value on one
    /// side only, a list's length, an event's kind).
    Floats,
    /// Two numbers are compared as the integer `Math.trunc` makes of them.
    Trunc,
    /// Two numbers are compared as the integer `Math.round` makes of them.
    Round,
}

/// The class: a difference takes the rule of the first pattern its path (list indexes left out) matches. In a
/// pattern `*` stands for any text without a `.`, `**` for any text.
pub const STABLE: &[(&str, Rule)] = &[
    // The inputs as given to the step: the recorded ones are fed back as they are; a bot's are its choices.
    ("inputs[].**", Rule::Keep),
    ("world.fighters[].inp.**", Rule::Keep),
    ("world.fighters[].prev.**", Rule::Keep),
    // The damage as the HUD shows it (UI_IGv3_PlayerMarker SetPercentage takes `Math.trunc(damage)`), the damage
    // dealt as the results screen does (`Math.round`).
    ("world.fighters[].damage", Rule::Trunc),
    ("world.fighters[].damage_dealt", Rule::Round),
    // Numbers that are not ticks, ids or counts though their names say so: seconds, a meter, a Map's key.
    ("world.fighters[].wall_fatigue_at", Rule::Floats),
    ("world.fighters[].evade_count", Rule::Floats),
    ("world.fighters[].active_hits[].key", Rule::Floats),
    ("world.fighters[].active_hits[].index", Rule::Floats),
    ("**.blastAt", Rule::Floats),
    ("**.gravAt", Rule::Floats),
    ("**.lifeAt", Rule::Floats),
    ("**.splineAt", Rule::Floats),
    ("**.evadeAt", Rule::Floats),
    ("**.jumpAt", Rule::Floats),
    ("**.nextTick", Rule::Floats),
    ("**.projAim.at", Rule::Floats),
    ("**.chargeRamp.at", Rule::Floats),
    ("**.disint.inc_at", Rule::Floats),
    ("**.ice_timer_at", Rule::Floats),
    // The World's counters, ticks, ids and scores.
    ("world.tick", Rule::Int),
    ("world.intro_ticks", Rule::Int),
    ("world.rng", Rule::Int),
    ("world.actor_seq", Rule::Int),
    ("world.item_seq", Rule::Int),
    ("world.winning_team", Rule::Int),
    ("world.match_start_tick", Rule::Int),
    ("world.match_over_tick", Rule::Int),
    ("world.ko_point_multiplier", Rule::Int),
    ("world.team_knockouts[]", Rule::Int),
    ("world.stocks[]", Rule::Int),
    ("world.ring_out_anchors", Rule::Int),
    ("world.announcer_until", Rule::Int),
    ("world.vo_speakers", Rule::Int),
    ("world.frame_hits[]", Rule::Int),
    ("world.bonus_trackers[].spree", Rule::Int),
    ("world.timers[].at", Rule::Int),
    ("world.timers[].key", Rule::Int),
    // A fighter's counters, frame counts, tick stamps, ids and enums held as numbers.
    ("world.fighters[].id", Rule::Int),
    ("world.fighters[].team", Rule::Int),
    ("world.fighters[].rng", Rule::Int),
    ("world.fighters[].facing", Rule::Int),
    ("world.fighters[].facing_dir", Rule::Int),
    ("world.fighters[].move_in", Rule::Int),
    ("world.fighters[].hitstun", Rule::Int),
    ("world.fighters[].hitpause", Rule::Int),
    ("world.fighters[].true_hitstun", Rule::Int),
    ("world.fighters[].ko_hitstun", Rule::Int),
    ("world.fighters[].queued_hit_pause", Rule::Int),
    ("world.fighters[].most_recent_attacker", Rule::Int),
    ("world.fighters[].previous_attacker", Rule::Int),
    ("world.fighters[].di_combo", Rule::Int),
    ("world.fighters[].attack_count", Rule::Int),
    ("world.fighters[].jumps", Rule::Int),
    ("world.fighters[].jump_frame", Rule::Int),
    ("world.fighters[].air_evades", Rule::Int),
    ("world.fighters[].air_specials", Rule::Int),
    ("world.fighters[].evade_invuln", Rule::Int),
    ("world.fighters[].invuln", Rule::Int),
    ("world.fighters[].invuln_count", Rule::Int),
    ("world.fighters[].rage_timer", Rule::Int),
    ("world.fighters[].kos", Rule::Int),
    ("world.fighters[].ring_outs", Rule::Int),
    ("world.fighters[].double_ring_outs", Rule::Int),
    ("world.fighters[].off_platform_ring_outs", Rule::Int),
    ("world.fighters[].ring_outs_assisted", Rule::Int),
    ("world.fighters[].assists", Rule::Int),
    ("world.fighters[].attacks_dodged", Rule::Int),
    ("world.fighters[].held_item", Rule::Int),
    ("world.fighters[].armor_vis", Rule::Int),
    ("world.fighters[].attack_is_active", Rule::Int),
    ("world.fighters[].attack_slot", Rule::Int),
    ("world.fighters[].taunt_slot", Rule::Int),
    ("world.fighters[].bounce_type", Rule::Int),
    ("world.fighters[].bounce_pose", Rule::Int),
    ("world.fighters[].hit_react_int", Rule::Int),
    ("world.fighters[].last_direction", Rule::Int),
    ("world.fighters[].hitstun_count", Rule::Int),
    ("world.fighters[].hitstun_counted", Rule::Int),
    ("world.fighters[].true_hitstun_counted", Rule::Int),
    ("world.fighters[].juggle_count", Rule::Int),
    ("world.fighters[].knocked_back_count", Rule::Int),
    ("world.fighters[].rapid_bounce_count", Rule::Int),
    ("world.fighters[].down_press_count", Rule::Int),
    ("world.fighters[].gravity_mod_count", Rule::Int),
    ("world.fighters[].outline_count", Rule::Int),
    ("world.fighters[].wall_touches", Rule::Int),
    ("world.fighters[].wall_stall", Rule::Int),
    ("world.fighters[].wall_fatigue", Rule::Int),
    ("world.fighters[].current_decay_sequence", Rule::Int),
    ("world.fighters[].last_decay_stacks", Rule::Int),
    ("world.fighters[].force_move_seq", Rule::Int),
    ("world.fighters[].linger_seq", Rule::Int),
    ("world.fighters[].loco_gen", Rule::Int),
    ("world.fighters[].vo_seq", Rule::Int),
    ("world.fighters[].vo_recent", Rule::Int),
    ("world.fighters[].charge_sound_seq", Rule::Int),
    ("world.fighters[].charge_notify", Rule::Int),
    ("world.fighters[].charge_ramp_delay", Rule::Int),
    ("world.fighters[].charge_hit_small_at", Rule::Int),
    ("world.fighters[].charge_hit_large_at", Rule::Int),
    ("world.fighters[].air_attack_delay", Rule::Int),
    ("world.fighters[].anim_attack_off", Rule::Int),
    ("world.fighters[].assist_clear_tick", Rule::Int),
    ("world.fighters[].blast_in", Rule::Int),
    ("world.fighters[].bounce_fx_until", Rule::Int),
    ("world.fighters[].buff_stun", Rule::Int),
    ("world.fighters[].buff_stun_age", Rule::Int),
    ("world.fighters[].combo_reset_tick", Rule::Int),
    ("world.fighters[].dash_tick", Rule::Int),
    ("world.fighters[].down_window_tick", Rule::Int),
    ("world.fighters[].evade_regen_start", Rule::Int),
    ("world.fighters[].ff_check_tick", Rule::Int),
    ("world.fighters[].floor_swept", Rule::Int),
    ("world.fighters[].ground_jump_tick", Rule::Int),
    ("world.fighters[].jump_cut_at", Rule::Int),
    ("world.fighters[].last_knocked_back", Rule::Int),
    ("world.fighters[].no_wall_slide_until", Rule::Int),
    ("world.fighters[].plat_ignored", Rule::Int),
    ("world.fighters[].pre_spawn_delay", Rule::Int),
    ("world.fighters[].quick_fall_at", Rule::Int),
    ("world.fighters[].rot_lock_until", Rule::Int),
    ("world.fighters[].true_hitstun_set_tick", Rule::Int),
    ("world.fighters[].true_hitstun_tick_at", Rule::Int),
    ("world.fighters[].tick", Rule::Int),
    ("world.fighters[].spawned[]", Rule::Int),
    ("world.fighters[].landed_attack_ids[]", Rule::Int),
    ("world.fighters[].landed_decay_sequences[]", Rule::Int),
    ("world.fighters[].combo_res_map[].value", Rule::Int),
    ("world.fighters[].active_hits[].attack_id", Rule::Int),
    ("world.fighters[].active_hits[].hit_set[]", Rule::Int),
    ("world.fighters[].pending_ends[].index", Rule::Int),
    ("world.fighters[].late_branch.windows[].index", Rule::Int),
    ("world.fighters[].**queued.dir", Rule::Int),
    ("world.fighters[].**queued.raw", Rule::Int),
    ("world.fighters[].**queued.windows[]", Rule::Int),
    ("world.fighters[].*montage.active[]", Rule::Int),
    ("world.fighters[].*montage.age", Rule::Int),
    ("world.fighters[].*montage.blend_left", Rule::Int),
    ("world.fighters[].rage_input.active[]", Rule::Int),
    ("world.fighters[].rage_input.age", Rule::Int),
    ("world.fighters[].rage_input.blend_left", Rule::Int),
    ("world.fighters[].abilities[].ammo", Rule::Int),
    ("world.fighters[].abilities[].cd_frames", Rule::Int),
    ("world.fighters[].abilities[].ammo_frames", Rule::Int),
    ("world.fighters[].buffs[].stacks", Rule::Int),
    ("world.fighters[].buffs[].born", Rule::Int),
    ("world.fighters[].buffs[].by", Rule::Int),
    ("world.fighters[].buffs[].rescue", Rule::Int),
    ("world.fighters[].buffs[].gate", Rule::Int),
    ("world.fighters[].buffs[].add_age", Rule::Int),
    ("world.fighters[].armor[].max_hits", Rule::Int),
    ("world.fighters[].net.kind", Rule::Int),
    ("world.fighters[].impulse.kind", Rule::Int),
    ("world.fighters[].move_cmds[].index", Rule::Int),
    // The events' numbers that are not floats: counts, slots, ammo, scores, enums.
    ("world.events[]<abilityAmmo>.ammo", Rule::Int),
    ("world.events[]<abilityUiCreate>.maxCount", Rule::Int),
    ("world.events[]<aryaKnife>.knife", Rule::Int),
    ("world.events[]<comboCounts>.hitstunCount", Rule::Int),
    ("world.events[]<comboCounts>.juggleCount", Rule::Int),
    ("world.events[]<hitVfx>.decayStacks", Rule::Int),
    ("world.events[]<itemUiCreate>.action", Rule::Int),
    ("world.events[]<shockwaveVfx>.n", Rule::Int),
    ("world.events[]<teamScore>.score", Rule::Int),
    ("world.events[]<uiAbility>.ammo", Rule::Int),
    ("world.events[]<uiAbility>.n", Rule::Int),
    ("world.events[]<uiState>.op.add.stateType", Rule::Int),
    ("world.events[]<uiState>.op.add.displayType", Rule::Int),
    // By name, anywhere (the events, the actors' and the characters' own state, a buff's extension): which fighter,
    // actor or item, which attack, which notify track, stacks, counts, sequence numbers and tick stamps.
    ("**.id", Rule::Int),
    ("**.fighter", Rule::Int),
    ("**.owner", Rule::Int),
    ("**.holder", Rule::Int),
    ("**.victim", Rule::Int),
    ("**.attacker", Rule::Int),
    ("**.instigator", Rule::Int),
    ("**.juggler", Rule::Int),
    ("**.target", Rule::Int),
    ("**.actor", Rule::Int),
    ("**.item", Rule::Int),
    ("**.team", Rule::Int),
    ("**.slot", Rule::Int),
    ("**.track", Rule::Int),
    ("**.attackId", Rule::Int),
    ("**.AttackID", Rule::Int),
    ("**.attack_id", Rule::Int),
    ("**.stacks", Rule::Int),
    ("**.StackCount", Rule::Int),
    ("**.set[]", Rule::Int),
    ("**.tick", Rule::Int),
    ("**Tick", Rule::Int),
    ("**_tick", Rule::Int),
    ("**.count", Rule::Int),
    ("**Count", Rule::Int),
    ("**_count", Rule::Int),
    ("**.seq", Rule::Int),
    ("**Seq", Rule::Int),
    ("**_seq", Rule::Int),
    ("**At", Rule::Int),
    ("**_at", Rule::Int),
    // Every other number is a float (a position, a velocity, a time in seconds, a blend, a meter, a scale, a data
    // constant); every other value is compared (a state, a name, a flag, a list's length, an event's kind).
    ("**", Rule::Floats),
];

/// The rule of a path (`world.fighters[1].x` or `world.fighters[].x`).
pub fn rule_of(path: &str) -> Rule {
    thread_local! {
        // (a frame of a float32 run differs from the oracle's in thousands of floats: each path is matched once)
        static RULES: RefCell<HashMap<String, Rule>> = RefCell::new(HashMap::new());
    }
    let g = general_path(path);
    RULES.with(|m| {
        if let Some(&r) = m.borrow().get(&g) {
            return r;
        }
        let r = STABLE.iter().find(|(p, _)| glob(p.as_bytes(), g.as_bytes())).map_or(Rule::Floats, |&(_, r)| r);
        m.borrow_mut().insert(g, r);
        r
    })
}

/// Whether a difference is one the class counts.
pub fn keeps(d: &Difference) -> bool {
    let nums = parse_shown_f64(&d.expected).zip(parse_shown_f64(&d.actual));
    match (rule_of(&d.path), nums) {
        (Rule::Keep | Rule::Int, _) => true,
        (Rule::Skip, _) => false,
        (Rule::Floats | Rule::Trunc | Rule::Round, None) => true,
        (Rule::Floats, Some(_)) => false,
        (Rule::Trunc, Some((a, b))) => !same_int(a.trunc(), b.trunc()),
        (Rule::Round, Some((a, b))) => !same_int(js_round(a), js_round(b)),
    }
}

fn same_int(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

/// JavaScript's `Math.round`: the nearest integer, a half up (-2.5 to -2).
fn js_round(v: f64) -> f64 {
    let f = v.floor();
    if v - f >= 0.5 { f + 1.0 } else { f }
}

/// `p` matches all of `s`: `**` any bytes, `*` any bytes but `.`.
fn glob(p: &[u8], s: &[u8]) -> bool {
    match p {
        [] => s.is_empty(),
        [b'*', b'*', rest @ ..] => (0..=s.len()).any(|i| glob(rest, &s[i..])),
        [b'*', rest @ ..] => {
            let end = s.iter().position(|&c| c == b'.').unwrap_or(s.len());
            (0..=end).any(|i| glob(rest, &s[i..]))
        }
        [c, rest @ ..] => s.first() == Some(c) && glob(rest, &s[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_matches() {
        assert!(glob(b"**", b"world.x"));
        assert!(glob(b"world.*.x", b"world.a[].x"));
        assert!(!glob(b"world.*.x", b"world.a.b.x"));
        assert!(glob(b"world.events[]<*>.fighter", b"world.events[]<hit>.fighter"));
        assert!(glob(b"**.id", b"world.actors[].rest.a.id"));
        assert!(!glob(b"**.id", b"world.actors[].rest.a.kid"));
    }

    #[test]
    fn js_round_halves_go_up() {
        assert_eq!((js_round(2.5), js_round(-2.5), js_round(-2.6), js_round(0.49)), (3.0, -2.0, -3.0, 0.0));
    }
}
