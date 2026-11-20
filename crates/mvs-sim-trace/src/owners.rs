//! Each field of the record to the mvs-sim module that owns it (the crate README's module tree): where a mismatch
//! is to be looked for.

/// A fighter's typed rows, by record field.
const FIGHTER_ROWS: &[(&str, &str)] = &[
    ("abducted", "fighter::match_side"),
    ("abilities", "fighter::control"),
    ("active_hits", "fighter::anim"),
    ("add_kb_vx", "combat::hit"),
    ("add_kb_vy", "combat::hit"),
    ("aim_direction", "fighter::anim"),
    ("air_attack_available", "fighter::moves"),
    ("air_attack_delay", "fighter::moves"),
    ("air_boost_mult", "fighter::moves"),
    ("air_control", "fighter::moves"),
    ("air_control_gate", "fighter::moves"),
    ("air_evades", "fighter::moves"),
    ("air_specials", "fighter::moves"),
    ("ak_x", "fighter::body"),
    ("ak_y", "fighter::body"),
    ("anim", "fighter::anim"),
    ("anim_attack_off", "fighter::control"),
    ("anim_fast_fall_off", "fighter::moves"),
    ("anim_key", "fighter::anim"),
    ("anim_time", "fighter::anim"),
    ("anim_updated", "fighter::anim"),
    ("armor", "combat::defense"),
    ("armor_vis", "combat::defense"),
    ("assist_clear_tick", "combat::decay"),
    ("assists", "fighter::match_side"),
    ("attack_count", "combat::decay"),
    ("attack_decay", "combat::decay"),
    ("attack_is_active", "fighter::control"),
    ("attack_slot", "fighter::control"),
    ("attacks_dodged", "fighter::match_side"),
    ("base_vx", "combat::hit"),
    ("base_vy", "combat::hit"),
    ("begin_attack", "fighter::control"),
    ("blast_in", "fighter::match_side"),
    ("blast_listed", "fighter::match_side"),
    ("block_jump_end_launch", "fighter::moves"),
    ("block_land_reset", "fighter::moves"),
    ("block_pick_up", "fighter::match_side"),
    ("block_taunt_anims", "fighter::control"),
    ("bot_controlled", "fighter::control"),
    ("bounce_fx_until", "combat::hit"),
    ("bounce_pitch", "combat::hit"),
    ("bounce_pose", "fighter::anim"),
    ("bounce_type", "combat::hit"),
    ("bounce_z_offset", "combat::hit"),
    ("bouncing", "combat::hit"),
    ("br_axis_x", "fighter::anim"),
    ("br_axis_y", "fighter::anim"),
    ("br_stick_x", "fighter::anim"),
    ("branch_montage", "fighter::anim"),
    ("buff_stun", "combat::buffs"),
    ("buff_stun_age", "combat::buffs"),
    ("buff_stun_len", "combat::buffs"),
    ("buff_stunned", "combat::buffs"),
    ("buffs", "combat::buffs"),
    ("cam_focus", "fighter::match_side"),
    ("can_bounce_no_hitstun", "combat::hit"),
    ("cant_die", "fighter::match_side"),
    ("cap_half_u", "fighter::body"),
    ("cap_radius_u", "fighter::body"),
    ("cap_resized", "fighter::body"),
    ("capsule_off", "fighter::body"),
    ("character", "fighter"),
    ("charge_btn", "fighter::anim"),
    ("charge_buff_mult", "fighter::control"),
    ("charge_dmg_mult", "fighter::control"),
    ("charge_hit_large_at", "fighter::control"),
    ("charge_hit_small_at", "fighter::control"),
    ("charge_mult", "fighter::control"),
    ("charge_notify", "fighter::control"),
    ("charge_pct", "fighter::control"),
    ("charge_ramp_delay", "fighter::anim"),
    ("charge_released", "fighter::control"),
    ("charge_sound_key", "fighter::anim"),
    ("charge_sound_seq", "fighter::anim"),
    ("charge_start_time", "fighter::control"),
    ("charge_value", "fighter::control"),
    ("cmc_crouched", "fighter::body"),
    ("cmd_jumped", "fighter::moves"),
    ("combo_res_map", "combat::decay"),
    ("combo_reset_tick", "combat::decay"),
    ("combo_resistance", "combat::decay"),
    ("cooldown_multiplier", "fighter::control"),
    ("creation_time", "fighter::match_side"),
    ("crouch_fx", "fighter::moves"),
    ("crouch_jump", "fighter::moves"),
    ("crouch_locked", "fighter::moves"),
    ("crouch_owed", "fighter::moves"),
    ("crouching", "fighter::moves"),
    ("current_decay_sequence", "combat::decay"),
    ("damage", "combat::hit + combat::buffs"),
    ("damage_dealt", "fighter::match_side"),
    ("damage_disabled", "combat::defense"),
    ("dash_fall_gate", "fighter::moves"),
    ("dash_game_time", "fighter::moves"),
    ("dash_montage", "fighter::anim"),
    ("dash_stop_blend", "fighter::anim"),
    ("dash_stop_delay", "fighter::anim"),
    ("dash_tick", "fighter::moves"),
    ("dead", "fighter::match_side"),
    ("defer_queued_ends", "fighter::anim"),
    ("di_combo", "combat::decay"),
    ("direction_soft", "combat::hit"),
    ("dodged_successfully", "fighter::control"),
    ("double_ring_outs", "fighter::match_side"),
    ("down_btn", "fighter::moves"),
    ("down_press_active", "fighter::body"),
    ("down_press_count", "fighter::body"),
    ("down_stick", "fighter::moves"),
    ("down_window_tick", "fighter::body"),
    ("dropping_down", "fighter::body"),
    ("end_jump_latent", "fighter::moves"),
    ("equipped", "fighter::anim"),
    ("evade_count", "fighter::control"),
    ("evade_crouch_rate", "fighter::control"),
    ("evade_invuln", "fighter::control"),
    ("evade_invuln_time", "fighter::control"),
    ("evade_regen_once", "fighter::control"),
    ("evade_regen_start", "fighter::control"),
    ("evading", "fighter::control"),
    ("evading_through_platform", "fighter::body"),
    ("facing", "fighter::tick"),
    ("facing_dir", "fighter::body"),
    ("fade_anim", "fighter::anim"),
    ("fade_dur", "fighter::anim"),
    ("fade_el", "fighter::anim"),
    ("fade_loop", "fighter::anim"),
    ("fade_rate", "fighter::anim"),
    ("fade_time", "fighter::anim"),
    ("fall_lat_friction", "fighter::moves"),
    ("fall_lean", "combat::hit"),
    ("fast_falling", "fighter::moves"),
    ("ff_check_tick", "fighter::moves"),
    ("ff_mult", "fighter::moves"),
    ("ff_triggered", "fighter::moves"),
    ("floor_bounce_hurt", "combat::hit"),
    ("floor_swept", "fighter::body"),
    ("fluid_friction", "fighter::moves"),
    ("flying", "fighter::moves"),
    ("force_additive_kb", "combat::decay"),
    ("force_move_seq", "fighter::moves"),
    ("frame_mode", "fighter::tick"),
    ("free_launch", "combat::hit"),
    ("friction_zero", "fighter::moves"),
    ("grappled", "combat::buffs"),
    ("gravity_mod_count", "fighter::moves"),
    ("gravity_scale", "fighter::moves"),
    ("ground_jump_tick", "fighter::moves"),
    ("ground_state", "fighter::anim"),
    ("ground_state_time", "fighter::anim"),
    ("grounded", "fighter::tick"),
    ("gs_rel", "fighter::anim"),
    ("has_hit_a_fighter", "combat::decay"),
    ("has_spawn_invuln", "combat::defense"),
    ("held_item", "fighter::match_side"),
    ("hit_by_multiple_attackers", "combat::decay"),
    ("hit_launch", "fighter::moves"),
    ("hit_neutral", "combat::hit"),
    ("hit_pause_time", "combat::hit"),
    ("hit_react_int", "combat::hit"),
    ("hit_stun_ended_bound", "combat::decay"),
    ("hit_stun_kb_vx", "combat::hit"),
    ("hit_stun_kb_vy", "combat::hit"),
    ("hit_stunned_before_hit", "combat::decay"),
    ("hit_this_frame", "fighter::tick"),
    ("hitpause", "combat::hit"),
    ("hitstun", "combat::hit"),
    ("hitstun_count", "combat::decay"),
    ("hitstun_counted", "combat::hit"),
    ("hitstun_damage", "combat::decay"),
    ("hitstun_end_time", "combat::hit"),
    ("hitstun_paused", "combat::hit"),
    ("hitstun_pending", "combat::hit"),
    ("hitstun_rate", "combat::hit"),
    ("holding_enemy", "fighter::tick"),
    ("hr_branch", "combat::hit"),
    ("hr_intro", "combat::hit"),
    ("id", "fighter"),
    ("ignore_platforms", "fighter::body"),
    ("impulse", "fighter::moves"),
    ("in_ability_timers", "fighter::control"),
    ("in_hit_pause_pose", "combat::hit"),
    ("inp", "fighter::control"),
    ("instant_cooldowns", "fighter::control"),
    ("invisible", "fighter::anim"),
    ("invuln", "combat::defense"),
    ("invuln_count", "combat::defense"),
    ("is_minion", "fighter::match_side"),
    ("item_holstered", "fighter::match_side"),
    ("item_reachable", "fighter::match_side"),
    ("juggle_count", "combat::decay"),
    ("juggle_damage", "combat::decay"),
    ("jump_cut_at", "fighter::moves"),
    ("jump_frame", "fighter::moves"),
    ("jump_launched", "fighter::moves"),
    ("jump_min_time", "fighter::moves"),
    ("jump_released", "fighter::moves"),
    ("jump_start_time", "fighter::moves"),
    ("jump_stop_gate", "fighter::moves"),
    ("jump_v", "fighter::moves"),
    ("jumps", "fighter::moves"),
    ("kb_facing", "combat::hit"),
    ("kb_mode", "combat::hit"),
    ("knock_back_angle", "combat::hit"),
    ("knocked_back_count", "combat::decay"),
    ("ko_hitstun", "fighter::match_side"),
    ("kos", "world::flow"),
    ("landed_attack_ids", "combat::decay"),
    ("landed_decay_sequences", "combat::decay"),
    ("last_attacker_off_ledge", "combat::decay"),
    ("last_decay_stacks", "combat::decay"),
    ("last_direction", "fighter::moves"),
    ("last_evade_spent_meter", "fighter::control"),
    ("last_evade_time", "fighter::control"),
    ("last_hitstun_time", "combat::hit"),
    ("last_jump_time", "fighter::moves"),
    ("last_jump_v", "fighter::moves"),
    ("last_knocked_back", "combat::decay"),
    ("last_move_in", "fighter::moves"),
    ("last_ring_out_time", "fighter::match_side"),
    ("late_branch", "fighter::anim"),
    ("late_branch_due", "fighter::anim"),
    ("launch", "combat::hit"),
    ("layer_missed", "fighter::control"),
    ("linger_seq", "fighter::anim"),
    ("loco_anim", "fighter::anim"),
    ("loco_gen", "fighter::anim"),
    ("loco_loop", "fighter::anim"),
    ("loco_rate", "fighter::anim"),
    ("loco_time", "fighter::anim"),
    ("mesh_x", "combat::hit"),
    ("mesh_z", "combat::hit"),
    ("montage", "fighter::anim"),
    ("montage_abort", "fighter::anim"),
    ("montage_hit", "fighter::anim"),
    ("most_recent_attacker", "combat::decay"),
    ("move_cmds", "fighter::moves"),
    ("move_end", "fighter::moves"),
    ("move_in", "fighter::moves"),
    ("moved", "fighter::tick"),
    ("net", "fighter::moves"),
    ("net_friction", "fighter::moves"),
    ("net_gravity", "fighter::moves"),
    ("no_walk_off_ledges", "fighter::body"),
    ("no_wall_slide_until", "fighter::moves"),
    ("not_player_character", "fighter::match_side"),
    ("off_platform_ring_outs", "fighter::match_side"),
    ("on_camera", "fighter::anim"),
    ("on_floor", "fighter::body"),
    ("outline_count", "fighter::match_side"),
    ("pawn_vy", "fighter::body"),
    ("pending_ends", "fighter::anim"),
    ("pickup_request", "fighter::match_side"),
    ("plain_is_intro", "fighter::anim"),
    ("plat_ignored", "fighter::body"),
    ("played_in_advance", "fighter::anim"),
    ("pose_ticked", "fighter::anim"),
    ("pre_spawn_delay", "fighter::match_side"),
    ("prev", "fighter::control"),
    ("prev_bounce_time", "combat::hit"),
    ("prev_time", "fighter::match_side"),
    ("prev_x", "fighter::body"),
    ("prev_y", "fighter::body"),
    ("previous_attacker", "combat::decay"),
    ("projectile_armor", "combat::defense"),
    ("pushing_down", "fighter::moves"),
    ("queued", "fighter::anim"),
    ("queued_hit_pause", "combat::hit"),
    ("queued_hit_pause_sec", "combat::hit"),
    ("quick_fall_at", "fighter::moves"),
    ("rage", "combat::defense"),
    ("rage_charging", "combat::defense"),
    ("rage_enabled", "combat::defense"),
    ("rage_filled", "combat::defense"),
    ("rage_fx", "combat::defense"),
    ("rage_glow_shown", "combat::defense"),
    ("rage_input", "combat::defense"),
    ("rage_residual", "combat::defense"),
    ("rage_started", "combat::defense"),
    ("rage_timer", "combat::defense"),
    ("rapid_bounce_count", "combat::hit"),
    ("reach_recheck", "fighter::match_side"),
    ("recent_attacks", "fighter::control"),
    ("recent_hit_actor", "combat::decay"),
    ("rest", "combat::decay"),
    ("ring_outs", "world::flow"),
    ("ring_outs_assisted", "fighter::match_side"),
    ("rng", "rng"),
    ("root_dx", "fighter::anim"),
    ("root_dy", "fighter::anim"),
    ("root_has", "fighter::anim"),
    ("rot_lock_until", "fighter::control"),
    ("same_attack_blocked", "fighter::anim"),
    ("seq_delta", "fighter::anim"),
    ("seq_from", "fighter::anim"),
    ("size_scale", "fighter::body"),
    ("spawned", "fighter::match_side"),
    ("speed_mod", "combat::buffs"),
    ("starting_damage", "fighter::match_side"),
    ("state", "chars"),
    ("stop_ahead", "fighter::moves"),
    ("stun_source", "combat::decay"),
    ("suspended", "combat::buffs"),
    ("taunt_slot", "fighter::control"),
    ("team", "fighter::match_side"),
    ("terminal_v", "combat::hit"),
    ("tick", "fighter::match_side"),
    ("time", "fighter::match_side"),
    ("time_in_air", "fighter::moves"),
    ("total_charged_time", "fighter::control"),
    ("true_hitstun", "combat::hit"),
    ("true_hitstun_counted", "combat::hit"),
    ("true_hitstun_rate", "combat::hit"),
    ("true_hitstun_set_tick", "combat::hit"),
    ("true_hitstun_tick_at", "combat::hit"),
    ("vel_clamped", "fighter::body"),
    ("vo_current", "fighter::match_side"),
    ("vo_recent", "fighter::match_side"),
    ("vo_seq", "fighter::match_side"),
    ("vo_speaking", "fighter::match_side"),
    ("vx", "fighter::body"),
    ("vy", "fighter::body"),
    ("walk_drop", "fighter::body"),
    ("wall_capped", "fighter::moves"),
    ("wall_fatigue", "fighter::moves"),
    ("wall_fatigue_at", "fighter::moves"),
    ("wall_mult", "fighter::moves"),
    ("wall_skate", "fighter::moves"),
    ("wall_sliding", "fighter::moves"),
    ("wall_stall", "fighter::moves"),
    ("wall_timer_pending", "fighter::moves"),
    ("wall_touches", "fighter::moves"),
    ("was_attacking", "fighter::moves"),
    ("wind_on", "combat::hit"),
    ("x", "fighter::body"),
    ("y", "fighter::body"),
    ("zero_out_vel_for_kb", "combat::decay"),
];

