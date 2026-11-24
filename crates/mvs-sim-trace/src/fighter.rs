//! A fighter's record (`FighterRecord`): each typed row from its Rust field, and the
//! `rest` bag, every other own key of the oracle Fighter by its oracle key, from the Rust field that holds it.

use mvs_sim::Fighter;
use mvs_sim::actors::ActorRef;
use mvs_sim::chars::CharState;
use mvs_sim::chars::shaggy::state::KickMix;
use mvs_sim::combat::decay::ComboReset;
use mvs_sim::fighter::body::PlatformRef;
use mvs_sim::fighter::tick::OnTurn;
use mvs_sim::names::Name;
use mvs_trace::{
    AryaRecord, BatmanRecord, BlackAdamRecord, CharRecord, FighterRecord, HarleyRecord, KickMixRecord, LebronRecord,
    LightningDirRecord, LightningRecord, LightningTargetRecord, MortyRecord, ReindogRecord, ShaggyRecord,
    SlotBlendRecord, SlotPlayersRecord, SlotPoseRecord, SupermanRecord, Value, WonderWomanRecord,
};

use crate::conv::{Plain, Rec, as_obj, check_keys, obj, opaque, plain_keys, plain_struct, rec_rows, rec_struct};
use crate::cx::{AIM_THROW, Lx, Wx, throw_aim_p};
use crate::parts::same_value;
use mvs_sim::events::NotifyRef;
use mvs_sim::fighter::match_side::{Aimer, ThrowAimInfo};

pub fn put(f: &Fighter, x: &Wx) -> FighterRecord {
    let mut r = FighterRecord {
        id: f64::from(f.id),
        character: x.str(f.ch).to_owned(),
        // (the oracle getters `facing` and `grounded` read `facingDir` and `onFloor`)
        facing: f64::from(f.body.facing_dir),
        grounded: f.body.on_floor,
        bot_controlled: f.controls.bot_controlled,
        state: char_put(f, x),
        rest: rest_put(f, x),
        ..FighterRecord::default()
    };
    r.inp = crate::parts::fighter_input(&f.controls.inp, x);
    r.prev = crate::parts::fighter_input(&f.controls.prev, x);
    // A VO line's key as the oracle writes it, `{id}:vo{n}` (the Rust line holds `n`).
    r.vo_current = f.vo.vo_current.map(|n| vo_key(f.id, n));
    // `chargeSoundKey` as the oracle writes it, `{id}:charge{seq}` (the Rust key holds the tag `:charge`).
    let ms = &f.montages;
    r.charge_sound_key = if ms.charge_sound_key == Name::NONE {
        String::new()
    } else {
        format!("{}{}{}", f.id, x.str(ms.charge_sound_key), ms.charge_sound_seq)
    };
    rec_rows!(put f, x, r;
        team: team.team,
        rng: rng,
        x: body.x,
        y: body.y,
        vx: body.vx,
        vy: body.vy,
        time: clock.time,
        kb_mode: hit.kb_mode,
        flying: moves.flying,
        ground_state: sm.gs,
        ground_state_time: sm.gs_time,
        crouching: crouch.crouching,
        fast_falling: fast_fall.fast_falling,
        wall_sliding: wall.wall_sliding,
        evading: evade.evading,
        move_in: air.move_in,
        damage: stats.damage,
        hitstun: hit.hitstun,
        hitpause: hit.hitpause,
        true_hitstun: hit.true_hitstun,
        ko_hitstun: life.ko_hitstun,
        launch: hit.launch,
        most_recent_attacker: attackers.most_recent_attacker,
        di_combo: attackers.di_combo,
        attack_count: decay.attack_count,
        jumps: jump.jumps,
        jump_frame: jump.jump_frame,
        air_evades: air.air_evades,
        air_specials: air.air_specials,
        air_attack_available: air.air_attack_available,
        evade_count: evade.evade_count,
        evade_invuln: evade.evade_invuln,
        invuln: invuln.invuln,
        invuln_count: invuln.invuln_count,
        rage: rage.rage,
        rage_enabled: rage.rage_enabled,
        rage_charging: rage.rage_charging,
        rage_filled: rage.rage_filled,
        rage_timer: rage.rage_timer,
        charge_pct: charge.charge_pct,
        dead: life.dead,
        invisible: vis.invisible,
        abducted: life.abducted,
        kos: stats.kos,
        ring_outs: stats.ring_outs,
        damage_dealt: stats.damage_dealt,
        size_scale: body.size_scale,
        gravity_scale: moves.gravity_scale,
        speed_mod: buffs.speed_mod,
        montage: montages.montage,
        dash_montage: montages.dash_montage,
        anim: anim.anim,
        anim_time: anim.anim_time,
        loco_anim: anim.loco_anim,
        loco_time: anim.loco_time,
        buffs: buffs.buffs,
        armor: armor.armor_packs,
        abilities: abilities.abilities,
        held_item: items.held_item,
        queued: branching.queued,
        late_branch: branching.late_branch,
        active_hits: montages.active_hits,
        pending_ends: branching.pending_ends,
        attack_decay: decay.attack_decay,
        landed_decay_sequences: decay.landed_decay_sequences,
        combo_res_map: decay.combo_res_map,
        recent_attacks: controls.recent_attacks,
        landed_attack_ids: decay.landed_attack_ids,
        equipped: pose.equipped,
        net: moves.net,
        impulse: moves.impulse,
        move_cmds: moves.move_cmds,
        move_end: moves.move_end,
        spawned: items.spawned_actors,
        add_kb_vx: hit.add_kb_vx,
        add_kb_vy: hit.add_kb_vy,
        aim_direction: pose.aim_direction,
        air_attack_delay: air.air_attack_delay,
        air_boost_mult: air.air_boost_mult,
        air_control: air.air_control,
        air_control_gate: air.air_control_gate,
        ak_x: body.ak_x,
        ak_y: body.ak_y,
        anim_attack_off: charge.anim_attack_off,
        anim_fast_fall_off: fast_fall.anim_fast_fall_off,
        anim_key: anim.anim_key,
        anim_updated: root.anim_updated,
        armor_vis: armor.armor_vis,
        assist_clear_tick: attackers.assist_clear_tick,
        assists: stats.assists,
        attack_is_active: charge.attack_is_active,
        attack_slot: abilities.attack_slot,
        attacks_dodged: stats.attacks_dodged,
        base_vx: hit.base_vx,
        base_vy: hit.base_vy,
        begin_attack: charge.begin_attack,
        blast_in: life.blast_in,
        blast_listed: life.blast_listed,
        block_jump_end_launch: jump.block_jump_end_launch,
        block_land_reset: crouch.block_land_reset,
        block_pick_up: items.block_pick_up,
        block_taunt_anims: controls.block_taunt_anims,
        bounce_fx_until: hit.bounce_fx_until,
        bounce_pitch: react.bounce_pitch,
        bounce_pose: pose.bounce_pose,
        bounce_type: hit.bounce_type,
        bounce_z_offset: hit.bounce_z_offset,
        bouncing: hit.bouncing,
        br_axis_x: branching.br_axis_x,
        br_axis_y: branching.br_axis_y,
        br_stick_x: branching.br_stick_x,
        branch_montage: branching.branch_montage,
        buff_stun: buffs.buff_stun,
        buff_stun_age: buffs.buff_stun_age,
        buff_stun_len: buffs.buff_stun_len,
        buff_stunned: buffs.buff_stunned,
        cam_focus: life.cam_focus,
        can_bounce_no_hitstun: hit.can_bounce_no_hitstun,
        cant_die: life.cant_die,
        cap_half_u: capsule.cap_half_u,
        cap_radius_u: capsule.cap_radius_u,
        cap_resized: capsule.cap_resized,
        capsule_off: capsule.capsule_off,
        charge_btn: branching.charge_btn,
        charge_buff_mult: charge.charge_buff_mult,
        charge_dmg_mult: charge.charge_dmg_mult,
        charge_hit_large_at: charge.charge_hit_large_at,
        charge_hit_small_at: charge.charge_hit_small_at,
        charge_mult: charge.charge_mult,
        charge_notify: charge.charge_notify,
        charge_ramp_delay: montages.charge_ramp_delay,
        charge_released: charge.charge_released,
        charge_sound_seq: montages.charge_sound_seq,
        charge_start_time: charge.charge_start_time,
        charge_value: montages.charge_value,
        cmc_crouched: capsule.cmc_crouched,
        cmd_jumped: moves.cmd_jumped,
        combo_reset_tick: attackers.combo_reset_tick,
        combo_resistance: decay.combo_resistance,
        cooldown_multiplier: abilities.cooldown_multiplier,
        creation_time: clock.creation_time,
        crouch_fx: crouch.crouch_fx,
        crouch_jump: jump.crouch_jump,
        crouch_locked: crouch.crouch_locked,
        crouch_owed: crouch.crouch_owed,
        current_decay_sequence: decay.current_decay_sequence,
        damage_disabled: invuln.damage_disabled,
        dash_fall_gate: air.dash_fall_gate,
        dash_game_time: air.dash_game_time,
        dash_stop_blend: sm.dash_stop_blend,
        dash_stop_delay: sm.dash_stop_delay,
        dash_tick: air.dash_tick,
        defer_queued_ends: branching.defer_queued_ends,
        direction_soft: react.direction_soft,
        dodged_successfully: evade.dodged_successfully,
        double_ring_outs: stats.double_ring_outs,
        down_btn: crouch.down_btn,
        down_press_active: platforms.down_press_active,
        down_press_count: platforms.down_press_count,
        down_stick: crouch.down_stick,
        down_window_tick: platforms.down_window_tick,
        dropping_down: platforms.dropping_down,
        end_jump_latent: jump.end_jump_latent,
        evade_crouch_rate: evade.evade_crouch_rate,
        evade_invuln_time: evade.evade_invuln_time,
        evade_regen_once: evade.evade_regen_once,
        evade_regen_start: evade.evade_regen_start,
        evading_through_platform: platforms.evading_through_platform,
        facing_dir: body.facing_dir,
        fade_anim: anim.fade_anim,
        fade_dur: anim.fade_dur,
        fade_el: anim.fade_el,
        fade_loop: anim.fade_loop,
        fade_rate: anim.fade_rate,
        fade_time: anim.fade_time,
        fall_lat_friction: air.fall_lat_friction,
        fall_lean: react.fall_lean,
        ff_check_tick: fast_fall.ff_check_tick,
        ff_mult: fast_fall.ff_mult,
        ff_triggered: fast_fall.ff_triggered,
        floor_bounce_hurt: hit.floor_bounce_hurt,
        floor_swept: body.floor_swept,
        fluid_friction: moves.fluid_friction,
        force_additive_kb: attackers.force_additive_kb,
        force_move_seq: moves.force_move_seq,
        frame_mode: frame.frame_mode,
        free_launch: hit.free_launch,
        friction_zero: moves.friction_zero,
        grappled: buffs.grappled,
        gravity_mod_count: moves.gravity_mod_count,
        ground_jump_tick: jump.ground_jump_tick,
        gs_rel: sm.gs_rel,
        has_hit_a_fighter: decay.has_hit_a_fighter,
        has_spawn_invuln: invuln.has_spawn_invuln,
        hit_by_multiple_attackers: attackers.hit_by_multiple_attackers,
        hit_launch: moves.hit_launch,
        hit_neutral: react.hit_neutral,
        hit_pause_time: react.hit_pause_time,
        hit_react_int: react.hit_react_int,
        hit_stun_ended_bound: attackers.hit_stun_ended_bound,
        hit_stun_kb_vx: react.hit_stun_kb_vx,
        hit_stun_kb_vy: react.hit_stun_kb_vy,
        hit_stunned_before_hit: attackers.hit_stunned_before_hit,
        hit_this_frame: frame.hit_this_frame,
        hitstun_count: attackers.hitstun_count,
        hitstun_counted: hit.hitstun_counted,
        hitstun_damage: attackers.hitstun_damage,
        hitstun_end_time: hit.hitstun_end_time,
        hitstun_paused: hit.hitstun_paused,
        hitstun_pending: hit.hitstun_pending,
        hitstun_rate: hit.hitstun_rate,
        holding_enemy: holds.holding_enemy,
        hr_branch: react.hr_branch,
        hr_intro: react.hr_intro,
        ignore_platforms: platforms.ignore_platforms,
        in_ability_timers: abilities.in_ability_timers,
        in_hit_pause_pose: hit.in_hit_pause_pose,
        instant_cooldowns: abilities.instant_cooldowns,
        is_minion: life.is_minion,
        item_holstered: items.item_holstered,
        item_reachable: items.item_reachable,
        juggle_count: attackers.juggle_count,
        juggle_damage: attackers.juggle_damage,
        jump_cut_at: jump.jump_cut_at,
        jump_launched: moves.jump_launched,
        jump_min_time: jump.jump_min_time,
        jump_released: jump.jump_released,
        jump_start_time: jump.jump_start_time,
        jump_stop_gate: jump.jump_stop_gate,
        jump_v: jump.jump_v,
        kb_facing: react.kb_facing,
        knock_back_angle: react.knock_back_angle,
        knocked_back_count: attackers.knocked_back_count,
        last_attacker_off_ledge: attackers.last_attacker_off_ledge,
        last_decay_stacks: attackers.last_decay_stacks,
        last_direction: air.last_direction,
        last_evade_spent_meter: evade.last_evade_spent_meter,
        last_evade_time: evade.last_evade_time,
        last_hitstun_time: hit.last_hitstun_time,
        last_jump_time: jump.last_jump_time,
        last_jump_v: moves.last_jump_v,
        last_knocked_back: attackers.last_knocked_back,
        last_move_in: air.last_move_in,
        last_ring_out_time: stats.last_ring_out_time,
        late_branch_due: branching.late_branch_due,
        layer_missed: charge.layer_missed,
        linger_seq: montages.linger_seq,
        loco_gen: anim.loco_gen,
        loco_loop: anim.loco_loop,
        loco_rate: anim.loco_rate,
        mesh_x: hit.mesh_x,
        mesh_z: hit.mesh_z,
        montage_abort: montages.montage_abort,
        montage_hit: montages.montage_hit,
        moved: frame.moved,
        net_friction: moves.net_friction,
        net_gravity: moves.net_gravity,
        no_walk_off_ledges: platforms.no_walk_off_ledges,
        no_wall_slide_until: wall.no_wall_slide_until,
        not_player_character: life.not_player_character,
        off_platform_ring_outs: stats.off_platform_ring_outs,
        on_camera: vis.on_camera,
        on_floor: body.on_floor,
        outline_count: life.outline_count,
        pawn_vy: platforms.pawn_vy,
        pickup_request: items.pickup_request,
        plain_is_intro: anim.plain_is_intro,
        plat_ignored: platforms.plat_ignored,
        played_in_advance: montages.played_in_advance,
        pose_ticked: root.pose_ticked,
        pre_spawn_delay: life.pre_spawn_delay,
        prev_bounce_time: hit.prev_bounce_time,
        prev_time: clock.prev_time,
        prev_x: body.prev_x,
        prev_y: body.prev_y,
        previous_attacker: attackers.previous_attacker,
        projectile_armor: invuln.projectile_armor,
        pushing_down: crouch.pushing_down,
        queued_hit_pause: hit.queued_hit_pause,
        queued_hit_pause_sec: hit.queued_hit_pause_sec,
        quick_fall_at: jump.quick_fall_at,
        rage_fx: rage.rage_fx,
        rage_glow_shown: rage.rage_glow_shown,
        rage_residual: rage.rage_residual,
        rage_started: rage.rage_started,
        rapid_bounce_count: hit.rapid_bounce_count,
        reach_recheck: items.reach_recheck,
        recent_hit_actor: decay.recent_hit_actor,
        ring_outs_assisted: stats.ring_outs_assisted,
        root_dx: root.root_dx,
        root_dy: root.root_dy,
        root_has: root.root_has,
        rot_lock_until: controls.rot_lock_until,
        same_attack_blocked: branching.same_attack_blocked,
        seq_delta: anim.seq_delta,
        seq_from: anim.seq_from,
        starting_damage: stats.starting_damage,
        stop_ahead: crouch.stop_ahead,
        stun_source: attackers.stun_source,
        suspended: buffs.suspended,
        taunt_slot: controls.taunt_slot,
        terminal_v: hit.terminal_v,
        tick: clock.tick,
        time_in_air: air.time_in_air,
        total_charged_time: charge.total_charged_time,
        true_hitstun_counted: hit.true_hitstun_counted,
        true_hitstun_rate: hit.true_hitstun_rate,
        true_hitstun_set_tick: hit.true_hitstun_set_tick,
        true_hitstun_tick_at: hit.true_hitstun_tick_at,
        vel_clamped: body.vel_clamped,
        vo_recent: vo.vo_recent,
        vo_seq: vo.vo_seq,
        vo_speaking: vo.vo_speaking,
        walk_drop: platforms.walk_drop,
        wall_capped: wall.wall_capped,
        wall_fatigue: wall.wall_fatigue,
        wall_fatigue_at: wall.wall_fatigue_at,
        wall_mult: wall.wall_mult,
        wall_skate: moves.wall_skate,
        wall_stall: wall.wall_stall,
        wall_timer_pending: wall.wall_timer_pending,
        wall_touches: wall.wall_touches,
        was_attacking: air.was_attacking,
        wind_on: hit.wind_on,
        zero_out_vel_for_kb: attackers.zero_out_vel_for_kb,
    );
    r.rage_input = rage_input_put(f, x);
    r
}

