//! The record of one tick: the inputs given to the step and the World's state after it. The oracle's writer
//! has the same tables; each field's doc comment names the oracle field it reads
//! (`Fighter.x`). Every number is the oracle number's f64 (counters too); an id that names a sim object (a fighter, an
//! item) is its `id`, -1 for none.
//!
//! What is recorded: every own key of a fighter, a projectile, an item or another actor and of the World, but the
//! ones the writer's EXCLUDED names and the ones the oracle snapshot leaves out as no sim state. A key is a typed
//! field, or it goes in the record's `rest`, a plain value of every key no field writes: a sim object in it as
//! `{ref: family, id}`, the loaded game data as `{static: name}`, a notify by its identity, a Map as
//! `{map: [[key, value], ...]}`, a Set as `{set: [...]}`, any other object by its keys.

use crate::TraceError;
use crate::field::{Cursor, Difference, Field, push_path, split_path};
use crate::record;
use crate::value::{EventRecord, Value};

record! {
    /// One fighter's `InputState` as given to `World.step`; `None` where the oracle
    /// object has no value (`undefined`).
    pub struct InputRecord {
        /// `InputState.x`
        pub x: f64,
        /// `InputState.y`
        pub y: f64,
        /// `InputState.attack`
        pub attack: bool,
        /// `InputState.special`
        pub special: bool,
        /// `InputState.jump`
        pub jump: bool,
        /// `InputState.dodge`
        pub dodge: bool,
        /// `InputState.jumpLocal`
        pub jump_local: Option<bool>,
        /// `InputState.item`
        pub item: Option<bool>,
        /// `InputState.order`: the press names in order (`attack`, `special`, `jump`, `dodge`, `item`)
        pub order: Option<Vec<String>>,
        /// `InputState.downBtn`
        pub down_btn: Option<bool>,
        /// `InputState.atkDir` (the `Dir` byte)
        pub atk_dir: Option<f64>,
        /// `InputState.forceDir`
        pub force_dir: Option<f64>,
        /// `InputState.forcePress` (`attack` / `special`)
        pub force_press: Option<String>,
        /// `InputState.forceRelease`
        pub force_release: Option<String>,
        /// `InputState.force`
        pub force: Option<bool>,
        /// `InputState.neutralEvade`
        pub neutral_evade: Option<bool>,
        /// `InputState.taunt`
        pub taunt: Option<f64>,
        /// `InputState.tauntHeld`
        pub taunt_held: Option<bool>,
        /// `InputState.rsx`
        pub rsx: Option<f64>,
        /// `InputState.axis`
        pub axis: Option<bool>,
        /// `InputState.callX`
        pub call_x: Option<bool>,
        /// `InputState.callY`
        pub call_y: Option<bool>,
    }
}

record! {
    /// An AnimNotify of the loaded data by its identity (`cls`, `inst`, `t`, `track`, `seq`, `slot`), not its
    /// parameters.
    pub struct NotifyRecord {
        /// `notify.cls`
        pub class: String,
        /// `notify.inst`
        pub inst: Option<String>,
        /// `notify.t` (seconds into the montage)
        pub t: f64,
        /// `notify.track`
        pub track: f64,
        /// `notify.seq`
        pub seq: Option<String>,
        /// `notify.slot`
        pub slot: Option<String>,
    }
}

record! {
    /// A notify with its index in the montage's list (`Fighter.pendingEnds` `{idx, n}`, a branch window `{i, n}`).
    pub struct NotifyAtRecord {
        /// `.idx` / `.i`
        pub index: f64,
        /// `.n`
        pub notify: NotifyRecord,
    }
}

record! {
    /// A montage instance (`MontagePlayer`).
    pub struct MontageRecord {
        /// `MontagePlayer.name` (the montage asset's name)
        pub name: String,
        /// the name of `m.sections[sectionIndexAt(pos)]`, the section the position is in
        pub section: String,
        /// `MontagePlayer.pos` (seconds)
        pub position: f64,
        /// `MontagePlayer.weight` (the blend weight 0..1)
        pub weight: f64,
        /// `MontagePlayer.age` (weight updates since the start)
        pub age: f64,
        /// `MontagePlayer.playRate`
        pub play_rate: f64,
        /// `MontagePlayer.ended`
        pub ended: bool,
        /// `MontagePlayer.terminated`
        pub terminated: bool,
        /// `MontagePlayer.blendingOut`
        pub blending_out: bool,
        /// `MontagePlayer.active`: the notify states open, by notify index, in the Set's order
        pub active: Vec<f64>,
        /// `MontagePlayer.started` (a bool, or the section a jump started it at)
        pub started: Value,
        /// `MontagePlayer.blendOutFired`
        pub blend_out_fired: bool,
        /// `MontagePlayer.interruptFired`
        pub interrupt_fired: bool,
        /// `MontagePlayer.atEnd`
        pub at_end: bool,
        /// `MontagePlayer.blendLeft`
        pub blend_left: f64,
        /// `MontagePlayer.blendTime`
        pub blend_time: f64,
        /// `MontagePlayer.inAdvance`
        pub in_advance: bool,
        /// `MontagePlayer.walking`
        pub walking: bool,
        /// `MontagePlayer.proxy`
        pub proxy: String,
    }
}

record! {
    /// A buff entry (`FighterBuff`).
    pub struct BuffRecord {
        /// `FighterBuff.cls` (the Blueprint class, `_C` kept)
        pub class: String,
        /// `FighterBuff.frames` (frames left)
        pub frames: f64,
        /// `FighterBuff.stacks`
        pub stacks: f64,
        /// `FighterBuff.stackFrames`
        pub stack_frames: f64,
        /// `FighterBuff.heal`
        pub heal: Option<f64>,
        /// `FighterBuff.healAmt`
        pub heal_amt: Option<f64>,
        /// `FighterBuff.born`
        pub born: Option<f64>,
        /// `FighterBuff.rescue`
        pub rescue: Option<f64>,
        /// `FighterBuff.by`
        pub by: Option<f64>,
        /// `FighterBuff.pulled`
        pub pulled: Option<bool>,
        /// `FighterBuff.notify`
        pub notify: Option<NotifyRecord>,
        /// `FighterBuff.fire`
        pub fire: Option<f64>,
        /// `FighterBuff.fireDmg`
        pub fire_dmg: Option<f64>,
        /// `FighterBuff.src` (the instigator)
        pub src: Option<RefRecord>,
        /// `FighterBuff.ext`
        pub ext: Value,
        /// `FighterBuff.stun`
        pub stun: Option<bool>,
        /// `FighterBuff.stunBy`
        pub stun_by: Option<RefRecord>,
        /// `FighterBuff.modsOff`
        pub mods_off: Option<bool>,
        /// `FighterBuff.own`
        pub own: Value,
        /// `FighterBuff.gate`
        pub gate: Option<f64>,
        /// `FighterBuff.addAge`
        pub add_age: Option<f64>,
        /// `FighterBuff.allyHit`
        pub ally_hit: Option<bool>,
        /// `FighterBuff.swept`
        pub swept: Option<bool>,
    }
}

record! {
    /// A Comp_Armor pack (`ArmorPack`, `Fighter.armorPacks`).
    pub struct ArmorRecord {
        /// `ArmorPack.type`
        pub kind: String,
        /// `ArmorPack.amount`
        pub amount: f64,
        /// `ArmorPack.maxHits`
        pub max_hits: f64,
        /// `ArmorPack.threshold`
        pub threshold: f64,
        /// `ArmorPack.dmgRes`
        pub dmg_res: f64,
        /// `ArmorPack.hsRes`
        pub hs_res: f64,
        /// `ArmorPack.kbRes`
        pub kb_res: f64,
        /// `ArmorPack.canOverride`
        pub can_override: bool,
    }
}

record! {
    /// An ability slot of Comp_Pawn_AbilityManager (`AbilityState`, `Fighter.abilities`).
    pub struct AbilityRecord {
        /// `AbilityState.cooldown`
        pub cooldown: bool,
        /// `AbilityState.cdFrames`
        pub cd_frames: f64,
        /// `AbilityState.ammo`
        pub ammo: f64,
        /// `AbilityState.ammoCooldown`
        pub ammo_cooldown: bool,
        /// `AbilityState.ammoFrames`
        pub ammo_frames: f64,
        /// `AbilityState.defaultCd` (DefaultAbilityCooldown)
        pub default_cd: f64,
        /// `AbilityState.activeCd` (ActiveAbilityCooldown)
        pub active_cd: f64,
        /// `AbilityState.ammoMax` (GetMaxAmmoCooldown)
        pub ammo_max: f64,
    }
}

record! {
    /// The launch a knockback holds (`Fighter.launch`).
    pub struct LaunchRecord {
        /// `launch.vx`
        pub vx: f64,
        /// `launch.vy`
        pub vy: f64,
        /// `launch.di`
        pub di: bool,
        /// `launch.wall`
        pub wall: bool,
        /// `launch.kbi`
        pub kbi: f64,
    }
}

record! {
    /// A press a bot forces (`ForcedPress`).
    pub struct ForcedRecord {
        /// `.dir` (the absolute InputDirection byte)
        pub dir: f64,
    }
}

record! {
    /// The press the input buffer holds (`QueuedPress` / `QueuedAction`).
    pub struct QueuedRecord {
        /// `.press`
        pub press: String,
        /// `.dir`
        pub dir: f64,
        /// `.force`
        pub force: Option<ForcedRecord>,
        /// `.taunt` (the taunt's branch)
        pub taunt: Value,
        /// `.raw`
        pub raw: f64,
        /// `.windows` (a QueuedAction's)
        pub windows: Option<Vec<f64>>,
        /// `.older`
        pub older: Value,
    }
}

record! {
    /// A branch the montage takes late (`Fighter.lateBranch`).
    pub struct LateBranchRecord {
        /// `.cur`
        pub current: String,
        /// `.q` (null: none)
        pub queued: Option<QueuedRecord>,
        /// `.ws`
        pub windows: Vec<NotifyAtRecord>,
    }
}

record! {
    /// A hit window open on the fighter (`Fighter.activeHits`, a Map by notify index, in its order).
    pub struct ActiveHitRecord {
        /// the Map's key
        pub key: f64,
        /// `.attackId`
        pub attack_id: f64,
        /// `.attackName`
        pub attack_name: String,
        /// `.chargedTime`
        pub charged_time: f64,
        /// `.idx`
        pub index: f64,
        /// `.n`
        pub notify: NotifyRecord,
        /// `.hitSet` (the ids hit, in the Set's order)
        pub hit_set: Vec<f64>,
        /// `.copyPose`
        pub copy_pose: Value,
        /// `.openPose`
        pub open_pose: Value,
    }
}

record! {
    /// A Map entry of a name and a number (`Fighter.comboResMap`).
    pub struct NamedRecord {
        pub key: String,
        pub value: f64,
    }
}

record! {
    /// An attack of the combo decay's list (`Fighter.recentAttacks`).
    pub struct RecentAttackRecord {
        /// `.btn`
        pub button: String,
        /// `.montage`
        pub montage: Option<String>,
    }
}

record! {
    /// An impulse (`Fighter.net`, NetImpulse; `Fighter.impulse`, LaunchSelf).
    pub struct ImpulseRecord {
        /// `.type`
        pub kind: f64,
        /// `.vx`
        pub vx: f64,
        /// `.vy`
        pub vy: f64,
        /// `.xy`
        pub xy: bool,
        /// `.z`
        pub z: bool,
        /// `.d`: the impulse's UE X, only when not 0
        pub depth: Option<f64>,
    }
}

record! {
    /// What a Command_Movement does at its end (`MoveEnd`, `Fighter.moveEnd`).
    pub struct MoveEndRecord {
        /// `.stop`
        pub stop: bool,
        /// `.clamp`
        pub clamp: bool,
        /// `.min` (a V2)
        pub min: Value,
        /// `.max`
        pub max: Value,
    }
}

record! {
    /// A Command_Movement (`MoveCmd`, `Fighter.moveCmds`).
    pub struct MoveCmdRecord {
        /// `.idx`
        pub index: f64,
        /// `.total`
        pub total: f64,
        /// `.cur`
        pub current: f64,
        /// `.vx`
        pub vx: f64,
        /// `.vy`
        pub vy: f64,
        /// `.bx`
        pub bx: f64,
        /// `.by`
        pub by: f64,
        /// `.wallSkate`
        pub wall_skate: bool,
        /// `.corner`
        pub corner: Option<bool>,
        /// `.upGate`
        pub up_gate: Option<bool>,
        /// `.sideGate`
        pub side_gate: Option<bool>,
        /// `.oldFoot`
        pub old_foot: Option<bool>,
        /// `.ctl` (MoveCtl)
        pub control: Value,
        /// `.stop`
        pub stop: bool,
        /// `.clamp`
        pub clamp: bool,
        /// `.min`
        pub min: Value,
        /// `.max`
        pub max: Value,
    }
}

record! {
    /// The kick blend of Shaggy_AnimBP (`ShaggyState.kickMix`).
    pub struct KickMixRecord {
        /// `.anim`
        pub anim: String,
        /// `.time`
        pub time: f64,
        /// `.w`
        pub w: f64,
    }
}

record! {
    /// Pawn_Shaggy's and Shaggy_AnimBP's own state (`ShaggyState`).
    pub struct ShaggyRecord {
        /// `ShaggyState.rock` (RockMesh in IsEquippedComponents)
        pub rock: bool,
        /// `ShaggyState.holdPose` (SandwichEquipped)
        pub hold_pose: bool,
        /// `ShaggyState.rageShirtTime`
        pub rage_shirt_time: f64,
        /// `ShaggyState.rageShirt`
        pub rage_shirt: f64,
        /// `ShaggyState.kickW`
        pub kick_w: f64,
        /// `ShaggyState.kickMix`
        pub kick_mix: Option<KickMixRecord>,
        /// `ShaggyState.lastSnackSpawn` (Comp_Pawn_Rage LastSnackSpawnTime)
        pub last_snack_spawn: Option<f64>,
        /// `ShaggyState.f` (its fighter's id)
        pub fighter: f64,
    }
}

record! {
    /// A pose of a character's `slotMix` (`{ anim, time, w }`).
    pub struct SlotPoseRecord {
        /// `.anim`
        pub anim: String,
        /// `.time`
        pub time: f64,
        /// `.w`
        pub w: f64,
    }
}