/// A fighter's `rest` keys, by oracle key.
const FIGHTER_REST: &[(&str, &str)] = &[
    ("disguise", "combat::disguise"),
    ("polymorph", "combat::polymorph"),
    ("isPolymorphed", "combat::polymorph"),
    ("carried", "fighter"),
    ("fireWall", "actors::firewall"),
    ("comboReset", "combat::decay"),
    ("hold", "combat::holds"),
    ("holdBuff", "combat::holds"),
    ("holdHits", "combat::holds"),
    ("holdVictims", "combat::holds"),
    ("a2gTimes", "fighter::anim"),
    ("abilityOver", "fighter::control"),
    ("activateRequests", "fighter::match_side"),
    ("activeInTutorialIntro", "fighter::match_side"),
    ("actorScale", "fighter::body"),
    ("advancing", "fighter::anim"),
    ("aimer", "fighter::match_side"),
    ("aimerUnder", "fighter::match_side"),
    ("airSpecialMaxSet", "fighter::moves"),
    ("allowWallPlay", "fighter::moves"),
    ("ally", "fighter::match_side"),
    ("allyBuffRequests", "fighter::match_side"),
    ("alwaysVisible", "fighter::anim"),
    ("animBounce", "combat::hit"),
    ("armorHid", "combat::defense"),
    ("armorShake", "combat::hit"),
    ("assistAttacker", "combat::decay"),
    ("attackSpamTimes", "fighter::control"),
    ("base", "fighter::body"),
    ("blendOutSubs", "fighter::anim"),
    ("blockCooldowns", "fighter::control"),
    ("blockEvadeMontage", "fighter::control"),
    ("blockFFTime", "fighter::moves"),
    ("bounceTimes", "fighter::anim"),
    ("buffDecays", "fighter::moves"),
    ("buttonStates", "fighter::anim"),
    ("cancelEvadeBuffs", "fighter::moves"),
    ("chargeAttackMultiplier", "fighter::control"),
    ("chargeRamp", "fighter::anim"),
    ("chargeTimer", "fighter::control"),
    ("chargeTotalTime", "fighter::control"),
    ("chargeUi", "fighter::control"),
    ("condObjs", "combat::buffs"),
    ("decayT0", "fighter::moves"),
    ("despawnRequests", "fighter::match_side"),
    ("disableCD", "fighter::control"),
    ("disableDash", "fighter::moves"),
    ("dispatchOwed", "fighter::anim"),
    ("dodge", "fighter::control"),
    ("dropTutorialAt", "fighter::match_side"),
    ("edgeTimes", "fighter::anim"),
    ("fadeEnds", "fighter::anim"),
    ("fading", "fighter::anim"),
    ("fastFallStartY", "fighter::moves"),
    ("fly", "fighter::moves"),
    ("flyInput", "fighter::moves"),
    ("folds", "fighter::moves"),
    ("forceFinish", "fighter::moves"),
    ("forceMove", "fighter::moves"),
    ("foreignWins", "fighter::anim"),
    ("friction", "fighter::moves"),
    ("gravityOverride", "fighter::moves"),
    ("groundJump", "fighter::moves"),
    ("grouped", "fighter::anim"),
    ("headScale", "fighter::anim"),
    ("hitMirror", "fighter::anim"),
    ("hitPauseTimer", "combat::hit"),
    ("hitRotationOff", "combat::hit"),
    ("holdCharge", "fighter::control"),
    ("holdPack", "fighter::anim"),
    ("hrTimes", "combat::hit"),
    ("hurtOff", "combat::hit"),
    ("inputOv", "fighter::moves"),
    ("invisMore", "fighter::anim"),
    ("invisShown", "fighter::anim"),
    ("itemReach", "fighter::match_side"),
    ("itemReleases", "fighter::match_side"),
    ("jumpHoldTimer", "fighter::moves"),
    ("lastAttackTutorialEvent", "fighter::match_side"),
    ("lateAuto", "fighter::anim"),
    ("launchOnHit", "combat::hit"),
    ("lightProjectileArmor", "combat::defense"),
    ("locoBlend", "fighter::anim"),
    ("maxAccel", "fighter::moves"),
    ("minionCreator", "fighter::match_side"),
    ("montageMeshes", "fighter::anim"),
    ("mostRecentFriendlyFire", "combat::decay"),
    ("moveNone", "fighter::moves"),
    ("nearbyBuffRequests", "fighter::match_side"),
    ("netLate", "fighter::moves"),
    ("noGroundEvade", "fighter::control"),
    ("noWallBounce", "combat::hit"),
    ("onEdge", "fighter::body"),
    ("onTurn", "fighter::tick"),
    ("overridePacks", "fighter::anim"),
    ("pendingMoveEnds", "fighter::anim"),
    ("prevAtkCancelled", "fighter::moves"),
    ("projAim", "fighter::match_side"),
    ("proxyOwed", "fighter::anim"),
    ("pull", "fighter::moves"),
    ("rotThruAtkCount", "fighter::control"),
    ("runSub", "fighter::anim"),
    ("seqOn", "fighter::anim"),
    ("slot", "fighter::match_side"),
    ("smActive", "fighter::anim"),
    ("smTimes", "fighter::anim"),
    ("spamDelayed", "fighter::control"),
    ("spawnRequests", "fighter::match_side"),
    ("speedOverride", "fighter::moves"),
    ("struggle", "combat::buffs"),
    ("subHidden", "fighter::anim"),
    ("surfaceTraces", "fighter::tick"),
    ("sweptBase", "fighter::body"),
    ("tempHealth", "fighter::match_side"),
    ("timedArmor", "combat::defense"),
    ("uncontrolled", "fighter::match_side"),
    ("voLines", "fighter::match_side"),
    ("walkBase", "fighter::body"),
    ("wallBounceForceVertMin", "combat::hit"),
    ("wallBounceForcedVertMinAmount", "combat::hit"),
    ("wasAttackingDuringFastfall", "fighter::moves"),
    ("weightSet", "fighter::body"),
    ("winCleared", "fighter::anim"),
    ("winEnds", "fighter::anim"),
    ("wobbleTimes", "combat::hit"),
];