/// `rageInput`: the main montage instance the charge began on. Written as the main instance's record while that is
/// the instance, else its montage's name alone (the rest named unmapped: the record holds no instance that left the
/// main slot).
fn rage_input_put(f: &Fighter, x: &Wx) -> Option<mvs_trace::MontageRecord> {
    let n = f.rage.rage_input?;
    if let Some(m) = f.montages.montage.as_ref().filter(|m| m.montage == n) {
        return Some(m.put(x));
    }
    x.unmapped("rageInput past its main instance (the record holds the main instance only)");
    Some(mvs_trace::MontageRecord { name: x.str(n.name).to_owned(), ..mvs_trace::MontageRecord::default() })
}

/// Makes the data packs [`Plain::from_value`] left as placeholders (`from_data`, no rows) the packs of the data: the
/// only pack the oracle writes as a static object is Tom and Jerry's NoJerryTauntOverride (`f.data.pawn`).
pub fn resolve_data_packs(w: &mut mvs_sim::World, id: mvs_sim::fighter::FighterId, l: &mut Lx) {
    let holds = |w: &mvs_sim::World| {
        w.fighter(id).branching.override_packs.iter().flatten().any(|p| p.from_data && p.overrides.is_empty())
    };
    if !holds(w) {
        return;
    }
    if w.names.str(w.fighter(id).ch) != mvs_sim::chars::tom_and_jerry::KEY {
        l.miss("a data override pack of a character but Tom and Jerry");
        return;
    }
    let mut pack = Some(mvs_sim::chars::tom_and_jerry::pawn::no_jerry_taunt_pack(w, id));
    for p in w.fighter_mut(id).branching.override_packs.iter_mut().flatten() {
        if p.from_data && p.overrides.is_empty() {
            match pack.take() {
                Some(d) => *p = d,
                None => l.miss("a second data override pack"),
            }
        }
    }
}

pub fn load(f: &mut Fighter, r: &FighterRecord, l: &mut Lx) {
    if f64::from(f.id) != r.id {
        l.miss(format_args!("the record's fighter {} loaded into fighter {}", r.id, f.id));
    }
    if l.str(f.ch) != r.character {
        l.at("character", |l| l.miss(format_args!("{} (the World's fighter is another character)", r.character)));
    }
    l.at("inp", |l| f.controls.inp.load(&r.inp, l));
    l.at("prev", |l| f.controls.prev.load(&r.prev, l));
    l.at("vo_current", |l| {
        f.vo.vo_current = match &r.vo_current {
            None => None,
            Some(k) => {
                let n = vo_key_n(f.id, k);
                if n.is_none() {
                    l.miss(format_args!("{k} (not a VO key of fighter {})", f.id));
                }
                n
            }
        }
    });
    l.at("charge_sound_key", |l| {
        f.montages.charge_sound_key = if r.charge_sound_key.is_empty() {
            Name::NONE
        } else if let Some((_, tag)) = crate::events::fighter_key_parse(&r.charge_sound_key) {
            l.name(tag)
        } else {
            l.miss(format_args!("{} (not a charge sound key)", r.charge_sound_key));
            Name::NONE
        }
    });
    rec_rows!(load f, r, l;
        team: team.team,
        rng: rng,
        x: body.x,
        y: body.y,
        vx: body.vx,
        vy: body.vy,
        time: clock.time,
        kb_mode: hit.kb_mode,
        flying: moves.flying,
        ground_state: sm.gs,
        ground_state_time: sm.gs_time,
        crouching: crouch.crouching,
        fast_falling: fast_fall.fast_falling,
        wall_sliding: wall.wall_sliding,
        evading: evade.evading,
        move_in: air.move_in,
        damage: stats.damage,
        hitstun: hit.hitstun,
        hitpause: hit.hitpause,
        true_hitstun: hit.true_hitstun,
        ko_hitstun: life.ko_hitstun,
        launch: hit.launch,
        most_recent_attacker: attackers.most_recent_attacker,
        di_combo: attackers.di_combo,
        attack_count: decay.attack_count,
        jumps: jump.jumps,
        jump_frame: jump.jump_frame,
        air_evades: air.air_evades,
        air_specials: air.air_specials,
        air_attack_available: air.air_attack_available,
        evade_count: evade.evade_count,
        evade_invuln: evade.evade_invuln,
        invuln: invuln.invuln,
        invuln_count: invuln.invuln_count,
        rage: rage.rage,
        rage_enabled: rage.rage_enabled,
        rage_charging: rage.rage_charging,
        rage_filled: rage.rage_filled,
        rage_timer: rage.rage_timer,
        charge_pct: charge.charge_pct,
        dead: life.dead,
        invisible: vis.invisible,
        abducted: life.abducted,
        kos: stats.kos,
        ring_outs: stats.ring_outs,
        damage_dealt: stats.damage_dealt,
        size_scale: body.size_scale,
        gravity_scale: moves.gravity_scale,
        speed_mod: buffs.speed_mod,
        montage: montages.montage,
        dash_montage: montages.dash_montage,
        anim: anim.anim,
        anim_time: anim.anim_time,
        loco_anim: anim.loco_anim,
        loco_time: anim.loco_time,
        buffs: buffs.buffs,
        armor: armor.armor_packs,
        abilities: abilities.abilities,
        held_item: items.held_item,
        queued: branching.queued,
        late_branch: branching.late_branch,
        active_hits: montages.active_hits,
        pending_ends: branching.pending_ends,
        attack_decay: decay.attack_decay,
        landed_decay_sequences: decay.landed_decay_sequences,
        combo_res_map: decay.combo_res_map,
        recent_attacks: controls.recent_attacks,
        landed_attack_ids: decay.landed_attack_ids,
        equipped: pose.equipped,
        net: moves.net,
        impulse: moves.impulse,
        move_cmds: moves.move_cmds,
        move_end: moves.move_end,
        spawned: items.spawned_actors,
        add_kb_vx: hit.add_kb_vx,
        add_kb_vy: hit.add_kb_vy,
        aim_direction: pose.aim_direction,
        air_attack_delay: air.air_attack_delay,
        air_boost_mult: air.air_boost_mult,
        air_control: air.air_control,
        air_control_gate: air.air_control_gate,
        ak_x: body.ak_x,
        ak_y: body.ak_y,
        anim_attack_off: charge.anim_attack_off,
        anim_fast_fall_off: fast_fall.anim_fast_fall_off,
        anim_key: anim.anim_key,
        anim_updated: root.anim_updated,
        armor_vis: armor.armor_vis,
        assist_clear_tick: attackers.assist_clear_tick,
        assists: stats.assists,
        attack_is_active: charge.attack_is_active,
        attack_slot: abilities.attack_slot,
        attacks_dodged: stats.attacks_dodged,
        base_vx: hit.base_vx,
        base_vy: hit.base_vy,
        begin_attack: charge.begin_attack,
        blast_in: life.blast_in,
        blast_listed: life.blast_listed,
        block_jump_end_launch: jump.block_jump_end_launch,
        block_land_reset: crouch.block_land_reset,
        block_pick_up: items.block_pick_up,
        block_taunt_anims: controls.block_taunt_anims,
        bounce_fx_until: hit.bounce_fx_until,
        bounce_pitch: react.bounce_pitch,
        bounce_pose: pose.bounce_pose,
        bounce_type: hit.bounce_type,
        bounce_z_offset: hit.bounce_z_offset,
        bouncing: hit.bouncing,
        br_axis_x: branching.br_axis_x,
        br_axis_y: branching.br_axis_y,
        br_stick_x: branching.br_stick_x,
        branch_montage: branching.branch_montage,
        buff_stun: buffs.buff_stun,
        buff_stun_age: buffs.buff_stun_age,
        buff_stun_len: buffs.buff_stun_len,
        buff_stunned: buffs.buff_stunned,
        cam_focus: life.cam_focus,
        can_bounce_no_hitstun: hit.can_bounce_no_hitstun,
        cant_die: life.cant_die,
        cap_half_u: capsule.cap_half_u,
        cap_radius_u: capsule.cap_radius_u,
        cap_resized: capsule.cap_resized,
        capsule_off: capsule.capsule_off,
        charge_btn: branching.charge_btn,
        charge_buff_mult: charge.charge_buff_mult,
        charge_dmg_mult: charge.charge_dmg_mult,
        charge_hit_large_at: charge.charge_hit_large_at,
        charge_hit_small_at: charge.charge_hit_small_at,
        charge_mult: charge.charge_mult,
        charge_notify: charge.charge_notify,
        charge_ramp_delay: montages.charge_ramp_delay,
        charge_released: charge.charge_released,
        charge_sound_seq: montages.charge_sound_seq,
        charge_start_time: charge.charge_start_time,
        charge_value: montages.charge_value,
        cmc_crouched: capsule.cmc_crouched,
        cmd_jumped: moves.cmd_jumped,
        combo_reset_tick: attackers.combo_reset_tick,
        combo_resistance: decay.combo_resistance,
        cooldown_multiplier: abilities.cooldown_multiplier,
        creation_time: clock.creation_time,
        crouch_fx: crouch.crouch_fx,
        crouch_jump: jump.crouch_jump,
        crouch_locked: crouch.crouch_locked,
        crouch_owed: crouch.crouch_owed,
        current_decay_sequence: decay.current_decay_sequence,
        damage_disabled: invuln.damage_disabled,
        dash_fall_gate: air.dash_fall_gate,
        dash_game_time: air.dash_game_time,
        dash_stop_blend: sm.dash_stop_blend,
        dash_stop_delay: sm.dash_stop_delay,
        dash_tick: air.dash_tick,
        defer_queued_ends: branching.defer_queued_ends,
        direction_soft: react.direction_soft,
        dodged_successfully: evade.dodged_successfully,
        double_ring_outs: stats.double_ring_outs,
        down_btn: crouch.down_btn,
        down_press_active: platforms.down_press_active,
        down_press_count: platforms.down_press_count,
        down_stick: crouch.down_stick,
        down_window_tick: platforms.down_window_tick,
        dropping_down: platforms.dropping_down,
        end_jump_latent: jump.end_jump_latent,
        evade_crouch_rate: evade.evade_crouch_rate,
        evade_invuln_time: evade.evade_invuln_time,
        evade_regen_once: evade.evade_regen_once,
        evade_regen_start: evade.evade_regen_start,
        evading_through_platform: platforms.evading_through_platform,
        facing_dir: body.facing_dir,
        fade_anim: anim.fade_anim,
        fade_dur: anim.fade_dur,
        fade_el: anim.fade_el,
        fade_loop: anim.fade_loop,
        fade_rate: anim.fade_rate,
        fade_time: anim.fade_time,
        fall_lat_friction: air.fall_lat_friction,
        fall_lean: react.fall_lean,
        ff_check_tick: fast_fall.ff_check_tick,
        ff_mult: fast_fall.ff_mult,
        ff_triggered: fast_fall.ff_triggered,
        floor_bounce_hurt: hit.floor_bounce_hurt,
        floor_swept: body.floor_swept,
        fluid_friction: moves.fluid_friction,
        force_additive_kb: attackers.force_additive_kb,
        force_move_seq: moves.force_move_seq,
        frame_mode: frame.frame_mode,
        free_launch: hit.free_launch,
        friction_zero: moves.friction_zero,
        grappled: buffs.grappled,
        gravity_mod_count: moves.gravity_mod_count,
        ground_jump_tick: jump.ground_jump_tick,
        gs_rel: sm.gs_rel,
        has_hit_a_fighter: decay.has_hit_a_fighter,
        has_spawn_invuln: invuln.has_spawn_invuln,
        hit_by_multiple_attackers: attackers.hit_by_multiple_attackers,
        hit_launch: moves.hit_launch,
        hit_neutral: react.hit_neutral,
        hit_pause_time: react.hit_pause_time,
        hit_react_int: react.hit_react_int,
        hit_stun_ended_bound: attackers.hit_stun_ended_bound,
        hit_stun_kb_vx: react.hit_stun_kb_vx,
        hit_stun_kb_vy: react.hit_stun_kb_vy,
        hit_stunned_before_hit: attackers.hit_stunned_before_hit,
        hit_this_frame: frame.hit_this_frame,
        hitstun_count: attackers.hitstun_count,
        hitstun_counted: hit.hitstun_counted,
        hitstun_damage: attackers.hitstun_damage,
        hitstun_end_time: hit.hitstun_end_time,
        hitstun_paused: hit.hitstun_paused,
        hitstun_pending: hit.hitstun_pending,
        hitstun_rate: hit.hitstun_rate,
        holding_enemy: holds.holding_enemy,
        hr_branch: react.hr_branch,
        hr_intro: react.hr_intro,
        ignore_platforms: platforms.ignore_platforms,
        in_ability_timers: abilities.in_ability_timers,
        in_hit_pause_pose: hit.in_hit_pause_pose,
        instant_cooldowns: abilities.instant_cooldowns,
        is_minion: life.is_minion,
        item_holstered: items.item_holstered,
        item_reachable: items.item_reachable,
        juggle_count: attackers.juggle_count,
        juggle_damage: attackers.juggle_damage,
        jump_cut_at: jump.jump_cut_at,
        jump_launched: moves.jump_launched,
        jump_min_time: jump.jump_min_time,
        jump_released: jump.jump_released,
        jump_start_time: jump.jump_start_time,
        jump_stop_gate: jump.jump_stop_gate,
        jump_v: jump.jump_v,
        kb_facing: react.kb_facing,
        knock_back_angle: react.knock_back_angle,
        knocked_back_count: attackers.knocked_back_count,
        last_attacker_off_ledge: attackers.last_attacker_off_ledge,
        last_decay_stacks: attackers.last_decay_stacks,
        last_direction: air.last_direction,
        last_evade_spent_meter: evade.last_evade_spent_meter,
        last_evade_time: evade.last_evade_time,
        last_hitstun_time: hit.last_hitstun_time,
        last_jump_time: jump.last_jump_time,
        last_jump_v: moves.last_jump_v,
        last_knocked_back: attackers.last_knocked_back,
        last_move_in: air.last_move_in,
        last_ring_out_time: stats.last_ring_out_time,
        late_branch_due: branching.late_branch_due,
        layer_missed: charge.layer_missed,
        linger_seq: montages.linger_seq,
        loco_gen: anim.loco_gen,
        loco_loop: anim.loco_loop,
        loco_rate: anim.loco_rate,
        mesh_x: hit.mesh_x,
        mesh_z: hit.mesh_z,
        montage_abort: montages.montage_abort,
        montage_hit: montages.montage_hit,
        moved: frame.moved,
        net_friction: moves.net_friction,
        net_gravity: moves.net_gravity,
        no_walk_off_ledges: platforms.no_walk_off_ledges,
        no_wall_slide_until: wall.no_wall_slide_until,
        not_player_character: life.not_player_character,
        off_platform_ring_outs: stats.off_platform_ring_outs,
        on_camera: vis.on_camera,
        on_floor: body.on_floor,
        outline_count: life.outline_count,
        pawn_vy: platforms.pawn_vy,
        pickup_request: items.pickup_request,
        plain_is_intro: anim.plain_is_intro,
        plat_ignored: platforms.plat_ignored,
        played_in_advance: montages.played_in_advance,
        pose_ticked: root.pose_ticked,
        pre_spawn_delay: life.pre_spawn_delay,
        prev_bounce_time: hit.prev_bounce_time,
        prev_time: clock.prev_time,
        prev_x: body.prev_x,
        prev_y: body.prev_y,
        projectile_armor: invuln.projectile_armor,
        pushing_down: crouch.pushing_down,
        queued_hit_pause: hit.queued_hit_pause,
        queued_hit_pause_sec: hit.queued_hit_pause_sec,
        quick_fall_at: jump.quick_fall_at,
        rage_fx: rage.rage_fx,
        rage_glow_shown: rage.rage_glow_shown,
        rage_residual: rage.rage_residual,
        rage_started: rage.rage_started,
        rapid_bounce_count: hit.rapid_bounce_count,
        reach_recheck: items.reach_recheck,
        recent_hit_actor: decay.recent_hit_actor,
        ring_outs_assisted: stats.ring_outs_assisted,
        root_dx: root.root_dx,
        root_dy: root.root_dy,
        root_has: root.root_has,
        rot_lock_until: controls.rot_lock_until,
        same_attack_blocked: branching.same_attack_blocked,
        seq_delta: anim.seq_delta,
        seq_from: anim.seq_from,
        starting_damage: stats.starting_damage,
        stop_ahead: crouch.stop_ahead,
        stun_source: attackers.stun_source,
        suspended: buffs.suspended,
        taunt_slot: controls.taunt_slot,
        terminal_v: hit.terminal_v,
        tick: clock.tick,
        time_in_air: air.time_in_air,
        total_charged_time: charge.total_charged_time,
        true_hitstun_counted: hit.true_hitstun_counted,
        true_hitstun_rate: hit.true_hitstun_rate,
        true_hitstun_set_tick: hit.true_hitstun_set_tick,
        true_hitstun_tick_at: hit.true_hitstun_tick_at,
        vel_clamped: body.vel_clamped,
        vo_recent: vo.vo_recent,
        vo_seq: vo.vo_seq,
        vo_speaking: vo.vo_speaking,
        walk_drop: platforms.walk_drop,
        wall_capped: wall.wall_capped,
        wall_fatigue: wall.wall_fatigue,
        wall_fatigue_at: wall.wall_fatigue_at,
        wall_mult: wall.wall_mult,
        wall_skate: moves.wall_skate,
        wall_stall: wall.wall_stall,
        wall_timer_pending: wall.wall_timer_pending,
        wall_touches: wall.wall_touches,
        was_attacking: air.was_attacking,
        wind_on: hit.wind_on,
        zero_out_vel_for_kb: attackers.zero_out_vel_for_kb,
    );
    // Unverified: `previousAttacker` is LocalDmgDlrOwner: a fighter but for a level actor with no Instigator, so an
    // id a fighter and an actor share is the fighter's.
    let prev = r.previous_attacker;
    f.attackers.previous_attacker =
        l.at("previous_attacker", |l| match l.fighters.iter().find(|(t, _)| f64::from(*t) == prev) {
            Some(&(_, id)) => Some(mvs_sim::actors::ActorRef::Fighter(id)),
            None if prev < -1.0 => crate::world::buff_attacker_load(prev, l),
            None => Option::<mvs_sim::actors::ActorRef>::make(&prev, l).flatten(),
        });
    f.rage.rage_input = r.rage_input.as_ref().map(|m| {
        let name = l.name(&m.name);
        match &f.montages.montage {
            Some(main) if main.montage.name == name => main.montage,
            _ => crate::parts::loaded_key(name),
        }
    });
    l.at("facing", |l| f.body.facing_dir.load(&r.facing, l));
    l.at("grounded", |l| f.body.on_floor.load(&r.grounded, l));
    // (Team.botControlled and Controls.botControlled are the oracle fighter's one key)
    f.controls.bot_controlled = r.bot_controlled;
    f.team.bot_controlled = r.bot_controlled;
    l.at("state", |l| char_load(f, &r.state, l));
    // a buff the record adds gets the next serial (the Rust buffs' identity; the oracle buffs' is the object)
    for b in &mut f.buffs.buffs {
        if b.serial == u32::MAX {
            b.serial = f.buffs.next_serial;
            f.buffs.next_serial += 1;
        }
    }
    batman_refs_load(f, &r.state, l);
    crate::taz::taz_refs_load(f, &r.state, l);
    crate::jake::jake_refs_load(f, &r.state, l);
    l.ch = Some(l.str(f.ch).to_owned());
    l.at("rest", |l| rest_load(f, &r.rest, l));
    l.ch = None;
}

