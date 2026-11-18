//! Every Blueprint node class the bots' trees name (the shared tasks, decorators and services, and the characters'
//! own tasks): a node's state is the variant of its class, found by the tree's `type` once,
//! when the node instance is made. A class not listed is one nobody ported: the task fails, the decorator reads
//! false, the service does nothing, and the runtime names it once. The engine's own classes (BTTask_Wait,
//! BTTask_RunBehavior, BTDecorator_Blackboard, the composites ...) are the runtime's, not listed here.

use crate::Real;
use crate::ai::cx::{Cx, Dec, Res, Resume, Step, Svc, Task};

/// `bt_classes!`: the state enum of one node kind over its classes' structs, each entry `"<class>" => <Variant>(<the
/// struct's path>)` (an entry may carry attributes: a test's own classes are `#[cfg(test)]`): `of` finds a class by
/// its name, `class` gives it back, and the kind's trait methods dispatch to the variant's struct.
macro_rules! bt_classes {
    (@enum $(#[$m:meta])* $Enum:ident { $( $(#[$a:meta])* $class:literal => $Var:ident($ty:path) ),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug)]
        pub enum $Enum {
            $( $(#[$a])* $Var($ty), )*
        }

        impl $Enum {
            /// The class of a tree node's `type`, in its starting state; `None` for a class not ported.
            pub fn of(class: &str) -> Option<$Enum> {
                match class {
                    $( $(#[$a])* $class => Some($Enum::$Var(<$ty>::default())), )*
                    _ => None,
                }
            }

            /// The class's name.
            pub fn class(&self) -> &'static str {
                match self {
                    $( $(#[$a])* $Enum::$Var(_) => $class, )*
                }
            }
        }
    };
    ($(#[$m:meta])* task $Enum:ident { $( $(#[$a:meta])* $class:literal => $Var:ident($ty:path) ),* $(,)? }) => {
        bt_classes!(@enum $(#[$m])* $Enum { $( $(#[$a])* $class => $Var($ty) ),* });

        impl $Enum {
            /// [`Task::OWN_STALE`].
            pub fn own_stale(&self) -> bool {
                match self {
                    $( $(#[$a])* $Enum::$Var(_) => <$ty as Task>::OWN_STALE, )*
                }
            }

            /// [`Task::run`].
            pub fn run(&mut self, cx: &mut Cx, r: Resume) -> Step {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.run(cx, r), )*
                }
            }

            /// [`Task::tick`].
            pub fn tick(&mut self, cx: &mut Cx, dt: Real) -> Option<bool> {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.tick(cx, dt), )*
                }
            }

            /// [`Task::abort`].
            pub fn abort(&mut self, cx: &mut Cx) {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.abort(cx), )*
                }
            }
        }
    };
    ($(#[$m:meta])* dec $Enum:ident { $( $(#[$a:meta])* $class:literal => $Var:ident($ty:path) ),* $(,)? }) => {
        bt_classes!(@enum $(#[$m])* $Enum { $( $(#[$a])* $class => $Var($ty) ),* });

        impl $Enum {
            /// [`Dec::test`].
            pub fn test(&mut self, cx: &mut Cx) -> bool {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.test(cx), )*
                }
            }

            /// [`Dec::start`].
            pub fn start(&mut self, cx: &mut Cx) {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.start(cx), )*
                }
            }

            /// [`Dec::finish`].
            pub fn finish(&mut self, cx: &mut Cx, res: Res) {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.finish(cx, res), )*
                }
            }

            /// [`Dec::tick`].
            pub fn tick(&mut self, cx: &mut Cx, dt: Real) {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.tick(cx, dt), )*
                }
            }
        }
    };
    ($(#[$m:meta])* svc $Enum:ident { $( $(#[$a:meta])* $class:literal => $Var:ident($ty:path) ),* $(,)? }) => {
        bt_classes!(@enum $(#[$m])* $Enum { $( $(#[$a])* $class => $Var($ty) ),* });

        impl $Enum {
            /// [`Svc::tick`].
            pub fn tick(&mut self, cx: &mut Cx, dt: Real) {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.tick(cx, dt), )*
                }
            }

            /// [`Svc::activate`].
            pub fn activate(&mut self, cx: &mut Cx) {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.activate(cx), )*
                }
            }

            /// [`Svc::deactivate`].
            pub fn deactivate(&mut self, cx: &mut Cx) {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.deactivate(cx), )*
                }
            }

            /// [`Svc::search_start`].
            pub fn search_start(&mut self, cx: &mut Cx) {
                match self {
                    $( $(#[$a])* $Enum::$Var(s) => s.search_start(cx), )*
                }
            }
        }
    };
}

