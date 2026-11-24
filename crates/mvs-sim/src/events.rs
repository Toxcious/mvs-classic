//! What a step tells the presentation: one enum, a variant per event
//! `type`, its fields typed. [`Event::type_name`] gives the type tag, which the trace writes.
//!
//! A fighter is its id (`Fighter.id`: the player's slot, a pawn's number), -1 where the event is written with -1 for
//! "the match". Events are the step's output, not its state: the World's buffer is outside the snapshot and the hash.

use crate::Real;
use crate::names::Name;

/// A fighter's id (`Fighter.id`), -1 for none.
pub type Fid = i32;

/// A notify of a montage (the `Notify` object): the montage's name and the notify's index in it.
#[derive(crate::State, Copy, PartialEq, Eq, Debug)]
pub struct NotifyRef {
    pub montage: Name,
    pub index: u32,
}

/// A sound's key, which is built as a string at run time (`'proj' + pr.id`): its fixed part and its number
/// (a tag with a `#` takes the number there: `proj#d` is `proj7d`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SoundKey {
    pub tag: Name,
    pub n: i32,
}

/// The `hit` part of a `sound` event (Wwise's surface and charge switches of a hit sound).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SoundHit {
    pub victim: Fid,
    pub z_floor: Option<Real>,
    pub charge: Option<Real>,
    pub surface: Option<i32>,
    pub instigator: Option<Fid>,
}

/// `popText`'s `kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PopKind {
    WallFatigue,
    AttackDecay,
    BuffDecay,
    Healing,
    TurnAttack,
    FfaRingout,
    FfaAssist,
    FfaRungout,
    Dodged,
    DodgedOff,
    LastStand,
    SpecialsRefreshed,
    AirRefreshed,
    ReflectProjectiles,
    Caught,
    JumpsReset,
    FinnTreasure,
    FinnHighFive,
    FinnBoots,
    FinnSale,
    NotifyText,
    TjCheese,
    GizmoForceDance,
    GizmoAttach,
    Transferred,
    /// (the port's own) The Chaos Wheel's Hot Potato over its holder (`world::wheel_events`).
    HotPotato,
}

/// `popText`'s `params`: a notify's own, or the few a caller builds.
#[derive(Clone, PartialEq, Debug)]
pub enum PopParams {
    Notify(NotifyRef),
    At {
        x: Real,
        y: Real,
    },
    Values(Vec<(Name, Real)>),
    /// A Blueprint graph's UI_PopTextParams (`GraphPopText`).
    Graph(Box<crate::combat::pop_text::GraphPopText>),
}

/// `aryaKnife`'s `kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KnifeKind {
    Stick,
    Ground,
    Dash,
}

/// `electricFx`'s `kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElectricKind {
    Loop,
    Burst,
    Beam,
}

/// `tazHealFx`'s `kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TazHealKind {
    Lick,
    Pickup,
}

/// `moveFx`'s `kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MoveFxKind {
    Jump,
    DoubleJump,
    Land,
    FastFall,
    WallOn,
    WallOff,
}

/// `disguiseFx`'s `kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisguiseFxKind {
    Activate,
    Destroy,
}

/// `EndPhase`: the match's end phases.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndPhase {
    GameEnd,
    PreTravel,
    Cleanup,
    Travel,
}

/// `UiStateParams`.
#[derive(Clone, PartialEq, Debug)]
pub struct UiStateParams {
    pub id: Name,
    /// UI_StateType.
    pub state_type: i32,
    /// UI_StateDisplayType.
    pub display_type: i32,
    pub current_value: Real,
    pub max_value: Real,
    pub timer: Real,
    pub icon_material: Name,
    pub active: bool,
    pub pop_text: Name,
    pub auto_remove_time: Real,
    pub subicon: Option<Name>,
}

/// `UiStateOp`.
#[derive(Clone, PartialEq, Debug)]
pub enum UiStateOp {
    Add(UiStateParams),
    Remove(Name),
}