// ------------------------------------------------------------------ the character's state

rec_struct!(KickMix => KickMixRecord { anim: anim, time: time, w: w });

fn char_put(f: &Fighter, x: &Wx) -> CharRecord {
    match &f.cs {
        CharState::None => CharRecord::None,
        CharState::Shaggy(s) => {
            let mut r = ShaggyRecord {
                // (ShaggyState.rock: the rock is equipped)
                rock: f.pose.equipped.iter().any(|&n| x.str(n) == "rockmesh"),
                fighter: f64::from(f.id),
                ..ShaggyRecord::default()
            };
            rec_rows!(put s, x, r;
                hold_pose: hold_pose, rage_shirt_time: rage_shirt_time, rage_shirt: rage_shirt, kick_w: kick_w,
                kick_mix: kick_mix, last_snack_spawn: last_snack_spawn,
            );
            CharRecord::Shaggy(r)
        }
        CharState::Batman(s) => {
            let mut r = BatmanRecord {
                occluded_speed_buff: buff_ref_put(f, s.occluded_speed_buff),
                occluded_evade_buff: buff_ref_put(f, s.occluded_evade_buff),
                slot_mix: s
                    .slot_mix
                    .slot_mix
                    .iter()
                    .map(|p| SlotPoseRecord {
                        anim: x.str(p.anim).to_owned(),
                        time: f64::from(p.time),
                        w: f64::from(p.w),
                    })
                    .collect(),
                springs: crate::conv::wide(&s.springs),
                ..BatmanRecord::default()
            };
            rec_rows!(put s, x, r;
                gliding: gliding, anim_gliding: anim_gliding, glide_once_closed: glide_once_closed,
                original_gravity_scale: original_gravity_scale, glide_hurt_delay: glide_hurt_delay,
                idle_fade: idle_fade, speed_on: speed_on, not_occluded_delay: not_occluded_delay,
                aim_direction: aim_direction, horizontal_divided_by_max: horizontal_divided_by_max,
                glide_alpha: glide_alpha, glide_w: glide_w, glide_sm: glide_sm, glide_sm_time: glide_sm_time,
                glide_sm_first: glide_sm_first, glide_loop_x: glide_loop_x, glide_loop_t: glide_loop_t,
                glide_add_x: glide_add_x, glide_add_t: glide_add_t, spring_rem: spring_rem, spring_init: spring_init,
                glide_j1: glide_j1, glide_j2: glide_j2, grapple_info: grapple_info, grapple_dest_x: grapple_dest_x,
                grapple_dest_y: grapple_dest_y, grapple_time: grapple_time, grapple_notify: grapple_notify,
                grapple_timer_at: grapple_timer_at, skip_impulse_validation: skip_impulse_validation,
            );
            CharRecord::Batman(r)
        }
        CharState::Superman(s) => {
            let mut r = SupermanRecord::default();
            rec_rows!(put s, x, r;
                ice_count: ice_count, ice_delay: ice_delay, ice_timer: ice_timer, ice_timer_t: ice_timer_t,
                ice_timer_at: ice_timer_at, ice_shakes: ice_shakes, cap_half: cap_half, cap_radius: cap_radius,
                aim_on: aim_on, angle_to_velocity_alpha: angle_to_velocity_alpha, direction: direction,
                direction_soft: direction_soft, fly_angle: fly_angle, fly_lean: fly_lean,
                aim_dash_from_x: aim_dash_from_x, aim_dash_from_y: aim_dash_from_y,
            );
            CharRecord::Superman(r)
        }
        CharState::Lebron(s) => {
            let mut r = LebronRecord::default();
            rec_rows!(put s, x, r;
                has_ball: has_ball, anim_has_ball: anim_has_ball, ball_delay: ball_delay, begin_delay: begin_delay,
                ball_sets: ball_sets, teleport_dunk_ready: teleport_dunk_ready,
                is_teleport_dunking: is_teleport_dunking, cap_half: cap_half, cap_radius: cap_radius,
            );
            CharRecord::Lebron(r)
        }
        CharState::Arya(s) => {
            let mut r = AryaRecord::default();
            rec_rows!(put s, x, r;
                branch_override_active: branch_override_active, tracked: tracked, active_branches: active_branches,
                is_assassinating: is_assassinating, hitboxes_off: hitboxes_off, begin_delay: begin_delay,
                ko_bound: ko_bound, ko_closed: ko_closed, ko_delay: ko_delay,
            );
            CharRecord::Arya(r)
        }
        CharState::Creature(s) => {
            let mut r = ReindogRecord::default();
            rec_rows!(put s, x, r;
                original_cap_half: original_cap_half, original_cap_radius: original_cap_radius, is_held: is_held,
                tether: tether, head_scale: head_scale, bounce_delay: bounce_delay, com_spin: com_spin,
                spin_rotation: spin_rotation, spin_cur: spin_cur, stop_cur: stop_cur, cur_at: cur_at,
                spin_old: spin_old, stop_old: stop_old, is_hit_stunned: is_hit_stunned, is_respawning: is_respawning,
                respawn_delay: respawn_delay,
            );
            CharRecord::Creature(r)
        }
        CharState::BlackAdam(s) => {
            let lightning = LightningRecord {
                targets: s
                    .lightning
                    .targets
                    .iter()
                    .map(|(a, &id)| LightningTargetRecord { actor: a.put(x), id: f64::from(id) })
                    .collect(),
                dirs: s.lightning.dirs.iter().map(|(&id, &up)| LightningDirRecord { id: f64::from(id), up }).collect(),
            };
            CharRecord::BlackAdam(BlackAdamRecord {
                hover_x: f64::from(s.hover_x),
                hover_y: f64::from(s.hover_y),
                list_w: crate::conv::wide(&s.list_w),
                list_index: s.list_index.iter().map(|&i| f64::from(i)).collect(),
                slot_mix: s
                    .slot_mix
                    .slot_mix
                    .iter()
                    .map(|p| SlotPoseRecord {
                        anim: x.str(p.anim).to_owned(),
                        time: f64::from(p.time),
                        w: f64::from(p.w),
                    })
                    .collect(),
                lightning,
            })
        }
        CharState::WonderWoman(s) => CharRecord::WonderWoman(WonderWomanRecord {
            meter: f64::from(s.meter),
            bracer_charge: f64::from(s.bracer_charge),
            bracer_gate: s.bracer_gate,
            lasso_aim_u: f64::from(s.lasso_aim_u),
            lasso_aim_d: f64::from(s.lasso_aim_d),
            shield_move_w: f64::from(s.shield_move_w),
            shield_fwd_w: f64::from(s.shield_fwd_w),
            slot_mix: slot_mix_put(&s.slot_mix.slot_mix, x),
        }),
        CharState::Harley(s) => CharRecord::Harley(HarleyRecord { hold_idle: s.hold_idle }),
        CharState::Morty(s) => CharRecord::Morty(MortyRecord {
            cook_grenade_behind: s.cook_grenade_behind,
            slot_move_w: f64::from(s.slot_move_w),
            slot_back_w: f64::from(s.slot_back_w),
            slot_mix: slot_mix_put(&s.slot_mix.slot_mix, x),
        }),
        CharState::IronGiant(s) => CharRecord::IronGiant(crate::iron_giant::giant_put(s, x)),
        CharState::Steven(s) => CharRecord::Steven(crate::steven::steven_put(s, x)),
        CharState::Garnet(s) => CharRecord::Garnet(crate::garnet::garnet_put(s, x)),
        CharState::Taz(s) => CharRecord::Taz(crate::taz::taz_put(s, f, x)),
        CharState::Bugs(s) => CharRecord::Bugs(crate::bugs::bugs_put(s, x)),
        CharState::Rick(s) => CharRecord::Rick(crate::rick::rick_put(s, x)),
        CharState::Finn(s) => CharRecord::Finn(crate::finn::finn_put(s, x)),
        CharState::Stripe(s) => CharRecord::Stripe(crate::stripe::stripe_put(s, x)),
        CharState::Jake(s) => CharRecord::Jake(crate::jake::jake_put(s, f, x)),
        CharState::TomAndJerry(s) => CharRecord::TomAndJerry(crate::tom_and_jerry::tj_put(s, x)),
        CharState::Velma(s) => CharRecord::Velma(crate::velma::velma_put(s, x)),
        CharState::VelmaPolice(s) => CharRecord::VelmaPolice(crate::velma::police_put(s, x)),
        CharState::Marvin(s) => CharRecord::Marvin(crate::marvin::marvin_put(s, x)),
        CharState::Gizmo(s) => CharRecord::Gizmo(crate::gizmo::gizmo_put(s, x)),
    }
}

/// A character's slot mix (`SlotMix.slot_mix`) as the record writes it.
pub(crate) fn slot_mix_put(m: &[mvs_sim::fighter::anim::SlotPose], x: &Wx) -> Vec<SlotPoseRecord> {
    m.iter()
        .map(|p| SlotPoseRecord { anim: x.str(p.anim).to_owned(), time: f64::from(p.time), w: f64::from(p.w) })
        .collect()
}

/// [`slot_mix_put`]'s inverse.
pub(crate) fn slot_mix_load(r: &[SlotPoseRecord], l: &mut Lx) -> Vec<mvs_sim::fighter::anim::SlotPose> {
    r.iter()
        .map(|p| mvs_sim::fighter::anim::SlotPose {
            anim: l.name(&p.anim),
            time: mvs_sim::math::narrow(p.time),
            w: mvs_sim::math::narrow(p.w),
        })
        .collect()
}

/// A character's timed BlendListByBool nodes (`SlotMix.slot_blend`) as the record writes them; `None` (a graph with
/// none) as the empty record.
pub(crate) fn slot_blend_put(s: Option<&mvs_sim::fighter::anim::SlotBlend>, x: &Wx) -> SlotBlendRecord {
    let Some(s) = s else { return SlotBlendRecord::default() };
    SlotBlendRecord {
        last: s.last.put(x),
        w: crate::conv::wide(&s.w),
        from: crate::conv::wide(&s.from),
        rem: crate::conv::wide(&s.rem),
        time: crate::conv::wide(&s.time),
    }
}

/// [`slot_blend_put`]'s inverse, into the state's own (a graph that has the nodes).
pub(crate) fn slot_blend_load(s: &mut mvs_sim::fighter::anim::SlotBlend, r: &SlotBlendRecord, l: &mut Lx) {
    s.last.load(&r.last, l);
    crate::conv::narrow_into(&mut s.w, &r.w);
    crate::conv::narrow_into(&mut s.from, &r.from);
    crate::conv::narrow_into(&mut s.rem, &r.rem);
    crate::conv::narrow_into(&mut s.time, &r.time);
}