record! {
    /// Pawn_BatmanV2's and Batman_AnimBP's own state (`BatmanState`). Its two
    /// buff references are written as the buff's place in its fighter's list (-1 null, -2 a buff in no list: ended);
    /// `mixBuf` is no state.
    pub struct BatmanRecord {
        /// `BatmanState.gliding` (isGliding)
        pub gliding: bool,
        /// `BatmanState.animGliding` (Batman_AnimBP.IsGliding)
        pub anim_gliding: bool,
        /// `BatmanState.glideOnceClosed`
        pub glide_once_closed: bool,
        /// `BatmanState.originalGravityScale`
        pub original_gravity_scale: f64,
        /// `BatmanState.glideHurtDelay`
        pub glide_hurt_delay: f64,
        /// `BatmanState.idleFade`
        pub idle_fade: bool,
        /// `BatmanState.speedOn`
        pub speed_on: bool,
        /// `BatmanState.occludedSpeedBuff`: its index in the fighter's buffs, -1 null, -2 ended
        pub occluded_speed_buff: f64,
        /// `BatmanState.occludedEvadeBuff`: its index in the fighter's buffs, -1 null, -2 ended
        pub occluded_evade_buff: f64,
        /// `BatmanState.notOccludedDelay`
        pub not_occluded_delay: f64,
        /// `BatmanState.aimDirection`
        pub aim_direction: f64,
        /// `BatmanState.horizontalDividedByMax`
        pub horizontal_divided_by_max: f64,
        /// `BatmanState.slotMix`
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `BatmanState.glideAlpha`
        pub glide_alpha: f64,
        /// `BatmanState.glideW`
        pub glide_w: f64,
        /// `BatmanState.glideSm`
        pub glide_sm: f64,
        /// `BatmanState.glideSmTime`
        pub glide_sm_time: f64,
        /// `BatmanState.glideSmFirst`
        pub glide_sm_first: bool,
        /// `BatmanState.glideLoopX`
        pub glide_loop_x: f64,
        /// `BatmanState.glideLoopT`
        pub glide_loop_t: f64,
        /// `BatmanState.glideAddX`
        pub glide_add_x: f64,
        /// `BatmanState.glideAddT`
        pub glide_add_t: f64,
        /// `BatmanState.springs`
        pub springs: Vec<f64>,
        /// `BatmanState.springRem`
        pub spring_rem: f64,
        /// `BatmanState.springInit`
        pub spring_init: bool,
        /// `BatmanState.glideJ1`
        pub glide_j1: f64,
        /// `BatmanState.glideJ2`
        pub glide_j2: f64,
        /// `BatmanState.grappleInfo` (GrappleInfoRecieved)
        pub grapple_info: bool,
        /// `BatmanState.grappleDestX`
        pub grapple_dest_x: f64,
        /// `BatmanState.grappleDestY`
        pub grapple_dest_y: f64,
        /// `BatmanState.grappleTime`
        pub grapple_time: f64,
        /// `BatmanState.grappleNotify` (-1 none)
        pub grapple_notify: f64,
        /// `BatmanState.grappleTimerAt` (-1 none)
        pub grapple_timer_at: f64,
        /// `BatmanState.skipImpulseValidation`
        pub skip_impulse_validation: bool,
    }
}

record! {
    /// Pawn_Superman's and Superman_AnimBP's own state (`SupermanState`).
    pub struct SupermanRecord {
        /// `SupermanState.iceCount` (IceCount)
        pub ice_count: f64,
        /// `SupermanState.iceDelay` (-1 none)
        pub ice_delay: f64,
        /// `SupermanState.iceTimer` (0 none, 1 pending, 2 active)
        pub ice_timer: f64,
        /// `SupermanState.iceTimerT`
        pub ice_timer_t: f64,
        /// `SupermanState.iceTimerAt`
        pub ice_timer_at: f64,
        /// `SupermanState.iceShakes`
        pub ice_shakes: f64,
        /// `SupermanState.capHalf` (SupermanCapsuleHight)
        pub cap_half: f64,
        /// `SupermanState.capRadius` (SupermanCapsuleRadius)
        pub cap_radius: f64,
        /// `SupermanState.aimOn` (Superman_AnimBP.AimOn)
        pub aim_on: bool,
        /// `SupermanState.angleToVelocityAlpha`
        pub angle_to_velocity_alpha: f64,
        /// `SupermanState.direction`
        pub direction: f64,
        /// `SupermanState.directionSoft`
        pub direction_soft: f64,
        /// `SupermanState.flyAngle`
        pub fly_angle: f64,
        /// `SupermanState.flyLean`
        pub fly_lean: f64,
        /// `SupermanState.aimDashFromX`
        pub aim_dash_from_x: f64,
        /// `SupermanState.aimDashFromY`
        pub aim_dash_from_y: f64,
    }
}

record! {
    /// Pawn_C016's and c016_AnimBP's own state (`LebronState`).
    pub struct LebronRecord {
        /// `LebronState.hasBall` (HasBall)
        pub has_ball: bool,
        /// `LebronState.animHasBall` (c016_AnimBP.HasBall)
        pub anim_has_ball: bool,
        /// `LebronState.ballDelay` (BPI_SetHasBall's Delay(0) pending)
        pub ball_delay: bool,
        /// `LebronState.beginDelay` (ReceiveBeginPlay's Delay(0) pending)
        pub begin_delay: bool,
        /// `LebronState.ballSets` (SetHasBall calls)
        pub ball_sets: f64,
        /// `LebronState.teleportDunkReady` (TeleportDunkReady)
        pub teleport_dunk_ready: bool,
        /// `LebronState.isTeleportDunking` (IsTeleportDunking)
        pub is_teleport_dunking: bool,
        /// `LebronState.capHalf` (LeBronCapsuleHight)
        pub cap_half: f64,
        /// `LebronState.capRadius` (LeBronCapsuleRadius)
        pub cap_radius: f64,
    }
}

record! {
    /// Pawn_Arya's own state (`AryaState`).
    pub struct AryaRecord {
        /// `AryaState.branchOverrideActive` (BranchOverrideActive)
        pub branch_override_active: bool,
        /// `AryaState.tracked` (TrackedDisguiseNotifies)
        pub tracked: Vec<NotifyRecord>,
        /// `AryaState.activeBranches` (ActiveDisguiseBranchesWithAttacks)
        pub active_branches: f64,
        /// `AryaState.isAssassinating` (IsAssassinating)
        pub is_assassinating: bool,
        /// `AryaState.hitboxesOff` ("BPI Disable Normal Hitboxes"'s last value)
        pub hitboxes_off: bool,
        /// `AryaState.beginDelay` (ReceiveBeginPlay's Delay(1.0), ticks left)
        pub begin_delay: f64,
        /// `AryaState.koBound` (OnPlayerKnockedOut bound)
        pub ko_bound: bool,
        /// `AryaState.koClosed` (OnPlayerKnockedOut's DoOnce closed)
        pub ko_closed: bool,
        /// `AryaState.koDelay` (that DoOnce's Delay(2.0), ticks left)
        pub ko_delay: f64,
    }
}

record! {
    /// Pawn_Reindog's and Creature_AnimBP's own state (`ReindogState`).
    pub struct ReindogRecord {
        /// `ReindogState.originalCapHalf` (Original Capsule Half Height)
        pub original_cap_half: f64,
        /// `ReindogState.originalCapRadius` (Original Capsule Radius)
        pub original_cap_radius: f64,
        /// `ReindogState.isHeld` (Is Held)
        pub is_held: bool,
        /// `ReindogState.tether` (Tether: the TetherProjectile)
        pub tether: Option<RefRecord>,
        /// `ReindogState.headScale` (CreatureHeadScale)
        pub head_scale: f64,
        /// `ReindogState.bounceDelay` (the wall bounce's Delay(0.01) pending)
        pub bounce_delay: bool,
        /// `ReindogState.comSpin` (COM_Spin)
        pub com_spin: bool,
        /// `ReindogState.spinRotation` (Spin_Rotation)
        pub spin_rotation: f64,
        /// `ReindogState.spinCur` (this frame's COM_Spin write: -1 none, 0 false, 1 true)
        pub spin_cur: f64,
        /// `ReindogState.stopCur` (this frame's StopRotation)
        pub stop_cur: bool,
        /// `ReindogState.curAt` (the frame of the `cur` writes)
        pub cur_at: f64,
        /// `ReindogState.spinOld` (earlier frames' COM_Spin write not yet seen)
        pub spin_old: f64,
        /// `ReindogState.stopOld` (earlier frames' StopRotation not yet seen)
        pub stop_old: bool,
        /// `ReindogState.isHitStunned` (Is Hit Stunned)
        pub is_hit_stunned: bool,
        /// `ReindogState.isRespawning` (Is Respawning)
        pub is_respawning: bool,
        /// `ReindogState.respawnDelay` (its Delay(0.1), ticks left)
        pub respawn_delay: f64,
    }
}

record! {
    /// An entry of Comp_C021_LightningManager's TargetedActors: an actor and the latest AttackID that chained it.
    pub struct LightningTargetRecord {
        pub actor: RefRecord,
        pub id: f64,
    }
}

record! {
    /// An entry of Comp_C021_LightningManager's ThrowDirections: an AttackID and its ThrowUp.
    pub struct LightningDirRecord {
        pub id: f64,
        pub up: bool,
    }
}

record! {
    /// Comp_C021_LightningManager (`LightningManager`): its two maps
    /// in their insertion order.
    pub struct LightningRecord {
        /// `.targets` (TargetedActors)
        pub targets: Vec<LightningTargetRecord>,
        /// `.dirs` (ThrowDirections)
        pub dirs: Vec<LightningDirRecord>,
    }
}

record! {
    /// Pawn_C021's and C021_AnimBP's own state (`BlackAdamState`);
    /// `mixBuf` is no state.
    pub struct BlackAdamRecord {
        /// `BlackAdamState.hoverDirection.x` (Hover Direction X)
        pub hover_x: f64,
        /// `BlackAdamState.hoverDirection.y` (Hover Direction Y)
        pub hover_y: f64,
        /// `BlackAdamState.listW` (the four hover BlendListByInt's child-1 weights)
        pub list_w: Vec<f64>,
        /// `BlackAdamState.listIndex` (their last active index, -1 before the first update)
        pub list_index: Vec<f64>,
        /// `BlackAdamState.slotMix`
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `BlackAdamState.lightning` (Comp_C021_LightningManager)
        pub lightning: LightningRecord,
    }
}

record! {
    /// Pawn_WonderWomanV3's, Comp_BracerCharge's and WonderWoman_AnimBPV2's own state (`WonderWomanState`);
    /// `mixBuf` is no state.
    pub struct WonderWomanRecord {
        /// `WonderWomanState.meter` (MeterPercentage)
        pub meter: f64,
        /// `WonderWomanState.bracerCharge` (Comp_BracerCharge.ChargeAmount)
        pub bracer_charge: f64,
        /// `WonderWomanState.bracerGate` (Comp_BracerCharge.ChargeGate)
        pub bracer_gate: bool,
        /// `WonderWomanState.lassoAimU` (LassoAimU)
        pub lasso_aim_u: f64,
        /// `WonderWomanState.lassoAimD` (LassoAimD)
        pub lasso_aim_d: f64,
        /// `WonderWomanState.shieldMoveW` (BlendListByBool_2's pose-0 weight)
        pub shield_move_w: f64,
        /// `WonderWomanState.shieldFwdW` (BlendListByBool_1's pose-0 weight, -1 before its first update)
        pub shield_fwd_w: f64,
        /// `WonderWomanState.slotMix`
        pub slot_mix: Vec<SlotPoseRecord>,
    }
}

record! {
    /// Pawn_Harley's and HarleyQuinn_AnimBP's own state (`HarleyState`).
    pub struct HarleyRecord {
        /// `HarleyState.holdIdle` (SetIdleAnim's linked idle: the hold-item one)
        pub hold_idle: bool,
    }
}

record! {
    /// Pawn_C019's and C019_AnimBP's own state (`MortyState`); `teamGrenades`,
    /// `hasMarker` and `mixBuf` are no state.
    pub struct MortyRecord {
        /// `MortyState.cookGrenadeBehind` (c019CookGrenadeOutProjectileCondition's Behind)
        pub cook_grenade_behind: bool,
        /// `MortyState.slotMoveW` (BlendListByBool(IsMoving)'s weight)
        pub slot_move_w: f64,
        /// `MortyState.slotBackW` (BlendListByBool(IsMovingBackwards)'s weight, -1 before its first update)
        pub slot_back_w: f64,
        /// `MortyState.slotMix`
        pub slot_mix: Vec<SlotPoseRecord>,
    }
}

record! {
    /// Comp_C017_Hover_Rage (`HoverRage`).
    pub struct HoverRageRecord {
        /// `.fuel` (FuelAmount)
        pub fuel: f64,
        /// `.usingFuel`
        pub using_fuel: bool,
        /// `.fuelUseType` (0 hover, 1 an attack)
        pub fuel_use_type: f64,
        /// `.attackFuelPercent`
        pub attack_fuel_percent: f64,
        /// `.attackSection`
        pub attack_section: String,
        /// `.attackMontage`
        pub attack_montage: String,
        /// `.emptyJumped` (the empty tank's DoOnce)
        pub empty_jumped: bool,
        /// `.rage` (RageAmount)
        pub rage: f64,
        /// `.usingWeaponMode`
        pub using_weapon_mode: bool,
        /// `.respawnRage` (RespawnRage)
        pub respawn_rage: f64,
        /// `.fuelUi` (the count of SetFuelLevel calls)
        pub fuel_ui: f64,
        /// `.fuelUiValue` (the last SetFuelLevel's value)
        pub fuel_ui_value: f64,
        /// `.fuelRtpc` (the FuelAmount last sent as the hover fuel RTPC, -1 none)
        pub fuel_rtpc: f64,
        /// `.frames` (the frames counted towards the component's next tick)
        pub frames: f64,
    }
}

record! {
    /// Comp_C017_Ally_Turret's variables (`AllyTurret`): a fighter by
    /// its id (-1 none), PilotBuff by its place in `buffOn`'s buff list (-1 none, -2 ended).
    pub struct AllyTurretRecord {
        /// `.inWeaponMode` (IronGiantInWeaponMode)
        pub in_weapon_mode: bool,
        /// `.enter` (AllowPilotEnter's delegates bound)
        pub enter: bool,
        /// `.seed` (bound since the last tick)
        pub seed: bool,
        /// `.over` (the teammates overlapping at the last tick)
        pub over: Vec<f64>,
        /// `.jumpOnAlly`
        pub jump_on_ally: f64,
        /// `.allyPrev` (jumpOnAlly's buttons at the last tick)
        pub ally_prev: f64,
        /// `.hasPilot` (HasPilot)
        pub has_pilot: bool,
        /// `.pilot` (ActivePilot)
        pub pilot: f64,
        /// `.buff` (PilotBuff)
        pub buff: f64,
        /// `.buffOn` (PilotBuff's owner)
        pub buff_on: f64,
        /// `.pilotPrev` (the pilot's buttons at the last tick)
        pub pilot_prev: f64,
        /// `.reticle` (C017_Pilot_Reticle lives)
        pub reticle: bool,
        /// `.aimX` (AimLocation)
        pub aim_x: f64,
        /// `.aimY`
        pub aim_y: f64,
        /// `.aimVX` (AimVector)
        pub aim_vx: f64,
        /// `.aimVY`
        pub aim_vy: f64,
        /// `.left` (the corners, sim axes)
        pub left: f64,
        /// `.right`
        pub right: f64,
        /// `.bottom`
        pub bottom: f64,
        /// `.top`
        pub top: f64,
        /// `.held` (IsNormalAttackHeld)
        pub held: bool,
        /// `.fire` (AutoFireTimer: 0 not set, 1 pending, 2 active)
        pub fire: f64,
        /// `.fireRate`
        pub fire_rate: f64,
        /// `.fireT`
        pub fire_t: f64,
        /// `.fireAt`
        pub fire_at: f64,
        /// `.shot` (AttackMontageNumber)
        pub shot: f64,
        /// `.bomb` (PilotLargeAttackVisuals' Delay, ticks left)
        pub bomb: f64,
        /// `.kick` (KickPilot's Delay, ticks left)
        pub kick: f64,
        /// `.moveOpen` (the move sounds' DoOnce)
        pub move_open: bool,
        /// `.stopOpen` (the loop's stop DoOnce)
        pub stop_open: bool,
    }
}