/// `UiAbilityOp`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum UiAbilityOp {
    Exit,
    Error,
    Update { ammo: Real, cd: Real, cd_max: Real, ammo_cd: Real, ammo_max: Real, pulse: bool, pulse_ammo: bool },
}

/// `TutorialUiCall`.
#[derive(Clone, PartialEq, Debug)]
pub enum TutorialUiCall {
    Intro,
    Widget,
    Setup { step: i32, index: i32, count: i32 },
    TaskCount { task: i32, count: i32, index: i32, total: i32 },
    Jiggle { task: i32 },
    FadeOut { task: i32 },
    FadeIn,
    Concept { step: i32, start_disabled: Real },
    Focus { on: bool },
    Cleanup,
    RemoveWidget,
    Congrats { text: i32 },
    Finished { actor: Name },
    EndScreen { has_next: bool },
    Scoreboard { visible: bool },
    MinigameCounter { task: i32, text: Name, visible: bool },
    MinigameCount { task: i32, count: i32 },
    RewardPop { fighter: Fid },
}

/// `SimEvent`, a variant per event `type`, in the order the trace tags them.
#[derive(Clone, PartialEq, Debug)]
pub enum Event {
    ComboCounts {
        fighter: Fid,
        juggler: Fid,
        juggle_count: i32,
        juggle_damage: Real,
        hitstun_count: i32,
        hitstun_damage: Real,
    },
    Sound {
        fighter: Fid,
        ev: Name,
        x: Option<Real>,
        y: Option<Real>,
        hit: Option<SoundHit>,
        pooled: bool,
        key: Option<SoundKey>,
    },
    /// SpawnAkComponentAtLocation at (x, y), the RTPC `rtpc` = `value` on that new component, then `ev` posted on it
    /// with the pawn `instigator` (Lib_Audio PF_Post_Event_On_AkComponent_WithInstigator). The port's own kind: the
    /// plain sound events carry no RTPC.
    SpawnedSound {
        instigator: Fid,
        ev: Name,
        x: Real,
        y: Real,
        rtpc: Name,
        value: Real,
    },
    SoundStart {
        fighter: Fid,
        ev: Name,
        key: SoundKey,
    },
    SoundStop {
        fighter: Fid,
        key: SoundKey,
        fade: Option<Real>,
        ev: Option<Name>,
        curve: Option<Name>,
        cut: bool,
    },
    Rtpc {
        fighter: Fid,
        name: Name,
        value: Real,
        interp_ms: Real,
    },
    Switch {
        fighter: Fid,
        group: Name,
        value: Name,
    },
    CombatInput {
        fighter: Fid,
    },
    Hit {
        attacker: Fid,
        victim: Fid,
        damage: Real,
        pain_damage: Real,
        speed: Real,
        hitpause: Real,
        lockout: Real,
        x: Real,
        y: Real,
        light: bool,
    },
    MarkerBurst {
        fighter: Fid,
    },
    ItemFx {
        cls: Name,
        item: i32,
        fighter: Fid,
        vfx: Name,
        x: Real,
        y: Real,
        scale: Real,
    },
    MortarBoom {
        fighter: Fid,
        x: Real,
        y: Real,
        ax: Real,
        ay: Real,
        facing: Real,
    },
    AcceleratorBuff {
        fighter: Fid,
        x: Real,
        y: Real,
    },
    AcceleratorEnd {
        fighter: Fid,
        x: Real,
        y: Real,
    },
    ProjectileDeath {
        cls: Name,
        fighter: Fid,
        vfx: Option<Name>,
        x: Real,
        y: Real,
        ox: Real,
        rot: [Real; 3],
        scale: Real,
        tint: Option<Real>,
        to: Option<[Real; 2]>,
    },
    ShockwaveVfx {
        fighter: Fid,
        cls: Name,
        n: i32,
        x: Real,
        y: Real,
        facing: Real,
        hit_pause: Real,
    },
    Ko {
        fighter: Fid,
    },
    ArmorHit {
        fighter: Fid,
        x: Real,
        y: Real,
    },
    ArmorBreak {
        fighter: Fid,
    },
    Respawn {
        fighter: Fid,
    },
    SlotBound {
        fighter: Fid,
        slot: i32,
    },
    SlotActive {
        slot: i32,
        on: bool,
    },
    PriorityAdd {
        fighter: Fid,
    },
    TutorialUi {
        call: TutorialUiCall,
    },
    CameraShake {
        cls: Name,
    },
    Rumble {
        fighter: Fid,
        name: Name,
        looping: bool,
        tag: Name,
        ignore_time_dilation: bool,
        play_while_paused: bool,
    },
    RumbleStop {
        fighter: Fid,
        name: Option<Name>,
        tag: Name,
    },
    BounceVfx {
        x: Real,
        y: Real,
        roll: Real,
    },
    /// A minion's ring-out sound (HandleBlastBoxDeathFX with NotPlayerCharacter): sfx_blastbox_default_ally for the
    /// local player's team (GetColorIndex != 0), else sfx_blastbox_default_enemy, at `p` (UE Y, Z). Which one is the
    /// view's: the colour index is the local player's.
    BlastBoxSound {
        fighter: Fid,
        p: [Real; 2],
    },
    KoVfx {
        fighter: Fid,
        asset: Name,
        hold: Real,
        p: [Real; 2],
        tl: [Real; 2],
        tr: [Real; 2],
        bl: [Real; 2],
        center: [Real; 2],
    },
    SmokeTrail {
        fighter: Fid,
        on: bool,
    },
    DiVisual {
        fighter: Fid,
        attacker: Fid,
        vx: Real,
        vy: Real,
        x: Real,
        y: Real,
    },
    RageVfx {
        fighter: Fid,
        on: bool,
    },
    SpawnFx {
        fighter: Fid,
        on: bool,
    },
    RageGlow {
        fighter: Fid,
        on: bool,
        shirt: bool,
    },
    SwordGlow {
        fighter: Fid,
        on: bool,
    },
    Backstab {
        fighter: Fid,
        victim: Fid,
    },
    AryaKnife {
        fighter: Fid,
        knife: i32,
        kind: KnifeKind,
        victim: Fid,
        x: Real,
        y: Real,
    },
    MatScalar {
        fighter: Fid,
        name: Name,
        value: Real,
    },
    EvadeFx {
        fighter: Fid,
    },
    ReflectFx {
        actor: i32,
        item: bool,
        owner: Fid,
    },
    PopText {
        fighter: Fid,
        kind: PopKind,
        stacks: Real,
        gain: Option<bool>,
        params: Option<PopParams>,
    },
    ItemUiCreate {
        item: i32,
        fighter: Fid,
        action: u8,
        on_ground: bool,
        actor: bool,
    },
    ItemUi {
        item: i32,
        fighter: Fid,
        hide: bool,
        on_ground: bool,
        actor: bool,
    },
    ItemUiExit {
        item: i32,
        fighter: Option<Fid>,
        actor: bool,
    },
    RageResidual {
        fighter: Fid,
        scale: Option<Real>,
    },
    SizeScale {
        fighter: Fid,
        scale: Real,
    },
    EnlargeFx {
        fighter: Fid,
        begin: bool,
    },
    GrowFx {
        fighter: Fid,
    },
    PolymorphFx {
        fighter: Fid,
        begin: bool,
        vfx: Name,
    },
    ElectricFx {
        fighter: Fid,
        kind: ElectricKind,
        on: Option<bool>,
        target: Option<Fid>,
        x: Option<Real>,
        y: Option<Real>,
    },
    GauntletHit {
        fighter: Fid,
        cls: Name,
        victim: Fid,
    },
    TazHealFx {
        fighter: Fid,
        kind: TazHealKind,
    },
    TjPoof {
        fighter: Fid,
        x: Real,
        y: Real,
        scale: Real,
    },
    TeamScore {
        team: i32,
        score: Real,
        is_leader: bool,
        tied: bool,
    },
    Stocks {
        fighter: Fid,
        stocks: i32,
    },
    MatchPoint {
        team: i32,
    },
    MatchPointShow {
        team: i32,
    },
    DoublePoints,
    MutatorPopText {
        cls: Name,
        field: Name,
        text: Option<Name>,
    },
    /// The port's own Chaos Wheel (`world::wheel`): the next mutator drawn, the set-up's `warn` ahead of the change
    /// (`index` into the wheel's pool).
    WheelNext {
        index: i32,
    },
    /// The port's own Chaos Wheel: the window's mutator came on (`index` into the wheel's pool).
    WheelChange {
        index: i32,
    },
    TimeUp,
    MatchOver {
        team: i32,
    },
    Abduct {
        fighter: Fid,
    },
    CameraMovement {
        on: bool,
    },
    IntroFx {
        fighter: Fid,
    },
    IntroReveal {
        fighter: Fid,
    },
    MatchEnd {
        phase: EndPhase,
        team: i32,
    },
    UiState {
        fighter: Fid,
        op: UiStateOp,
    },
    EatUi {
        fighter: Fid,
        on: bool,
        on_ground: bool,
    },
    CloudLabel {
        fighter: Fid,
        owner: Fid,
        on: bool,
        hide: Option<bool>,
    },
    StruggleUi {
        fighter: Fid,
        time_left: Real,
        total: Real,
        hide_input: bool,
    },
    UiAbility {
        fighter: Fid,
        n: i32,
        op: UiAbilityOp,
    },
    AbilityAmmo {
        fighter: Fid,
        slot: i32,
        ammo: Real,
        increase: bool,
    },
    MoveFx {
        fighter: Fid,
        kind: MoveFxKind,
        x: Real,
        y: Real,
    },
    NotifyVfx {
        fighter: Fid,
        n: NotifyRef,
        on: bool,
        left: bool,
        copy: bool,
        poly: bool,
    },
    EmoteVfx {
        fighter: Fid,
        n: NotifyRef,
        on: bool,
        ps: Name,
        scale: [Real; 3],
    },
    ActorVfx {
        fighter: Fid,
        actor: i32,
        n: NotifyRef,
        on: bool,
        x: Real,
        y: Real,
    },
    ParticleNotify {
        fighter: Fid,
        n: NotifyRef,
        on: bool,
        left: bool,
        copy: bool,
        poly: bool,
    },
    SubMeshAnim {
        fighter: Fid,
        tag: Name,
        anim: Option<Name>,
        looping: bool,
    },
    PulseHit {
        fighter: Fid,
        pain_damage: Real,
    },
    AbilityUiCreate {
        fighter: Fid,
        slot: i32,
        icon: Name,
        max_count: i32,
    },
    DisguiseFx {
        fighter: Fid,
        kind: DisguiseFxKind,
    },
    HitVfx {
        victim: Fid,
        x: Real,
        y: Real,
        z_floor: Real,
        vx: Real,
        vy: Real,
        dist: Real,
        decay_stacks: Real,
        override_vfx: Option<Name>,
        force_small: bool,
    },
}