/// A character's SequencePlayers and state machines (`SlotMix.slot_players`) as the record writes them: `first` as
/// 1 / 0, each machine's crossfades as six numbers each (from, to, duration, elapsed, alpha, blend option 1
/// HermiteCubic).
pub(crate) fn slot_players_put(s: Option<&mvs_sim::fighter::anim::SlotPlayers>, x: &Wx) -> SlotPlayersRecord {
    use mvs_sim::fighter::anim::slot_blend::BlendOpt;
    let Some(s) = s else { return SlotPlayersRecord::default() };
    SlotPlayersRecord {
        frame: f64::from(s.frame),
        time: crate::conv::wide(&s.time),
        w: crate::conv::wide(&s.w),
        seen: s.seen.put(x),
        last: s.last.put(x),
        cur: s.cur.put(x),
        first: s.first.iter().map(|&b| if b { 1.0 } else { 0.0 }).collect(),
        act: s
            .act
            .iter()
            .map(|m| {
                m.iter()
                    .flat_map(|c| {
                        let opt = if c.opt == BlendOpt::HermiteCubic { 1.0 } else { 0.0 };
                        [f64::from(c.from), f64::from(c.to), f64::from(c.dur), f64::from(c.el), f64::from(c.alpha), opt]
                    })
                    .collect()
            })
            .collect(),
    }
}

/// [`slot_players_put`]'s inverse, into the state's own.
pub(crate) fn slot_players_load(s: &mut mvs_sim::fighter::anim::SlotPlayers, r: &SlotPlayersRecord, l: &mut Lx) {
    use mvs_sim::fighter::anim::slot_blend::{BlendOpt, Crossfade};
    s.frame.load(&r.frame, l);
    crate::conv::narrow_into(&mut s.time, &r.time);
    crate::conv::narrow_into(&mut s.w, &r.w);
    s.seen.load(&r.seen, l);
    s.last.load(&r.last, l);
    s.cur.load(&r.cur, l);
    s.first = r.first.iter().map(|&v| v != 0.0).collect();
    s.act = r
        .act
        .iter()
        .map(|m| {
            m.as_chunks::<6>()
                .0
                .iter()
                .map(|c| {
                    let mut x = Crossfade {
                        dur: mvs_sim::math::narrow(c[2]),
                        el: mvs_sim::math::narrow(c[3]),
                        alpha: mvs_sim::math::narrow(c[4]),
                        ..Crossfade::default()
                    };
                    x.from.load(&c[0], l);
                    x.to.load(&c[1], l);
                    x.opt = if c[5] == 1.0 { BlendOpt::HermiteCubic } else { BlendOpt::Linear };
                    x
                })
                .collect()
        })
        .collect();
}

/// A buff reference of a character state as the record writes it: its index in the fighter's buffs, -1 none, -2 a buff
/// in no list (ended; a reference to another fighter's buff is not written: Batman's are his own).
pub(crate) fn buff_ref_put(f: &Fighter, b: Option<mvs_sim::combat::BuffRef>) -> f64 {
    let Some(b) = b else { return -1.0 };
    f.buffs.buffs.iter().position(|x| x.serial == b.serial).map_or(-2.0, |i| i as f64)
}

/// A buff reference read back: the entry at the index, an ended one as a serial no entry has.
pub(crate) fn buff_ref_load(
    f: &Fighter,
    id: Option<mvs_sim::fighter::FighterId>,
    v: f64,
    l: &mut Lx,
) -> Option<mvs_sim::combat::BuffRef> {
    if v == -1.0 {
        return None;
    }
    let Some(fighter) = id else {
        l.miss(format_args!("a buff reference of fighter {}, which the World does not have", f.id));
        return None;
    };
    if v == -2.0 {
        // (never handed out: serials count up from 0)
        return Some(mvs_sim::combat::BuffRef { fighter, serial: u32::MAX - 1 });
    }
    match f.buffs.buffs.get(v as usize) {
        Some(b) => Some(mvs_sim::combat::BuffRef { fighter, serial: b.serial }),
        None => {
            l.miss(format_args!("buff reference {v} past fighter {}'s buffs", f.id));
            None
        }
    }
}

/// BatmanState's two buff references, loaded once the fighter's buffs have their serials.
fn batman_refs_load(f: &mut Fighter, r: &CharRecord, l: &mut Lx) {
    let CharRecord::Batman(r) = r else { return };
    let id = l.fighter(f64::from(f.id));
    let sb = buff_ref_load(f, id, r.occluded_speed_buff, l);
    let eb = buff_ref_load(f, id, r.occluded_evade_buff, l);
    if let CharState::Batman(s) = &mut f.cs {
        s.occluded_speed_buff = sb;
        s.occluded_evade_buff = eb;
    }
}

fn char_load(f: &mut Fighter, r: &CharRecord, l: &mut Lx) {
    // (a World whose fighter has no character state yet: made, then loaded)
    match (&f.cs, r) {
        (CharState::None, CharRecord::Shaggy(_)) => {
            f.cs = CharState::Shaggy(mvs_sim::chars::shaggy::ShaggyState::default());
        }
        (CharState::None, CharRecord::Batman(_)) => {
            f.cs = CharState::Batman(mvs_sim::chars::batman::BatmanState::default());
        }
        (CharState::None, CharRecord::Superman(_)) => {
            f.cs = CharState::Superman(mvs_sim::chars::superman::SupermanState::default());
        }
        (CharState::None, CharRecord::Lebron(_)) => {
            f.cs = CharState::Lebron(mvs_sim::chars::lebron::LebronState::default());
        }
        (CharState::None, CharRecord::Arya(_)) => {
            f.cs = CharState::Arya(mvs_sim::chars::arya::AryaState::default());
        }
        (CharState::None, CharRecord::Creature(_)) => {
            f.cs = CharState::Creature(mvs_sim::chars::creature::ReindogState::default());
        }
        (CharState::None, CharRecord::BlackAdam(_)) => {
            f.cs = CharState::BlackAdam(mvs_sim::chars::black_adam::BlackAdamState::default());
        }
        (CharState::None, CharRecord::WonderWoman(_)) => {
            f.cs = CharState::WonderWoman(mvs_sim::chars::wonder_woman::WonderWomanState::default());
        }
        (CharState::None, CharRecord::Harley(_)) => {
            f.cs = CharState::Harley(mvs_sim::chars::harley::HarleyState::default());
        }
        (CharState::None, CharRecord::Morty(_)) => {
            f.cs = CharState::Morty(mvs_sim::chars::morty::MortyState::default());
        }
        (CharState::None, CharRecord::IronGiant(_)) => {
            f.cs = CharState::IronGiant(mvs_sim::chars::iron_giant::GiantState::default());
        }
        (CharState::None, CharRecord::Steven(_)) => {
            f.cs = CharState::Steven(mvs_sim::chars::steven::StevenState::default());
        }
        (CharState::None, CharRecord::Garnet(_)) => {
            f.cs = CharState::Garnet(mvs_sim::chars::garnet::GarnetState::default());
        }
        (CharState::None, CharRecord::Taz(_)) => {
            f.cs = CharState::Taz(mvs_sim::chars::taz::TazState::default());
        }
        (CharState::None, CharRecord::Bugs(_)) => {
            f.cs = CharState::Bugs(mvs_sim::chars::bugs::BugsState::default());
        }
        (CharState::None, CharRecord::Rick(_)) => {
            f.cs = CharState::Rick(mvs_sim::chars::rick::RickState::default());
        }
        (CharState::None, CharRecord::Finn(_)) => {
            f.cs = CharState::Finn(mvs_sim::chars::finn::FinnState::default());
        }
        (CharState::None, CharRecord::Stripe(_)) => {
            f.cs = CharState::Stripe(mvs_sim::chars::stripe::StripeState::default());
        }
        (CharState::None, CharRecord::Jake(_)) => {
            f.cs = CharState::Jake(mvs_sim::chars::jake::JakeState::default());
        }
        (CharState::None, CharRecord::TomAndJerry(_)) => {
            f.cs = CharState::TomAndJerry(mvs_sim::chars::tom_and_jerry::TomAndJerryState::default());
        }
        (CharState::None, CharRecord::Velma(_)) => {
            f.cs = CharState::Velma(mvs_sim::chars::velma::VelmaState::default());
        }
        (CharState::None, CharRecord::VelmaPolice(_)) => {
            f.cs = CharState::VelmaPolice(mvs_sim::chars::velma::VelmaPoliceState::default());
        }
        (CharState::None, CharRecord::Marvin(_)) => {
            f.cs = CharState::Marvin(mvs_sim::chars::marvin::MarvinState::default());
        }
        (CharState::None, CharRecord::Gizmo(_)) => {
            f.cs = CharState::Gizmo(mvs_sim::chars::gizmo::GizmoState::default());
        }
        _ => {}
    }
    match (&mut f.cs, r) {
        (CharState::None, CharRecord::None) => {}
        (CharState::Shaggy(s), CharRecord::Shaggy(r)) => {
            rec_rows!(load s, r, l;
                hold_pose: hold_pose, rage_shirt_time: rage_shirt_time, rage_shirt: rage_shirt, kick_w: kick_w,
                kick_mix: kick_mix, last_snack_spawn: last_snack_spawn,
            );
            // (rock is read from `equipped`, fighter is the fighter's id: both loaded there)
        }
        (CharState::Batman(s), CharRecord::Batman(r)) => {
            rec_rows!(load s, r, l;
                gliding: gliding, anim_gliding: anim_gliding, glide_once_closed: glide_once_closed,
                original_gravity_scale: original_gravity_scale, glide_hurt_delay: glide_hurt_delay,
                idle_fade: idle_fade, speed_on: speed_on, not_occluded_delay: not_occluded_delay,
                aim_direction: aim_direction, horizontal_divided_by_max: horizontal_divided_by_max,
                glide_alpha: glide_alpha, glide_w: glide_w, glide_sm: glide_sm, glide_sm_time: glide_sm_time,
                glide_sm_first: glide_sm_first, glide_loop_x: glide_loop_x, glide_loop_t: glide_loop_t,
                glide_add_x: glide_add_x, glide_add_t: glide_add_t, spring_rem: spring_rem, spring_init: spring_init,
                glide_j1: glide_j1, glide_j2: glide_j2, grapple_info: grapple_info, grapple_dest_x: grapple_dest_x,
                grapple_dest_y: grapple_dest_y, grapple_time: grapple_time, grapple_notify: grapple_notify,
                grapple_timer_at: grapple_timer_at, skip_impulse_validation: skip_impulse_validation,
            );
            s.slot_mix.slot_mix = r
                .slot_mix
                .iter()
                .map(|p| mvs_sim::fighter::anim::SlotPose {
                    anim: l.name(&p.anim),
                    time: mvs_sim::math::narrow(p.time),
                    w: mvs_sim::math::narrow(p.w),
                })
                .collect();
            for (a, b) in s.springs.iter_mut().zip(&r.springs) {
                *a = mvs_sim::math::narrow(*b);
            }
            // (the two buff references: `batman_refs_load`, once the buffs have their serials)
        }
        (CharState::Superman(s), CharRecord::Superman(r)) => {
            rec_rows!(load s, r, l;
                ice_count: ice_count, ice_delay: ice_delay, ice_timer: ice_timer, ice_timer_t: ice_timer_t,
                ice_timer_at: ice_timer_at, ice_shakes: ice_shakes, cap_half: cap_half, cap_radius: cap_radius,
                aim_on: aim_on, angle_to_velocity_alpha: angle_to_velocity_alpha, direction: direction,
                direction_soft: direction_soft, fly_angle: fly_angle, fly_lean: fly_lean,
                aim_dash_from_x: aim_dash_from_x, aim_dash_from_y: aim_dash_from_y,
            );
        }
        (CharState::Lebron(s), CharRecord::Lebron(r)) => {
            rec_rows!(load s, r, l;
                has_ball: has_ball, anim_has_ball: anim_has_ball, ball_delay: ball_delay, begin_delay: begin_delay,
                ball_sets: ball_sets, teleport_dunk_ready: teleport_dunk_ready,
                is_teleport_dunking: is_teleport_dunking, cap_half: cap_half, cap_radius: cap_radius,
            );
        }
        (CharState::Arya(s), CharRecord::Arya(r)) => {
            rec_rows!(load s, r, l;
                branch_override_active: branch_override_active, tracked: tracked, active_branches: active_branches,
                is_assassinating: is_assassinating, hitboxes_off: hitboxes_off, begin_delay: begin_delay,
                ko_bound: ko_bound, ko_closed: ko_closed, ko_delay: ko_delay,
            );
        }
        (CharState::Creature(s), CharRecord::Creature(r)) => {
            rec_rows!(load s, r, l;
                original_cap_half: original_cap_half, original_cap_radius: original_cap_radius, is_held: is_held,
                tether: tether, head_scale: head_scale, bounce_delay: bounce_delay, com_spin: com_spin,
                spin_rotation: spin_rotation, spin_cur: spin_cur, stop_cur: stop_cur, cur_at: cur_at,
                spin_old: spin_old, stop_old: stop_old, is_hit_stunned: is_hit_stunned, is_respawning: is_respawning,
                respawn_delay: respawn_delay,
            );
        }
        (CharState::BlackAdam(s), CharRecord::BlackAdam(r)) => {
            s.hover_x = mvs_sim::math::narrow(r.hover_x);
            s.hover_y = mvs_sim::math::narrow(r.hover_y);
            if r.list_w.len() != 4 || r.list_index.len() != 4 {
                l.miss("BlackAdamState's four hover lists");
            }
            for (a, b) in s.list_w.iter_mut().zip(&r.list_w) {
                *a = mvs_sim::math::narrow(*b);
            }
            for (a, b) in s.list_index.iter_mut().zip(&r.list_index) {
                l.at("list_index", |l| a.load(b, l));
            }
            s.slot_mix.slot_mix = r
                .slot_mix
                .iter()
                .map(|p| mvs_sim::fighter::anim::SlotPose {
                    anim: l.name(&p.anim),
                    time: mvs_sim::math::narrow(p.time),
                    w: mvs_sim::math::narrow(p.w),
                })
                .collect();
            s.lightning.targets.clear();
            for t in &r.lightning.targets {
                let Some(a) = l.at("lightning.targets", |l| mvs_sim::actors::ActorRef::make(&t.actor, l)) else {
                    continue;
                };
                let mut id = 0;
                l.at("lightning.targets", |l| id.load(&t.id, l));
                s.lightning.targets.insert(a, id);
            }
            s.lightning.dirs.clear();
            for d in &r.lightning.dirs {
                let mut id = 0;
                l.at("lightning.dirs", |l| id.load(&d.id, l));
                s.lightning.dirs.insert(id, d.up);
            }
        }
        (CharState::WonderWoman(s), CharRecord::WonderWoman(r)) => {
            s.meter = mvs_sim::math::narrow(r.meter);
            s.bracer_charge = mvs_sim::math::narrow(r.bracer_charge);
            s.bracer_gate = r.bracer_gate;
            s.lasso_aim_u = mvs_sim::math::narrow(r.lasso_aim_u);
            s.lasso_aim_d = mvs_sim::math::narrow(r.lasso_aim_d);
            s.shield_move_w = mvs_sim::math::narrow(r.shield_move_w);
            s.shield_fwd_w = mvs_sim::math::narrow(r.shield_fwd_w);
            s.slot_mix.slot_mix = slot_mix_load(&r.slot_mix, l);
        }
        (CharState::Harley(s), CharRecord::Harley(r)) => s.hold_idle = r.hold_idle,
        (CharState::Morty(s), CharRecord::Morty(r)) => {
            s.cook_grenade_behind = r.cook_grenade_behind;
            s.slot_move_w = mvs_sim::math::narrow(r.slot_move_w);
            s.slot_back_w = mvs_sim::math::narrow(r.slot_back_w);
            s.slot_mix.slot_mix = slot_mix_load(&r.slot_mix, l);
        }
        // (the turret's PilotBuff, on another fighter: `iron_giant::resolve_pilot_buffs`, once every fighter has its
        // buffs)
        (CharState::IronGiant(s), CharRecord::IronGiant(r)) => crate::iron_giant::giant_load(s, r, l),
        (CharState::Steven(s), CharRecord::Steven(r)) => crate::steven::steven_load(s, r, l),
        (CharState::Garnet(s), CharRecord::Garnet(r)) => crate::garnet::garnet_load(s, r, l),
        // (the tornado buff: `taz::taz_refs_load`, once the buffs have their serials)
        (CharState::Taz(s), CharRecord::Taz(r)) => crate::taz::taz_load(s, r, l),
        (CharState::Bugs(s), CharRecord::Bugs(r)) => crate::bugs::bugs_load(s, r, l),
        (CharState::Rick(s), CharRecord::Rick(r)) => crate::rick::rick_load(s, r, l),
        (CharState::Finn(s), CharRecord::Finn(r)) => crate::finn::finn_load(s, r, l),
        (CharState::Stripe(s), CharRecord::Stripe(r)) => crate::stripe::stripe_load(s, r, l),
        // (the slow buff: `jake::jake_refs_load`, once the buffs have their serials)
        (CharState::Jake(s), CharRecord::Jake(r)) => crate::jake::jake_load(s, r, l),
        (CharState::TomAndJerry(s), CharRecord::TomAndJerry(r)) => crate::tom_and_jerry::tj_load(s, r, l),
        (CharState::Velma(s), CharRecord::Velma(r)) => crate::velma::velma_load(s, r, l),
        (CharState::VelmaPolice(s), CharRecord::VelmaPolice(r)) => crate::velma::police_load(s, r, l),
        (CharState::Marvin(s), CharRecord::Marvin(r)) => crate::marvin::marvin_load(s, r, l),
        (CharState::Gizmo(s), CharRecord::Gizmo(r)) => crate::gizmo::gizmo_load(s, r, l),
        (_, r) => l.miss(format_args!("{r:?} for the World's character state")),
    }
}