record! {
    /// C017_AnimBP's own variables (`GiantAnim`).
    pub struct GiantAnimRecord {
        /// `.loopT` (SequencePlayer_8's time)
        pub loop_t: f64,
        /// `.aimVY` (TenticleAimVector's Y)
        pub aim_vy: f64,
        /// `.aimVZ` (TenticleAimVector's Z)
        pub aim_vz: f64,
        /// `.aimDist` (TenticleAimDistanceAlpha)
        pub aim_dist: f64,
        /// `.aimAlpha2` (AimAlpha2)
        pub aim_alpha2: f64,
        /// `.splitActive` (MovementSplitActive)
        pub split_active: bool,
        /// `.jumpCheck` (SplitMovementJumpCheck)
        pub jump_check: bool,
        /// `.split` (the handlers' value at the last update)
        pub split: bool,
        /// `.falling` (IsFalling)
        pub falling: bool,
        /// `.moving` (Moving)
        pub moving: bool,
        /// `.back` (ModifyBone_6's turn at the last update)
        pub back: bool,
        /// `.lean` (AttackLean)
        pub lean: f64,
    }
}

record! {
    /// Pawn_C017's and C017_AnimBP's own state (`GiantState`);
    /// `hoverRageOwner` is no state.
    pub struct GiantRecord {
        /// `GiantState.hoverRage` (Comp_C017_Hover_Rage)
        pub hover_rage: Option<HoverRageRecord>,
        /// `GiantState.isMaxRage` (IsMaxRage)
        pub is_max_rage: bool,
        /// `GiantState.alternateMode` (AlternateMode: weapon mode)
        pub alternate_mode: bool,
        /// `GiantState.gliding` (isGliding)
        pub gliding: bool,
        /// `GiantState.canAttackHover` (CanAttackHover)
        pub can_attack_hover: bool,
        /// `GiantState.attackForceHover` (AttackForceHover)
        pub attack_force_hover: bool,
        /// `GiantState.hoverIgnoreFuel` (IgnoreFuel)
        pub hover_ignore_fuel: bool,
        /// `GiantState.hoverFastFallBlock` (HoverFastFallBlock)
        pub hover_fast_fall_block: bool,
        /// `GiantState.allyRageDelay` (the component's Delay(2.0), ticks left)
        pub ally_rage_delay: f64,
        /// `GiantState.outOfRageBound` (OutOfRageInturupted bound)
        pub out_of_rage_bound: bool,
        /// `GiantState.giantBlocking` (IsBlocking)
        pub giant_blocking: bool,
        /// `GiantState.turretAimNumber` (TurretAimNumber)
        pub turret_aim_number: f64,
        /// `GiantState.turretAimOn` ('Turrent Aim On')
        pub turret_aim_on: bool,
        /// `GiantState.turretAimStartLeft` (TurretAimStartFacingLeft)
        pub turret_aim_start_left: bool,
        /// `GiantState.allyTurret` (Comp_C017_Ally_Turret, from its first ToggleAllyTurret)
        pub ally_turret: Option<AllyTurretRecord>,
        /// `GiantState.giantAnim` (C017_AnimBP's variables)
        pub giant_anim: Option<GiantAnimRecord>,
        /// `GiantState.holdValid` (HoldingBuffValid)
        pub hold_valid: bool,
        /// `GiantState.holdAlpha` (HoldAlpha)
        pub hold_alpha: f64,
        /// `GiantState.giantCrouched` (Comp_Pawn_Giant GiantCrouched, from its first set)
        pub giant_crouched: Option<bool>,
        /// `GiantState.crouchListeners` (the actors bound to GiantCrouchChanged, from the first bind)
        pub crouch_listeners: Option<Vec<RefRecord>>,
    }
}

record! {
    /// Pawn_StevenUniverse's core variables (`StevenPawnState`).
    pub struct StevenPawnRecord {
        /// `.gliding` (isGliding)
        pub gliding: bool,
        /// `.animGliding` (Steven_AnimBP.Gliding)
        pub anim_gliding: bool,
        /// `.glideOnceClosed` (ReadGlideInput's DoOnce)
        pub glide_once_closed: bool,
        /// `.originalGravityScale` (OriginalGravityScale)
        pub original_gravity_scale: f64,
        /// `.upShieldAmmo` (UpShieldAmmo)
        pub up_shield_ammo: f64,
        /// `.upShieldAmmoMax` (UpShieldAmmoMax)
        pub up_shield_ammo_max: f64,
        /// `.watermelonSteven` (WatermelonSteven: the minion's fighter id, -1 none)
        pub watermelon_steven: f64,
        /// `.classChecked` (Comp_Spawner.SpawnedActors_ClassChecked as BPI_HasWatermelonSpawned reads it: the actors)
        pub class_checked: Vec<RefRecord>,
    }
}

record! {
    /// Comp_StevenShield's variables (`StevenShieldState`).
    pub struct StevenShieldRecord {
        /// `.charges` (CurrentShieldCharges)
        pub charges: f64,
        /// `.shielding` (Shielding)
        pub shielding: bool,
        /// `.ally` (Ally: a fighter id, -1 none)
        pub ally: f64,
        /// `.allyBound` (OnAllyRespawn bound: a bit per fighter id)
        pub ally_bound: f64,
    }
}

record! {
    /// A pending SpawnWatermelonSteven's location (`StevenWatermelonState.pendingSpawn`).
    pub struct PendingSpawnRecord {
        /// `.x`
        pub x: f64,
        /// `.y`
        pub y: f64,
    }
}

record! {
    /// The heal and the Watermelon's variables on the pawn (`StevenWatermelonState`).
    pub struct StevenWatermelonRecord {
        /// `.isWatermelonSteven` (isWatermelonSteven)
        pub is_watermelon_steven: bool,
        /// `.pendingSpawn` (SpawnWatermelonSteven's LoadAsset pending)
        pub pending_spawn: Option<PendingSpawnRecord>,
        /// `.cleanupPending` (BPI_Cleanup's EarlyDestroyWatermelonSteven pending)
        pub cleanup_pending: bool,
    }
}

record! {
    /// Steven_AnimBP's glide list and SlowFallState under the shapes (`StevenGlideState`).
    pub struct StevenGlideRecord {
        /// `.w`
        pub w: f64,
        /// `.fromW`
        pub from_w: f64,
        /// `.toW`
        pub to_w: f64,
        /// `.blend`
        pub blend: f64,
        /// `.elapsed`
        pub elapsed: f64,
        /// `.last`
        pub last: f64,
        /// `.sm`
        pub sm: f64,
        /// `.smTime`
        pub sm_time: f64,
        /// `.smFirst`
        pub sm_first: bool,
    }
}

record! {
    /// Steven's own state (`StevenState`): a record per file; `tether`,
    /// `barrier` and `bubble` are empty and left out.
    pub struct StevenRecord {
        /// `StevenState.pawn`
        pub pawn: StevenPawnRecord,
        /// `StevenState.shield`
        pub shield: StevenShieldRecord,
        /// `StevenState.watermelon`
        pub watermelon: StevenWatermelonRecord,
        /// `StevenState.glide`
        pub glide: StevenGlideRecord,
    }
}

record! {
    /// Pawn_Garnet's core variables (`GarnetPawnState`); `mixBuf` is no state.
    pub struct GarnetPawnRecord {
        /// `.markerLocationBeforeDestroyX` (MarkerLocationBeforeDestroy)
        pub marker_location_before_destroy_x: f64,
        /// `.markerLocationBeforeDestroyY`
        pub marker_location_before_destroy_y: f64,
        /// `.slotMix` (Garnet_AnimBP's slot graph mix)
        pub slot_mix: Vec<SlotPoseRecord>,
    }
}

record! {
    /// Garnet's aim (`GarnetAimState`).
    pub struct GarnetAimRecord {
        /// `.aimDirection` (AimDirection)
        pub aim_direction: f64,
        /// `.aimDirectionBeforeReset` (AimDirectionBeforeReset)
        pub aim_direction_before_reset: f64,
        /// `.resetOn` (AimResetTimeline playing)
        pub reset_on: bool,
        /// `.resetPos` (AimResetTimeline's position)
        pub reset_pos: f64,
        /// `.aimFlag` (AimFlag)
        pub aim_flag: bool,
        /// `.setAimValueFromMovement` (SetAimValueFromMovement)
        pub set_aim_value_from_movement: bool,
        /// `.readAimAsHorizontalMovement` (ReadAimAsHorizontalMovement)
        pub read_aim_as_horizontal_movement: bool,
    }
}

record! {
    /// Garnet's ground down probes (`GarnetGroundState`).
    pub struct GarnetGroundRecord {
        /// `.leftPositions` (LeftPositions: x0 y0 x1 y1 ...)
        pub left_positions: Vec<f64>,
        /// `.rightPositions` (RightPositions)
        pub right_positions: Vec<f64>,
        /// `.leftX` (LeftPosition)
        pub left_x: f64,
        /// `.leftY`
        pub left_y: f64,
        /// `.rightX` (RightPosition)
        pub right_x: f64,
        /// `.rightY`
        pub right_y: f64,
        /// `.checkingLeft` (IsCheckingLeft)
        pub checking_left: bool,
        /// `.checkingRight` (IsCheckingRight)
        pub checking_right: bool,
    }
}

record! {
    /// Garnet's electric charge (`GarnetZoneState`).
    pub struct GarnetZoneRecord {
        /// `.chargingElectricity` (ChargingElectricity)
        pub charging_electricity: bool,
        /// `.electricityFullyCharged` (ElectricityFullyCharged)
        pub electricity_fully_charged: bool,
    }
}

record! {
    /// Garnet's neutral special (`GarnetSingState`).
    pub struct GarnetSingRecord {
        /// `.overlappingSingActors` (OverlappingSingActors)
        pub overlapping_sing_actors: Vec<RefRecord>,
        /// `.inside` (the spawned actors the Sing Sphere overlaps)
        pub inside: Vec<RefRecord>,
        /// `.singingVfx` (SingingVFX valid)
        pub singing_vfx: bool,
        /// `.songEntering` (ApplySongBuffToArea's Entering)
        pub song_entering: bool,
        /// `.songRetries` (ApplySongBuffToArea's retries)
        pub song_retries: f64,
        /// `.songRetryPending` (its Delay(0) pending)
        pub song_retry_pending: bool,
    }
}

record! {
    /// Garnet's own state (`GarnetState`): a record per file and the shared
    /// fields; `gauntlets`, `marker`, `dash`, `hold` and `stopproj` are empty and left out.
    pub struct GarnetRecord {
        /// `GarnetState.pawn`
        pub pawn: GarnetPawnRecord,
        /// `GarnetState.aim`
        pub aim: GarnetAimRecord,
        /// `GarnetState.ground`
        pub ground: GarnetGroundRecord,
        /// `GarnetState.zone`
        pub zone: GarnetZoneRecord,
        /// `GarnetState.sing`
        pub sing: GarnetSingRecord,
        /// `GarnetState.idleGauntlet` (IdleGauntlet: the marker out)
        pub idle_gauntlet: Option<RefRecord>,
        /// `GarnetState.projectileGauntlets` (ProjectileGauntlets: the gloves in flight)
        pub projectile_gauntlets: Option<RefRecord>,
        /// `GarnetState.locationBeforeMoveX` (LocationBeforeMove)
        pub location_before_move_x: f64,
        /// `GarnetState.locationBeforeMoveY`
        pub location_before_move_y: f64,
        /// `GarnetState.chargingToMarker` (ChargingToMarker)
        pub charging_to_marker: bool,
        /// `GarnetState.isActiveSinging` (IsActiveSinging)
        pub is_active_singing: bool,
    }
}

record! {
    /// An entry of `TazState.eatenFighters` (`{ f, v, slot }`: the fighter by id).
    pub struct EatenFighterRecord {
        /// `.f`
        pub f: f64,
        /// `.v`
        pub v: f64,
        /// `.slot`
        pub slot: f64,
    }
}

record! {
    /// `TazState.spitOffset` (`{ vic, dx, dy }`: the target by id).
    pub struct SpitOffsetRecord {
        /// `.vic`
        pub vic: f64,
        /// `.dx`
        pub dx: f64,
        /// `.dy`
        pub dy: f64,
    }
}

record! {
    /// Pawn_C015's and C015_AnimBP's own state (`TazState`); `mixBuf` is no state.
    pub struct TazRecord {
        /// `.eatLevel` (EatLevel)
        pub eat_level: f64,
        /// `.eatenFighters` (EatenFighters)
        pub eaten_fighters: Vec<EatenFighterRecord>,
        /// `.eatFree` (the map's freed indices)
        pub eat_free: Vec<f64>,
        /// `.eatSlots` (the map's element array length)
        pub eat_slots: f64,
        /// `.spitHit` (this frame's BPI_SpitFighters Hit: montage data)
        pub spit_hit: Option<Value>,
        /// `.spitOffset` (OffsetSpitFighter's pending Delay(0.0))
        pub spit_offset: Option<SpitOffsetRecord>,
        /// `.projectilesBeingEaten` (ProjectilesBeingEaten)
        pub projectiles_being_eaten: f64,
        /// `.storedProjectile` (StoredProjectile's class, '' none)
        pub stored_projectile: String,
        /// `.storedStartingSpeed` (StoredStartingSpeed)
        pub stored_starting_speed: f64,
        /// `.storedMaxSpeed` (StoredMaxSpeed)
        pub stored_max_speed: f64,
        /// `.burpingProjectile` (BurpingProjectile)
        pub burping_projectile: bool,
        /// `.hittingProjectiles` (HittingProjectiles)
        pub hitting_projectiles: bool,
        /// `.tornadoActive` (TornadoActive)
        pub tornado_active: bool,
        /// `.tornadoMovementOn` (TazTornadoMovementOn)
        pub tornado_movement_on: bool,
        /// `.isNonCooldownTornado` (IsNonCooldownTornado)
        pub is_non_cooldown_tornado: bool,
        /// `.tornadoingAlly` (TornadoingAlly)
        pub tornadoing_ally: bool,
        /// `.hitAlly` (HitAlly)
        pub hit_ally: bool,
        /// `.hitAllies` (HitAllies: fighter ids)
        pub hit_allies: Vec<f64>,
        /// `.listeningForAllyHits` (ListeningForAllyHits)
        pub listening_for_ally_hits: bool,
        /// `.tornadoBuff` (TazTornadoBuff: its place in his buff list, -1 none, -2 ended)
        pub tornado_buff: f64,
        /// `.timerValid` (TornadoTimer's handle valid)
        pub timer_valid: bool,
        /// `.timerAt` (the tick it fires at, -1 none)
        pub timer_at: f64,
        /// `.timerSet` (the tick it was set in)
        pub timer_set: f64,
        /// `.timerRate` (its Rate)
        pub timer_rate: f64,
        /// `.allyHitClosed` (OnAllyHit's DoOnce)
        pub ally_hit_closed: bool,
        /// `.dogPileActive` (DogPileActive)
        pub dog_pile_active: bool,
        /// `.dogPileAt` (DogPileTimer's tick, -1 none)
        pub dog_pile_at: f64,
        /// `.dogPileEndAt` (the held AnimNotify_C015Dogpile NotifyEnd's tick, -1 none)
        pub dog_pile_end_at: f64,
        /// `.fightCloudHurt` (HurtBoxFightCloud's collision)
        pub fight_cloud_hurt: bool,
        /// `.spawningMeleeBuffActor` (SpawningMeleeBuffActor)
        pub spawning_melee_buff_actor: bool,
        /// `.slotMix` (C015_AnimBP's slot graph mix)
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `.runValue` (RunValue)
        pub run_value: f64,
        /// `.turningBlend` (TurningBlend)
        pub turning_blend: f64,
        /// `.wasFacingLeft`
        pub was_facing_left: bool,
        /// `.landingBlend` (LandingBlend)
        pub landing_blend: f64,
        /// `.movingInAirBlend` (MovingInAirBlend)
        pub moving_in_air_blend: f64,
        /// `.wasMovingOnGround`
        pub was_moving_on_ground: bool,
        /// `.percentCharged` (Charge Comp's value 4)
        pub percent_charged: f64,
        /// `.aimDirection` (from the Spawner's ProjectileAimVector.Z)
        pub aim_direction: f64,
    }
}