/// The World's rows and `rest` keys that are not `world`'s own.
const WORLD: &[(&str, &str)] = &[
    ("ground", "world::stage"),
    ("stagePlatforms", "world::stage"),
    ("blast", "world::blast"),
    ("blastOuter", "world::blast"),
    ("blast_shrink", "world::blast"),
    ("blastShrinkTick", "world::blast"),
    ("bonus_trackers", "world::flow"),
    ("comboReset", "combat::phases"),
    ("frame_hits", "combat::phases"),
    ("koBox", "world::blast"),
    ("koBox0", "world::blast"),
    ("overlapMidFrame", "combat::phases"),
    ("reach", "world::items"),
    ("ring_out_anchors", "world::blast"),
    ("shrunk", "world::blast"),
    ("timers", "world::flow"),
    ("runtimeSolids", "world::stage"),
    ("pawnCtl", "world::pawns"),
    ("waves", "modes::waves"),
    ("newPawns", "world::pawns"),
];

// Unverified: an event kind's owner is the module that pushes it in the oracle where one module does; the rest are
// `events`.
const EVENTS: &[(&str, &str)] = &[
    ("hit", "combat::hits + combat::hit"),
    ("pulseHit", "combat::buffs"),
    ("armorHit", "combat::defense"),
    ("armorBreak", "combat::defense"),
    ("ko", "world::blast"),
    ("koVfx", "world::blast"),
    ("blastBoxSound", "world::blast"),
    ("respawn", "fighter::tick"),
    ("teamScore", "world::flow"),
    ("stocks", "world::flow"),
    ("matchPoint", "world::flow"),
    ("matchPointShow", "world::flow"),
    ("doublePoints", "world::flow"),
    ("timeUp", "world::flow"),
    ("matchOver", "world::flow"),
    ("matchEnd", "world::flow"),
    ("introReveal", "world::flow"),
    ("introFx", "world::flow"),
    ("itemUiCreate", "world::items"),
    ("itemUi", "world::items"),
    ("itemUiExit", "world::items"),
    ("polymorphFx", "combat::polymorph"),
    ("reflectFx", "combat::reflect"),
    ("evadeFx", "fighter::control"),
    ("comboCounts", "combat::hit"),
    ("spawnedSound", "stages::tree_fort::pillar"),
    ("tutorialUi", "modes::tutorial::run"),
];