// ------------------------------------------------------------------ rest

plain_struct!(ComboReset { "fighter" = fighter, "left" = left, "pending" = pending });
// `lateAuto`: the montage's name and the windows still filed.
plain_struct!(mvs_sim::fighter::anim::LateAuto { "cur" = cur, "ws" = ws } make |v, l| {
    let mut a = mvs_sim::fighter::anim::LateAuto { ws: Vec::new(), cur: Name::NONE };
    a.load_value(v, l);
    Some(a)
});

// `buffDecays`: `{cls, timer, stacks}`.
plain_struct!(mvs_sim::combat::buffs::BuffDecay { "cls" = cls, "timer" = timer, "stacks" = stacks } make |v, l| {
    let mut d = mvs_sim::combat::buffs::BuffDecay { cls: Name::NONE, timer: 0.0, stacks: 0.0 };
    d.load_value(v, l);
    Some(d)
});

// `inputOv`: `id` only where set.
plain_struct!(mvs_sim::fighter::moves::InputOverrideState {
    "ox" = ox, "xv" = xv, "oy" = oy, "yv" = yv, "mx" = mx, "my" = my, "rx" = rx, "ry" = ry, "jd" = jd, "ed" = ed,
    "ad" = ad, "id" ~ id,
});

// `FireWallComp`: `{walls, wait}` and `max` once SetMaxFireWalls wrote it.
plain_struct!(mvs_sim::combat::shapes::FireWallComp { "walls" = walls, "wait" = wait, "max" ~ max });
// `ChargeTimer`: a charge under a ChargeMultiplier other than 1 with a section override (Ice Charge's faster charge).
plain_struct!(mvs_sim::fighter::control::ChargeTimer { "left" = left, "sec" = sec, "ticks" = ticks });

// `holdCharge`: Comp_HoldCharge's variables.
plain_struct!(mvs_sim::fighter::control::HoldCharge {
    "atMax" = at_max, "defaults" = defaults, "keys" = keys, "lastHold" = last_hold, "multiplier" = multiplier,
    "pack" = pack, "timer" = timer, "timerTicks" = timer_ticks,
});

// `forceMove`: `ease` only where set.
plain_struct!(mvs_sim::fighter::moves::ForceMove {
    "id" = id, "sx" = sx, "sy" = sy, "tx" = tx, "ty" = ty, "dur" = dur, "time" = time, "done" = done, "mode" = mode,
    "clamp" = clamp, "setVx" = set_vx, "setVy" = set_vy, "ease" ~ ease,
});

// `pull`: a pull toward a point; `by` the caller's id (the tether's).
plain_struct!(mvs_sim::fighter::moves::Pull {
    "sx" = sx, "sy" = sy, "dx" = dx, "dy" = dy, "k" = k, "n" = n, "rate" = rate, "end" = end, "by" = by,
});

// `struggle`: a buff stun's button mash.
plain_struct!(mvs_sim::combat::buffs::Struggle {
    "avail" = avail, "count" = count, "inc" = inc, "left" = left, "max" = max, "period" = period,
});

// `forceFinish`: the finish velocity of an ended forced move.
plain_struct!(mvs_sim::fighter::moves::ForceFinish { "mode" = mode, "clamp" = clamp, "setVx" = set_vx, "setVy" = set_vy });

// `fly`: Comp_Pawn_Fly's variables and two CharacterMovement values.
plain_struct!(mvs_sim::fighter::moves::FlyState {
    "isFlying" = is_flying, "prevFlySpeed" = prev_fly_speed, "facingLeft" = facing_left, "lastX" = last_x,
    "lastY" = last_y, "maxFlySpeed" = max_fly_speed, "brakingDecelFlying" = braking_decel_flying,
});

/// `friction`: `mods` a Map of name to value (the port holds it as pairs in
/// insertion order), `bff` only where a graph's direct write set it.
impl Plain for mvs_sim::fighter::moves::FrictionState {
    fn to_value(&self, x: &Wx) -> Value {
        let mut o: Vec<(String, Value)> = Vec::new();
        plain_keys!(put self, x, o; "gf" = gf, "bf" = bf, "bdw" = bdw, "bdf" = bdf, "mcb" = mcb, "bff" ~ bff);
        let mods: indexmap::IndexMap<Name, mvs_sim::Real> = self.mods.iter().copied().collect();
        o.push(("mods".to_owned(), mods.to_value(x)));
        Value::Obj(mvs_trace::Obj::new(o))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(ob) = as_obj(v, l) else { return };
        plain_keys!(load self, ob, l; "gf" = gf, "bf" = bf, "bdw" = bdw, "bdf" = bdf, "mcb" = mcb, "bff" ~ bff);
        match ob.get("mods") {
            Some(m) => {
                let mut mods: indexmap::IndexMap<Name, mvs_sim::Real> = self.mods.iter().copied().collect();
                l.at("mods", |l| mods.load_value(m, l));
                self.mods = mods.into_iter().collect();
            }
            None => l.miss("key mods absent"),
        }
        check_keys(ob, &["gf", "bf", "bdw", "bdf", "mcb", "bff", "mods"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        crate::conv::via_default(v, l, <Self as Plain>::load_value)
    }
}

/// The properties of the notify `n` in the match's data (any character's montage of that name).
pub(crate) fn notify_props_any<'a>(x: &Wx<'a>, n: NotifyRef) -> Option<&'a mvs_data::value::Props> {
    let m = x.str(n.montage);
    x.w.data.chars.values().find_map(|c| c.fighter.montages.get(m)?.notifies.get(n.index as usize)).map(|n| &n.p)
}

/// A `spawnRequests` entry: `{cls, p, attackId, charge}`, `foreign`, `copy` and
/// `instigator` where set. `p` is the request's own object written whole (a graph's spawn: the spawn area's), else
/// the notify's properties, a static of the data.
impl Plain for mvs_sim::fighter::match_side::SpawnRequest {
    fn to_value(&self, x: &Wx) -> Value {
        use mvs_sim::fighter::match_side::Foreign;
        let p = match (&self.props, &self.aim) {
            (Some(p), None) => crate::parts::data_put(&mvs_data::value::Value::Obj((**p).clone())),
            (None, None) => match notify_props_any(x, self.n) {
                Some(p) => crate::parts::static_put(p),
                None => {
                    x.unmapped("a spawn request of a notify not in the data");
                    Value::Null
                }
            },
            (_, Some(_)) => {
                x.unmapped("an aimed spawn request's p (no rule to write it)");
                Value::Null
            }
        };
        let mut e = vec![
            ("cls", Value::Str(x.str(self.cls).to_owned())),
            ("p", p),
            ("attackId", Value::Num(f64::from(self.attack_id))),
            ("charge", self.charge.to_value(x)),
        ];
        match self.foreign {
            Foreign::None => {}
            Foreign::Disguise => e.push(("foreign", Value::Str("disguise".to_owned()))),
            Foreign::Burp => e.push(("foreign", Value::Str("burp".to_owned()))),
        }
        if let Some(at) = self.copy {
            let at = at.map_or(Value::Null, |v| {
                obj(vec![("x", Value::Num(f64::from(v.x))), ("y", Value::Num(f64::from(v.y)))])
            });
            e.push(("copy", obj(vec![("at", at)])));
        }
        if let Some(i) = self.instigator {
            e.push(("instigator", i.to_value(x)));
        }
        if let Some(o) = self.origin {
            e.push(("origin", obj(vec![("x", Value::Num(f64::from(o.x))), ("y", Value::Num(f64::from(o.y)))])));
        }
        obj(e)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(r) = Self::from_value(v, l) {
            *self = r;
        }
    }
    // Only a request with its own `p` is made (a notify's static `p` names no notify).
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        use mvs_sim::fighter::match_side::{Foreign, SpawnRequest};
        let o = as_obj(v, l)?;
        let props = match o.get("p") {
            Some(Value::Obj(p)) if p.get("static").is_none() => match crate::parts::data_load(&Value::Obj(p.clone())) {
                mvs_data::value::Value::Obj(p) => p,
                _ => return None,
            },
            _ => {
                l.miss("a spawn request of a notify's properties (no rule to make it)");
                return None;
            }
        };
        let cls = match o.get("cls") {
            Some(Value::Str(s)) => l.name(s),
            _ => {
                l.miss("a spawn request with no cls");
                return None;
            }
        };
        let attack_id = match o.get("attackId") {
            Some(Value::Num(n)) => crate::conv::int(*n, l)?,
            _ => 0,
        };
        let charge = match o.get("charge") {
            Some(c) => l.at("charge", |l| Plain::from_value(c, l))?,
            None => None,
        };
        let foreign = match o.get("foreign") {
            Some(Value::Str(s)) if s == "disguise" => Foreign::Disguise,
            Some(Value::Str(s)) if s == "burp" => Foreign::Burp,
            _ => Foreign::None,
        };
        let copy = match o.get("copy").and_then(|c| match c {
            Value::Obj(c) => c.get("at").cloned(),
            _ => None,
        }) {
            Some(Value::Obj(a)) => {
                let n = |k: &str| match a.get(k) {
                    Some(Value::Num(n)) => *n,
                    _ => 0.0,
                };
                Some(Some(mvs_sim::fighter::moves::V2 {
                    x: mvs_sim::math::narrow(n("x")),
                    y: mvs_sim::math::narrow(n("y")),
                }))
            }
            Some(_) => Some(None),
            None => None,
        };
        let instigator = match o.get("instigator") {
            Some(i) => l.at("instigator", |l| Plain::from_value(i, l)),
            None => None,
        };
        let origin = match o.get("origin") {
            Some(Value::Obj(a)) => {
                let n = |k: &str| match a.get(k) {
                    Some(Value::Num(n)) => *n,
                    _ => 0.0,
                };
                Some(mvs_sim::fighter::moves::V2 { x: mvs_sim::math::narrow(n("x")), y: mvs_sim::math::narrow(n("y")) })
            }
            _ => None,
        };
        check_keys(o, &["cls", "p", "attackId", "charge", "foreign", "copy", "instigator", "origin"], l);
        Some(SpawnRequest {
            cls,
            n: NotifyRef { montage: Name::NONE, index: 0 },
            attack_id,
            foreign,
            copy,
            charge,
            aim: None,
            props: Some(std::sync::Arc::new(props)),
            instigator,
            origin,
        })
    }
}

/// An attack table row as the oracle object holds it (Struct_AttackData: the data's keys, AbilitySlot only where set).
fn attack_value(a: &mvs_data::fighter::AttackEntry) -> Value {
    let conds =
        Value::List(a.conditions.iter().map(|c| c.as_ref().map_or(Value::Null, |s| Value::Str(s.clone()))).collect());
    let mut e = vec![
        ("RequiredButtonInput", Value::Num(f64::from(a.required_button_input))),
        ("RequiredDirectionInput", Value::Num(f64::from(a.required_direction_input))),
        ("AttackMontage", Value::Str(a.attack_montage.clone())),
        ("DisplayName", a.display_name.as_ref().map_or(Value::Null, |d| Value::Str(d.clone()))),
        ("Conditions", conds),
        ("AllowPreAttackRotation", Value::Bool(a.allow_pre_attack_rotation)),
    ];
    if let Some(s) = a.ability_slot {
        e.push(("AbilitySlot", Value::Num(f64::from(s))));
    }
    obj(e)
}