record! {
    /// The timed BlendListByBool nodes' state of a character's anim instance (`SlotBlend`), per graph node.
    pub struct SlotBlendRecord {
        /// `.last` (the active child, -1 INDEX_NONE)
        pub last: Vec<f64>,
        /// `.w` (child 0's weight)
        pub w: Vec<f64>,
        /// `.from` (child 0's weight at the switch)
        pub from: Vec<f64>,
        /// `.rem` (the blend's remaining time)
        pub rem: Vec<f64>,
        /// `.time` (its whole time)
        pub time: Vec<f64>,
    }
}

record! {
    /// Its SequencePlayers' and state machines' state (`SlotPlayers`): `first` as 1 / 0, `act` as each machine's
    /// flat crossfade numbers (six each: from, to, duration, elapsed, alpha, blend option 1 HermiteCubic).
    pub struct SlotPlayersRecord {
        /// `.frame` (the instance's update count)
        pub frame: f64,
        /// `.time` (a player's position)
        pub time: Vec<f64>,
        /// `.w` (the weight of its last update)
        pub w: Vec<f64>,
        /// `.seen` (the frame a machine was last updated, -1 never)
        pub seen: Vec<f64>,
        /// `.last` (an untimed list's active child)
        pub last: Vec<f64>,
        /// `.cur` (a machine's current state)
        pub cur: Vec<f64>,
        /// `.first` (before its first update)
        pub first: Vec<f64>,
        /// `.act` (its active crossfades)
        pub act: Vec<Vec<f64>>,
    }
}

record! {
    /// Pawn_BugsBunny's own state (`BugsState`).
    pub struct BugsRecord {
        /// `.gliding` (isGliding)
        pub gliding: bool,
        /// `.animGliding` (BugsBunny_AnimBP.IsGliding)
        pub anim_gliding: bool,
        /// `.glideW` (the glide BlendListByBool's child 0 weight)
        pub glide_w: f64,
        /// `.glideLast` (its active child, -1 before the first update)
        pub glide_last: f64,
        /// `.glideFromW`
        pub glide_from_w: f64,
        /// `.glideToW`
        pub glide_to_w: f64,
        /// `.glideBlend`
        pub glide_blend: f64,
        /// `.glideElapsed`
        pub glide_elapsed: f64,
        /// `.glideTime` (the PropellerEars player's time)
        pub glide_time: f64,
        /// `.glideSeqFrom`
        pub glide_seq_from: f64,
        /// `.glideSeqDelta`
        pub glide_seq_delta: f64,
        /// `.glideOnceClosed` (ReadGlideInput's DoOnce)
        pub glide_once_closed: bool,
        /// `.originalGravityScale` (OriginalGravityScale)
        pub original_gravity_scale: f64,
        /// `.skipImpulseValidation` (SkipImpulseValidation)
        pub skip_impulse_validation: bool,
        /// `.lingeringKiss` (As Lingering Love Projectile)
        pub lingering_kiss: Option<RefRecord>,
        /// `.itemEquip` (ItemEquip)
        pub item_equip: String,
    }
}

record! {
    /// Comp_Pawn_Portal's variables (`PortalComp`).
    pub struct PortalCompRecord {
        /// `.allied` (AlliedPortals)
        pub allied: Vec<RefRecord>,
        /// `.exitLeft` (TeleportVisuals' Delay: frames left)
        pub exit_left: f64,
        /// `.vfxX`
        pub vfx_x: f64,
        /// `.vfxY`
        pub vfx_y: f64,
    }
}

record! {
    /// Pawn_C020's and C020_AnimBPV2's own state (`RickState`); `mixBuf` is no state.
    pub struct RickRecord {
        /// `.listening` (ListeningForAbilityHit)
        pub listening: bool,
        /// `.chargeAmount` (ChargeAmount)
        pub charge_amount: f64,
        /// `.aimDirection` (Aim Direction)
        pub aim_direction: f64,
        /// `.slotMix` (C020_AnimBPV2's slot graph mix)
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `.portalComp` (Comp_Pawn_Portal)
        pub portal_comp: PortalCompRecord,
    }
}

record! {
    /// Finn's pawn core (`FinnPawnState`); `mixBuf` is no state.
    pub struct FinnPawnRecord {
        /// `.slotMix` (Finn_AnimBP's slot graph mix)
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `.slotBlend` (its timed lists)
        pub slot_blend: SlotBlendRecord,
    }
}

record! {
    /// Finn's coins (`FinnCoinsState`).
    pub struct FinnCoinsRecord {
        /// `.spawnX` (TreasureSpawnLocation)
        pub spawn_x: f64,
        /// `.spawnY`
        pub spawn_y: f64,
        /// `.knockBack` (Treasure Knock Back)
        pub knock_back: f64,
        /// `.dirX` (Treasure Knock Back Direction Vector)
        pub dir_x: f64,
        /// `.dirY`
        pub dir_y: f64,
        /// `.lastStackCount` (FinnTreasureBuff's LastStackCount)
        pub last_stack_count: f64,
    }
}

record! {
    /// Finn's side special (`FinnDashState`).
    pub struct FinnDashRecord {
        /// `.target` (DashTarget)
        pub target: Option<RefRecord>,
        /// `.trailAt`
        pub trail_at: f64,
        /// `.trailPitch`
        pub trail_pitch: f64,
    }
}

record! {
    /// Comp_FinnMomentumCharge's variables (`FinnMomentum`).
    pub struct FinnMomentumRecord {
        /// `.stored` (StoredMomentum)
        pub stored: f64,
        /// `.max` (MaxStoredMomentum)
        pub max: f64,
        /// `.amount` (MomentumAmount)
        pub amount: f64,
        /// `.dmgMult` (DamageMult)
        pub dmg_mult: f64,
        /// `.kbMult` (KnockBackMult)
        pub kb_mult: f64,
        /// `.calculating` (CalculateMomentum)
        pub calculating: bool,
        /// `.highFive` (HighFiveCharge)
        pub high_five: f64,
        /// `.highFivePct` (HighFivePercent)
        pub high_five_pct: f64,
        /// `.highFiveMax` (HighFiveMaxCharge)
        pub high_five_max: f64,
        /// `.meter` (the run meter's fill)
        pub meter: f64,
    }
}

record! {
    /// Pawn_Finn's own state (`FinnState`): a record per file and the shared
    /// fields; `gem`, `store`, `chop` and `perks` are empty and left out.
    pub struct FinnRecord {
        /// `.pawn`
        pub pawn: FinnPawnRecord,
        /// `.coins`
        pub coins: FinnCoinsRecord,
        /// `.dash`
        pub dash: FinnDashRecord,
        /// `.momentum`
        pub momentum: FinnMomentumRecord,
        /// `.lastHitActor` (LastHitActor)
        pub last_hit_actor: Option<RefRecord>,
        /// `.lastHitTime` (LastHitTime)
        pub last_hit_time: f64,
        /// `.dropCoinOverride` (DropCoinOverride)
        pub drop_coin_override: bool,
        /// `.dropCoinAmount` (DropCoinAmount)
        pub drop_coin_amount: f64,
        /// `.aimer` (the live Finn_Dash_AimerExperiment)
        pub aimer: Option<RefRecord>,
        /// `.shop` (the live Finn_Shop)
        pub shop: Option<RefRecord>,
        /// `.bmo` (the live BMO_FallowActor)
        pub bmo: Option<RefRecord>,
        /// `.runUi` (the run meter on)
        pub run_ui: bool,
    }
}

record! {
    /// Stripe's pawn core and C023B_AnimBP's sim fields (`StripePawnState`); `mixBuf` is no state.
    pub struct StripePawnRecord {
        /// `.slotMix` (C023B_AnimBP's slot graph mix)
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `.slotBlend` (its timed lists)
        pub slot_blend: SlotBlendRecord,
        /// `.slotPlayers` (its players and untimed lists)
        pub slot_players: SlotPlayersRecord,
        /// `.evKbOverride` (OverrideKB at the last update)
        pub ev_kb_override: bool,
        /// `.comAngle` (COM_Override_Angle)
        pub com_angle: f64,
        /// `.comAlpha` (COM_Override_Alpha)
        pub com_alpha: f64,
        /// `.comBlendStart` (COM_BlendStartTime)
        pub com_blend_start: f64,
        /// `.comBlendTime` (COM_BlendTime)
        pub com_blend_time: f64,
        /// `.fallOn` (COM_Fall_Override_On)
        pub fall_on: bool,
        /// `.comFall` (COM_FallOverride)
        pub com_fall: f64,
        /// `.chainSawOn` (ChainSawOverride)
        pub chain_saw_on: bool,
        /// `.chainSawRot` (ChainSawRot)
        pub chain_saw_rot: f64,
        /// `.evComAlpha`
        pub ev_com_alpha: f64,
        /// `.evComPitch`
        pub ev_com_pitch: f64,
        /// `.evFallOn`
        pub ev_fall_on: bool,
        /// `.evFall`
        pub ev_fall: f64,
        /// `.evSawOn`
        pub ev_saw_on: bool,
        /// `.evSawRot`
        pub ev_saw_rot: f64,
    }
}

record! {
    /// Comp_C023B_TargetTracker's timer and binds (`StripeGunState`).
    pub struct StripeGunRecord {
        /// `.timer` (TargetTimer: 0 invalid, 1 pending, 2 active, 3 paused, 4 fired)
        pub timer: f64,
        /// `.rate`
        pub rate: f64,
        /// `.elapsed`
        pub elapsed: f64,
        /// `.exitBound` (fighter ids)
        pub exit_bound: Vec<f64>,
        /// `.endBound` (projectile ids)
        pub end_bound: Vec<f64>,
    }
}

record! {
    /// Comp_C023B_Bomb's variables (`StripeBombState`).
    pub struct StripeBombRecord {
        /// `.equipped` (HasBomb)
        pub equipped: bool,
        /// `.active` (BombTimer running)
        pub active: bool,
        /// `.timer` (BombTimer: its tick, 0 none)
        pub timer: f64,
        /// `.timerValid`
        pub timer_valid: bool,
        /// `.timerSet`
        pub timer_set: f64,
        /// `.timerRate`
        pub timer_rate: f64,
        /// `.timeRemainingOnDrop` (TimeRemainingOnDrop)
        pub time_remaining_on_drop: f64,
        /// `.fuseOffset`
        pub fuse_offset: f64,
        /// `.fuseAt`
        pub fuse_at: f64,
        /// `.knockedBound`
        pub knocked_bound: bool,
        /// `.hitSoundBound`
        pub hit_sound_bound: bool,
        /// `.stunEndBound`
        pub stun_end_bound: bool,
        /// `.bounceBound`
        pub bounce_bound: bool,
        /// `.kbOverride` (OverrideKB)
        pub kb_override: bool,
        /// `.bodyDirX` ('Hit Box Damage'.KnockBackDirectionVector)
        pub body_dir_x: f64,
        /// `.bodyDirY`
        pub body_dir_y: f64,
    }
}

record! {
    /// Stripe's perks (`StripePerkState`).
    pub struct StripePerkRecord {
        /// `.launchNextLeap` (Buff_C023B_Leap LaunchNextLeap)
        pub launch_next_leap: bool,
    }
}

record! {
    /// Pawn_C023B's own state (`StripeState`): a record per file and the shared
    /// fields; `saw` is empty and left out.
    pub struct StripeRecord {
        /// `.pawn`
        pub pawn: StripePawnRecord,
        /// `.gun`
        pub gun: StripeGunRecord,
        /// `.bomb`
        pub bomb: StripeBombRecord,
        /// `.perks`
        pub perks: StripePerkRecord,
        /// `.lastDecay` (LastDecay)
        pub last_decay: String,
        /// `.target` (the TargetTracker's current target)
        pub target: Option<RefRecord>,
        /// `.aimBack` (AimBack)
        pub aim_back: f64,
        /// `.aimDirection`
        pub aim_direction: f64,
        /// `.chainsawVisible` (ChainsawVisibilityMI)
        pub chainsaw_visible: bool,
        /// `.dashListener` (the live C023B_DashListener)
        pub dash_listener: Option<RefRecord>,
        /// `.saws` (the live saws' projectile ids)
        pub saws: Vec<f64>,
    }
}