/// The Superman module that writes a `SupermanState` field.
fn superman_owner(key: &str) -> &'static str {
    match key {
        "angle_to_velocity_alpha" | "direction" | "direction_soft" | "fly_angle" | "fly_lean" => {
            "chars::superman::flight"
        }
        "aim_dash_from_x" | "aim_dash_from_y" => "chars::superman::aim_punch",
        _ => "chars::superman::pawn",
    }
}

/// The LeBron module that writes a `LebronState` field: the ball's hit writes
/// TeleportDunkReady (the dash and the pawn clear it), the dash starts the teleport dunk.
fn lebron_owner(key: &str) -> &'static str {
    match key {
        "teleport_dunk_ready" => "chars::lebron::ball",
        "is_teleport_dunking" => "chars::lebron::later",
        _ => "chars::lebron::pawn",
    }
}

/// The Arya module that writes an `AryaState` field: every one is the pawn's.
fn arya_owner(key: &str) -> &'static str {
    let _ = key;
    "chars::arya::pawn"
}

/// The Reindog module that writes a `ReindogState` field: the tether's setTether, the
/// pawn the rest (`is_held`: the pawn's setIsBeingHeld hook, which the fastball calls).
fn creature_owner(key: &str) -> &'static str {
    match key {
        "tether" => "chars::creature::tether",
        _ => "chars::creature::pawn",
    }
}

/// The Black Adam module that writes a `BlackAdamState` field: the pawn's tick the
/// hover direction, the AnimBP the lists and the slot mix, the chains the lightning manager.
fn black_adam_owner(key: &str) -> &'static str {
    match key {
        "hover_x" | "hover_y" => "chars::black_adam::pawn",
        "lightning" => "chars::black_adam::chain",
        _ => "chars::black_adam::anim",
    }
}

/// The Wonder Woman module that writes a `WonderWomanState` field: the meter its
/// own helpers (called from every file), the lasso's aim the lasso actor, the rest the pawn and its AnimBP.
fn wonder_woman_owner(key: &str) -> &'static str {
    match key {
        "meter" => "chars::wonder_woman::state",
        "lasso_aim_u" | "lasso_aim_d" => "chars::wonder_woman::actors",
        _ => "chars::wonder_woman::pawn",
    }
}

/// The Morty module that writes a `MortyState` field: the cook grenade's condition the pawn,
/// the slot graph's weights the fighter's tick, its mix the fighter's anim.
fn morty_owner(key: &str) -> &'static str {
    match key {
        "cook_grenade_behind" => "chars::morty::pawn",
        "slot_move_w" | "slot_back_w" => "fighter::tick",
        _ => "fighter::anim",
    }
}

/// The Iron Giant module that writes a `GiantState` field: the component's own
/// files (the hover rage, the ally turret, the AnimBP's variables), the grab's hold its two AnimBP flags, the crouch
/// listeners the state's binders (called by the smoke bomb), the rest the pawn.
fn iron_giant_owner(key: &str) -> &'static str {
    match key {
        "hover_rage" => "chars::iron_giant::hover_rage",
        "ally_turret" => "chars::iron_giant::turret",
        "giant_anim" => "chars::iron_giant::pose + chars::iron_giant::split",
        "hold_valid" | "hold_alpha" => "chars::iron_giant::hold",
        "crouch_listeners" => "chars::iron_giant::state",
        _ => "chars::iron_giant::pawn",
    }
}