fn attack_load(v: &Value, l: &mut Lx) -> Option<mvs_data::fighter::AttackEntry> {
    let o = as_obj(v, l)?;
    let num = |k: &str, l: &mut Lx| match o.get(k) {
        Some(Value::Num(n)) => crate::conv::int(*n, l),
        _ => None,
    };
    let conditions = match o.get("Conditions") {
        Some(Value::List(c)) => c
            .iter()
            .map(|c| match c {
                Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let e = mvs_data::fighter::AttackEntry {
        required_button_input: num("RequiredButtonInput", l).unwrap_or(0),
        required_direction_input: num("RequiredDirectionInput", l).unwrap_or(0),
        attack_montage: match o.get("AttackMontage") {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        },
        ability_slot: num("AbilitySlot", l),
        display_name: match o.get("DisplayName") {
            Some(Value::Str(d)) => Some(d.clone()),
            _ => None,
        },
        conditions,
        allow_pre_attack_rotation: matches!(o.get("AllowPreAttackRotation"), Some(Value::Bool(true))),
    };
    check_keys(
        o,
        &[
            "RequiredButtonInput",
            "RequiredDirectionInput",
            "AttackMontage",
            "AbilitySlot",
            "DisplayName",
            "Conditions",
            "AllowPreAttackRotation",
        ],
        l,
    );
    Some(e)
}

/// An `OverridePack`: `{Priority, Overrides: [{AerialMove, Attack}]}`.
impl Plain for mvs_sim::fighter::anim::OverridePack {
    fn to_value(&self, _: &Wx) -> Value {
        if self.from_data {
            return obj(vec![("static", Value::Str("object".to_owned()))]);
        }
        let rows = self
            .overrides
            .iter()
            .map(|r| obj(vec![("AerialMove", Value::Bool(r.aerial_move)), ("Attack", attack_value(&r.attack))]))
            .collect();
        obj(vec![("Priority", Value::Num(f64::from(self.priority))), ("Overrides", Value::List(rows))])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(p) = Self::from_value(v, l) {
            *self = p;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        // A pack of the loaded game data is written as `{static: "object"}`: a placeholder here, which
        // [`resolve_data_packs`] turns into the character's data pack.
        if matches!(o.get("static"), Some(Value::Str(s)) if s == "object") {
            return Some(mvs_sim::fighter::anim::OverridePack {
                priority: 0.0,
                overrides: Vec::new(),
                from_data: true,
            });
        }
        let priority = match o.get("Priority") {
            Some(Value::Num(n)) => *n,
            _ => {
                l.miss("an override pack with no Priority");
                return None;
            }
        };
        let mut overrides = Vec::new();
        if let Some(Value::List(rows)) = o.get("Overrides") {
            for r in rows {
                let ro = as_obj(r, l)?;
                let attack = attack_load(ro.get("Attack").unwrap_or(&Value::Null), l)?;
                let aerial_move = matches!(ro.get("AerialMove"), Some(Value::Bool(true)));
                overrides.push(mvs_sim::fighter::anim::OverrideRow { aerial_move, attack });
            }
        }
        check_keys(o, &["Priority", "Overrides"], l);
        Some(mvs_sim::fighter::anim::OverridePack {
            priority: mvs_sim::math::narrow(priority),
            overrides,
            from_data: false,
        })
    }
}

// A `tempHealth` pack: `{type, amount, time, added, fire}`.
plain_struct!(mvs_sim::fighter::match_side::TempHealth {
    "added" = added, "amount" = amount, "fire" = fire, "time" = time, "type" = kind,
});

/// The keys `rest_put` writes by hand (the generated list's are its own).
const HAND_KEYS: [&str; 17] = [
    "actorScale",
    "chargeUi",
    "comboReset",
    "locoBlend",
    "wobbleTimes",
    "hold",
    "holdBuff",
    "holdHits",
    "holdVictims",
    "onTurn",
    "surfaceTraces",
    "disguise",
    "polymorph",
    "isPolymorphed",
    "carried",
    "fireWall",
    "grouped",
];

fn rest_put(f: &Fighter, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put f, x, o;
        "base" = platforms.base,
        "slot" ~ life.slot,
        "itemReach" ~ items.item_reach,
        "uncontrolled" ~ life.uncontrolled,
        "lastAttackTutorialEvent" ~ tutorial.last_attack_tutorial_event,
        "activeInTutorialIntro" ~ tutorial.active_in_tutorial_intro,
        "disableCD" ~ abilities.disable_cd,
        "jumpHoldTimer" ~ jump.jump_hold_timer,
        "wasAttackingDuringFastfall" ~ fast_fall.was_attacking_during_fastfall,
        "dropTutorialAt" ~ tutorial.drop_tutorial_at,
        "minionCreator" = life.minion_creator,
        "walkBase" = platforms.walk_base,
        "sweptBase" = platforms.swept_base,
        "tempHealth" = stats.temp_health,
        "dodge" ~ evade.dodge,
        "lightProjectileArmor" ~ invuln.light_projectile_armor,
        "airSpecialMaxSet" ~ air.air_special_max_set,
        "blockCooldowns" ~ abilities.block_cooldowns,
        "blockFFTime" ~ fast_fall.block_ff_time,
        "holdCharge" ~ charge.hold_charge,
        "allowWallPlay" ~ wall.allow_wall_play,
        "disableDash" ~ moves.disable_dash,
        "wallBounceForceVertMin" ~ hit.wall_bounce_force_vert_min,
        "wallBounceForcedVertMinAmount" ~ hit.wall_bounce_forced_vert_min_amount,
        "launchOnHit" ~ hit.launch_on_hit,
        "headScale" ~ pose.head_scale,
        "animBounce" ~ hit.anim_bounce,
        "hitRotationOff" ~ react.hit_rotation_off,
        "chargeAttackMultiplier" ~ charge.charge_attack_multiplier,
        "chargeTotalTime" ~ charge.charge_total_time,
        "chargeTimer" ~ charge.charge_timer,
        "winEnds" ~ branching.win_ends,
        "buttonStates" ~ branching.button_states,
        "fading" = montages.fading,
        "blockEvadeMontage" ~ evade.block_evade_montage,
        "moveNone" ~ moves.move_none,
        "noGroundEvade" ~ evade.no_ground_evade,
        "invisMore" ~ vis.invis_more,
        "alwaysVisible" ~ vis.always_visible,
        "invisShown" ~ vis.invis_shown,
        "condObjs" ~ buffs.cond_objs,
        "hitMirror" ~ vis.hit_mirror,
        "abilityOver" # abilities.ability_over,
        "advancing" = montages.advancing,
        "dispatchOwed" = root.dispatch_owed,
        "pendingMoveEnds" = branching.pending_move_ends,
        "lateAuto" = branching.late_auto,
        "winCleared" = branching.win_cleared,
        "overridePacks" = branching.override_packs,
        "foreignWins" = branching.foreign_wins,
        "chargeRamp" = montages.charge_ramp,
        "fadeEnds" = montages.fade_ends,
        "proxyOwed" = montages.proxy_owed,
        "netLate" = moves.net_late,
        "gravityOverride" ~ moves.gravity_override,
        "groundJump" = moves.ground_jump,
        "friction" ~ moves.friction,
        "inputOv" ~ moves.input_ov,
        "buffDecays" ~ moves.buff_decays,
        "decayT0" ~ moves.decay_t0,
        "fly" ~ moves.fly,
        "maxAccel" ~ moves.max_accel,
        "speedOverride" ~ moves.speed_override,
        "folds" ~ moves.folds,
        "prevAtkCancelled" ~ moves.prev_atk_cancelled,
        "cancelEvadeBuffs" ~ moves.cancel_evade_buffs,
        "timedArmor" ~ armor.timed_armor,
        "flyInput" = moves.fly_input,
        "armorHid" ~ armor.armor_hid,
        "fastFallStartY" ~ fast_fall.fast_fall_start_y,
        "armorShake" ~ hit.armor_shake,
        "noWallBounce" ~ hit.no_wall_bounce,
        "hurtOff" = hit.hurt_off,
        "subHidden" = pose.sub_hidden,
        "blendOutSubs" = pose.blend_out_subs,
        "seqOn" = anim.seq_on,
        "hrTimes" = react.hr_times,
        "hitPauseTimer" = hit.hit_pause_timer,
        "mostRecentFriendlyFire" ~ attackers.most_recent_friendly_fire,
        "assistAttacker" = attackers.assist_attacker,
        "attackSpamTimes" = controls.attack_spam_times,
        "spamDelayed" = controls.spam_delayed,
        "rotThruAtkCount" ~ controls.rot_thru_atk_count,
        "spawnRequests" = requests.spawn_requests,
        "activateRequests" = requests.activate_requests,
        "despawnRequests" = requests.despawn_requests,
        "nearbyBuffRequests" = requests.nearby_buff_requests,
        "allyBuffRequests" = requests.ally_buff_requests,
        "aimer" = items.aimer,
        "aimerUnder" = items.aimer_under,
        "projAim" = items.proj_aim,
        "holdPack" = anim.hold_pack,
        "itemReleases" = items.item_releases,
        "montageMeshes" = montages.montage_meshes,
        "runSub" ~ sm.run_sub,
        "onEdge" ~ platforms.on_edge,
        "smActive" = sm.sm_active,
        "smTimes" = sm.sm_times,
        "a2gTimes" = sm.a2g_times,
        "edgeTimes" ~ sm.edge_times,
        "struggle" = buffs.struggle,
        "pull" = moves.pull,
        "forceMove" = moves.force_move,
        "forceFinish" = moves.force_finish,
        "ally" = team.ally,
        "weightSet" ~ body.weight_set,
        "bounceTimes" = pose.bounce_times,
    );
    let pair = |a: &str, av: Value, b: &str, bv: Value| obj(vec![(a, av), (b, bv)]);
    // `actorScale`: `{xy, z}`, left out while unset.
    if let Some([xy, z]) = f.body.actor_scale {
        o.push(("actorScale".into(), pair("xy", Value::Num(f64::from(xy)), "z", Value::Num(f64::from(z)))));
    }
    let (start, total) = f.charge.charge_ui;
    o.push(("chargeUi".into(), pair("start", Value::Num(f64::from(start)), "total", Value::Num(f64::from(total)))));
    let (dur, hermite) = f.anim.loco_blend;
    o.push(("locoBlend".into(), pair("dur", Value::Num(f64::from(dur)), "hermite", Value::Bool(hermite))));
    let (front, back) = f.react.wobble_times;
    o.push(("wobbleTimes".into(), pair("front", Value::Num(f64::from(front)), "back", Value::Num(f64::from(back)))));
    // (the World's combo-reset state, which the oracle fighter holds a reference to)
    o.push(("comboReset".into(), x.w.combo_reset.to_value(x)));
    o.push(("hold".into(), f.holds.hold.to_value(x)));
    o.push(("holdBuff".into(), f.holds.hold_buff.to_value(x)));
    o.push(("holdHits".into(), f.holds.hold_hits.to_value(x)));
    o.push(("holdVictims".into(), f.holds.hold_victims.to_value(x)));
    o.push((
        "onTurn".into(),
        match f.frame.on_turn {
            OnTurn::None => Value::Null,
            OnTurn::Overlaps => {
                x.unmapped("an onTurn of Overlaps (no rule to write the oracle closure)");
                Value::Null
            }
        },
    ));
    o.push(("surfaceTraces".into(), f.frame.surface_traces.to_value(x)));
    let lines =
        f.vo.vo_lines
            .iter()
            .map(|v| obj(vec![("key", Value::Str(vo_key(f.id, v.key))), ("until", Value::Num(f64::from(v.until)))]));
    o.push(("voLines".into(), Value::List(lines.collect())));
    if let Some(g) = &f.montages.grouped {
        o.push(("grouped".into(), crate::parts::grouped_put(g, x)));
    }
    let s = &f.shapes;
    for (k, v, unset) in [
        ("disguise", s.disguise.to_value(x), s.disguise.is_none()),
        ("polymorph", s.polymorph.to_value(x), s.polymorph.is_none()),
        ("isPolymorphed", s.is_polymorphed.to_value(x), !s.is_polymorphed),
        ("carried", s.carried.to_value(x), s.carried.is_none()),
        ("fireWall", s.fire_wall.to_value(x), s.fire_wall.is_none()),
    ] {
        if !unset {
            o.push((k.into(), v));
        }
    }
    Value::Obj(mvs_trace::Obj::new(o))
}

fn rest_load(f: &mut Fighter, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    plain_keys!(load f, ob, l;
        "base" = platforms.base,
        "slot" ~ life.slot,
        "itemReach" ~ items.item_reach,
        "uncontrolled" ~ life.uncontrolled,
        "lastAttackTutorialEvent" ~ tutorial.last_attack_tutorial_event,
        "activeInTutorialIntro" ~ tutorial.active_in_tutorial_intro,
        "disableCD" ~ abilities.disable_cd,
        "jumpHoldTimer" ~ jump.jump_hold_timer,
        "wasAttackingDuringFastfall" ~ fast_fall.was_attacking_during_fastfall,
        "dropTutorialAt" ~ tutorial.drop_tutorial_at,
        "minionCreator" = life.minion_creator,
        "walkBase" = platforms.walk_base,
        "sweptBase" = platforms.swept_base,
        "tempHealth" = stats.temp_health,
        "dodge" ~ evade.dodge,
        "lightProjectileArmor" ~ invuln.light_projectile_armor,
        "airSpecialMaxSet" ~ air.air_special_max_set,
        "blockCooldowns" ~ abilities.block_cooldowns,
        "blockFFTime" ~ fast_fall.block_ff_time,
        "holdCharge" ~ charge.hold_charge,
        "allowWallPlay" ~ wall.allow_wall_play,
        "disableDash" ~ moves.disable_dash,
        "wallBounceForceVertMin" ~ hit.wall_bounce_force_vert_min,
        "wallBounceForcedVertMinAmount" ~ hit.wall_bounce_forced_vert_min_amount,
        "launchOnHit" ~ hit.launch_on_hit,
        "headScale" ~ pose.head_scale,
        "animBounce" ~ hit.anim_bounce,
        "hitRotationOff" ~ react.hit_rotation_off,
        "chargeAttackMultiplier" ~ charge.charge_attack_multiplier,
        "chargeTotalTime" ~ charge.charge_total_time,
        "chargeTimer" ~ charge.charge_timer,
        "winEnds" ~ branching.win_ends,
        "buttonStates" ~ branching.button_states,
        "fading" = montages.fading,
        "blockEvadeMontage" ~ evade.block_evade_montage,
        "moveNone" ~ moves.move_none,
        "noGroundEvade" ~ evade.no_ground_evade,
        "invisMore" ~ vis.invis_more,
        "alwaysVisible" ~ vis.always_visible,
        "invisShown" ~ vis.invis_shown,
        "condObjs" ~ buffs.cond_objs,
        "hitMirror" ~ vis.hit_mirror,
        "abilityOver" # abilities.ability_over,
        "advancing" = montages.advancing,
        "dispatchOwed" = root.dispatch_owed,
        "pendingMoveEnds" = branching.pending_move_ends,
        "lateAuto" = branching.late_auto,
        "winCleared" = branching.win_cleared,
        "overridePacks" = branching.override_packs,
        "foreignWins" = branching.foreign_wins,
        "chargeRamp" = montages.charge_ramp,
        "fadeEnds" = montages.fade_ends,
        "proxyOwed" = montages.proxy_owed,
        "netLate" = moves.net_late,
        "gravityOverride" ~ moves.gravity_override,
        "groundJump" = moves.ground_jump,
        "friction" ~ moves.friction,
        "inputOv" ~ moves.input_ov,
        "buffDecays" ~ moves.buff_decays,
        "decayT0" ~ moves.decay_t0,
        "fly" ~ moves.fly,
        "maxAccel" ~ moves.max_accel,
        "speedOverride" ~ moves.speed_override,
        "folds" ~ moves.folds,
        "prevAtkCancelled" ~ moves.prev_atk_cancelled,
        "cancelEvadeBuffs" ~ moves.cancel_evade_buffs,
        "timedArmor" ~ armor.timed_armor,
        "flyInput" = moves.fly_input,
        "armorHid" ~ armor.armor_hid,
        "fastFallStartY" ~ fast_fall.fast_fall_start_y,
        "armorShake" ~ hit.armor_shake,
        "noWallBounce" ~ hit.no_wall_bounce,
        "hurtOff" = hit.hurt_off,
        "subHidden" = pose.sub_hidden,
        "blendOutSubs" = pose.blend_out_subs,
        "seqOn" = anim.seq_on,
        "hrTimes" = react.hr_times,
        "hitPauseTimer" = hit.hit_pause_timer,
        "mostRecentFriendlyFire" ~ attackers.most_recent_friendly_fire,
        "assistAttacker" = attackers.assist_attacker,
        "attackSpamTimes" = controls.attack_spam_times,
        "spamDelayed" = controls.spam_delayed,
        "rotThruAtkCount" ~ controls.rot_thru_atk_count,
        "spawnRequests" = requests.spawn_requests,
        "activateRequests" = requests.activate_requests,
        "despawnRequests" = requests.despawn_requests,
        "nearbyBuffRequests" = requests.nearby_buff_requests,
        "allyBuffRequests" = requests.ally_buff_requests,
        "aimer" = items.aimer,
        "aimerUnder" = items.aimer_under,
        "projAim" = items.proj_aim,
        "holdPack" = anim.hold_pack,
        "itemReleases" = items.item_releases,
        "montageMeshes" = montages.montage_meshes,
        "runSub" ~ sm.run_sub,
        "onEdge" ~ platforms.on_edge,
        "smActive" = sm.sm_active,
        "smTimes" = sm.sm_times,
        "a2gTimes" = sm.a2g_times,
        "edgeTimes" ~ sm.edge_times,
        "struggle" = buffs.struggle,
        "pull" = moves.pull,
        "forceMove" = moves.force_move,
        "forceFinish" = moves.force_finish,
        "ally" = team.ally,
        "weightSet" ~ body.weight_set,
        "bounceTimes" = pose.bounce_times,
    );
    let pair = |k: &str, a: &str, b: &str, l: &mut Lx| -> Option<(Value, Value)> {
        let o = as_obj(ob.get(k)?, l)?;
        Some((o.get(a)?.clone(), o.get(b)?.clone()))
    };
    l.at("chargeUi", |l| match pair("chargeUi", "start", "total", l) {
        Some((a, b)) => {
            f.charge.charge_ui.0.load_value(&a, l);
            f.charge.charge_ui.1.load_value(&b, l);
        }
        None => l.miss("absent or not {start, total}"),
    });
    l.at("locoBlend", |l| match pair("locoBlend", "dur", "hermite", l) {
        Some((a, b)) => {
            f.anim.loco_blend.0.load_value(&a, l);
            f.anim.loco_blend.1.load_value(&b, l);
        }
        None => l.miss("absent or not {dur, hermite}"),
    });
    l.at("wobbleTimes", |l| match pair("wobbleTimes", "front", "back", l) {
        Some((a, b)) => {
            f.react.wobble_times.0.load_value(&a, l);
            f.react.wobble_times.1.load_value(&b, l);
        }
        None => l.miss("absent or not {front, back}"),
    });
    // (comboReset is the World's, loaded with the World's rest)
    let key = |k: &str, l: &mut Lx, set: &mut dyn FnMut(&Value, &mut Lx)| match ob.get(k) {
        Some(v) => l.at(k, |l| set(v, l)),
        None => l.miss(format_args!("key {k} absent")),
    };
    key("hold", l, &mut |v, l| f.holds.hold.load_value(v, l));
    key("holdBuff", l, &mut |v, l| f.holds.hold_buff.load_value(v, l));
    key("holdHits", l, &mut |v, l| f.holds.hold_hits.load_value(v, l));
    key("holdVictims", l, &mut |v, l| f.holds.hold_victims.load_value(v, l));
    key("onTurn", l, &mut |v, l| match v {
        Value::Null => f.frame.on_turn = OnTurn::None,
        v => l.miss(format_args!("{v:?} (no rule to load an onTurn closure)")),
    });
    key("surfaceTraces", l, &mut |v, l| f.frame.surface_traces.load_value(v, l));
    key("voLines", l, &mut |v, l| vo_lines_load(f, v, l));
    // (a shape left out is unset)
    match ob.get("disguise") {
        Some(v) => l.at("disguise", |l| f.shapes.disguise.load_value(v, l)),
        None => f.shapes.disguise = None,
    }
    l.at("grouped", |l| crate::parts::grouped_load(&mut f.montages.grouped, ob.get("grouped"), l));
    match ob.get("polymorph") {
        Some(v) => l.at("polymorph", |l| f.shapes.polymorph.load_value(v, l)),
        None => f.shapes.polymorph = None,
    }
    match ob.get("carried") {
        Some(v) => l.at("carried", |l| f.shapes.carried.load_value(v, l)),
        None => f.shapes.carried = None,
    }
    match ob.get("fireWall") {
        Some(v) => l.at("fireWall", |l| f.shapes.fire_wall.load_value(v, l)),
        None => f.shapes.fire_wall = None,
    }
    match ob.get("actorScale") {
        Some(v) => l.at("actorScale", |l| {
            let Some(o) = as_obj(v, l) else { return };
            let mut at = f.body.actor_scale.unwrap_or([1.0, 1.0]);
            for (i, k) in ["xy", "z"].into_iter().enumerate() {
                match o.get(k) {
                    Some(v) => l.at(k, |l| at[i].load_value(v, l)),
                    None => l.miss(format_args!("key {k} absent")),
                }
            }
            check_keys(o, &["xy", "z"], l);
            f.body.actor_scale = Some(at);
        }),
        None => f.body.actor_scale = None,
    }
    match ob.get("isPolymorphed") {
        Some(v) => l.at("isPolymorphed", |l| f.shapes.is_polymorphed.load_value(v, l)),
        None => f.shapes.is_polymorphed = false,
    }
    let mut known: Vec<&str> = plain_keys!(names
        "base" = platforms.base,
        "slot" ~ life.slot,
        "itemReach" ~ items.item_reach,
        "uncontrolled" ~ life.uncontrolled,
        "lastAttackTutorialEvent" ~ tutorial.last_attack_tutorial_event,
        "activeInTutorialIntro" ~ tutorial.active_in_tutorial_intro,
        "disableCD" ~ abilities.disable_cd,
        "jumpHoldTimer" ~ jump.jump_hold_timer,
        "wasAttackingDuringFastfall" ~ fast_fall.was_attacking_during_fastfall,
        "dropTutorialAt" ~ tutorial.drop_tutorial_at,
        "minionCreator" = life.minion_creator,
        "walkBase" = platforms.walk_base,
        "sweptBase" = platforms.swept_base,
        "tempHealth" = stats.temp_health,
        "dodge" ~ evade.dodge,
        "lightProjectileArmor" ~ invuln.light_projectile_armor,
        "airSpecialMaxSet" ~ air.air_special_max_set,
        "blockCooldowns" ~ abilities.block_cooldowns,
        "blockFFTime" ~ fast_fall.block_ff_time,
        "holdCharge" ~ charge.hold_charge,
        "allowWallPlay" ~ wall.allow_wall_play,
        "disableDash" ~ moves.disable_dash,
        "wallBounceForceVertMin" ~ hit.wall_bounce_force_vert_min,
        "wallBounceForcedVertMinAmount" ~ hit.wall_bounce_forced_vert_min_amount,
        "launchOnHit" ~ hit.launch_on_hit,
        "headScale" ~ pose.head_scale,
        "animBounce" ~ hit.anim_bounce,
        "hitRotationOff" ~ react.hit_rotation_off,
        "chargeAttackMultiplier" ~ charge.charge_attack_multiplier,
        "chargeTotalTime" ~ charge.charge_total_time,
        "chargeTimer" ~ charge.charge_timer,
        "winEnds" ~ branching.win_ends,
        "buttonStates" ~ branching.button_states,
        "fading" = montages.fading,
        "blockEvadeMontage" ~ evade.block_evade_montage,
        "moveNone" ~ moves.move_none,
        "noGroundEvade" ~ evade.no_ground_evade,
        "invisMore" ~ vis.invis_more,
        "alwaysVisible" ~ vis.always_visible,
        "invisShown" ~ vis.invis_shown,
        "condObjs" ~ buffs.cond_objs,
        "hitMirror" ~ vis.hit_mirror,
        "abilityOver" # abilities.ability_over,
        "advancing" = montages.advancing,
        "dispatchOwed" = root.dispatch_owed,
        "pendingMoveEnds" = branching.pending_move_ends,
        "lateAuto" = branching.late_auto,
        "winCleared" = branching.win_cleared,
        "overridePacks" = branching.override_packs,
        "foreignWins" = branching.foreign_wins,
        "chargeRamp" = montages.charge_ramp,
        "fadeEnds" = montages.fade_ends,
        "proxyOwed" = montages.proxy_owed,
        "netLate" = moves.net_late,
        "gravityOverride" ~ moves.gravity_override,
        "groundJump" = moves.ground_jump,
        "friction" ~ moves.friction,
        "inputOv" ~ moves.input_ov,
        "buffDecays" ~ moves.buff_decays,
        "decayT0" ~ moves.decay_t0,
        "fly" ~ moves.fly,
        "maxAccel" ~ moves.max_accel,
        "speedOverride" ~ moves.speed_override,
        "folds" ~ moves.folds,
        "prevAtkCancelled" ~ moves.prev_atk_cancelled,
        "cancelEvadeBuffs" ~ moves.cancel_evade_buffs,
        "timedArmor" ~ armor.timed_armor,
        "flyInput" = moves.fly_input,
        "armorHid" ~ armor.armor_hid,
        "fastFallStartY" ~ fast_fall.fast_fall_start_y,
        "armorShake" ~ hit.armor_shake,
        "noWallBounce" ~ hit.no_wall_bounce,
        "hurtOff" = hit.hurt_off,
        "subHidden" = pose.sub_hidden,
        "blendOutSubs" = pose.blend_out_subs,
        "seqOn" = anim.seq_on,
        "hrTimes" = react.hr_times,
        "hitPauseTimer" = hit.hit_pause_timer,
        "mostRecentFriendlyFire" ~ attackers.most_recent_friendly_fire,
        "assistAttacker" = attackers.assist_attacker,
        "attackSpamTimes" = controls.attack_spam_times,
        "spamDelayed" = controls.spam_delayed,
        "rotThruAtkCount" ~ controls.rot_thru_atk_count,
        "spawnRequests" = requests.spawn_requests,
        "activateRequests" = requests.activate_requests,
        "despawnRequests" = requests.despawn_requests,
        "nearbyBuffRequests" = requests.nearby_buff_requests,
        "allyBuffRequests" = requests.ally_buff_requests,
        "aimer" = items.aimer,
        "aimerUnder" = items.aimer_under,
        "projAim" = items.proj_aim,
        "holdPack" = anim.hold_pack,
        "itemReleases" = items.item_releases,
        "montageMeshes" = montages.montage_meshes,
        "runSub" ~ sm.run_sub,
        "onEdge" ~ platforms.on_edge,
        "smActive" = sm.sm_active,
        "smTimes" = sm.sm_times,
        "a2gTimes" = sm.a2g_times,
        "edgeTimes" ~ sm.edge_times,
        "voLines" = vo.vo_lines,
        "struggle" = buffs.struggle,
        "pull" = moves.pull,
        "forceMove" = moves.force_move,
        "forceFinish" = moves.force_finish,
        "ally" = team.ally,
        "weightSet" ~ body.weight_set,
        "bounceTimes" = pose.bounce_times,
    )
    .to_vec();
    known.extend(HAND_KEYS);
    check_keys(ob, &known, l);
}

/// A condition class's state (HitCond `{hasHit, bound}`; Finn's aim dash `{hasHit, hitCheck}`,
/// both always false: nothing sets them).
impl Plain for mvs_sim::combat::conditions::CondState {
    fn to_value(&self, _x: &Wx) -> Value {
        use mvs_sim::combat::conditions::CondState;
        match *self {
            CondState::Hit { has_hit, bound } => {
                obj(vec![("bound", Value::Bool(bound)), ("hasHit", Value::Bool(has_hit))])
            }
            CondState::FinnAimDash => obj(vec![("hasHit", Value::Bool(false)), ("hitCheck", Value::Bool(false))]),
        }
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(s) = Self::from_value(v, l) {
            *self = s;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let ob = as_obj(v, l)?;
        if ob.get("hitCheck").is_some() {
            check_keys(ob, &["hasHit", "hitCheck"], l);
            return Some(mvs_sim::combat::conditions::CondState::FinnAimDash);
        }
        check_keys(ob, &["bound", "hasHit"], l);
        let has_hit = bool::from_value(ob.get("hasHit")?, l)?;
        let bound = bool::from_value(ob.get("bound")?, l)?;
        Some(mvs_sim::combat::conditions::CondState::Hit { has_hit, bound })
    }
}

// The rest's types whose values the traces never hold: written as null and named, never loaded.
opaque!(
    mvs_sim::combat::shapes::Carried,
    mvs_sim::fighter::match_side::ActivateRequest,
    mvs_sim::fighter::match_side::DespawnRequest,
    mvs_sim::combat::defense::TimedArmor,
);

// An item let go this step, its throw still to come (`{it, dx, dy, x, y, vx, vy, attackId}`): a release made after
// the step's item pass is held to the next.
impl crate::conv::Plain for mvs_sim::fighter::match_side::ItemRelease {
    fn to_value(&self, x: &crate::cx::Wx) -> mvs_trace::Value {
        use mvs_trace::Value::Num;
        let it = mvs_sim::actors::ActorRef::Item(self.it).to_value(x);
        crate::conv::obj(vec![
            ("attackId", Num(f64::from(self.attack_id))),
            ("dx", Num(f64::from(self.dx))),
            ("dy", Num(f64::from(self.dy))),
            ("it", it),
            ("vx", Num(f64::from(self.vx))),
            ("vy", Num(f64::from(self.vy))),
            ("x", Num(f64::from(self.x))),
            ("y", Num(f64::from(self.y))),
        ])
    }
    fn load_value(&mut self, v: &mvs_trace::Value, l: &mut crate::cx::Lx) {
        if let Some(r) = Self::from_value(v, l) {
            *self = r;
        }
    }
    fn from_value(v: &mvs_trace::Value, l: &mut crate::cx::Lx) -> Option<Self> {
        let mvs_trace::Value::Obj(o) = v else {
            l.miss("an item release that is not an object");
            return None;
        };
        let num = |k: &str, l: &mut crate::cx::Lx| match o.get(k) {
            Some(mvs_trace::Value::Num(n)) => Some(mvs_sim::math::narrow(*n)),
            _ => {
                l.miss(format_args!("an item release with no {k}"));
                None
            }
        };
        let Some(mvs_sim::actors::ActorRef::Item(it)) = mvs_sim::actors::ActorRef::from_value(o.get("it")?, l) else {
            l.miss("an item release whose item is no item");
            return None;
        };
        let attack_id = match o.get("attackId") {
            #[allow(clippy::cast_possible_truncation, reason = "an attack count")]
            Some(mvs_trace::Value::Num(n)) => *n as i32,
            _ => {
                l.miss("an item release with no attackId");
                return None;
            }
        };
        Some(Self {
            it,
            dx: num("dx", l)?,
            dy: num("dy", l)?,
            x: num("x", l)?,
            y: num("y", l)?,
            vx: num("vx", l)?,
            vy: num("vy", l)?,
            attack_id,
        })
    }
}

// BaseFighter's JumpHoldTimer (a tutorial's only: `{left, ticks}`).
crate::conv::plain_struct!(mvs_sim::fighter::moves::HoldTimer { "left" = left, "ticks" = ticks });

// A mode pawn's own item reach (`{items, closest}`, the oracle's ItemReach).
crate::conv::plain_struct!(mvs_sim::fighter::match_side::ItemReach { "closest" = closest, "items" = items });

/// A TutorialEvent: its byte (the oracle enum's number).
impl crate::conv::Plain for mvs_sim::fighter::match_side::TutorialEvent {
    fn to_value(&self, _: &crate::cx::Wx) -> mvs_trace::Value {
        mvs_trace::Value::Num(f64::from(self.byte()))
    }
    fn load_value(&mut self, v: &mvs_trace::Value, l: &mut crate::cx::Lx) {
        if let Some(e) = Self::from_value(v, l) {
            *self = e;
        }
    }
    fn from_value(v: &mvs_trace::Value, l: &mut crate::cx::Lx) -> Option<Self> {
        let n = crate::conv::num(v, l)?;
        crate::conv::int(n, l).map(Self::from_byte)
    }
}

/// A platform reference (`base`, `walkBase`, `sweptBase`): Morty's ship is an oracle World list's own object (the Ship
/// class is its own Platform), written by reference `{ref: "ships", id}`. A world actor's box (a SpawnedPlatform part,
/// StevenCollisionBlock's Box) is not in a list: the oracle writes it whole, as its actor's `solids` write it. The
/// other platforms have no rule yet.
/// The part index a live world actor's box waits with, plus its place in `Lx::pending_plats`, until
/// [`resolve_actor_plats`] (no actor holds that many parts).
const PENDING_PLAT: u32 = 1 << 30;

/// Turns the markers `PlatformRef::from_value` left on the fighters' platform references into the actor's part whose
/// flags the record wrote; the first part of the same wall flag where none matches (named).
pub fn resolve_actor_plats(w: &mut mvs_sim::World, l: &mut Lx) {
    let resolve = |w: &mvs_sim::World, l: &mut Lx, r: PlatformRef| -> PlatformRef {
        let PlatformRef::Spawned(a @ ActorRef::Actor(id), j) = r else { return r };
        if j < PENDING_PLAT {
            return r;
        }
        let Some(&(wall, ignored, trace, nav, item)) = l.pending_plats.get((j - PENDING_PLAT) as usize) else {
            return r;
        };
        let parts = w.actors.actors.get(id).and_then(|x| x.plat.as_ref()).map_or(&[][..], |p| &p.parts[..]);
        let same = |p: &&mvs_sim::world::spawned_platform::PlatformPart| {
            p.wall == wall && p.ignored == ignored && p.trace == trace && p.nav == nav && p.item == item
        };
        let i = match parts.iter().position(|p| same(&p)) {
            Some(i) => i,
            None => {
                l.miss("a world actor's box whose flags no part of the actor has");
                parts.iter().position(|p| p.wall == wall).unwrap_or(usize::from(wall))
            }
        };
        PlatformRef::Spawned(a, u32::try_from(i).unwrap_or(0))
    };
    let ids: Vec<_> = w.fighters.iter().map(|(id, _)| id).collect();
    for id in ids {
        let (base, list) = {
            let p = &w.fighter(id).platforms;
            (p.base, p.platforms.clone())
        };
        let base = base.map(|r| resolve(w, l, r));
        let list: Vec<_> = list.into_iter().map(|r| resolve(w, l, r)).collect();
        let p = &mut w.fighter_mut(id).platforms;
        p.base = base;
        p.platforms = list;
    }
}

impl Plain for PlatformRef {
    fn to_value(&self, x: &Wx) -> Value {
        match *self {
            PlatformRef::Spawned(r @ ActorRef::Ship(_), 0) => r.to_value(x),
            PlatformRef::Spawned(ActorRef::Actor(id), i)
                if let Some(a) = x.w.actors.actors.get(id)
                    && let Some(pt) = a.plat.as_ref().and_then(|pl| pl.parts.get(i as usize)) =>
            {
                // (a UFO's three boxes are `UfoPart`s: their `comp` and `main` besides)
                match &a.state {
                    mvs_sim::actors::world_actor::WorldActorState::Ufo(_) if i < 3 => {
                        crate::marvin::ufo::ufo_part_value(pt, a.seq, x, i as usize)
                    }
                    _ => crate::shield_actor::part_put(pt, a.seq, x),
                }
            }
            // A projectile's box (ProjectileBox, `rest.solids`): written whole with its projectile by reference.
            PlatformRef::Spawned(ActorRef::Projectile(id), i)
                if let Some(p) = x.w.actors.projectiles.get(id)
                    && let Some(b) = p.solids.as_ref().and_then(|bs| bs.get(i as usize)) =>
            {
                crate::actors::solid_put(b, p.seq, x)
            }
            // (a projectile gone from its list: its boxes as they ended, its actor `{ref: 'spawned', id}`)
            PlatformRef::Spawned(ActorRef::Projectile(id), i)
                if let Some((_, seq, bs)) = x.w.actors.gone_boxes.iter().find(|g| g.0 == id)
                    && let Some(b) = bs.get(i as usize) =>
            {
                let mut v = crate::actors::solid_put(b, *seq, x);
                if let Value::Obj(o) = &mut v {
                    o.insert(
                        "actor".into(),
                        obj(vec![("id", Value::Num(f64::from(*seq))), ("ref", Value::Str("spawned".into()))]),
                    );
                }
                v
            }
            // (one whose actor left World.actors: the box as it ended, its actor `{ref: 'spawned', id}`)
            PlatformRef::Spawned(r @ ActorRef::Actor(id), i)
                if let Some(pt) =
                    x.w.actors.gone_plats.iter().find(|g| g.id == id).and_then(|g| g.parts.get(i as usize)) =>
            {
                let mut v = crate::shield_actor::part_put(pt, 0, x);
                if let Value::Obj(o) = &mut v {
                    o.insert("actor".into(), r.to_value(x));
                }
                v
            }
            // A stage platform or a stage solid's box: the object the stage's record holds, written whole.
            PlatformRef::Stage(i) if (i as usize) < x.w.stage.stage_platforms.len() => {
                crate::stages::platform_put(x.w, i as usize, x)
            }
            PlatformRef::Solid(i)
                if let Some(r) = x.w.stage_run.as_ref()
                    && let Some(b) = r.boxes.get(i as usize) =>
            {
                crate::stages::box_put(r, b, x)
            }
            // A stage solid's polygon (`StageSolidPoly`): written whole, its solid's state as `s`.
            PlatformRef::Poly(i)
                if let Some(r) = x.w.stage_run.as_ref()
                    && let Some(s) = r.solids.get(i as usize) =>
            {
                let list = |v: &[f64]| Value::List(v.iter().map(|&n| Value::Num(n)).collect());
                mvs_sim::fighter::traces::with_poly(x.w, i, |p| {
                    obj(vec![
                        ("b0", p.b0.to_value(x)),
                        ("dead", Value::Bool(false)),
                        ("dx", Value::Num(0.0)),
                        ("dy", Value::Num(0.0)),
                        ("flags", p.flags.to_value(x)),
                        ("s", s.to_value(x)),
                        ("t0", p.t0.to_value(x)),
                        ("vx", Value::Num(0.0)),
                        ("vy", Value::Num(0.0)),
                        ("xs", list(&p.poly.xs)),
                        ("ys", list(&p.poly.ys)),
                    ])
                })
            }
            _ => {
                x.unmapped("a mvs_sim::fighter::body::PlatformRef other than a ship (no rule to write it)");
                Value::Null
            }
        }
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(r) = Self::from_value(v, l) {
            *self = r;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        // A world actor's box, written whole: its actor, the part by its wall flag.
        // Unverified: the part's index is taken as Collision 0, WallbounceCollision 1 (the parent's push order); the
        // record names no index, and the load has no World to match the box against.
        if let Value::Obj(ob) = v
            && ob.get("ref").is_none()
            && let Some(a) = ob.get("actor")
        {
            let r = ActorRef::from_value(a, l)?;
            let wall = matches!(ob.get("wall"), Some(Value::Bool(true)));
            // (a UFO's `UfoPart`: Collision 0, CollisionMidLeft 1, CollisionMidRight 2)
            let ufo = match ob.get("comp") {
                Some(Value::Str(c)) => {
                    ["Collision", "CollisionMidLeft", "CollisionMidRight"].iter().position(|n| n == c)
                }
                _ => None,
            };
            if let Some(i) = ufo {
                return Some(PlatformRef::Spawned(r, u32::try_from(i).unwrap_or(0)));
            }
            let i = u32::from(wall);
            // (a projectile gone from its list: its box as it ended joins the World's `gone_boxes`)
            if let ActorRef::Projectile(id) = r
                && !l.projectiles.iter().any(|(_, q)| *q == id)
                && let Value::Obj(ao) = a
                && let Some(seq) = ao.get("id").and_then(|v| i32::from_value(v, l))
                && let Some(b) = mvs_sim::actors::projectile::ProjectileBox::from_value(v, l)
            {
                let k = match l.gone_boxes.iter().position(|g| g.0 == id) {
                    Some(k) => k,
                    None => {
                        l.gone_boxes.push((id, seq, Vec::new()));
                        l.gone_boxes.len() - 1
                    }
                };
                let bs = &mut l.gone_boxes[k].2;
                if bs.len() <= i as usize {
                    bs.resize_with(i as usize + 1, Default::default);
                }
                bs[i as usize] = b;
            }
            // (a world actor gone from its list: its box as it ended joins the World's `gone_plats`)
            if let ActorRef::Actor(id) = r
                && !l.actors.iter().any(|(_, q)| *q == id)
            {
                let k = match l.gone_plats.iter().position(|g| g.0 == id) {
                    Some(k) => k,
                    None => {
                        l.gone_plats.push((id, Vec::new()));
                        l.gone_plats.len() - 1
                    }
                };
                // (a part below `i` the record does not show is this one's copy)
                while l.gone_plats[k].1.len() <= i as usize {
                    let part = crate::shield_actor::part_make(v, l);
                    l.gone_plats[k].1.push(part);
                }
                let part = crate::shield_actor::part_make(v, l);
                l.gone_plats[k].1[i as usize] = part;
            }
            // (a live world actor's box: a marker until its part is matched by its flags, as an actor may hold two
            // boxes of one wall flag: Steven's shield actor's Collision and WallBox)
            if let ActorRef::Actor(id) = r
                && l.actors.iter().any(|(_, q)| *q == id)
            {
                let flag = |k: &str| matches!(ob.get(k), Some(Value::Bool(true)));
                let ignored = ob.get("ignored").and_then(|v| i32::from_value(v, l)).unwrap_or(0);
                l.pending_plats.push((wall, ignored, flag("trace"), flag("nav"), flag("item")));
                let k = u32::try_from(l.pending_plats.len() - 1).unwrap_or(0);
                return Some(PlatformRef::Spawned(r, PENDING_PLAT + k));
            }
            return Some(PlatformRef::Spawned(r, i));
        }
        // A stage solid's polygon written whole: its solid by `s.id`.
        // Unverified: the solid's place in `StageState::solids` is taken as its id (`make_stage_run` lists the data's
        // solids in order, ids 0..n); the load has no World to look it up.
        if let Value::Obj(ob) = v
            && ob.get("ref").is_none()
            && ob.get("xs").is_some()
        {
            let id =
                ob.get("s").and_then(|s| as_obj(s, l)).and_then(|s| s.get("id")).and_then(|v| i32::from_value(v, l));
            return id.and_then(|i| u32::try_from(i).ok()).map(PlatformRef::Poly);
        }
        // A stage solid's box written whole: its place in the frame's `stageRun.boxes` (the same object).
        if let Value::Obj(ob) = v
            && ob.get("ref").is_none()
            && ob.get("l0").is_some()
            && ob.get("s").is_some()
        {
            return match l.stage_boxes.iter().position(|b| b == v) {
                Some(i) => u32::try_from(i).ok().map(PlatformRef::Solid),
                None => {
                    l.miss("a stage solid's box the frame's stageRun.boxes has not");
                    None
                }
            };
        }
        // A stage platform written whole: its `stageIndex`.
        if let Value::Obj(ob) = v
            && ob.get("ref").is_none()
            && let Some(i) = ob.get("stageIndex")
        {
            return i32::from_value(i, l).and_then(|i| u32::try_from(i).ok()).map(PlatformRef::Stage);
        }
        match ActorRef::from_value(v, l)? {
            r @ ActorRef::Ship(_) => Some(PlatformRef::Spawned(r, 0)),
            _ => {
                l.miss("a platform reference other than a ship (no rule to load it)");
                None
            }
        }
    }
}

/// The keys of an aimer but `p`.
macro_rules! aimer_keys {
    ($($pre:tt)*) => {
        plain_keys!($($pre)*
            "attackId" = attack_id, "sy" = sy, "sz" = sz, "ay" = ay, "az" = az, "throwOffset" ~ throw_offset,
            "fl" ~ fl, "pm" ~ pm,
        )
    };
}

/// `Aimer`: `{p, attackId, sy, sz, ay, az}` and `throwOffset`, `fl`, `pm` where
/// set. `p` is the Spawner notify's properties (a static object of the data) or a throw aimer's `{AimInfo}`.
impl Plain for Aimer {
    fn to_value(&self, x: &Wx) -> Value {
        let p = match &self.info {
            Some(i) => throw_aim_p(i),
            None => match x.book.aims.get(&(x.str(self.n.montage).to_owned(), self.n.index)) {
                Some(p) => p.clone(),
                None => {
                    x.unmapped("an aimer of a notify with no AimOn");
                    Value::Null
                }
            },
        };
        let mut o: Vec<(String, Value)> = vec![("p".to_owned(), p)];
        aimer_keys!(put self, x, o;);
        Value::Obj(mvs_trace::Obj::new(o))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(ob) = as_obj(v, l) else { return };
        if let Some(n) = aimer_notify(ob, Some(self.n), l) {
            self.n = n;
        }
        aimer_keys!(load self, ob, l;);
        self.info = aimer_info(ob, l);
        let mut known = vec!["p"];
        known.extend(aimer_keys!(names));
        check_keys(ob, &known, l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let ob = as_obj(v, l)?;
        let n = aimer_notify(ob, None, l)?;
        let mut a = mvs_sim::registry::spawner_notifies::new_aimer(n, 0, 0.0, 0.0, None);
        a.load_value(v, l);
        Some(a)
    }
}

/// The notify an aimer's record names by its `p` ([`crate::cx::Book::aims`]), among the loading fighter's own
/// character's notifies: `keep` where its `p` is the same, else the first whose aimer's `p` is (a throw aimer, with
/// `throwOffset`, of AnimNotify_AimItemThrow, a Spawner aimer of a Spawner notify).
// Unverified: two notifies whose aimers write the same whole `p` (the same properties in two montages) are one to the
// record and aim alike (the aimer reads only its notify's properties); the first is taken, and a spawn it makes names
// that one. Two whose `p` is written as `{static: ...}` (large property sets) need not be alike: such a tie is a miss.
fn aimer_notify(ob: &mvs_trace::Obj, keep: Option<NotifyRef>, l: &mut Lx) -> Option<NotifyRef> {
    let Some(p) = ob.get("p") else {
        l.miss("key p absent");
        return None;
    };
    let throw = ob.get("throwOffset").is_some();
    let book = l.book;
    let Some(own) = l.ch.as_ref().and_then(|c| book.char_owners.get(c)) else {
        l.miss("an aimer outside a fighter of a character of the data");
        return None;
    };
    let fits = |(m, i): &(String, u32), q: &Value| {
        let cls_throw = book.notify(m, *i).is_some_and(|id| id.cls == AIM_THROW);
        own.contains(m) && cls_throw == throw && same_value(q, p)
    };
    let found: Vec<&(String, u32)> = book.aims.iter().filter(|(k, q)| fits(k, q)).map(|(k, _)| k).collect();
    let is_static = matches!(p, Value::Obj(o) if o.get("static").is_some());
    if is_static && found.len() > 1 {
        l.at("p", |l| l.miss(format_args!("{} aimer notifies whose p is written as the same static", found.len())));
    }
    if let Some(k) = keep {
        let key = (l.str(k.montage).to_owned(), k.index);
        if found.contains(&&key) {
            return Some(k);
        }
    }
    match found.first() {
        Some((m, i)) => Some(NotifyRef { montage: l.name(m), index: *i }),
        None => {
            l.at("p", |l| l.miss("no aimer notify of the data with this p"));
            None
        }
    }
}

/// A throw aimer's AimInfo from its `p` (`None` for a Spawner aimer, which has no `throwOffset`).
fn aimer_info(ob: &mvs_trace::Obj, l: &mut Lx) -> Option<ThrowAimInfo> {
    ob.get("throwOffset")?;
    let info = match ob.get("p") {
        Some(Value::Obj(p)) => p.get("AimInfo"),
        _ => None,
    };
    let Some(Value::Obj(i)) = info else {
        l.miss("a throw aimer's p with no AimInfo");
        return None;
    };
    let mut get = |k: &str| match i.get(k) {
        Some(Value::Num(n)) => *n,
        _ => {
            l.miss(format_args!("p.AimInfo.{k} absent"));
            0.0
        }
    };
    Some(ThrowAimInfo {
        max: mvs_sim::math::narrow(get("AimAngleMax")),
        min: mvs_sim::math::narrow(get("AimAngleMin")),
        speed: mvs_sim::math::narrow(get("AimSpeed")),
        input_rotation: mvs_sim::math::narrow(get("InputRotationOffset")),
    })
}

/// A VO line's key as the oracle builds it: the fighter's id, `:vo`, the line's number.
fn vo_key(id: i32, n: i32) -> String {
    format!("{id}:vo{n}")
}

/// The line's number of a VO key of fighter `id`, `None` for any other string.
fn vo_key_n(id: i32, k: &str) -> Option<i32> {
    let n = k.strip_prefix(&format!("{id}:vo"))?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// `voLines`: `{ key, until }` each.
fn vo_lines_load(f: &mut Fighter, v: &Value, l: &mut Lx) {
    let Value::List(items) = v else {
        l.miss("not a list");
        return;
    };
    f.vo.vo_lines.clear();
    for (i, it) in items.iter().enumerate() {
        l.at(&i.to_string(), |l| {
            let Some(o) = as_obj(it, l) else { return };
            let key = match o.get("key") {
                Some(Value::Str(k)) => vo_key_n(f.id, k),
                _ => None,
            };
            let Some(key) = key else {
                l.miss("key absent or not a VO key of this fighter");
                return;
            };
            let mut until = 0.0;
            match o.get("until") {
                Some(v) => l.at("until", |l| until.load_value(v, l)),
                None => l.miss("key until absent"),
            }
            check_keys(o, &["key", "until"], l);
            f.vo.vo_lines.push(mvs_sim::fighter::match_side::VoLine { key, until });
        });
    }
}

// `launchOnHit` (the BPI-written launch a hit applies): every key written.
crate::conv::plain_struct!(mvs_sim::combat::hit::LaunchOnHit {
    "canFastFall" = can_fast_fall,
    "dirY" = dir_y,
    "dirZ" = dir_z,
    "max" = max,
    "min" = min,
    "speedClamp" = speed_clamp,
    "xy" = xy,
    "z" = z,
});

crate::conv::named_enum!(mvs_sim::fighter::anim::RunSubState, "run sub-state", [
    mvs_sim::fighter::anim::RunSubState::Run => "run",
    mvs_sim::fighter::anim::RunSubState::CrouchRun => "crouchRun",
    mvs_sim::fighter::anim::RunSubState::CrouchRunIn => "crouchRunIn",
    mvs_sim::fighter::anim::RunSubState::CrouchIntoCrouchRun => "crouchIntoCrouchRun",
]);