record! {
    /// Pawn_Jake's core and Jake_AnimBP's slot mix (`JakePawnState`).
    pub struct JakePawnRecord {
        /// `.slotMix`
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `.slotBlend` (Horse Montage2's IsFalling lists)
        pub slot_blend: SlotBlendRecord,
        /// `.sectionOverride` (MontageSectionOverride)
        pub section_override: String,
        /// `.chargeTotalTime` (ChargeTotalTime)
        pub charge_total_time: f64,
        /// `.bindDelay` (BeginPlay's Delay 0.5: its time left)
        pub bind_delay: f64,
        /// `.chargeBound`
        pub charge_bound: bool,
        /// `.eat` (the EatOverride machine's state)
        pub eat: f64,
        /// `.eatTimes` (each state's player time)
        pub eat_times: Vec<f64>,
        /// `.walkTime`
        pub walk_time: f64,
        /// `.walkW`
        pub walk_w: f64,
        /// `.walkFrom`
        pub walk_from: f64,
        /// `.walkRem`
        pub walk_rem: f64,
        /// `.walkDur`
        pub walk_dur: f64,
        /// `.walkLast`
        pub walk_last: f64,
        /// `.eatFirst`
        pub eat_first: bool,
        /// `.fadeFrom`
        pub fade_from: f64,
        /// `.fadeDur`
        pub fade_dur: f64,
        /// `.fadeT`
        pub fade_t: f64,
    }
}

record! {
    /// Comp_JakeEat's own variables (`JakeEatState`).
    pub struct JakeEatRecord {
        /// `.timer` (EatTimer: 0 none, 1 pending, 2 active, 3 paused, 4 fired)
        pub timer: f64,
        /// `.rate`
        pub rate: f64,
        /// `.elapsed`
        pub elapsed: f64,
        /// `.timerActive` (TimerActive)
        pub timer_active: bool,
        /// `.struggleCount` (StuggleCount)
        pub struggle_count: f64,
        /// `.struggleBound` (fighter ids)
        pub struggle_bound: Vec<f64>,
        /// `.slowBuff` (SlowBuff: its place in his buffs, -1 none, -2 ended)
        pub slow_buff: f64,
        /// `.canAttack` (CanAttack)
        pub can_attack: bool,
        /// `.canDropAlly` (CanDropAlly)
        pub can_drop_ally: bool,
        /// `.spitFighter` (SpitFighter: fighter ids)
        pub spit_fighter: Vec<f64>,
        /// `.closed` (the DoOnce @6194)
        pub closed: bool,
        /// `.spitBound` (JakeSpitEvent bound)
        pub spit_bound: bool,
    }
}

record! {
    /// The forward-air aim punch (`JakeAimState`).
    pub struct JakeAimRecord {
        /// `.aimPunchY` (AimPunchVector Y)
        pub aim_punch_y: f64,
        /// `.aimPunchZ` (AimPunchVector Z)
        pub aim_punch_z: f64,
    }
}

record! {
    /// Jake_AnimBP's variables the sim reads (`JakeAnim`).
    pub struct JakeAnimRecord {
        /// `.chargeAmount` (ChargeAmount)
        pub charge_amount: f64,
        /// `.stretchOn` (StretchOn)
        pub stretch_on: bool,
        /// `.calculateStretchAngle` (CalculateStretchAngle)
        pub calculate_stretch_angle: bool,
        /// `.stretchDirection` (StretchDirection)
        pub stretch_direction: f64,
        /// `.aimDirection` (AimDirection)
        pub aim_direction: f64,
        /// `.aimDistanceAlpha` (AimDistanceAlpha)
        pub aim_distance_alpha: f64,
        /// `.eatOverride` (EatOverrideBool)
        pub eat_override: bool,
    }
}

record! {
    /// Pawn_Jake's own state (`JakeState`): a record per group of files and the
    /// shared fields; `stretch`, `house`, `horse`, `spit`, `allySpit` and `weapons` are empty and left out.
    pub struct JakeRecord {
        /// `.pawn`
        pub pawn: JakePawnRecord,
        /// `.eat`
        pub eat: JakeEatRecord,
        /// `.aim`
        pub aim: JakeAimRecord,
        /// `.anim` (Jake_AnimBP's inputs)
        pub anim: JakeAnimRecord,
        /// `.reachCheck` (ReachCheck)
        pub reach_check: bool,
        /// `.headScale` (HeadScale, UE order)
        pub head_scale: Vec<f64>,
        /// `.holdingActor` (Comp_JakeEat HoldingActor)
        pub holding_actor: bool,
        /// `.heldFighters` (Comp_JakeEat HeldFighters: fighter ids)
        pub held_fighters: Vec<f64>,
        /// `.weapon` (Comp_WeaponSwap CurrentWeapon)
        pub weapon: f64,
        /// `.weaponEquipped` (WeaponEquiped)
        pub weapon_equipped: bool,
        /// `.weaponsHidden`
        pub weapons_hidden: bool,
        /// `.stretchActor` (the live JakeStretch)
        pub stretch_actor: Option<RefRecord>,
        /// `.houseActor` (the live JakeHouse_Actor)
        pub house_actor: Option<RefRecord>,
        /// `.horseActor` (the live JakeHorse_Actor3)
        pub horse_actor: Option<RefRecord>,
        /// `.aimer` (the live JakeAimer)
        pub aimer: Option<RefRecord>,
        /// `.spitAimer` (the live ProjectileAimer_SpitJake)
        pub spit_aimer: Option<RefRecord>,
        /// `.allyAim` (the live JakeEatAlly_Aim)
        pub ally_aim: Option<RefRecord>,
    }
}

record! {
    /// Tom and Jerry's pawn core (`TomAndJerryPawnState`).
    pub struct TomAndJerryPawnRecord {
        /// `.slotMix` (Tom_and_Jerry_AnimBP's slot graph mix)
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `.slotBlend` (its timed lists)
        pub slot_blend: SlotBlendRecord,
        /// `.jerryOffsetAlpha` (JerryOffsetAlpha)
        pub jerry_offset_alpha: f64,
        /// `.pointerVisible` (JerryPointer's visibility)
        pub pointer_visible: bool,
        /// `.hideDone` (the pointer's DoOnce)
        pub hide_done: bool,
        /// `.gateOpen` (the pointer's Gate)
        pub gate_open: bool,
        /// `.spawnedJerry` (SpawnedJerryAcor)
        pub spawned_jerry: Option<RefRecord>,
    }
}

record! {
    /// Jerry's follow variables (`TomAndJerryFollowState`).
    pub struct TomAndJerryFollowRecord {
        /// `.jerryId` (the Jerry actor's id, -1 none)
        pub jerry_id: f64,
        /// `.followY` (Follow_Y_Offset)
        pub follow_y: f64,
        /// `.followZ` (Follow_Z_Offset)
        pub follow_z: f64,
        /// `.crouchHeight` (CrouchHeight)
        pub crouch_height: f64,
        /// `.isCrouching` (IsCrouching)
        pub is_crouching: bool,
        /// `.relY` (the relative location's Y)
        pub rel_y: f64,
        /// `.relZ` (the relative location's Z)
        pub rel_z: f64,
        /// `.bound` (the ally's delegates bound)
        pub bound: bool,
        /// `.delay` (Delay(0.1): ticks left)
        pub delay: f64,
    }
}

record! {
    /// Jerry's champagne variables (`TomAndJerryChampagneState`).
    pub struct TomAndJerryChampagneRecord {
        /// `.actorId` (the Jerry actor's id, -1 none)
        pub actor_id: f64,
        /// `.gate` (ChampagneGate)
        pub gate: bool,
        /// `.gateLeft` (ChampagneTimer: frames left)
        pub gate_left: f64,
        /// `.startTime` (ChampagneStartTime)
        pub start_time: f64,
        /// `.queuedFacing` (CorkQuedFacing)
        pub queued_facing: bool,
        /// `.widgetVisible` (UI_JerryCorkCount's visibility)
        pub widget_visible: bool,
        /// `.bottleAnim` (the Champagne mesh's animation)
        pub bottle_anim: String,
        /// `.bottleTick` (its start tick)
        pub bottle_tick: f64,
        /// `.popTick` (the last launch's tick)
        pub pop_tick: f64,
    }
}

record! {
    /// The lure's count of the main mesh's montage ends (`TomAndJerryFishingState`).
    pub struct TomAndJerryFishingRecord {
        /// `.montageEnds`
        pub montage_ends: f64,
    }
}

record! {
    /// Tom_and_Jerry_AnimBP's variables the sim reads (`TomAndJerryAnim`).
    pub struct TomAndJerryAnimRecord {
        /// `.aimDirection` (AimDirection)
        pub aim_direction: f64,
        /// `.wallAlpha` (WallAlpha)
        pub wall_alpha: f64,
        /// `.turnAlpha` (TurnAlpha)
        pub turn_alpha: f64,
        /// `.ignoreWallOffset` (IgnoreJerryWallOffset)
        pub ignore_wall_offset: bool,
        /// `.rocketControl` (RocketControlBool)
        pub rocket_control: bool,
        /// `.arms` (BlendListByBool_1's blend)
        pub arms: SlotBlendRecord,
        /// `.overrideMirror` (OverrideMirror?)
        pub override_mirror: bool,
        /// `.mirrorOverride` (MirrorOverride)
        pub mirror_override: bool,
        /// `.jerryWallOffset` (JerryWallOffset)
        pub jerry_wall_offset: f64,
        /// `.withJerry` (WithJerry)
        pub with_jerry: bool,
        /// `.oldFacingLeft` (OldFacingLeft)
        pub old_facing_left: bool,
    }
}

record! {
    /// Pawn_TomAndJerry's own state (`TomAndJerryState`): a record per
    /// file (jerry, tennis, slingshot, rocket, kicked and trap hold nothing) and the fields more than one file reads.
    pub struct TomAndJerryRecord {
        /// `.pawn`
        pub pawn: TomAndJerryPawnRecord,
        /// `.follow`
        pub follow: TomAndJerryFollowRecord,
        /// `.champagne`
        pub champagne: TomAndJerryChampagneRecord,
        /// `.fishing`
        pub fishing: TomAndJerryFishingRecord,
        /// `.anim` (Tom_and_Jerry_AnimBP's inputs)
        pub anim: TomAndJerryAnimRecord,
        /// `.jerryAttached` (JerryAttached)
        pub jerry_attached: bool,
        /// `.visibleCooldown` (VisibleCoolDown)
        pub visible_cooldown: bool,
        /// `.jerryOffsetOverrideTrace` (JerryOffsetOverrideTrace)
        pub jerry_offset_override_trace: bool,
        /// `.jerryActor` (the live Jerry actor)
        pub jerry_actor: Option<RefRecord>,
        /// `.slingAimer` (the live TomAndJerry_SlingShot_Aim_02)
        pub sling_aimer: Option<RefRecord>,
        /// `.noJerryAimer` (the live TomAndJerry_NoJerry_SlingShot_Aim)
        pub no_jerry_aimer: Option<RefRecord>,
        /// `.tennisBalls` (the tennis balls in play)
        pub tennis_balls: Vec<RefRecord>,
        /// `.corkAmmo` (CorkAmmo)
        pub cork_ammo: f64,
    }
}

record! {
    /// Velma's pawn core (`VelmaPawnState`); `mixBuf` is no state.
    pub struct VelmaPawnRecord {
        /// `.slotMix` (Velma_AnimBP's slot graph mix)
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `.slotBlend` (its timed lists)
        pub slot_blend: SlotBlendRecord,
    }
}

record! {
    /// Velma's evidence timer (`VelmaEvidenceState`).
    pub struct VelmaEvidenceRecord {
        /// `.period` (the SpawnEvidence timer's Rate)
        pub period: f64,
        /// `.start` (the tick it was set at)
        pub start: f64,
        /// `.fired` (its expiries)
        pub fired: f64,
        /// `.timerAt` (the tick of its next expiry)
        pub timer_at: f64,
    }
}

record! {
    /// Velma's police call (`VelmaPoliceCallState`).
    pub struct VelmaPoliceCallRecord {
        /// `.delay1` (Delay 1, frames left)
        pub delay1: f64,
        /// `.delay8` (Delay 8, frames left)
        pub delay8: f64,
        /// `.lastSpawn` (the latest police spawned, its fighter id; -1 none)
        pub last_spawn: f64,
    }
}

record! {
    /// Velma's carry run (`VelmaCarryState`).
    pub struct VelmaCarryRecord {
        /// `.holdingAlly` (BaseFighter IsHoldingAlly)
        pub holding_ally: bool,
    }
}

record! {
    /// Pawn_Velma's own state (`VelmaState`): a record per group of files and the
    /// shared fields; `beam`, `marks`, `capture` and `gadgets` are empty and left out.
    pub struct VelmaRecord {
        /// `.pawn`
        pub pawn: VelmaPawnRecord,
        /// `.evidence`
        pub evidence: VelmaEvidenceRecord,
        /// `.call`
        pub call: VelmaPoliceCallRecord,
        /// `.carry`
        pub carry: VelmaCarryRecord,
        /// `.aimDirection` (Velma_AnimBP AimDirection)
        pub aim_direction: f64,
        /// `.carryingAlly` (Velma_AnimBP IsCarryingAlly)
        pub carrying_ally: bool,
        /// `.animHitStun` (Velma_AnimBP HitStun)
        pub anim_hit_stun: bool,
        /// `.allowPoliceCall` (AllowPoliceCall)
        pub allow_police_call: bool,
        /// `.police` (the live police minion, its fighter id; -1 none)
        pub police: f64,
        /// `.evidenceUi` (EvidenceCount, the UI's)
        pub evidence_ui: f64,
        /// `.carryRun` (the live VelmaCarryRun_Actor)
        pub carry_run: Option<RefRecord>,
        /// `.mark` (the live VelmaMarkProjectile)
        pub mark: Option<RefRecord>,
        /// `.glassesOffCount` (GlassesOffCount)
        pub glasses_off_count: f64,
    }
}

record! {
    /// Pawn_VelmaAIBotPolice's own state (`VelmaPoliceState`).
    pub struct VelmaPoliceRecord {
        /// `.arrestEnabled` (ArrestEnabled)
        pub arrest_enabled: bool,
        /// `.performedArrest` (PerformedArrest)
        pub performed_arrest: bool,
        /// `.targetedEnemy` (TargetedEnemy, its fighter id; -1 none)
        pub targeted_enemy: f64,
        /// `.initialX` (InitialLocation)
        pub initial_x: f64,
        /// `.initialY`
        pub initial_y: f64,
        /// `.delay3` (Delay(TimeBeforeArrest), frames left)
        pub delay3: f64,
        /// `.targetLoop` (the Delay(0) loop pending)
        pub target_loop: bool,
        /// `.visualsClosed` (ArrestedVisuals' DoOnce)
        pub visuals_closed: bool,
        /// `.inside` (the fighters FullBody overlaps, their ids)
        pub inside: Vec<f64>,
        /// `.pin` (the live VelmaJenkinsPin)
        pub pin: Option<RefRecord>,
    }
}

record! {
    /// Marvin's pawn core and C018_AnimBP's sim fields (`MarvinPawnState`); `mixBuf` is no state.
    pub struct MarvinPawnRecord {
        /// `.slotMix` (C018_AnimBP's slot graph mix)
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `.wallBlendSpaceValue` (Base_AnimBP_C018's WallBlendSpaceValue)
        pub wall_blend_space_value: f64,
    }
}

record! {
    /// Pawn_C018's disintegration variables (`MarvinDisint`).
    pub struct MarvinDisintRecord {
        /// `.taking` (IsTakingDisintegrationDamage)
        pub taking: bool,
        /// `.hasHit` (DisintegrationHasHit)
        pub has_hit: bool,
        /// `.incTimer` (the increment timer: 0 none, 1 pending, 2 active)
        pub inc_timer: f64,
        /// `.incT` (its InternalTime)
        pub inc_t: f64,
        /// `.incAt` (its ExpireTime)
        pub inc_at: f64,
        /// `.endTimer` (the max-duration timer: 0 none, 1 pending, 2 active)
        pub end_timer: f64,
        /// `.endT` (its InternalTime)
        pub end_t: f64,
        /// `.endAt` (its ExpireTime)
        pub end_at: f64,
        /// `.endRate` (the duration it was set with)
        pub end_rate: f64,
    }
}

record! {
    /// Pawn_C018's own state (`MarvinState`): a record per group of files and
    /// the shared fields; `platform`, `bubble`, `blaster`, `flag`, `swap` and `perks` are empty and left out.
    pub struct MarvinRecord {
        /// `.pawn`
        pub pawn: MarvinPawnRecord,
        /// `.aimDeg` (Anim Aim Rotation Degrees)
        pub aim_deg: f64,
        /// `.aimValue` (C018_AnimBP AimValue)
        pub aim_value: f64,
        /// `.disint`
        pub disint: MarvinDisintRecord,
        /// `.wallClingBlend` (WallClingFlagBlend)
        pub wall_cling_blend: f64,
        /// `.ufo` (the pawn's Cannon: the live C018_Platform or C018_PerkPlatform)
        pub ufo: Option<RefRecord>,
    }
}

// GizmoState: a record per file and the shared fields; `cars`, `music`, `popcorn` and `perks`
// hold nothing and are left out (the oracle's writer checks they stay empty).
record! {
    /// `GizmoPawnState` (`mixBuf` is no sim state).
    pub struct GizmoPawnRecord {
        /// `.slotMix` (C023A_AnimBP's slot graph mix)
        pub slot_mix: Vec<SlotPoseRecord>,
        /// `.slotBlend` (its timed lists)
        pub slot_blend: SlotBlendRecord,
        /// `.slotPlayers` (its players and the Driving machine)
        pub slot_players: SlotPlayersRecord,
        /// `.overrideFallLean` (OverrideFallLean)
        pub override_fall_lean: bool,
        /// `.stopOverrides` (Stop Overrides)
        pub stop_overrides: bool,
        /// `.rolling` (IsRolling)
        pub rolling: bool,
        /// `.turning` (IsTurrning)
        pub turning: bool,
        /// `.drivingTurning` (DrivingTurning: 1 left, 2 right)
        pub driving_turning: f64,
        /// `.turnDelay` (DrivingTurn's RetriggerableDelay, frames left)
        pub turn_delay: f64,
        /// `.comBlendStart` (COM_BlendStartTime)
        pub com_blend_start: f64,
        /// `.comBlendTime` (COM_BlendTime)
        pub com_blend_time: f64,
        /// `.visDelay` (BeginPlay's Delay(0.2), frames left)
        pub vis_delay: f64,
        /// `.visibilityDelay` (VisibilityDelay)
        pub visibility_delay: bool,
        /// `.hasMaxArrow` (HasMaxArrow)
        pub has_max_arrow: bool,
        /// `.fireFacing` (FacingChangeFire's last FacingLeft: 0 none, 1 left, 2 right)
        pub fire_facing: f64,
    }
}

record! {
    /// `GizmoAttach` (Comp_C023A_AllyAttach).
    pub struct GizmoAttachRecord {
        /// `.attached` (IsAttached)
        pub attached: bool,
        /// `.carrying` (CarryingActor's fighter id, -1 none)
        pub carrying: f64,
        /// `.moveBuff` (MoveToAllyBuff: its place in the owner's buffs, -1 none, -2 ended)
        pub move_buff: f64,
        /// `.meter` (TrackedMeter)
        pub meter: f64,
        /// `.offX` (AttachOffset X)
        pub off_x: f64,
        /// `.offZ` (AttachOffset Z)
        pub off_z: f64,
        /// `.relX` (RelativeLocation X under the ally's mesh)
        pub rel_x: f64,
        /// `.relZ` (RelativeLocation Z)
        pub rel_z: f64,
        /// `.turnX` (BPI_AttachTurn's Attach Offset X)
        pub turn_x: f64,
        /// `.turnZ` (its Z)
        pub turn_z: f64,
        /// `.turnOn` (CarryTurnTimeline playing)
        pub turn_on: bool,
        /// `.turnPos` (its position)
        pub turn_pos: f64,
        /// `.shareBuff` (DamageShareBuff: its place in the carried ally's buffs, -1 none, -2 ended)
        pub share_buff: f64,
        /// `.shrinkBuff` (ShrinkBuff: its place in the owner's buffs, -1 none, -2 ended)
        pub shrink_buff: f64,
        /// `.inputBound` (the owner's InputTriggered bound)
        pub input_bound: bool,
        /// `.allyBound` (the ally's delegates bound)
        pub ally_bound: bool,
        /// `.buffQueue` (BuffClassesToCheck)
        pub buff_queue: Vec<String>,
        /// `.queueDue` (its Delay(0.0) pending)
        pub queue_due: bool,
        /// `.timerSet` (AttachTimer's World tick, -1 none)
        pub timer_set: f64,
        /// `.timerNext` (its next expiry)
        pub timer_next: f64,
        /// `.collisionOff` (SetActorEnableCollision(false))
        pub collision_off: bool,
        /// `.notifyHolders` (the fighters ApplyBuffToAttached filed, by id)
        pub notify_holders: Vec<f64>,
        /// `.dashCar` (LastDashTarget when it is the car)
        pub dash_car: Option<RefRecord>,
    }
}

record! {
    /// Pawn_C023A's own state (`GizmoState`).
    pub struct GizmoRecord {
        /// `.pawn`
        pub pawn: GizmoPawnRecord,
        /// `.attach`
        pub attach: GizmoAttachRecord,
        /// `.car` (the live C023A_Car_SpawnedActor)
        pub car: Option<RefRecord>,
        /// `.drives` (the car he drives)
        pub drives: Option<RefRecord>,
        /// `.driving` (IsDriving)
        pub driving: bool,
        /// `.comOverride` (COM_Override_Angle)
        pub com_override: f64,
        /// `.comOverrideAlpha` (COM_Override_Alpha)
        pub com_override_alpha: f64,
        /// `.lantern` (the lantern submesh shown)
        pub lantern: bool,
        /// `.lastDashTarget` (LastDashTarget's fighter id, -1 none)
        pub last_dash_target: f64,
    }
}

/// A fighter's own character state (`Fighter.cs`): one variant per ported character.
// Deviation: a state of a character not yet ported is written as `None`.
#[derive(Clone, Debug, Default, PartialEq)]
// (an allow, not an expect: which variant is the largest changes as characters are added)
#[allow(clippy::large_enum_variant, reason = "a frame holds a few: one per fighter, read once")]
pub enum CharRecord {
    #[default]
    None,
    Shaggy(ShaggyRecord),
    Batman(BatmanRecord),
    Superman(SupermanRecord),
    Lebron(LebronRecord),
    Arya(AryaRecord),
    Creature(ReindogRecord),
    BlackAdam(BlackAdamRecord),
    WonderWoman(WonderWomanRecord),
    Harley(HarleyRecord),
    Morty(MortyRecord),
    IronGiant(GiantRecord),
    Steven(StevenRecord),
    Garnet(GarnetRecord),
    Taz(TazRecord),
    Bugs(BugsRecord),
    Rick(RickRecord),
    Finn(FinnRecord),
    Stripe(StripeRecord),
    Jake(JakeRecord),
    TomAndJerry(TomAndJerryRecord),
    Velma(VelmaRecord),
    VelmaPolice(VelmaPoliceRecord),
    Marvin(MarvinRecord),
    Gizmo(GizmoRecord),
}

impl CharRecord {
    /// The variants by their tag byte, the names the schema and the oracle writer use.
    pub const VARIANTS: [&'static str; 25] = [
        "none",
        "shaggy",
        "batman",
        "superman",
        "lebron",
        "arya",
        "creature",
        "blackadam",
        "wonderwoman",
        "harley",
        "morty",
        "irongiant",
        "steven",
        "garnet",
        "taz",
        "bugs",
        "rick",
        "finn",
        "stripe",
        "jake",
        "tomandjerry",
        "velma",
        "velmapolice",
        "marvin",
        "gizmo",
    ];
}

/// Each variant's arm of [`CharRecord`]'s `Field` methods: its tag byte, its path head and its record type.
macro_rules! char_record_field {
    ($($variant:ident = $tag:literal, $head:literal, $rec:ident;)*) => {
        impl Field for CharRecord {
            fn put(&self, w: &mut Vec<u8>) {
                match self {
                    CharRecord::None => w.push(0),
                    $(CharRecord::$variant(s) => {
                        w.push($tag);
                        s.put(w);
                    })*
                }
            }
            fn get(r: &mut Cursor) -> Result<Self, TraceError> {
                match r.u8()? {
                    0 => Ok(CharRecord::None),
                    $($tag => Ok(CharRecord::$variant($rec::get(r)?)),)*
                    _ => Err(TraceError::Bad(r.position() - 1, "character state tag")),
                }
            }
            fn diff(a: &Self, b: &Self, path: &mut String, out: &mut Vec<Difference>) {
                match (a, b) {
                    (CharRecord::None, CharRecord::None) => {}
                    $((CharRecord::$variant(x), CharRecord::$variant(y)) => {
                        let at = push_path(path, $head);
                        $rec::diff(x, y, path, out);
                        path.truncate(at);
                    })*
                    _ => out.push(Difference { path: path.clone(), expected: a.show(), actual: b.show() }),
                }
            }
            fn show(&self) -> String {
                match self {
                    CharRecord::None => "none".to_string(),
                    $(CharRecord::$variant(s) => format!("{} {}", Self::VARIANTS[$tag], s.show()),)*
                }
            }
            fn show_tree(&self, path: &mut String, out: &mut String) {
                match self {
                    CharRecord::None => {
                        use std::fmt::Write as _;
                        let _ = writeln!(out, "{path} = none");
                    }
                    $(CharRecord::$variant(s) => {
                        let at = push_path(path, $head);
                        s.show_tree(path, out);
                        path.truncate(at);
                    })*
                }
            }
            /// `<key>.x` of that character's state; of another variant, the variant.
            fn find(&self, path: &str) -> Option<String> {
                let (head, rest) = split_path(path);
                match self {
                    $(CharRecord::$variant(s) if head == $head => s.find(rest),)*
                    _ => Some(self.show()),
                }
            }
            fn schema(path: &str, out: &mut Vec<String>) {
                out.push(format!("{path}:enum({})", Self::VARIANTS.join(",")));
                $($rec::schema(&format!("{path}{}", $head), out);)*
            }
        }
    };
}

char_record_field! {
    Shaggy = 1, "<shaggy>", ShaggyRecord;
    Batman = 2, "<batman>", BatmanRecord;
    Superman = 3, "<superman>", SupermanRecord;
    Lebron = 4, "<lebron>", LebronRecord;
    Arya = 5, "<arya>", AryaRecord;
    Creature = 6, "<creature>", ReindogRecord;
    BlackAdam = 7, "<blackadam>", BlackAdamRecord;
    WonderWoman = 8, "<wonderwoman>", WonderWomanRecord;
    Harley = 9, "<harley>", HarleyRecord;
    Morty = 10, "<morty>", MortyRecord;
    IronGiant = 11, "<irongiant>", GiantRecord;
    Steven = 12, "<steven>", StevenRecord;
    Garnet = 13, "<garnet>", GarnetRecord;
    Taz = 14, "<taz>", TazRecord;
    Bugs = 15, "<bugs>", BugsRecord;
    Rick = 16, "<rick>", RickRecord;
    Finn = 17, "<finn>", FinnRecord;
    Stripe = 18, "<stripe>", StripeRecord;
    Jake = 19, "<jake>", JakeRecord;
    TomAndJerry = 20, "<tomandjerry>", TomAndJerryRecord;
    Velma = 21, "<velma>", VelmaRecord;
    VelmaPolice = 22, "<velmapolice>", VelmaPoliceRecord;
    Marvin = 23, "<marvin>", MarvinRecord;
    Gizmo = 24, "<gizmo>", GizmoRecord;
}

record! {
    /// A fighter (`Fighter`): `World.pawns`, or `World.fighters` in a match with no
    /// pawn past the players.
    pub struct FighterRecord {
        /// `Fighter.id`
        pub id: f64,
        /// `Fighter.ch.key` (the character)
        pub character: String,
        /// `Fighter.team`
        pub team: f64,
        /// `Fighter.rng`'s state (its mulberry32 `s`)
        pub rng: f64,
        /// `Fighter.x` (sim space, the capsule centre)
        pub x: f64,
        /// `Fighter.y`
        pub y: f64,
        /// `Fighter.vx`
        pub vx: f64,
        /// `Fighter.vy`
        pub vy: f64,
        /// `Fighter.facing` (1 or -1)
        pub facing: f64,
        /// `Fighter.time` (the fighter's world clock, seconds)
        pub time: f64,
        /// `Fighter.grounded` (MovementMode Walking)
        pub grounded: bool,
        /// `Fighter.kbMode` (MovementMode Custom 2, the knockback mode)
        pub kb_mode: bool,
        /// `Fighter.flying` (MovementMode Flying)
        pub flying: bool,
        /// `Fighter.gs` (the AnimBP's ground state)
        pub ground_state: String,
        /// `Fighter.gsTime`
        pub ground_state_time: f64,
        /// `Fighter.crouching` (isCrouching)
        pub crouching: bool,
        /// `Fighter.fastFalling`
        pub fast_falling: bool,
        /// `Fighter.wallSliding`
        pub wall_sliding: bool,
        /// `Fighter.evading`
        pub evading: bool,
        /// `Fighter.moveIn` (HandleMovementInput's AddMovementInput)
        pub move_in: f64,
        /// `Fighter.damage`
        pub damage: f64,
        /// `Fighter.hitstun` (frames)
        pub hitstun: f64,
        /// `Fighter.hitpause` (frames)
        pub hitpause: f64,
        /// `Fighter.trueHitstun`
        pub true_hitstun: f64,
        /// `Fighter.koHitstun`
        pub ko_hitstun: f64,
        /// `Fighter.launch`
        pub launch: Option<LaunchRecord>,
        /// `Fighter.mostRecentAttacker` (its id)
        pub most_recent_attacker: f64,
        /// `Fighter.diCombo`
        pub di_combo: f64,
        /// `Fighter.attackCount` (AttackID)
        pub attack_count: f64,
        /// `Fighter.jumps`
        pub jumps: f64,
        /// `Fighter.jumpFrame`
        pub jump_frame: f64,
        /// `Fighter.airEvades`
        pub air_evades: f64,
        /// `Fighter.airSpecials`
        pub air_specials: f64,
        /// `Fighter.airAttackAvailable`
        pub air_attack_available: bool,
        /// `Fighter.evadeCount`
        pub evade_count: f64,
        /// `Fighter.evadeInvuln` (EvadeInvulnerableTimer, frames left)
        pub evade_invuln: f64,
        /// `Fighter.invuln`
        pub invuln: f64,
        /// `Fighter.invulnCount`
        pub invuln_count: f64,
        /// `Fighter.rage`
        pub rage: f64,
        /// `Fighter.rageEnabled`
        pub rage_enabled: bool,
        /// `Fighter.rageCharging`
        pub rage_charging: bool,
        /// `Fighter.rageFilled`
        pub rage_filled: bool,
        /// `Fighter.rageTimer`
        pub rage_timer: f64,
        /// `Fighter.chargePct`
        pub charge_pct: f64,
        /// `Fighter.dead` (frames until the respawn)
        pub dead: f64,
        /// `Fighter.invisible`
        pub invisible: bool,
        /// `Fighter.abducted`
        pub abducted: bool,
        /// `Fighter.kos`
        pub kos: f64,
        /// `Fighter.ringOuts` (PFGPlayerState RingOuts)
        pub ring_outs: f64,
        /// `Fighter.damageDealt`
        pub damage_dealt: f64,
        /// `Fighter.sizeScale`
        pub size_scale: f64,
        /// `Fighter.gravityScale`
        pub gravity_scale: f64,
        /// `Fighter.speedMod`
        pub speed_mod: f64,
        /// `Fighter.montage` (the attack montage)
        pub montage: Option<MontageRecord>,
        /// `Fighter.dashMontage` (a montage without attack state: dash, wall jump, intro)
        pub dash_montage: Option<MontageRecord>,
        /// `Fighter.rageInput` (the rage build-up's montage)
        pub rage_input: Option<MontageRecord>,
        /// `Fighter.anim` (the shown locomotion animation)
        pub anim: String,
        /// `Fighter.animTime`
        pub anim_time: f64,
        /// `Fighter.locoAnim`
        pub loco_anim: String,
        /// `Fighter.locoTime`
        pub loco_time: f64,
        /// `Fighter.buffs`, in list order
        pub buffs: Vec<BuffRecord>,
        /// `Fighter.armorPacks`
        pub armor: Vec<ArmorRecord>,
        /// `Fighter.abilities`
        pub abilities: Vec<AbilityRecord>,
        /// `Fighter.heldItem` (its id)
        pub held_item: f64,
        /// `Fighter.cs`
        pub state: CharRecord,
        /// `Fighter.queued` (the input buffer's press)
        pub queued: Option<QueuedRecord>,
        /// `Fighter.lateBranch`
        pub late_branch: Option<LateBranchRecord>,
        /// `Fighter.inp` (the input the fighter last took)
        pub inp: InputRecord,
        /// `Fighter.prev` (the one before it)
        pub prev: InputRecord,
        /// `Fighter.activeHits`
        pub active_hits: Vec<ActiveHitRecord>,
        /// `Fighter.pendingEnds`
        pub pending_ends: Vec<NotifyAtRecord>,
        /// `Fighter.attackDecay` (the combo decay's attack names)
        pub attack_decay: Vec<String>,
        /// `Fighter.landedDecaySequences`
        pub landed_decay_sequences: Vec<f64>,
        /// `Fighter.comboResMap`, in the Map's order
        pub combo_res_map: Vec<NamedRecord>,
        /// `Fighter.recentAttacks`
        pub recent_attacks: Vec<RecentAttackRecord>,
        /// `Fighter.landedAttackIds`, in the Set's order
        pub landed_attack_ids: Vec<f64>,
        /// `Fighter.equipped`
        pub equipped: Vec<String>,
        /// `Fighter.net`
        pub net: Option<ImpulseRecord>,
        /// `Fighter.impulse`
        pub impulse: Option<ImpulseRecord>,
        /// `Fighter.moveCmds`
        pub move_cmds: Vec<MoveCmdRecord>,
        /// `Fighter.moveEnd`
        pub move_end: Option<MoveEndRecord>,
        /// `Fighter.spawnedActors` (their ids)
        pub spawned: Vec<f64>,
        /// `Fighter.addKbVx`
        pub add_kb_vx: f64,
        /// `Fighter.addKbVy`
        pub add_kb_vy: f64,
        /// `Fighter.aimDirection`
        pub aim_direction: f64,
        /// `Fighter.airAttackDelay`
        pub air_attack_delay: f64,
        /// `Fighter.airBoostMult`
        pub air_boost_mult: f64,
        /// `Fighter.airControl`
        pub air_control: f64,
        /// `Fighter.airControlGate`
        pub air_control_gate: bool,
        /// `Fighter.akX`
        pub ak_x: f64,
        /// `Fighter.akY`
        pub ak_y: f64,
        /// `Fighter.animAttackOff`
        pub anim_attack_off: f64,
        /// `Fighter.animFastFallOff`
        pub anim_fast_fall_off: Option<bool>,
        /// `Fighter.animKey`
        pub anim_key: String,
        /// `Fighter.animUpdated`
        pub anim_updated: bool,
        /// `Fighter.armorVis`
        pub armor_vis: Option<f64>,
        /// `Fighter.assistClearTick`
        pub assist_clear_tick: f64,
        /// `Fighter.assists`
        pub assists: f64,
        /// `Fighter.attackIsActive`
        pub attack_is_active: f64,
        /// `Fighter.attackSlot`
        pub attack_slot: f64,
        /// `Fighter.attacksDodged`
        pub attacks_dodged: f64,
        /// `Fighter.baseVx`
        pub base_vx: f64,
        /// `Fighter.baseVy`
        pub base_vy: f64,
        /// `Fighter.beginAttack`
        pub begin_attack: bool,
        /// `Fighter.blastIn`
        pub blast_in: f64,
        /// `Fighter.blastListed`
        pub blast_listed: bool,
        /// `Fighter.blockJumpEndLaunch`
        pub block_jump_end_launch: bool,
        /// `Fighter.blockLandReset`
        pub block_land_reset: bool,
        /// `Fighter.blockPickUp`
        pub block_pick_up: bool,
        /// `Fighter.blockTauntAnims`
        pub block_taunt_anims: bool,
        /// `Fighter.botControlled`
        pub bot_controlled: bool,
        /// `Fighter.bounceFxUntil`
        pub bounce_fx_until: f64,
        /// `Fighter.bouncePitch`
        pub bounce_pitch: f64,
        /// `Fighter.bouncePose`
        pub bounce_pose: f64,
        /// `Fighter.bounceType`
        pub bounce_type: f64,
        /// `Fighter.bounceZOffset`
        pub bounce_z_offset: f64,
        /// `Fighter.bouncing`
        pub bouncing: bool,
        /// `Fighter.brAxisX`
        pub br_axis_x: f64,
        /// `Fighter.brAxisY`
        pub br_axis_y: f64,
        /// `Fighter.brStickX`
        pub br_stick_x: f64,
        /// `Fighter.branchMontage`
        pub branch_montage: Option<String>,
        /// `Fighter.buffStun`
        pub buff_stun: f64,
        /// `Fighter.buffStunAge`
        pub buff_stun_age: f64,
        /// `Fighter.buffStunLen`
        pub buff_stun_len: f64,
        /// `Fighter.buffStunned`
        pub buff_stunned: bool,
        /// `Fighter.camFocus`
        pub cam_focus: bool,
        /// `Fighter.canBounceNoHitstun`
        pub can_bounce_no_hitstun: bool,
        /// `Fighter.cantDie`
        pub cant_die: bool,
        /// `Fighter.capHalfU`
        pub cap_half_u: f64,
        /// `Fighter.capRadiusU`
        pub cap_radius_u: f64,
        /// `Fighter.capResized`
        pub cap_resized: bool,
        /// `Fighter.capsuleOff`
        pub capsule_off: bool,
        /// `Fighter.chargeBtn`
        pub charge_btn: Option<String>,
        /// `Fighter.chargeBuffMult`
        pub charge_buff_mult: f64,
        /// `Fighter.chargeDmgMult`
        pub charge_dmg_mult: f64,
        /// `Fighter.chargeHitLargeAt`
        pub charge_hit_large_at: f64,
        /// `Fighter.chargeHitSmallAt`
        pub charge_hit_small_at: f64,
        /// `Fighter.chargeMult`
        pub charge_mult: f64,
        /// `Fighter.chargeNotify`
        pub charge_notify: f64,
        /// `Fighter.chargeRampDelay`
        pub charge_ramp_delay: f64,
        /// `Fighter.chargeReleased`
        pub charge_released: bool,
        /// `Fighter.chargeSoundKey`
        pub charge_sound_key: String,
        /// `Fighter.chargeSoundSeq`
        pub charge_sound_seq: f64,
        /// `Fighter.chargeStartTime`
        pub charge_start_time: f64,
        /// `Fighter.chargeValue`
        pub charge_value: f64,
        /// `Fighter.cmcCrouched`
        pub cmc_crouched: bool,
        /// `Fighter.cmdJumped`
        pub cmd_jumped: bool,
        /// `Fighter.comboResetTick`
        pub combo_reset_tick: f64,
        /// `Fighter.comboResistance`
        pub combo_resistance: bool,
        /// `Fighter.cooldownMultiplier`
        pub cooldown_multiplier: f64,
        /// `Fighter.creationTime`
        pub creation_time: f64,
        /// `Fighter.crouchFx`
        pub crouch_fx: bool,
        /// `Fighter.crouchJump`
        pub crouch_jump: bool,
        /// `Fighter.crouchLocked`
        pub crouch_locked: bool,
        /// `Fighter.crouchOwed`
        pub crouch_owed: bool,
        /// `Fighter.currentDecaySequence`
        pub current_decay_sequence: f64,
        /// `Fighter.damageDisabled`
        pub damage_disabled: bool,
        /// `Fighter.dashFallGate`
        pub dash_fall_gate: bool,
        /// `Fighter.dashGameTime`
        pub dash_game_time: f64,
        /// `Fighter.dashStopBlend`
        pub dash_stop_blend: bool,
        /// `Fighter.dashStopDelay`
        pub dash_stop_delay: f64,
        /// `Fighter.dashTick`
        pub dash_tick: f64,
        /// `Fighter.deferQueuedEnds`
        pub defer_queued_ends: bool,
        /// `Fighter.directionSoft`
        pub direction_soft: f64,
        /// `Fighter.dodgedSuccessfully`
        pub dodged_successfully: bool,
        /// `Fighter.doubleRingOuts`
        pub double_ring_outs: f64,
        /// `Fighter.downBtn`
        pub down_btn: bool,
        /// `Fighter.downPressActive`
        pub down_press_active: bool,
        /// `Fighter.downPressCount`
        pub down_press_count: f64,
        /// `Fighter.downStick`
        pub down_stick: bool,
        /// `Fighter.downWindowTick`
        pub down_window_tick: f64,
        /// `Fighter.droppingDown`
        pub dropping_down: bool,
        /// `Fighter.endJumpLatent`
        pub end_jump_latent: bool,
        /// `Fighter.evadeCrouchRate`
        pub evade_crouch_rate: bool,
        /// `Fighter.evadeInvulnTime`
        pub evade_invuln_time: f64,
        /// `Fighter.evadeRegenOnce`
        pub evade_regen_once: bool,
        /// `Fighter.evadeRegenStart`
        pub evade_regen_start: f64,
        /// `Fighter.evadingThroughPlatform`
        pub evading_through_platform: bool,
        /// `Fighter.facingDir`
        pub facing_dir: f64,
        /// `Fighter.fadeAnim`
        pub fade_anim: String,
        /// `Fighter.fadeDur`
        pub fade_dur: f64,
        /// `Fighter.fadeEl`
        pub fade_el: f64,
        /// `Fighter.fadeLoop`
        pub fade_loop: bool,
        /// `Fighter.fadeRate`
        pub fade_rate: f64,
        /// `Fighter.fadeTime`
        pub fade_time: f64,
        /// `Fighter.fallLatFriction`
        pub fall_lat_friction: f64,
        /// `Fighter.fallLean`
        pub fall_lean: f64,
        /// `Fighter.ffCheckTick`
        pub ff_check_tick: f64,
        /// `Fighter.ffMult`
        pub ff_mult: bool,
        /// `Fighter.ffTriggered`
        pub ff_triggered: bool,
        /// `Fighter.floorBounceHurt`
        pub floor_bounce_hurt: bool,
        /// `Fighter.floorSwept`
        pub floor_swept: f64,
        /// `Fighter.fluidFriction`
        pub fluid_friction: f64,
        /// `Fighter.forceAdditiveKb`
        pub force_additive_kb: bool,
        /// `Fighter.forceMoveSeq`
        pub force_move_seq: f64,
        /// `Fighter.frameMode`
        pub frame_mode: String,
        /// `Fighter.freeLaunch`
        pub free_launch: bool,
        /// `Fighter.frictionZero`
        pub friction_zero: bool,
        /// `Fighter.grappled`
        pub grappled: bool,
        /// `Fighter.gravityModCount`
        pub gravity_mod_count: f64,
        /// `Fighter.groundJumpTick`
        pub ground_jump_tick: f64,
        /// `Fighter.gsRel`
        pub gs_rel: f64,
        /// `Fighter.hasHitAFighter`
        pub has_hit_a_fighter: bool,
        /// `Fighter.hasSpawnInvuln`
        pub has_spawn_invuln: bool,
        /// `Fighter.hitByMultipleAttackers`
        pub hit_by_multiple_attackers: bool,
        /// `Fighter.hitLaunch`
        pub hit_launch: bool,
        /// `Fighter.hitNeutral`
        pub hit_neutral: bool,
        /// `Fighter.hitPauseTime`
        pub hit_pause_time: f64,
        /// `Fighter.hitReactInt`
        pub hit_react_int: f64,
        /// `Fighter.hitStunEndedBound`
        pub hit_stun_ended_bound: bool,
        /// `Fighter.hitStunKbVx`
        pub hit_stun_kb_vx: f64,
        /// `Fighter.hitStunKbVy`
        pub hit_stun_kb_vy: f64,
        /// `Fighter.hitStunnedBeforeHit`
        pub hit_stunned_before_hit: bool,
        /// `Fighter.hitThisFrame`
        pub hit_this_frame: bool,
        /// `Fighter.hitstunCount`
        pub hitstun_count: f64,
        /// `Fighter.hitstunCounted`
        pub hitstun_counted: f64,
        /// `Fighter.hitstunDamage`
        pub hitstun_damage: f64,
        /// `Fighter.hitstunEndTime`
        pub hitstun_end_time: f64,
        /// `Fighter.hitstunPaused`
        pub hitstun_paused: bool,
        /// `Fighter.hitstunPending`
        pub hitstun_pending: bool,
        /// `Fighter.hitstunRate`
        pub hitstun_rate: f64,
        /// `Fighter.holdingEnemy`
        pub holding_enemy: bool,
        /// `Fighter.hrBranch`
        pub hr_branch: String,
        /// `Fighter.hrIntro`
        pub hr_intro: bool,
        /// `Fighter.ignorePlatforms`
        pub ignore_platforms: bool,
        /// `Fighter.inAbilityTimers`
        pub in_ability_timers: bool,
        /// `Fighter.inHitPausePose`
        pub in_hit_pause_pose: bool,
        /// `Fighter.instantCooldowns`
        pub instant_cooldowns: bool,
        /// `Fighter.isMinion`
        pub is_minion: bool,
        /// `Fighter.itemHolstered`
        pub item_holstered: bool,
        /// `Fighter.itemReachable`
        pub item_reachable: bool,
        /// `Fighter.juggleCount`
        pub juggle_count: f64,
        /// `Fighter.juggleDamage`
        pub juggle_damage: f64,
        /// `Fighter.jumpCutAt`
        pub jump_cut_at: f64,
        /// `Fighter.jumpLaunched`
        pub jump_launched: bool,
        /// `Fighter.jumpMinTime`
        pub jump_min_time: f64,
        /// `Fighter.jumpReleased`
        pub jump_released: bool,
        /// `Fighter.jumpStartTime`
        pub jump_start_time: f64,
        /// `Fighter.jumpStopGate`
        pub jump_stop_gate: bool,
        /// `Fighter.jumpV`
        pub jump_v: f64,
        /// `Fighter.kbFacing`
        pub kb_facing: bool,
        /// `Fighter.knockBackAngle`
        pub knock_back_angle: f64,
        /// `Fighter.knockedBackCount`
        pub knocked_back_count: f64,
        /// `Fighter.lastAttackerOffLedge`
        pub last_attacker_off_ledge: bool,
        /// `Fighter.lastDecayStacks`
        pub last_decay_stacks: f64,
        /// `Fighter.lastDirection`
        pub last_direction: f64,
        /// `Fighter.lastEvadeSpentMeter`
        pub last_evade_spent_meter: bool,
        /// `Fighter.lastEvadeTime`
        pub last_evade_time: f64,
        /// `Fighter.lastHitstunTime`
        pub last_hitstun_time: f64,
        /// `Fighter.lastJumpTime`
        pub last_jump_time: f64,
        /// `Fighter.lastJumpV`
        pub last_jump_v: f64,
        /// `Fighter.lastKnockedBack`
        pub last_knocked_back: f64,
        /// `Fighter.lastMoveIn`
        pub last_move_in: f64,
        /// `Fighter.lastRingOutTime`
        pub last_ring_out_time: f64,
        /// `Fighter.lateBranchDue`
        pub late_branch_due: bool,
        /// `Fighter.layerMissed`
        pub layer_missed: bool,
        /// `Fighter.lingerSeq`
        pub linger_seq: f64,
        /// `Fighter.locoGen`
        pub loco_gen: f64,
        /// `Fighter.locoLoop`
        pub loco_loop: bool,
        /// `Fighter.locoRate`
        pub loco_rate: f64,
        /// `Fighter.meshX`
        pub mesh_x: f64,
        /// `Fighter.meshZ`
        pub mesh_z: f64,
        /// `Fighter.montageAbort`
        pub montage_abort: bool,
        /// `Fighter.montageHit`
        pub montage_hit: bool,
        /// `Fighter.moved`
        pub moved: bool,
        /// `Fighter.netFriction`
        pub net_friction: Option<bool>,
        /// `Fighter.netGravity`
        pub net_gravity: Option<f64>,
        /// `Fighter.noWalkOffLedges`
        pub no_walk_off_ledges: Option<bool>,
        /// `Fighter.noWallSlideUntil`
        pub no_wall_slide_until: f64,
        /// `Fighter.notPlayerCharacter`
        pub not_player_character: bool,
        /// `Fighter.offPlatformRingOuts`
        pub off_platform_ring_outs: f64,
        /// `Fighter.onCamera`
        pub on_camera: bool,
        /// `Fighter.onFloor`
        pub on_floor: bool,
        /// `Fighter.outlineCount`
        pub outline_count: f64,
        /// `Fighter.pawnVy`
        pub pawn_vy: f64,
        /// `Fighter.pickupRequest`
        pub pickup_request: bool,
        /// `Fighter.plainIsIntro`
        pub plain_is_intro: bool,
        /// `Fighter.platIgnored`
        pub plat_ignored: f64,
        /// `Fighter.playedInAdvance`
        pub played_in_advance: bool,
        /// `Fighter.poseTicked`
        pub pose_ticked: bool,
        /// `Fighter.preSpawnDelay`
        pub pre_spawn_delay: f64,
        /// `Fighter.prevBounceTime`
        pub prev_bounce_time: f64,
        /// `Fighter.prevTime`
        pub prev_time: f64,
        /// `Fighter.prevX`
        pub prev_x: f64,
        /// `Fighter.prevY`
        pub prev_y: f64,
        /// `Fighter.previousAttacker` (its id)
        pub previous_attacker: f64,
        /// `Fighter.projectileArmor`
        pub projectile_armor: bool,
        /// `Fighter.pushingDown`
        pub pushing_down: bool,
        /// `Fighter.queuedHitPause`
        pub queued_hit_pause: f64,
        /// `Fighter.queuedHitPauseSec`
        pub queued_hit_pause_sec: f64,
        /// `Fighter.quickFallAt`
        pub quick_fall_at: f64,
        /// `Fighter.rageFx`
        pub rage_fx: bool,
        /// `Fighter.rageGlowShown`
        pub rage_glow_shown: bool,
        /// `Fighter.rageResidual`
        pub rage_residual: bool,
        /// `Fighter.rageStarted`
        pub rage_started: bool,
        /// `Fighter.rapidBounceCount`
        pub rapid_bounce_count: f64,
        /// `Fighter.reachRecheck`
        pub reach_recheck: bool,
        /// `Fighter.recentHitActor` (a fighter or an actor)
        pub recent_hit_actor: Option<RefRecord>,
        /// `Fighter.ringOutsAssisted`
        pub ring_outs_assisted: f64,
        /// `Fighter.rootDx`
        pub root_dx: f64,
        /// `Fighter.rootDy`
        pub root_dy: f64,
        /// `Fighter.rootHas`
        pub root_has: bool,
        /// `Fighter.rotLockUntil`
        pub rot_lock_until: f64,
        /// `Fighter.sameAttackBlocked`
        pub same_attack_blocked: bool,
        /// `Fighter.seqDelta`
        pub seq_delta: f64,
        /// `Fighter.seqFrom`
        pub seq_from: f64,
        /// `Fighter.startingDamage`
        pub starting_damage: f64,
        /// `Fighter.stopAhead`
        pub stop_ahead: bool,
        /// `Fighter.stunSource`
        pub stun_source: bool,
        /// `Fighter.suspended`
        pub suspended: bool,
        /// `Fighter.tauntSlot`
        pub taunt_slot: Option<f64>,
        /// `Fighter.terminalV`
        pub terminal_v: f64,
        /// `Fighter.tick`
        pub tick: f64,
        /// `Fighter.timeInAir`
        pub time_in_air: f64,
        /// `Fighter.totalChargedTime`
        pub total_charged_time: f64,
        /// `Fighter.trueHitstunCounted`
        pub true_hitstun_counted: f64,
        /// `Fighter.trueHitstunRate`
        pub true_hitstun_rate: f64,
        /// `Fighter.trueHitstunSetTick`
        pub true_hitstun_set_tick: f64,
        /// `Fighter.trueHitstunTickAt`
        pub true_hitstun_tick_at: f64,
        /// `Fighter.velClamped`
        pub vel_clamped: f64,
        /// `Fighter.voCurrent`
        pub vo_current: Option<String>,
        /// `Fighter.voRecent`
        pub vo_recent: f64,
        /// `Fighter.voSeq`
        pub vo_seq: f64,
        /// `Fighter.voSpeaking`
        pub vo_speaking: bool,
        /// `Fighter.walkDrop`
        pub walk_drop: bool,
        /// `Fighter.wallCapped`
        pub wall_capped: bool,
        /// `Fighter.wallFatigue`
        pub wall_fatigue: f64,
        /// `Fighter.wallFatigueAt`
        pub wall_fatigue_at: f64,
        /// `Fighter.wallMult`
        pub wall_mult: f64,
        /// `Fighter.wallSkate`
        pub wall_skate: bool,
        /// `Fighter.wallStall`
        pub wall_stall: f64,
        /// `Fighter.wallTimerPending`
        pub wall_timer_pending: bool,
        /// `Fighter.wallTouches`
        pub wall_touches: f64,
        /// `Fighter.wasAttacking`
        pub was_attacking: bool,
        /// `Fighter.windOn`
        pub wind_on: bool,
        /// `Fighter.zeroOutVelForKb`
        pub zero_out_vel_for_kb: bool,
        /// every other own key of the Fighter (the module doc)
        pub rest: Value,
    }
}

record! {
    /// A projectile (`Projectile`, `World.projectiles`).
    pub struct ProjectileRecord {
        /// `Projectile.id` (World.actorSeq at the spawn)
        pub id: f64,
        /// `Projectile.cls`
        pub class: String,
        /// `Projectile.owner` (its id)
        pub owner: f64,
        /// `Projectile.x`
        pub x: f64,
        /// `Projectile.y`
        pub y: f64,
        /// `Projectile.vx`
        pub vx: f64,
        /// `Projectile.vy`
        pub vy: f64,
        /// `Projectile.facing`
        pub facing: f64,
        /// `Projectile.age` (ticks moved)
        pub age: f64,
        /// `Projectile.dead`
        pub dead: bool,
        /// `Projectile.hitActors`, in the Set's order
        pub hit_actors: Vec<Value>,
        /// every other own key
        pub rest: Value,
    }
}

record! {
    /// An actor of the World that is no fighter and no projectile: `family` names its list (`actors`: a registry
    /// actor, `items`, `markers`, `ships`, `mortars`, `aimPunches`, `accelerators`).
    pub struct ActorRecord {
        /// the World list it is in
        pub family: String,
        /// `.id`
        pub id: f64,
        /// `.cls` (empty for a class with none)
        pub class: String,
        /// `.owner` (its id: the spawning fighter, then whoever picked an item up last)
        pub owner: f64,
        /// `.holder` (an item's: the fighter it is attached to, -1 none)
        pub holder: Option<f64>,
        /// `.x`, where the actor has one
        pub x: Option<f64>,
        /// `.y`
        pub y: Option<f64>,
        /// `.vx`
        pub vx: Option<f64>,
        /// `.vy`
        pub vy: Option<f64>,
        /// `.dead`
        pub dead: bool,
        /// `.hitActors`, in the Set's order
        pub hit_actors: Vec<Value>,
        /// every other own key
        pub rest: Value,
    }
}

record! {
    /// A timer of the game mode (`World.timers`).
    pub struct TimerRecord {
        /// `timer.at` (the world tick it fires on)
        pub at: f64,
        /// `timer.key`
        pub key: Option<f64>,
        /// what `timer.fn` runs, by its site (the oracle's `TIMER_KINDS`: `ringOutAnchor`, `removeCameraFocus`,
        /// `abductTail`, `bonusSurvived`, `endWarning`, `blastShrink`, `introReveal`, `matchStartVo`, `introEnd`,
        /// `onCamera`, `visible`, `victoryVo`, `matchPointShow`)
        pub kind: String,
    }
}

record! {
    /// A sim object by its list and id.
    pub struct RefRecord {
        /// `fighter`, `projectile`, `item` or the World list's name
        pub family: String,
        pub id: f64,
    }
}

record! {
    /// A fighter's items in reach (`World.reach[id]`, Comp_ItemManager).
    pub struct ReachRecord {
        /// `.items`
        pub items: Vec<RefRecord>,
        /// `.closest`
        pub closest: Option<RefRecord>,
    }
}

record! {
    /// FFA's bonus tracker of a player (`World.bonusTrackers`).
    pub struct BonusRecord {
        /// `.alive`
        pub alive: bool,
        /// `.spree`
        pub spree: f64,
        /// `.spreeDone`
        pub spree_done: bool,
    }
}

record! {
    /// The World after a step (`World`).
    pub struct WorldRecord {
        /// `World.tick`
        pub tick: f64,
        /// `World.introTicks`
        pub intro_ticks: f64,
        /// `World.rng`'s state (its mulberry32 `s`)
        pub rng: f64,
        /// `World.actorSeq`
        pub actor_seq: f64,
        /// `World.itemSeq`
        pub item_seq: f64,
        /// `World.matchStarted`
        pub match_started: bool,
        /// `World.matchOver`
        pub match_over: bool,
        /// `World.winningTeam`
        pub winning_team: f64,
        /// `World.matchStartTick`
        pub match_start_tick: f64,
        /// `World.matchOverTick`
        pub match_over_tick: f64,
        /// `World.koPointMultiplier`
        pub ko_point_multiplier: f64,
        /// `World.teamKnockouts`
        pub team_knockouts: Vec<f64>,
        /// `World.teamDamage`
        pub team_damage: Vec<f64>,
        /// `World.stocks` (the experimental stock rule only)
        pub stocks: Option<Vec<f64>>,
        /// `World.stepRingOuts` (the live RingOutVfxAnchor actors)
        pub ring_out_anchors: f64,
        /// `World.blastShrink`
        pub blast_shrink: f64,
        /// `World.announcerUntil`
        pub announcer_until: f64,
        /// `World.voShared.speakers` (PandaGameState's VO speaker count)
        pub vo_speakers: f64,
        /// `World.victoryVo`
        pub victory_vo: bool,
        /// `World.frameHits`, in the Set's order
        pub frame_hits: Vec<Value>,
        /// `World.reach`
        pub reach: Vec<ReachRecord>,
        /// `World.bonusTrackers`
        pub bonus_trackers: Vec<BonusRecord>,
        /// `World.timers`
        pub timers: Vec<TimerRecord>,
        /// `World.pawns ?? World.fighters`
        pub fighters: Vec<FighterRecord>,
        /// `World.projectiles`
        pub projectiles: Vec<ProjectileRecord>,
        /// `World.actors`, `items`, `markers`, `ships`, `mortars`, `aimPunches`, `accelerators`, in that order
        pub actors: Vec<ActorRecord>,
        /// `World.events` of the step, in order
        pub events: Vec<EventRecord>,
        /// every other own key of the World
        pub rest: Value,
    }
}

record! {
    /// One tick of the trace: the inputs given to `World.step` and the World after it.
    pub struct Frame {
        /// the `InputState` per fighter given to the step
        pub inputs: Vec<InputRecord>,
        /// the World after the step
        pub world: WorldRecord,
    }
}

/// The schema of a frame: every leaf's path and kind in write order, the list a file's header holds.
pub fn frame_schema() -> Vec<String> {
    let mut out = Vec::new();
    Frame::schema("", &mut out);
    out
}

/// A page call a tutorial match makes between two steps: the
/// TutorialActor's calls the widgets make (`TutorialRun`). The header's `calls` holds each with the index of the frame
/// it precedes: the call is made before that frame's step, its events are that frame's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TutorialCall {
    /// The intro screen's BPI_IntroComplete.
    IntroComplete,
    /// The concept card's start button.
    StartPressed,
    /// The widget's skip button: BPI_AdvanceTutorial.
    Advance,
    /// SkipTutorialStep.
    SkipStep,
    /// BPI_SkipToTutorialEndStep.
    SkipToEnd,
    /// The end screen's BPI_DesaturateEnvironment.
    DesaturateEnvironment,
    /// The end screen's BPI_StartNextTutorial.
    StartNext,
}

/// The set-up's `wheel` of a match with the port's own Chaos Wheel rule as a header holds it: `{pool, seconds, warn}`
/// (`mvs_sim::setup::Wheel`). The oracle has no wheel: only the Rust recorder writes one.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelSpec {
    pub pool: Vec<String>,
    pub seconds: f64,
    pub warn: f64,
}

/// The set-up's `mode` of a match of the port's own Waves mode as a header holds it: `{kind: 'waves', variant, cast}`,
/// `variant` `'ringOuts100'` or `'endless'`. The oracle has no Waves mode: only the Rust recorder writes one.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WavesMode {
    pub kind: String,
    pub variant: String,
    pub cast: Vec<String>,
}

/// The set-up's `mode` of a tutorial match as a header holds it: `{kind: 'tutorial', actor, cast, data}`, `data` the
/// actor's tutorial data in the shape of `mvs_data::tutorial::TutorialActorData` (the oracle's `tutorials`, every
/// actor's, is the World's alone and not written).
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TutorialMode {
    pub kind: String,
    pub actor: String,
    pub cast: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