impl Event {
    /// The event's `type` tag.
    pub fn type_name(&self) -> &'static str {
        match self {
            Event::ComboCounts { .. } => "comboCounts",
            Event::Sound { .. } => "sound",
            Event::SpawnedSound { .. } => "spawnedSound",
            Event::SoundStart { .. } => "soundStart",
            Event::SoundStop { .. } => "soundStop",
            Event::Rtpc { .. } => "rtpc",
            Event::Switch { .. } => "switch",
            Event::CombatInput { .. } => "combatInput",
            Event::Hit { .. } => "hit",
            Event::MarkerBurst { .. } => "markerBurst",
            Event::ItemFx { .. } => "itemFx",
            Event::MortarBoom { .. } => "mortarBoom",
            Event::AcceleratorBuff { .. } => "acceleratorBuff",
            Event::AcceleratorEnd { .. } => "acceleratorEnd",
            Event::ProjectileDeath { .. } => "projectileDeath",
            Event::ShockwaveVfx { .. } => "shockwaveVfx",
            Event::Ko { .. } => "ko",
            Event::ArmorHit { .. } => "armorHit",
            Event::ArmorBreak { .. } => "armorBreak",
            Event::Respawn { .. } => "respawn",
            Event::SlotBound { .. } => "slotBound",
            Event::SlotActive { .. } => "slotActive",
            Event::PriorityAdd { .. } => "priorityAdd",
            Event::TutorialUi { .. } => "tutorialUi",
            Event::CameraShake { .. } => "cameraShake",
            Event::Rumble { .. } => "rumble",
            Event::RumbleStop { .. } => "rumbleStop",
            Event::BounceVfx { .. } => "bounceVfx",
            Event::BlastBoxSound { .. } => "blastBoxSound",
            Event::KoVfx { .. } => "koVfx",
            Event::SmokeTrail { .. } => "smokeTrail",
            Event::DiVisual { .. } => "diVisual",
            Event::RageVfx { .. } => "rageVfx",
            Event::SpawnFx { .. } => "spawnFx",
            Event::RageGlow { .. } => "rageGlow",
            Event::SwordGlow { .. } => "swordGlow",
            Event::Backstab { .. } => "backstab",
            Event::AryaKnife { .. } => "aryaKnife",
            Event::MatScalar { .. } => "matScalar",
            Event::EvadeFx { .. } => "evadeFx",
            Event::ReflectFx { .. } => "reflectFx",
            Event::PopText { .. } => "popText",
            Event::ItemUiCreate { .. } => "itemUiCreate",
            Event::ItemUi { .. } => "itemUi",
            Event::ItemUiExit { .. } => "itemUiExit",
            Event::RageResidual { .. } => "rageResidual",
            Event::SizeScale { .. } => "sizeScale",
            Event::EnlargeFx { .. } => "enlargeFx",
            Event::GrowFx { .. } => "growFx",
            Event::PolymorphFx { .. } => "polymorphFx",
            Event::ElectricFx { .. } => "electricFx",
            Event::GauntletHit { .. } => "gauntletHit",
            Event::TazHealFx { .. } => "tazHealFx",
            Event::TjPoof { .. } => "tjPoof",
            Event::TeamScore { .. } => "teamScore",
            Event::Stocks { .. } => "stocks",
            Event::MatchPoint { .. } => "matchPoint",
            Event::MatchPointShow { .. } => "matchPointShow",
            Event::DoublePoints => "doublePoints",
            Event::MutatorPopText { .. } => "mutatorPopText",
            Event::WheelNext { .. } => "wheelNext",
            Event::WheelChange { .. } => "wheelChange",
            Event::TimeUp => "timeUp",
            Event::MatchOver { .. } => "matchOver",
            Event::Abduct { .. } => "abduct",
            Event::CameraMovement { .. } => "cameraMovement",
            Event::IntroFx { .. } => "introFx",
            Event::IntroReveal { .. } => "introReveal",
            Event::MatchEnd { .. } => "matchEnd",
            Event::UiState { .. } => "uiState",
            Event::EatUi { .. } => "eatUi",
            Event::CloudLabel { .. } => "cloudLabel",
            Event::StruggleUi { .. } => "struggleUi",
            Event::UiAbility { .. } => "uiAbility",
            Event::AbilityAmmo { .. } => "abilityAmmo",
            Event::MoveFx { .. } => "moveFx",
            Event::NotifyVfx { .. } => "notifyVfx",
            Event::EmoteVfx { .. } => "emoteVfx",
            Event::ActorVfx { .. } => "actorVfx",
            Event::ParticleNotify { .. } => "particleNotify",
            Event::SubMeshAnim { .. } => "subMeshAnim",
            Event::PulseHit { .. } => "pulseHit",
            Event::AbilityUiCreate { .. } => "abilityUiCreate",
            Event::DisguiseFx { .. } => "disguiseFx",
            Event::HitVfx { .. } => "hitVfx",
        }
    }
}