/// Steven's state rows (`StevenRecord`): a record per file.
fn steven_owner(key: &str) -> &'static str {
    match key {
        "shield" => "chars::steven::shield",
        "watermelon" => "chars::steven::watermelon",
        "glide" => "chars::steven::glide",
        _ => "chars::steven::pawn",
    }
}

/// Bugs Bunny's state rows (`BugsRecord`): the glide blend's are the glide module's, the rest the pawn's.
fn bugs_owner(key: &str) -> &'static str {
    match key {
        k if k.starts_with("glide_") => "chars::bugs::glide",
        _ => "chars::bugs::pawn",
    }
}

/// Rick's state rows (`RickRecord`): Comp_Pawn_Portal's are the portal_comp module's, the rest the pawn's.
fn rick_owner(key: &str) -> &'static str {
    match key {
        "portal_comp" => "chars::rick::portal_comp",
        _ => "chars::rick::pawn",
    }
}

/// Finn's state rows (`FinnRecord`): a record per file.
fn finn_owner(key: &str) -> &'static str {
    match key {
        "coins" | "last_hit_actor" | "last_hit_time" | "drop_coin_override" | "drop_coin_amount" => {
            "chars::finn::coins"
        }
        "dash" | "aimer" => "chars::finn::aimer",
        "momentum" | "run_ui" => "chars::finn::momentum",
        "shop" => "chars::finn::shop",
        "bmo" => "chars::finn::bmo",
        _ => "chars::finn::pawn",
    }
}

/// jake's state rows: a record per group of files; the shared fields by the file that writes them.
fn jake_owner(key: &str) -> &'static str {
    match key {
        "eat" | "holding_actor" | "held_fighters" => "chars::jake::eat",
        "aim" | "aimer" => "chars::jake::aim",
        "weapon" | "weapon_equipped" | "weapons_hidden" => "chars::jake::weapons",
        "stretch_actor" => "chars::jake::stretch",
        "house_actor" => "chars::jake::house",
        "horse_actor" => "chars::jake::horse",
        "spit_aimer" => "chars::jake::spit",
        "ally_aim" => "chars::jake::ally_spit",
        _ => "chars::jake::pawn",
    }
}

/// Tom and Jerry's state rows (`TomAndJerryRecord`): a record per file; the AnimBP's `anim` aim.rs's (arms.rs writes
/// its `arms`), the live actors their files', the pawn's BPI flags and the slot mix the pawn's.
fn tom_and_jerry_owner(key: &str) -> &'static str {
    match key {
        "follow" => "chars::tom_and_jerry::follower",
        "champagne" | "cork_ammo" => "chars::tom_and_jerry::champagne",
        "fishing" => "chars::tom_and_jerry::fishing",
        "anim" => "chars::tom_and_jerry::aim",
        "jerry_actor" => "chars::tom_and_jerry::jerry",
        "sling_aimer" => "chars::tom_and_jerry::slingshot",
        "no_jerry_aimer" | "tennis_balls" => "chars::tom_and_jerry::tennis",
        _ => "chars::tom_and_jerry::pawn",
    }
}

/// Velma's state rows (`VelmaRecord`): a record per group of files; the shared fields by their writers (the
/// AnimBP's aim, carry and hit stun and GlassesOffCount the pawn's).
fn velma_owner(key: &str) -> &'static str {
    match key {
        "evidence" | "evidence_ui" => "chars::velma::evidence",
        "call" | "allow_police_call" | "police" => "chars::velma::police",
        "carry" | "carry_run" => "chars::velma::carry",
        "mark" => "chars::velma::mark",
        _ => "chars::velma::pawn",
    }
}

/// marvin's state rows: a record per file (the character's prep names them).
fn marvin_owner(key: &str) -> &'static str {
    match key {
        // AimValue: the aim module's update; the rest (the pawn's mix, aimDeg, disint, wallClingBlend, the ufo by
        // setCannon) the pawn module's.
        "aim_value" => "chars::marvin::aim",
        _ => "chars::marvin::pawn",
    }
}

/// Gizmo's state rows (`GizmoRecord`): a record per file; the last dash target is the attach's, the car and its
/// driving the car's, the COM override and the lantern the pawn's.
fn gizmo_owner(key: &str) -> &'static str {
    match key {
        "attach" | "last_dash_target" => "chars::gizmo::attach",
        "car" | "drives" | "driving" => "chars::gizmo::car",
        _ => "chars::gizmo::pawn",
    }
}

/// The velmapolice minion's state rows (`VelmaPoliceRecord`): police.rs's.
fn velma_police_owner(_key: &str) -> &'static str {
    "chars::velma::police"
}

/// Stripe's state rows (`StripeRecord`): a record per file; the tracker's target and aim are the gun's.
fn stripe_owner(key: &str) -> &'static str {
    match key {
        "gun" | "target" | "aim_back" | "aim_direction" => "chars::stripe::gun",
        "bomb" => "chars::stripe::bomb",
        "perks" => "chars::stripe::perks",
        "dash_listener" => "chars::stripe::dash",
        "saws" => "chars::stripe::saw",
        _ => "chars::stripe::pawn",
    }
}

/// Garnet's state rows (`GarnetRecord`): a record per file; the gauntlets' references the pawn's BPI, the dash's
/// fields the dash, IsActiveSinging the song.
fn garnet_owner(key: &str) -> &'static str {
    match key {
        "aim" => "chars::garnet::aim",
        "ground" => "chars::garnet::shockwave",
        "zone" => "chars::garnet::zone",
        "sing" | "is_active_singing" => "chars::garnet::sing",
        "location_before_move_x" | "location_before_move_y" | "charging_to_marker" => "chars::garnet::dash",
        _ => "chars::garnet::pawn",
    }
}

/// Taz's state rows (`TazRecord`, one flat record): by the file that writes each.
fn taz_owner(key: &str) -> &'static str {
    match key {
        "eat_level" => "chars::taz::pawn",
        "eaten_fighters" | "eat_free" | "eat_slots" | "spit_hit" | "spit_offset" => "chars::taz::eat",
        "projectiles_being_eaten" | "stored_projectile" | "stored_starting_speed" | "stored_max_speed" => {
            "chars::taz::projectile_eat + chars::taz::burp"
        }
        "burping_projectile" | "hitting_projectiles" => "chars::taz::burp",
        "dog_pile_active" | "dog_pile_at" | "dog_pile_end_at" | "fight_cloud_hurt" => "chars::taz::dogpile",
        "spawning_melee_buff_actor" => "chars::taz::tasty",
        "slot_mix"
        | "run_value"
        | "turning_blend"
        | "was_facing_left"
        | "landing_blend"
        | "moving_in_air_blend"
        | "was_moving_on_ground"
        | "percent_charged"
        | "aim_direction" => "chars::taz::pose",
        _ => "chars::taz::tornado",
    }
}