bt_classes! {
    /// A Blueprint task node's state (`TaskSt`): the shared tasks, then the
    /// characters' own (`chars/<key>/bots.rs`).
    task TaskSt {
    "BotAttack_BTT_C" => BotAttack(crate::ai::tasks::BotAttack),
    "BotAttackForward_BTT_C" => BotAttackForward(crate::ai::tasks::BotAttackForward),
    "BotSpecialForward_BTT_C" => BotSpecialForward(crate::ai::tasks::BotSpecialForward),
    "C019_RushAttack_BTT_C" => C019RushAttack(crate::ai::tasks::C019RushAttack),
    "C017_Cannonball_BTT_C" => C017Cannonball(crate::ai::tasks::C017Cannonball),
    "BotAttackWithAbort_BTT_C" => BotAttackWithAbort(crate::ai::tasks::BotAttackWithAbort),
    "BotHeavyAttack_BTT_C" => BotHeavyAttack(crate::ai::tasks::BotHeavyAttack),
    "BotStruggle_BTT_C" => BotStruggle(crate::ai::tasks::BotStruggle),
    "BotDownSpike_BTT_C" => BotDownSpike(crate::ai::tasks::BotDownSpike),
    "BotSpecial_BTT_C" => BotSpecial(crate::ai::tasks::BotSpecial),
    "BotUpSpecial_BTT_C" => BotUpSpecial(crate::ai::tasks::BotUpSpecial),
    "BotUpSpecialToPoint_BTT_C" => BotUpSpecialToPoint(crate::ai::tasks::BotUpSpecialToPoint),
    "BotSideSpecialToActor_BTT_C" => BotSideSpecialToActor(crate::ai::tasks::BotSideSpecialToActor),
    "BotSideSpecialToPoint_BTT_C" => BotSideSpecialToPoint(crate::ai::tasks::BotSideSpecialToPoint),
    "BotShortJump_BTT_C" => BotShortJump(crate::ai::tasks::BotShortJump),
    "BotJumpUp_BTT_C" => BotJumpUp(crate::ai::tasks::BotJumpUp),
    "BotDropThroughPlatform_BTT_C" => BotDropThroughPlatform(crate::ai::tasks::BotDropThroughPlatform),
    "BotJumpRandomDirection_BTT_C" => BotJumpRandomDirection(crate::ai::tasks::BotJumpRandomDirection),
    "BotJumpForward_BTT_C" => BotJumpForward(crate::ai::tasks::BotJumpForward),
    "BotJumpBackward_BTT_C" => BotJumpBackward(crate::ai::tasks::BotJumpBackward),
    "BotJumpToTarget_BTT_C" => BotJumpToTarget(crate::ai::tasks::BotJumpToTarget),
    "BotJumpToPoint_BTT_C" => BotJumpToPoint(crate::ai::tasks::BotJumpToPoint),
    "BotJumpToCenter_BTT_C" => BotJumpToCenter(crate::ai::tasks::BotJumpToCenter),
    "BotReleaseJumpButton_BTT_C" => BotReleaseJumpButton(crate::ai::tasks::BotReleaseJumpButton),
    "BotFastFall_BTT_C" => BotFastFall(crate::ai::tasks::BotFastFall),
    "BotCrouch_BTT_C" => BotCrouch(crate::ai::tasks::BotCrouch),
    "BotCrouchUnderProjectile_BTT_C" => BotCrouchUnderProjectile(crate::ai::tasks::BotCrouchUnderProjectile),
    "BotTaunt_BTT_C" => BotTaunt(crate::ai::tasks::BotTaunt),
    "BotExecutePause_BTT_C" => BotExecutePause(crate::ai::tasks::BotExecutePause),
    "BotPickUpItem_BTT_C" => BotPickUpItem(crate::ai::tasks::BotPickUpItem),
    "BotThrowItemAtAnyEnemy_C" => BotThrowItemAtAnyEnemy(crate::ai::tasks::BotThrowItemAtAnyEnemy),
    "BotBeCarried_BTT_C" => BotBeCarried(crate::ai::tasks::BotBeCarried),
    "ClearBlackboardValue_BTT_C" => ClearBlackboardValue(crate::ai::tasks::ClearBlackboardValue),
    "DifficultyCancel_BTT_C" => DifficultyCancel(crate::ai::tasks::DifficultyCancel),
    "DifficultyDelay_BTT_C" => DifficultyDelay(crate::ai::tasks::DifficultyDelay),
    "DifficultyPause_BTD_C" => DifficultyPause(crate::ai::tasks::DifficultyPause),
    "WaitUntilOnGround_BTD_C" => WaitUntilOnGround(crate::ai::tasks::WaitUntilOnGround),
    "BotDI_BTD_C" => BotDI(crate::ai::tasks::BotDI),
    "UpdateTargetPosition_BTT_C" => UpdateTargetPosition(crate::ai::tasks::UpdateTargetPosition),
    "BotEvadeDown_BTT_C" => BotEvadeDown(crate::ai::tasks::BotEvadeDown),
    "BotEvadeUp_BTT_C" => BotEvadeUp(crate::ai::tasks::BotEvadeUp),
    "BotEvadeNeutral_BTT_C" => BotEvadeNeutral(crate::ai::tasks::BotEvadeNeutral),
    "BotEvadeForward_BTT_C" => BotEvadeForward(crate::ai::tasks::BotEvadeForward),
    "BotEvadeToCenter_BTT_C" => BotEvadeToCenter(crate::ai::tasks::BotEvadeToCenter),
    "BotEvadeToPoint_BTT_C" => BotEvadeToPoint(crate::ai::tasks::BotEvadeToPoint),
    "BotEvadeToTarget_BTT_C" => BotEvadeToTarget(crate::ai::tasks::BotEvadeToTarget),
    "BotEvadeAwayFromTarget_BTT_C" => BotEvadeAwayFromTarget(crate::ai::tasks::BotEvadeAwayFromTarget),
    "BotEvadeRandomDirection_BTT_C" => BotEvadeRandomDirection(crate::ai::tasks::BotEvadeRandomDirection),
    "BotEvadeJumpFlee_BTD_C" => BotEvadeJumpFlee(crate::ai::tasks::BotEvadeJumpFlee),
    "ChooseMovementMode_BTT_C" => ChooseMovementMode(crate::ai::tasks::ChooseMovementMode),
    "SetTargetPointForSuicide_BTT_C" => SetTargetPointForSuicide(crate::ai::tasks::SetTargetPointForSuicide),
    "BotMoveUp_BTT_C" => BotMoveUp(crate::ai::tasks::BotMoveUp),
    "BotMoveDown_BTT_C" => BotMoveDown(crate::ai::tasks::BotMoveDown),
    "BotMoveForward_BTT_C" => BotMoveForward(crate::ai::tasks::BotMoveForward),
    "BotMoveSide_BTT_C" => BotMoveSide(crate::ai::tasks::BotMoveSide),
    "BotMoveRandomSide_BTT_C" => BotMoveRandomSide(crate::ai::tasks::BotMoveRandomSide),
    "BotMoveToward_BTT_C" => BotMoveToward(crate::ai::tasks::BotMoveToward),
    "BotStop_BTT_C" => BotStop(crate::ai::tasks::BotStop),
    "BotTurnToward_BTT_C" => BotTurnToward(crate::ai::tasks::BotTurnToward),
    "BotTurnAwayFromEnemy_BTT_C" => BotTurnAwayFromEnemy(crate::ai::tasks::BotTurnAwayFromEnemy),
    "BotTurnAround_BTT_C" => BotTurnAround(crate::ai::tasks::BotTurnAround),
    "BotFlee_BTT_C" => BotFlee(crate::ai::tasks::BotFlee),
    "BotMoveTo_BTT_C" => BotMoveTo(crate::ai::moves::BotMoveTo),
    "BotPathToPoint_BTT_C" => BotPathToPoint(crate::ai::moves::BotPathToPoint),
    "FindClosestWaypointToActor_BTT_C" => FindClosestWaypointToActor(crate::ai::moves::FindClosestWaypointToActor),
    "FindFurthestWaypointFromActor_BTT_C" => FindFurthestWaypointFromActor(crate::ai::moves::FindFurthestWaypointFromActor),
    "FindRandomWaypoint_BTT_C" => FindRandomWaypoint(crate::ai::moves::FindRandomWaypoint),
    "SetWaypointForRecoveryOnly_BTT_C" => SetWaypointForRecoveryOnly(crate::ai::moves::SetWaypointForRecoveryOnly),
    "GetSafeAttackPoint_BTT_C" => GetSafeAttackPoint(crate::ai::moves::GetSafeAttackPoint),
    "BatmanThrowBatarang_BTT_C" => BatmanThrowBatarang(crate::chars::batman::bots::BatmanThrowBatarang),
    "BatmanHookshot_BTT_C" => BatmanHookshot(crate::chars::batman::bots::BatmanHookshot),
    "BatmanHookshotForward_BTT_C" => BatmanHookshotForward(crate::chars::batman::bots::BatmanHookshotForward),
    "Finn_RushAttack_BTT_C" => FinnRushAttack(crate::chars::finn::bots::FinnRushAttack),
    "Finn_OpenBag_BTT_C" => FinnOpenBag(crate::chars::finn::bots::FinnOpenBag),
    "Finn_BuySomething_BTT_C" => FinnBuySomething(crate::chars::finn::bots::FinnBuySomething),
    "Finn_CloseBag_BTT_C" => FinnCloseBag(crate::chars::finn::bots::FinnCloseBag),
    "GarnetGauntlet_BTT_C" => GarnetGauntlet(crate::chars::garnet::bots::GarnetGauntlet),
    "GarnetSing_BTT_C" => GarnetSing(crate::chars::garnet::bots::GarnetSing),
    "C023A_ShootArrow_BTT_C" => C023AShootArrow(crate::chars::gizmo::bots::C023AShootArrow),
    "JakeStretchPunch_BTT_C" => JakeStretchPunch(crate::chars::jake::bots::JakeStretchPunch),
    "JakeSkateboard_BTT_C" => JakeSkateboard(crate::chars::jake::bots::JakeSkateboard),
    "StevenUpSpecialRecovery_BTT_C" => StevenUpSpecialRecovery(crate::chars::steven::bots::StevenUpSpecialRecovery),
    "Steven_AbortShield_BTT_C" => StevenAbortShield(crate::chars::steven::bots::StevenAbortShield),
    "WatermelonStevenUpdateTarget_BTT_C" => WatermelonStevenUpdateTarget(crate::chars::steven::bots::WatermelonStevenUpdateTarget),
    "WatermelonStevenEvade_BTT_C" => WatermelonStevenEvade(crate::chars::steven::bots::WatermelonStevenEvade),
    "SupermanFlyToPoint_BTT_C" => SupermanFlyToPoint(crate::chars::superman::bots::SupermanFlyToPoint),
    "SupermanFlyGrab_BTT_C" => SupermanFlyGrab(crate::chars::superman::bots::SupermanFlyGrab),
    "SupermanSuperpunch_BTT_C" => SupermanSuperpunch(crate::chars::superman::bots::SupermanSuperpunch),
    "TomAndJerry_GuideRocket_BTT_C" => TomAndJerryGuideRocket(crate::chars::tom_and_jerry::bots::TomAndJerryGuideRocket),
    "TomAndJerry_FireSlingshot_BTT_C" => TomAndJerryFireSlingshot(crate::chars::tom_and_jerry::bots::TomAndJerryFireSlingshot),
    "TomAndJerry_TennisBallReturn_BTT_C" => TomAndJerryTennisBallReturn(crate::chars::tom_and_jerry::bots::TomAndJerryTennisBallReturn),
    "VelmaBeam_BTT_C" => VelmaBeam(crate::chars::velma::bots::VelmaBeam),
    "Velma_CarryBTT_C" => VelmaCarry(crate::chars::velma::bots::VelmaCarry),
    #[cfg(test)] "T_Ok" => TOk(crate::ai::runner_tests::TOk),
    #[cfg(test)] "T_Fail" => TFail(crate::ai::runner_tests::TFail),
    #[cfg(test)] "T_Hang" => THang(crate::ai::runner_tests::THang),
    #[cfg(test)] "T_Delay" => TDelay(crate::ai::runner_tests::TDelay),
    #[cfg(test)] "T_Two" => TTwo(crate::ai::runner_tests::TTwo),
    #[cfg(test)] "T_FlagOff" => TFlagOff(crate::ai::runner_tests::TFlagOff),
    #[cfg(test)] "T_Blink" => TBlink(crate::ai::runner_tests::TBlink),
    }
}