/// The module that writes a buff class's extension (`FighterBuff.ext`, `BuffExt`) when it is not combat::buffs,
/// with combat::buffs that keeps the list: a track's `--owner` sees its own classes' extensions.
const BUFF_EXT: &[(&str, &str)] = &[
    ("Buff_ScoobySnack_C", "combat::buffs + chars::shaggy"),
    ("Buff_Shaggy_DyingRage_C", "combat::buffs + chars::shaggy::perks"),
    ("Buff_Invisibility_C", "combat::buffs + actors::smoke_bomb"),
    ("Buff_Invisibility_Fog_C", "combat::buffs + actors::smoke_bomb"),
    ("Buff_SpeedUp_C", "combat::buffs + actors::smoke_bomb"),
    ("Buff_DecrementEvadeCount_C", "combat::buffs + actors::smoke_bomb"),
    ("Buff_PassiveApplyBuffonHit_C", "combat::buffs + actors::smoke_bomb"),
    ("Buff_BatmanOnHitApplyBuff_C", "combat::buffs + chars::batman::bomb"),
    ("Buff_DelayKnockUp_C", "combat::buffs + chars::batman::bomb"),
    ("Buff_Superman_FireSlam_C", "combat::buffs + chars::superman::perks"),
    ("Buff_Superman_SniperPunch_C", "combat::buffs + chars::superman::perks"),
    ("Buff_BasketballPass_C", "combat::buffs + chars::lebron::ball"),
    ("Buff_IgnitedBasketballPass_C", "combat::buffs + chars::lebron::ball"),
    ("Buff_Arya_Trophy_C", "combat::buffs + chars::arya::perks"),
    ("Buff_AryaBackStab_C", "combat::buffs + chars::arya::pawn"),
    ("Buff_BackStab_C", "combat::buffs + chars::arya::pawn"),
    ("Buff_TakeFace_C", "combat::buffs + combat::disguise"),
    ("Buff_Creature_FireStorm_C", "combat::buffs + chars::creature::perks"),
    ("C021_Buff_ElectricChainVictim_C", "combat::buffs + chars::black_adam::chain"),
    ("Buff_C021_EarthSmashVictim_C", "combat::buffs + chars::black_adam::bolt"),
    ("Buff_C021_SetHomingTowardsTarget_C", "combat::buffs + chars::black_adam::field"),
    ("Buff_Damage_Electric_C", "combat::buffs + combat::electric"),
    ("Buff_Low_Damage_Electric_C", "combat::buffs + combat::electric"),
    ("Buff_FireGauntlets_C", "combat::buffs + combat::gauntlet"),
    ("Buff_AllyElectricGauntlets_C", "combat::buffs + combat::gauntlet"),
    ("Buff_GarnetElecInsideBubble_C", "combat::buffs + combat::gauntlet"),
    ("Buff_GarnetElecOutsideBubble_C", "combat::buffs + combat::gauntlet"),
    ("Buff_GarnetFullPowerGauntlets_C", "combat::buffs + combat::gauntlet"),
    ("Buff_SupermanAllyIceGauntlet_C", "combat::buffs + combat::gauntlet"),
    ("Buff_NullifyNextHit_C", "combat::buffs + chars::wonder_woman::actors"),
    ("Buff_Perk_WonderWoman_FastFallSlam_C", "combat::buffs + chars::wonder_woman::perks"),
    ("Buff_Perk_WonderWoman_ProjectileBlock_C", "combat::buffs + chars::wonder_woman::perks"),
    ("Buff_ConfettiCount_C", "combat::buffs + chars::harley::actors"),
    ("Buff_ConfettiExplosionCount_C", "combat::buffs + chars::harley::actors"),
    ("Buff_C019_Cook_C", "combat::buffs + chars::morty::perks"),
    ("Buff_C017_AllyBurst_C", "combat::buffs + chars::iron_giant::actors"),
    ("Buff_C017_Ally_Pilot_C", "combat::buffs + chars::iron_giant::turret"),
    ("C017_TempHealth_DamageReflect_Passive_C", "combat::buffs + chars::iron_giant::perks"),
    ("Buff_HeldByPlasmaBall_C", "combat::buffs + chars::iron_giant::plasma"),
    ("Buff_C017_FastWeaponMode_C", "combat::buffs + chars::iron_giant::perks"),
    ("Buff_C017_ScorchedEarth_C", "combat::buffs + chars::iron_giant::perks"),
    ("Buff_StevenSharedShield_C", "combat::buffs + chars::steven::shield"),
    ("Buff_BubbleCount_C", "combat::buffs + chars::steven::tether"),
    ("Buff_StevenBubbleEnemy_C", "combat::buffs + chars::steven::tether"),
    ("Buff_Steven_GreenThumbDamageBoost_C", "combat::buffs + chars::steven::watermelon"),
    ("Buff_Garnet_ElectricGroove_C", "combat::buffs + chars::garnet::groove"),
    ("Buff_GarnetStackingSinging_C", "combat::buffs + chars::garnet::sing"),
    ("Buff_Chicken_C", "combat::buffs + chars::taz::chicken"),
    ("Buff_C015AttachToDogpile_C", "combat::buffs + chars::taz::perks_cloud"),
    ("Buff_C015_TeamFightCloud_C", "combat::buffs + chars::taz::perks_cloud"),
    ("Buff_Tasty_C", "combat::buffs + chars::taz::tasty"),
    ("Buff_ApplyTasty_C", "combat::buffs + chars::taz::tasty"),
    ("Buff_TastyOnHit_C", "combat::buffs + chars::taz::tasty"),
    ("Buff_TastyBlock_C", "combat::buffs + chars::taz::tasty"),
    ("Buff_c015_ApplybuffOnOverlap_C", "combat::buffs + chars::taz::tasty"),
    ("Buff_Tunneling_C", "combat::buffs + chars::bugs::tunnel"),
    ("Buff_TunnelTeleport_C", "combat::buffs + chars::bugs::tunnel"),
    ("Buff_Charm_C", "combat::buffs + chars::bugs::charm"),
    ("C020_Buff_Silenced_C", "combat::buffs + chars::rick"),
    ("Buff_Teleporting3_C", "combat::buffs + chars::rick::teleport"),
    ("Buff_PortalBuff_C", "combat::buffs + chars::rick::teleport"),
    ("Buff_Finn_GemOnCharge_C", "combat::buffs + chars::finn::perks"),
    ("Buff_Finn_GoingOutOfBusiness_C", "combat::buffs + chars::finn::perks"),
    ("Finn_Shop_Boots_Buff_C", "combat::buffs + chars::finn::shop"),
    ("Finn_Shop_Boots_Buff_Ally_C", "combat::buffs + chars::finn::shop"),
    ("Finn_Shop_ArmorBuns_Buff_C", "combat::buffs + chars::finn::shop"),
    ("Buff_EvidenceCount_C", "combat::buffs + chars::velma::evidence"),
    ("Buff_Studied_C", "combat::buffs + chars::velma::perks"),
    ("Buff_Arrested_C", "combat::buffs + chars::velma::police"),
    ("Buff_VelmaHealOverTimeBlast_C", "combat::buffs + chars::velma::beam"),
    ("Buff_VelmaAttachToAlly_C", "combat::buffs + chars::velma::capture"),
    ("Buff_SlowIntoStun_C", "combat::buffs + chars::velma::gadgets"),
    ("C023B_DashProjectileBlock_C", "combat::buffs + chars::stripe::dash"),
    ("Buff_Jake_WeaponMaster_C", "combat::buffs + chars::jake::weapons"),
    ("Buff_Jake_SafetyNet_C", "combat::buffs + chars::jake::perks"),
    ("Buff_AirSuperiority_C", "combat::buffs + chars::jake::perks"),
    ("Buff_Cheese_C", "combat::buffs + chars::tom_and_jerry::cheese"),
    ("Buff_C018HomingVictim_C", "combat::buffs + chars::marvin::homing"),
    ("Buff_DynamicMoveToActor_C", "combat::buffs + combat::dynamic_move"),
    ("Buff_StunnedDynamicMoveToActor_C", "combat::buffs + combat::dynamic_move"),
    ("Buff_MoveToActor2_C", "combat::buffs + combat::move_to_actor_2"),
    ("C023A_BowSlow_C", "combat::buffs + chars::gizmo::bow"),
    ("Buff_C023A_Backpack_C", "combat::buffs + chars::gizmo::perks"),
    ("C023A_DanceCount_Debuff_C", "combat::buffs + chars::gizmo::perks"),
    ("C023A_Music_Buff_C", "combat::buffs + chars::gizmo::music"),
    ("C023A_Music_DeBuff_C", "combat::buffs + chars::gizmo::music"),
    ("C023A_Car_Buff_C", "combat::buffs + chars::gizmo::car"),
    ("Car_Buff_C", "combat::buffs + items::car"),
    ("Mutator_InstantKill_C", "combat::buffs + combat::mutator_buffs"),
    ("Mutator_GrowWithDamage_C", "combat::buffs + combat::grow_with_damage"),
];

/// The owner of a buff entry's extension (`fighters[i].buffs[j].ext...`), by the entry's class in the frame:
/// [`BUFF_EXT`], else a perk class's combat::perks, else combat::buffs.
fn buff_ext_owner(p: &str, frame: &mvs_trace::Frame) -> Option<&'static str> {
    let rest = p.strip_prefix("fighters[")?;
    let (fi, rest) = rest.split_once("].buffs[")?;
    let (bi, rest) = rest.split_once(']')?;
    if !rest.starts_with(".ext") {
        return None;
    }
    let b = frame.world.fighters.get(fi.parse::<usize>().ok()?)?.buffs.get(bi.parse::<usize>().ok()?)?;
    if let Some(o) = find(BUFF_EXT, &b.class) {
        return Some(o);
    }
    let c = mvs_sim::combat::buff_class::BuffClass::ALL.iter().copied().find(|c| c.registered_name() == Some(&b.class));
    Some(if c.is_some_and(|c| mvs_sim::combat::perks::PerkExt::for_class(c).is_some()) {
        "combat::buffs + combat::perks"
    } else {
        "combat::buffs"
    })
}

/// The owner of a difference's path in a frame: an actor's by its family (`items` are actors::item's), a buff
/// extension's by its class ([`BUFF_EXT`]), the rest by [`owner`].
pub fn owner_in(path: &str, frame: &mvs_trace::Frame) -> &'static str {
    let p = path.strip_prefix("world.").unwrap_or(path);
    if let Some(o) = buff_ext_owner(p, frame) {
        return o;
    }
    if let Some(rest) = p.strip_prefix("actors[")
        && let Some(i) = rest.split(']').next().and_then(|i| i.parse::<usize>().ok())
        && let Some(a) = frame.world.actors.get(i)
    {
        return match a.family.as_str() {
            "items" => "actors::item",
            "actors" => "actors::world_actor",
            _ => "actors",
        };
    }
    owner(path)
}

fn find(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|e| e.1)
}

/// The Batman module that writes a `BatmanState` field.
fn batman_owner(key: &str) -> &'static str {
    match key {
        "aim_direction" | "horizontal_divided_by_max" | "slot_mix" => "chars::batman::anim",
        k if k.starts_with("grapple_") || k == "skip_impulse_validation" => "chars::batman::grapple",
        "glide_once_closed" | "glide_hurt_delay" => "chars::batman::pawn",
        k if k.starts_with("glide_") || k.starts_with("spring") => "chars::batman::glide",
        _ => "chars::batman::pawn",
    }
}

/// The tutorial module that writes a `rest.tutorial` field (`tutorial.sched.timers[0].at` the latent actions',
/// `tutorial.demos[0].pad.x` the demo controllers', `tutorial.actor.minigames[1].state` the minigames').
fn tutorial_owner(parts: &[&str]) -> &'static str {
    let key = |i: usize| parts.get(i).map_or("", |k| k.split('[').next().unwrap_or(k));
    match (key(0), key(1)) {
        ("sched", _) => "modes::tutorial::latent",
        ("demos", _) => "modes::tutorial::demo",
        ("actor", "minigames") => "modes::tutorial::minigame",
        _ => "modes::tutorial::run",
    }
}

/// The owner of a difference's path (`fighters[0].rest.hitstun`, `actors[1].rest.age`, `events[3]<hit>.x`).
pub fn owner(path: &str) -> &'static str {
    let path = path.strip_prefix("world.").unwrap_or(path);
    let parts: Vec<&str> = path.split('.').collect();
    let head = parts[0];
    let base = head.split('[').next().unwrap_or(head);
    match base {
        "inputs" => "input",
        "events" => {
            // (`events[3]<hit>.x`: the kind between the brackets)
            let kind = head.split('<').nth(1).and_then(|k| k.split('>').next()).unwrap_or("");
            find(EVENTS, kind).unwrap_or("events")
        }
        // (FireWall's own keys)
        "projectiles"
            if parts.get(1) == Some(&"rest")
                && parts.get(2).is_some_and(|k| {
                    [
                        "boxZ", "boxHx", "boxHy", "hurtZ", "hurtHalf", "hadFire", "hadIce", "inProj", "inItems",
                        "spawnYaw", "fxTeam",
                    ]
                    .contains(&k.split('[').next().unwrap_or(k))
                }) =>
        {
            "actors::firewall"
        }
        "projectiles" => "actors::projectile",
        "actors" => "actors",
        "fighters" => match parts.get(1) {
            Some(&"rest") => {
                parts.get(2).and_then(|k| find(FIGHTER_REST, k.split('[').next().unwrap_or(k))).unwrap_or("fighter")
            }
            Some(&"state") | Some(&"state<shaggy>") => "chars::shaggy",
            Some(&"state<batman>") => batman_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<superman>") => superman_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<lebron>") => lebron_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<arya>") => arya_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<creature>") => creature_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<blackadam>") => {
                black_adam_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k)))
            }
            Some(&"state<wonderwoman>") => {
                wonder_woman_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k)))
            }
            Some(&"state<harley>") => "chars::harley::pawn",
            Some(&"state<morty>") => morty_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<irongiant>") => {
                iron_giant_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k)))
            }
            Some(&"state<steven>") => steven_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<garnet>") => garnet_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<bugs>") => bugs_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<rick>") => rick_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<finn>") => finn_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<stripe>") => stripe_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<jake>") => jake_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<tomandjerry>") => {
                tom_and_jerry_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k)))
            }
            Some(&"state<velma>") => velma_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<marvin>") => marvin_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<gizmo>") => gizmo_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(&"state<velmapolice>") => {
                velma_police_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k)))
            }
            Some(&"state<taz>") => taz_owner(parts.get(2).map_or("", |k| k.split('[').next().unwrap_or(k))),
            Some(k) => find(FIGHTER_ROWS, k.split('[').next().unwrap_or(k)).unwrap_or("fighter"),
            None => "fighter",
        },
        "rest" if parts.get(1) == Some(&"tutorial") => tutorial_owner(&parts[2..]),
        "rest" => parts.get(1).and_then(|k| find(WORLD, k.split('[').next().unwrap_or(k))).unwrap_or("world"),
        // (the typed rows: the World's clock, rng and sequences are the World's own, the rest the match flow's)
        k => find(WORLD, k).unwrap_or(if ["tick", "rng", "actor_seq", "item_seq"].contains(&k) {
            "world"
        } else {
            "world::flow"
        }),
    }
}