bt_classes! {
    /// A Blueprint decorator node's state (`DecSt`).
    dec DecSt {
    "NotEasy_BTD_C" => NotEasy(crate::ai::decorators::NotEasy),
    "NotVeryEasy_BTD_C" => NotVeryEasy(crate::ai::decorators::NotVeryEasy),
    "BotDifficulty_BTD_C" => BotDifficulty(crate::ai::decorators::BotDifficulty),
    "DifficultyCooldown_C" => DifficultyCooldown(crate::ai::decorators::DifficultyCooldown),
    "TimedChance_BTD_C" => TimedChance(crate::ai::decorators::TimedChance),
    "PersonaTimedChance_BTD_C" => PersonaTimedChance(crate::ai::decorators::PersonaTimedChance),
    "BotIsFleeing_BTD_C" => BotIsFleeing(crate::ai::decorators::BotIsFleeing),
    "BotIsFleeSuppressed_BTD_C" => BotIsFleeSuppressed(crate::ai::decorators::BotIsFleeSuppressed),
    "ShouldPause_BTD_C" => ShouldPause(crate::ai::decorators::ShouldPause),
    "ActorOlderThan_BTD_C" => ActorOlderThan(crate::ai::decorators::ActorOlderThan),
    "IsMatchOver_BTD_C" => IsMatchOver(crate::ai::decorators::IsMatchOver),
    "OnGround_BTD_C" => OnGround(crate::ai::decorators::OnGround),
    "WallSliding_BTD_C" => WallSliding(crate::ai::decorators::WallSliding),
    "TargetIsAlive_BTD_C" => TargetIsAlive(crate::ai::decorators::TargetIsAlive),
    "TargetIsAttacking_BTD_C" => TargetIsAttacking(crate::ai::decorators::TargetIsAttacking),
    "TargetHasBuff_BTD_C" => TargetHasBuff(crate::ai::decorators::TargetHasBuff),
    "DamageRequired_BTD_C" => DamageRequired(crate::ai::decorators::DamageRequired),
    "BotDamageGreaterThan_BTD_C" => BotDamageGreaterThan(crate::ai::decorators::BotDamageGreaterThan),
    "BotIsEvading_BTD_C" => BotIsEvading(crate::ai::decorators::BotIsEvading),
    "BotIsFalling_BTD_C" => BotIsFalling(crate::ai::decorators::BotIsFalling),
    "BotHasEvade_BTD_C" => BotHasEvade(crate::ai::decorators::BotHasEvade),
    "BotHasAllEvades_BTD_C" => BotHasAllEvades(crate::ai::decorators::BotHasAllEvades),
    "BotHasAttack_BTD_C" => BotHasAttack(crate::ai::decorators::BotHasAttack),
    "BotHasJump_BTD_C" => BotHasJump(crate::ai::decorators::BotHasJump),
    "BotNotHitstunned_BTD_C" => BotNotHitstunned(crate::ai::decorators::BotNotHitstunned),
    "BotRecentlyHitstunned_BTD_C" => BotRecentlyHitstunned(crate::ai::decorators::BotRecentlyHitstunned),
    "BotNeedsToStruggle_BTD_C" => BotNeedsToStruggle(crate::ai::decorators::BotNeedsToStruggle),
    "BotIsAbilityAvailable_BTD_C" => BotIsAbilityAvailable(crate::ai::decorators::BotIsAbilityAvailable),
    "ShaggyEnraged_BTD_C" => ShaggyEnraged(crate::ai::decorators::ShaggyEnraged),
    "ShaggyHasSandwich_BTD_C" => ShaggyHasSandwich(crate::ai::decorators::ShaggyHasSandwich),
    "C016_BallEnabled_BTD_C" => C016BallEnabled(crate::ai::decorators::C016BallEnabled),
    "ShaggyIsChargingRage_BTD_C" => ShaggyIsChargingRage(crate::ai::decorators::ShaggyIsChargingRage),
    "C017_HasFuel_BTD_C" => C017HasFuel(crate::ai::decorators::C017HasFuel),
    "C017_IsHoldingEnemy_BTD_C" => C017IsHoldingEnemy(crate::ai::decorators::C017IsHoldingEnemy),
    "C017_IsMaxRage_BTD_C" => C017IsMaxRage(crate::ai::decorators::C017IsMaxRage),
    "C017_WeaponMode_BTD_C" => C017WeaponMode(crate::ai::decorators::C017WeaponMode),
    "Arya_CanTeleport_BTD_C" => AryaCanTeleport(crate::ai::decorators::AryaCanTeleport),
    "BTD_BatmanLostBatarang_C" => BatmanLostBatarang(crate::ai::decorators::BatmanLostBatarang),
    "HarleyHasBomb_BTD_C" => HarleyHasBomb(crate::ai::decorators::HarleyHasBomb),
    "HarleyThrownBomb_BTD_C" => HarleyThrownBomb(crate::ai::decorators::HarleyThrownBomb),
    "HarleyBombAttachedToEnemy_BTD_C" => HarleyBombAttachedToEnemy(crate::ai::decorators::HarleyBombAttachedToEnemy),
    "OnPlatform_BTD_C" => OnPlatform(crate::ai::decorators::OnPlatform),
    "ShouldClimbWall_BTD_C" => ShouldClimbWall(crate::ai::decorators::ShouldClimbWall),
    "IsBotInNeverRecoverZone_C" => IsBotInNeverRecoverZone(crate::ai::decorators::IsBotInNeverRecoverZone),
    "CanCrouchUnderProjectile_BTD_C" => CanCrouchUnderProjectile(crate::ai::decorators::CanCrouchUnderProjectile),
    "IsHoldingMeeseeksBox_BTD_C" => IsHoldingMeeseeksBox(crate::ai::decorators::IsHoldingMeeseeksBox),
    "IsBugsPortal_BTD_C" => IsBugsPortal(crate::ai::decorators::IsBugsPortal),
    "Bugs_IsTunnelingBTD_C" => BugsIsTunneling(crate::ai::decorators::BugsIsTunneling),
    "Bugs_PieEquipped_BTD_C" => BugsPieEquipped(crate::ai::decorators::BugsPieEquipped),
    "HorizontalSpeedBelow_BTD_C" => HorizontalSpeedBelow(crate::ai::decorators::HorizontalSpeedBelow),
    "OnBugsRocket_BTD_C" => OnBugsRocket(crate::ai::decorators::OnBugsRocket),
    "ReindogUsingSpeedBoost_BTD_C" => ReindogUsingSpeedBoost(crate::ai::decorators::ReindogUsingSpeedBoost),
    "ReindogUsingTether_BTD_C" => ReindogUsingTether(crate::ai::decorators::ReindogUsingTether),
    "ReindogTetherAttachedToAlly_BTD_C" => ReindogTetherAttachedToAlly(crate::ai::decorators::ReindogTetherAttachedToAlly),
    "Reindog_AllyItemIsPickedUp_BTD_C" => ReindogAllyItemIsPickedUp(crate::ai::decorators::ReindogAllyItemIsPickedUp),
    "Reindog_AllyItemReadyToBePickedUp_BTD_C" => ReindogAllyItemReadyToBePickedUp(crate::ai::decorators::ReindogAllyItemReadyToBePickedUp),
    "TargetInAttackRange_BTD_C" => TargetInAttackRange(crate::ai::decorators::TargetInAttackRange),
    "TargetInRange_BTT_C" => TargetInRange(crate::ai::decorators::TargetInRange),
    "TargetInRangeOfOther_BTT_C" => TargetInRangeOfOther(crate::ai::decorators::TargetInRangeOfOther),
    "TargetInHorizontalRange_BTT_C" => TargetInHorizontalRange(crate::ai::decorators::TargetInHorizontalRange),
    "TargetInCone_BTD_C" => TargetInCone(crate::ai::decorators::TargetInCone),
    "PointInRange_BTT_C" => PointInRange(crate::ai::decorators::PointInRange),
    "TargetInBoxRange_BTD_C" => TargetInBoxRange(crate::ai::decorators::TargetInBoxRange),
    "TargetInVerticalColumn_BTD_C" => TargetInVerticalColumn(crate::ai::decorators::TargetInVerticalColumn),
    "TargetInRotatedVerticalColumn_BTD_C" => TargetInRotatedVerticalColumn(crate::ai::decorators::TargetInRotatedVerticalColumn),
    "TargetBelowMe_BTD_C" => TargetBelowMe(crate::ai::decorators::TargetBelowMe),
    "TargetAboveMe_BTD_C" => TargetAboveMe(crate::ai::decorators::TargetAboveMe),
    "ActorInFrontOfMe_BTD_C" => ActorInFrontOfMe(crate::ai::decorators::ActorInFrontOfMe),
    "PointInFrontOfMe_BTD_C" => PointInFrontOfMe(crate::ai::decorators::PointInFrontOfMe),
    "ProjectileETALessThan_BTD_C" => ProjectileETALessThan(crate::ai::decorators::ProjectileETALessThan),
    "IsProjectileInbound_BTD_C" => IsProjectileInbound(crate::ai::decorators::IsProjectileInbound),
    "FindEnemyInRange_BTD_C" => FindEnemyInRange(crate::ai::decorators::FindEnemyInRange),
    "FindAllyNotOverPlatform_BTD_C" => FindAllyNotOverPlatform(crate::ai::decorators::FindAllyNotOverPlatform),
    "Steven_HasWatermelon_BTD_C" => StevenHasWatermelon(crate::ai::decorators::StevenHasWatermelon),
    "GarnetIsElectricityCharged_BTD_C" => GarnetIsElectricityCharged(crate::ai::decorators::GarnetIsElectricityCharged),
    "GarnetIsReadyToSing_BTD_C" => GarnetIsReadyToSing(crate::ai::decorators::GarnetIsReadyToSing),
    "TargetBetweenMeAndActor_BTD_C" => TargetBetweenMeAndActor(crate::ai::decorators::TargetBetweenMeAndActor),
    "BotGetSpawnedActorOfClass_BTD_C" => BotGetSpawnedActorOfClass(crate::ai::decorators::BotGetSpawnedActorOfClass),
    "Finn_HasTreasure_BTD_C" => FinnHasTreasure(crate::ai::decorators::FinnHasTreasure),
    "Finn_DashThrownTargetIsReady_BTD_C" => FinnDashThrownTargetIsReady(crate::ai::decorators::FinnDashThrownTargetIsReady),
    "C023B_HasBomb_C" => C023BHasBomb(crate::ai::decorators::C023BHasBomb),
    "C023B_IsAttached_C" => C023BIsAttached(crate::ai::decorators::C023BIsAttached),
    "BTD_JakeHasEatenEnemy_C" => JakeHasEatenEnemy(crate::ai::decorators::JakeHasEatenEnemy),
    "TomAndJerry_IsJerryOut_BTD_C" => TomAndJerryIsJerryOut(crate::ai::decorators::TomAndJerryIsJerryOut),
    "TargetBetweenMeAndJerry_BTD_C" => TargetBetweenMeAndJerry(crate::ai::decorators::TargetBetweenMeAndJerry),
    "TomAndJerry_TennisBallInPlay_C" => TomAndJerryTennisBallInPlay(crate::ai::decorators::TomAndJerryTennisBallInPlay),
    "Velma_IsCarryRunning_BTD_C" => VelmaIsCarryRunning(crate::ai::decorators::VelmaIsCarryRunning),
    "VelmaPolice_HasPerformedArrest_BTD_C" => VelmaPoliceHasPerformedArrest(crate::ai::decorators::VelmaPoliceHasPerformedArrest),
    "VelmaPolice_InInitialDelay_BTD_C" => VelmaPoliceInInitialDelay(crate::ai::decorators::VelmaPoliceInInitialDelay),
    "HasBlastBoxBufferTopClearance_BTD_C" => HasBlastBoxBufferTopClearance(crate::ai::decorators::HasBlastBoxBufferTopClearance),
    "HasBlastBoxBottomClearance_BTD_C" => HasBlastBoxBottomClearance(crate::ai::decorators::HasBlastBoxBottomClearance),
    "HasBlastBoxSideClearance_BTD_C" => HasBlastBoxSideClearance(crate::ai::decorators::HasBlastBoxSideClearance),
    "BotNotOverPlatform_BTD_C" => BotNotOverPlatform(crate::ai::decorators::BotNotOverPlatform),
    "BotOverPlatform_BTD_C" => BotOverPlatform(crate::ai::decorators::BotOverPlatform),
    "RaycastTowardPoint_BTD_C" => RaycastTowardPoint(crate::ai::decorators::RaycastTowardPoint),
    "RaycastForward_BTD_C" => RaycastForward(crate::ai::decorators::RaycastForward),
    #[cfg(test)] "D_Flag" => DFlag(crate::ai::runner_tests::DFlag),
    }
}

bt_classes! {
    /// A Blueprint service node's state (`SvcSt`).
    svc SvcSt {
    "VelmaPolice_DetectEnemy_BTS_C" => VelmaPoliceDetectEnemy(crate::ai::services::VelmaPoliceDetectEnemy),
    "UpdateTargetPosition_BTS_C" => UpdateTargetPosition(crate::ai::services::UpdateTargetPosition),
    "BotUpdateDifficulty_BTS_C" => BotUpdateDifficulty(crate::ai::services::BotUpdateDifficulty),
    "UpdateFleeTime_BTS_C" => UpdateFleeTime(crate::ai::services::UpdateFleeTime),
    "UpdateRecoveryPoint_BTS_C" => UpdateRecoveryPoint(crate::ai::services::UpdateRecoveryPoint),
    "BotDetectEnemy_BTS_C" => BotDetectEnemy(crate::ai::services::BotDetectEnemy),
    #[cfg(test)] "S_Count" => SCount(crate::ai::runner_tests::SCount),
    }
}