/// The file or folder under `crates/mvs-sim/src` of a module path (`chars::morty` the folder, `fighter::body` the
/// file), if there is one.
fn module_exists(module: &str) -> bool {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mvs-sim/src").join(module.replace("::", "/"));
    p.is_dir() || p.with_extension("rs").is_file()
}

/// Whether some owner starts with `prefix` (a `--owner` that names none would filter every difference away): a
/// prefix of a table's owner, or a module of the sim's source tree (the owners the match arms name are its files).
pub fn is_owner_prefix(prefix: &str) -> bool {
    [FIGHTER_ROWS, FIGHTER_REST, WORLD, EVENTS, BUFF_EXT]
        .iter()
        .flat_map(|t| t.iter().map(|e| e.1))
        .any(|m| m.split(" + ").any(|m| m.starts_with(prefix)))
        || (!prefix.is_empty() && module_exists(prefix))
}

#[cfg(test)]
mod owner_tests {
    use super::*;

    /// Every owner a table names is a module of the sim (a renamed file shows here, not as an empty `--owner` run).
    #[test]
    fn every_table_owner_is_a_module() {
        let bad: Vec<&str> = [FIGHTER_ROWS, FIGHTER_REST, WORLD, EVENTS, BUFF_EXT]
            .iter()
            .flat_map(|t| t.iter().flat_map(|e| e.1.split(" + ")))
            .filter(|m| !module_exists(m))
            .collect();
        assert!(bad.is_empty(), "owners with no file under mvs-sim/src: {bad:?}");
    }

    /// Every class of [`BUFF_EXT`] is a registered buff class.
    #[test]
    fn every_buff_ext_class_is_registered() {
        use mvs_sim::combat::buff_class::BuffClass;
        let bad: Vec<&str> = BUFF_EXT
            .iter()
            .map(|e| e.0)
            .filter(|n| !BuffClass::ALL.iter().any(|c| c.registered_name() == Some(*n)))
            .collect();
        assert!(bad.is_empty(), "classes no module registers: {bad:?}");
    }

    #[test]
    fn a_buff_extension_is_its_class_module() {
        let mut frame = mvs_trace::Frame::default();
        let mut f = mvs_trace::record::FighterRecord::default();
        f.buffs.push(mvs_trace::record::BuffRecord { class: "Buff_C019_Cook_C".into(), ..Default::default() });
        f.buffs.push(mvs_trace::record::BuffRecord { class: "Buff_Nowhere_C".into(), ..Default::default() });
        frame.world.fighters.push(f);
        assert_eq!(owner_in("world.fighters[0].buffs[0].ext.left", &frame), "combat::buffs + chars::morty::perks");
        assert_eq!(owner_in("world.fighters[0].buffs[1].ext.left", &frame), "combat::buffs");
        assert_eq!(owner_in("world.fighters[0].buffs[0].frames", &frame), "combat::buffs");
        assert!(is_owner_prefix("chars::morty"));
        assert!(!is_owner_prefix("chars::nobody"));
    }
}

/// The path of a file under `crates/mvs-sim/src`, as `a/b.rs` (`crates/mvs-sim/src/a/b.rs`, `a\b.rs` and an
/// absolute path all give it), or `None` for a file outside that tree.
pub fn sim_source_rel(file: &str) -> Option<String> {
    let f = file.replace('\\', "/");
    let rel = match f.find("mvs-sim/src/") {
        Some(i) => &f[i + "mvs-sim/src/".len()..],
        None if f.starts_with("crates/") || f.starts_with('/') || f.contains(':') => return None,
        None => f.strip_prefix("src/").unwrap_or(&f),
    };
    rel.ends_with(".rs").then(|| rel.to_owned())
}

/// The module path of a source file of the sim (`chars/creature/tether.rs` is `chars::creature::tether`, a `mod.rs`
/// its folder's module). `None` for a file outside the sim or its crate root (`lib.rs`, which owns no module).
pub fn module_of_file(file: &str) -> Option<String> {
    let rel = sim_source_rel(file)?;
    let stem = rel.strip_suffix(".rs")?;
    let stem = stem.strip_suffix("/mod").unwrap_or(stem);
    (stem != "lib" && stem != "mod" && !stem.is_empty()).then(|| stem.replace('/', "::"))
}

/// The module path a `--owner` file names; the file must exist under `mvs-sim/src`.
pub fn module_of_existing_file(file: &str) -> Result<String, String> {
    let module =
        module_of_file(file).ok_or_else(|| format!("--owner {file} is no source file of crates/mvs-sim/src"))?;
    let rel = sim_source_rel(file).unwrap_or_default();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mvs-sim/src").join(&rel);
    if path.is_file() { Ok(module) } else { Err(format!("--owner {file}: no file {}", path.display())) }
}

#[cfg(test)]
mod file_module_tests {
    use super::module_of_file;

    #[test]
    fn a_source_file_is_its_module_path() {
        let m = |f| module_of_file(f).as_deref().map(str::to_owned);
        let tether = Some("chars::creature::tether".to_owned());
        assert_eq!(m("crates/mvs-sim/src/chars/creature/tether.rs"), tether);
        assert_eq!(m("chars/creature/tether.rs"), tether);
        assert_eq!(m(r"crates\mvs-sim\src\chars\creature\tether.rs"), tether);
        assert_eq!(m(r"chars\creature\tether.rs"), tether);
        assert_eq!(m("D:/src/mvs-wt1/crates/mvs-sim/src/chars/creature/tether.rs"), tether);
        assert_eq!(m("crates/mvs-sim/src/chars/creature/mod.rs"), Some("chars::creature".to_owned()));
        assert_eq!(m("fighter.rs"), Some("fighter".to_owned()));
        assert_eq!(m("crates/mvs-sim/src/lib.rs"), None);
        assert_eq!(m("crates/mvs-trace/src/record.rs"), None);
        assert_eq!(m("fighter::body"), None);
    }
}
