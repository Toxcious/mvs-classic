//! The ui kind's tables: the widgets the port shows and the textures and materials its Blueprints set at run time
//! (bytecode literals no tree holds). The tables are written out by hand. The paths are package paths below
//! [`PANDA_MAIN`] unless they begin with a `/`.

pub use super::PANDA_MAIN;

/// A character's own HUD textures and widgets: `textures` the ones it draws (below `PANDA_MAIN`), `widgets` the HUD
/// widgets only it shows, `undrawn` the brushes of those trees its Blueprint replaces before the first draw
/// (`CharTextures`).
pub struct CharTextures {
    pub textures: &'static [&'static str],
    pub widgets: &'static [&'static str],
    pub undrawn: &'static [&'static str],
}

/// The HUD bundle's widget packages.
pub const HUD_WIDGETS: &[&str] = &[
    "UI/In-Game/v3/UI_IGv3_AttachedBottom",
    "UI/In-Game/v3/UI_IGv3_AttachedTop",
    "UI/In-Game/v3/UI_IGv3_Percentage",
    "UI/In-Game/v3/UI_IGv3_PlayerMarker",
    "UI/In-Game/v3/UI_IGv3_DodgeMeter",
    "UI/In-Game/v3/UI_IGv3_Username",
    "UI/In-Game/v3/UI_IGv3_Flame",
    "UI/In-Game/v3/UI_IGv3_Charge",
    "Characters/Finn/UI/UI_FinnCharge",
    "Characters/Finn/UI/UI_Finn_DashTarget",
    "Characters/Finn/UI/UI_Finn_Shop_Cancel",
    "Characters/Finn/UI/UI_Finn_Shop_Boots",
    "Characters/Finn/UI/UI_Finn_Shop_Armor_Buns",
    "Characters/Finn/UI/UI_Finn_Shop_BMO",
    "UI/In-Game/v3/UI_IGv3_Cooldowns",
    "UI/In-Game/v3/UI_IGv3_Cooldown",
    "UI/In-Game/v3/UI_IGv3_States",
    "UI/In-Game/v3/UI_IGv3_State",
    "UI/In-Game/v3/UI_IGv3_Leader",
    "UI/In-Game/v3/UI_IGv3_Struggle",
    "UI/In-Game/v3/UI_IGv3_Scoreboard",
    "UI/In-Game/v3/UI_IGv3_Fixed",
    "UI/In-Game/v3/UI_IGv3_CharacterPortrait",
    "UI/In-Game/v3/UI_IGv3_PlayerScore",
    "UI/In-Game/v3/UI_IGv3_Clock",
    "UI/In-Game/v3/UI_IGv3_MatchInfo",
    "UI/In-Game/v3/UI_IGv3_ScorePips",
    "UI/In-Game/v3/UI_IGv3_ScorePip",
    "UI/SkillShots/UI_Skillshot_Aimpunch_Target",
    "UI/Prototype/HUD/UI_ItemIndicator",
    "UI/Prototype/HUD/UI_Statuses",
    "UI/Prototype/HUD/UI_Status",
    "UI/In-Game/v3/UI_IGv3_PauseHold",
    "UI/Prototype/Common/UI_PopText",
    "UI/Prototype/HUD/Countdown/UI_Countdown",
    "UI/Prototype/HUD/Countdown/UI_DetailedText",
    "UI/EndOfGame/UI_VictoryText",
    "UI/Prototype/Common/UI_PopInputLabel",
    "UI/Prototype/Common/UI_InputLabel",
    "Materials/Halftone/WBP_Quadrilateral",
    "UI/In-Game/MapSpecific/UI_SpaceJam_Scoreboard",
    "UI/In-Game/MapSpecific/UI_SpaceJam_ScoreboardTeam",
    "Characters/TomAndJerry/UI_JerryCorkCount",
    "Characters/C023B/BluePrints/UI_C023B_Target",
];

/// PFG composite font faces the HUD uses (FontFace asset -> its font file below `PANDA_MAIN`).
pub const FONTS: &[(&str, &str)] = &[
    ("TT_Norms_Pro_ExtraBold_Italic", "UI/Fonts/Norms/TT_Norms_Pro_ExtraBold_Italic.ufont"),
    ("TT_Norms_Std_Condensed_ExtraBold_Italic", "UI/Fonts/Norms/TT_Norms_Std_Condensed_ExtraBold_Italic.ufont"),
    ("TT_Norms_Std_Condensed_Medium", "UI/Fonts/Norms/TT_Norms_Std_Condensed_Medium.ufont"),
    ("TT_Norms_Std_Condensed_Medium_Italic", "UI/Fonts/Norms/TT_Norms_Std_Condensed_Medium_Italic.ufont"),
    ("TT_Norms_Pro_Bold", "UI/Fonts/Norms/TT_Norms_Pro_Bold.ufont"),
    ("TT_Bluescreens_ExtraBold_Italic", "UI/Fonts/Bluescreens/TT_Bluescreens_ExtraBold_Italic.ufont"),
];

pub const HUD_TEXTURES: &[&str] = &["UI/In-Game/Images/UI_MeTriangle"];

pub const MENU_TEXTURES: &[&str] = &[
    "UI/PreMatch/Images/Perks/ui_perkbg_attack",
    "UI/PreMatch/Images/Perks/ui_perkbg_defense",
    "UI/PreMatch/Images/Perks/ui_perkbg_utility",
    "UI/In-Game/LivePreview/UI_Settings_HUD_AllyNoOutline",
    "UI/In-Game/LivePreview/UI_Settings_HUD_AllyOutline",
    "UI/In-Game/LivePreview/UI_Settings_HUD_Enemy1NoOutline",
    "UI/In-Game/LivePreview/UI_Settings_HUD_Enemy1Outline",
    "UI/In-Game/LivePreview/UI_Settings_HUD_Enemy2NoOutline",
    "UI/In-Game/LivePreview/UI_Settings_HUD_Enemy2Outline",
];

pub const ABILITY_ICONS: &[&str] = &[
    "Characters/C019/UI/UI_AbilityIcons_C019_Blaster",
    "Characters/C019/UI/UI_AbilityIcons_C019_Grenade",
    "Characters/C019/UI/UI_AbilityIcons_C019_Silhouette",
    "Characters/C019/UI/UI_AbilityIcons_C019_Ship",
    "Characters/C019/UI/UI_AbilityIcons_C019_Portal",
];

/// What a character's own HUD draws and no other's (its textures are soft), by key.
pub const CHAR_TEXTURES: &[(&str, CharTextures)] = &[
    (
        "irongiant",
        CharTextures {
            textures: &[
                "Characters/C017/UI/C017_Bolt_UI",
                "Characters/C017/UI/C017_Bomb_UI",
                "Characters/C017/UI/C017_Cannon_UI",
                "Characters/C017/UI/C017_Plasma_UI",
                "Characters/C017/UI/C017_Shock_UI",
                "Characters/C017/UI/C017_Rocket_UI",
                "Characters/C017/UI/C017_Rage_UI",
            ],
            widgets: &["UI_FinnCharge"],
            undrawn: &["UI_icon_finnrun", "ui_icons_search"],
        },
    ),
    (
        "wonderwoman",
        CharTextures {
            textures: &[
                "Characters/WonderWomanV2/UI/UI_AbilityIcons_WW_Whip",
                "Characters/WonderWomanV2/UI/UI_AbilityIcons_WW_Shield",
                "Characters/WonderWomanV2/UI/UI_WW_ShieldCharge",
            ],
            widgets: &["UI_FinnCharge"],
            undrawn: &["UI_icon_finnrun", "ui_icons_search"],
        },
    ),
    (
        "harley",
        CharTextures {
            textures: &[
                "Characters/HarleyQuinn/UI/UI_AbilityIcons_Harley_JackInTheBox",
                "Characters/HarleyQuinn/UI/UI_AbilityIcons_Harley_Bomb",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "superman",
        CharTextures {
            textures: &["Characters/Superman/UI/UI_AbilityIcons_C003_Laser"],
            widgets: &["UI_FinnCharge"],
            undrawn: &["UI_icon_finnrun", "ui_icons_search"],
        },
    ),
    (
        "lebron",
        CharTextures {
            textures: &["Characters/c016/UI/UI_AbilityIcons_C016_Basketball"],
            widgets: &["UI_Finn_DashTarget"],
            undrawn: &[],
        },
    ),
    (
        "finn",
        CharTextures {
            textures: &[
                "Characters/Finn/UI/UIAbilityIconFinn_Kick",
                "Characters/Finn/UI/UIAbilityIconFinn_Apple",
                "Characters/Finn/UI/UI_icon_finn_highfive",
                "Characters/Finn/UI/UI_Coin_01",
                "Characters/Finn/UI/UI_Coin_02",
                "Characters/Finn/UI/UI_Coin_03",
                "Characters/Finn/UI/Finn_Shop_Boot",
            ],
            widgets: &[
                "UI_FinnCharge",
                "UI_Finn_DashTarget",
                "UI_Finn_Shop_Cancel",
                "UI_Finn_Shop_Boots",
                "UI_Finn_Shop_Armor_Buns",
                "UI_Finn_Shop_BMO",
            ],
            undrawn: &["ui_icons_search"],
        },
    ),
    (
        "arya",
        CharTextures {
            textures: &[
                "Characters/BatmanV2/UI/UI_AbilityIcons_Batman_Batarang",
                "Characters/Arya/UI/UI_AbilityIcons_Arya_Sharpen",
                "Characters/Arya/UI/UI_AbilityIcons_Arya_Dagger",
                "Characters/Arya/UI/UI_AbilityIcons_Arya_FaceSteal",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "batman",
        CharTextures {
            textures: &[
                "Characters/BatmanV2/UI/UI_AbilityIcons_Batman_Batarang",
                "Characters/BatmanV2/UI/UI_AbilityIcons_Batman_Smoke",
                "Characters/BatmanV2/UI/UI_AbilityIcons_Batman_Bomb",
                "Characters/BatmanV2/UI/UI_AbilityIcons_Batman_Grapple",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "creature",
        CharTextures {
            textures: &[
                "Characters/Creature/UI/UI_AbilityIcons_Reindog_Tether",
                "Characters/Creature/UI/UI_AbilityIcons_Reindog_Fireball",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "blackadam",
        CharTextures {
            textures: &[
                "Characters/C021/UI/UI_AbilityIcons_C021_Icon3",
                "Characters/C021/UI/UI_AbilityIcons_C021_Icon1",
                "Characters/C021/UI/UI_AbilityIcons_C021_Icon2",
                "Characters/C021/UI/UI_AbilityIcons_C021_Icon6",
                "Characters/C021/UI/UI_AbilityIcons_C021_Icon4",
                "Characters/C021/UI/UI_AbilityIcons_C021_Icon7",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "bugs",
        CharTextures {
            textures: &[
                "Characters/BugsBunnyV2/UI/UI_AbilityIcons_Bugs_Rocket",
                "Characters/BugsBunnyV2/UI/UI_AbilityIcons_Bugs_Tunnel",
                "Characters/BugsBunnyV2/UI/UI_AbilityIcons_Bugs_Safe",
                "Characters/BugsBunnyV2/UI/UI_AbilityIcons_Bugs_Pie",
                "Characters/BugsBunnyV2/UI/UI_AbilityIcons_Bugs_AcmeBox",
                "Characters/BugsBunnyV2/UI/UI_AbilityIcons_Bugs_Heart",
                "UI/Prototype/Images/InputIcons/PC/PC-Icon-ARROW-DOWN",
            ],
            widgets: &["UI_ItemIndicator", "UI_Statuses", "UI_Status"],
            undrawn: &[],
        },
    ),
    (
        "rick",
        CharTextures {
            textures: &[
                "Characters/C020/UI/UI_AbilityIcons_C020_Portal",
                "Characters/C020/UI/UI_AbilityIcons_C020_Polymorph",
                "Characters/C020/UI/UI_AbilityIcons_C020_Megaseed",
                "Characters/C020/UI/UI_AbilityIcons_C020_Meeseeks",
                "Characters/C020/UI/UI_AbilityIcons_C020_Fartbomb",
                "Characters/Superman/UI/UI_AbilityIcons_C003_Laser",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "taz",
        CharTextures {
            textures: &[
                "Characters/Jake/UI/UI_AbilityIcons_Jake_Eat",
                "Characters/C015/UI/UI_AbilityIcons_C015_FightCloud",
                "Characters/C015/UI/UI_AbilityIcons_C015_Burp",
                "Characters/C015/UI/UI_AbilityIcons_C015_Apple",
                "Characters/C015/UI/UI_AbilityIcons_C015_Tornado",
                "Characters/C015/UI/UI_AbilityIcons_C015_BurpCloud",
                "Characters/C015/UI/UI_AbilityIcons_C015_BurpAnvil",
                "UI/Prototype/Images/InputIcons/PC/PC_Icon_Direction_3Way",
            ],
            widgets: &["UI_FinnCharge"],
            undrawn: &["UI_icon_finnrun", "ui_icons_search"],
        },
    ),
    (
        "steven",
        CharTextures {
            textures: &[
                "Characters/Steven/UI/UI_AbilityIcons_Steven_Shield",
                "Characters/Steven/UI/UI_AbilityIcons_Steven_Rose",
                "Characters/Steven/UI/UI_AbilityIcons_Steven_Watermelon",
                "Characters/Steven/UI/UI_AbilityIcons_Steven_BubbleShield",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "garnet",
        CharTextures {
            textures: &[
                "Characters/Garnet/UI/UI_AbilityIcons_Garnet_Fist",
                "Characters/Garnet/UI/UI_AbilityIcons_Garnet_Star",
                "Characters/Garnet/UI/UI_AbilityIcons_Garnet_Shock",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "tomandjerry",
        CharTextures {
            textures: &[
                "Characters/TomAndJerry/UI/UI_AbilityIcons_TJ_Trap",
                "Characters/TomAndJerry/UI/UI_AbilityIcons_TJ_Dynamite",
                "Characters/BugsBunnyV2/UI/UI_AbilityIcons_Bugs_Rocket",
                "Characters/TomAndJerry/UI/UI_AbilityIcons_TJ_TennisBall",
                "Characters/TomAndJerry/UI/UI_AbilityIcons_TJ_Jerry",
                "UI/SkillShots/Images/BL_TomAndJerry/JerryPointer",
            ],
            widgets: &["UI_JerryCorkCount"],
            undrawn: &[],
        },
    ),
    (
        "jake",
        CharTextures {
            textures: &[
                "Characters/Jake/UI/UI_AbilityIcons_Jake_Stretch",
                "Characters/Jake/UI/UI_AbilityIcons_Jake_Eat",
                "Characters/Jake/UI/UI_AbilityIcons_Jake_BounceHouse",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "stripe",
        CharTextures {
            textures: &[
                "Characters/C023B/UI/UI_AbilityIcons_C023B_WheelSaw",
                "Characters/C023B/UI/UI_AbilityIcons_C023B_Bullet",
                "Characters/C023B/UI/UI_AbilityIcons_C023B_Bomb",
            ],
            widgets: &["UI_C023B_Target"],
            undrawn: &[],
        },
    ),
    (
        "marvin",
        CharTextures {
            textures: &[
                "Characters/C018/UI/UI_AbilityIcons_C018_Bubble",
                "Characters/C018/UI/UI_AbilityIcons_C018_Flag",
                "Characters/C018/UI/UI_AbilityIcons_C018_Ship",
                "Characters/C018/UI/UI_AbilityIcons_C018_Swap",
                "Characters/C018/UI/UI_AbilityIcons_C018_Atom",
            ],
            widgets: &[],
            undrawn: &[],
        },
    ),
    (
        "velma",
        CharTextures {
            textures: &[
                "Characters/Velma/UI/UI_AbilityIcons_Velma_SpeechBubble",
                "Characters/Velma/UI/UI_AbilityIcons_Velma_Book",
                "Characters/Velma/UI/UI_AbilityIcons_Velma_Vial",
                "Characters/Velma/UI/UI_AbilityIcons_Velma_Megaphone",
                "Characters/WonderWomanV2/UI/UI_AbilityIcons_WW_Shield",
            ],
            widgets: &["UI_FinnCharge"],
            undrawn: &["UI_icon_finnrun"],
        },
    ),
    (
        "gizmo",
        CharTextures {
            textures: &[
                "Characters/C023A/UI/UI_AbilityIcons_C023A_BowAndArrow",
                "Characters/C023A/UI/UI_AbilityIcons_C023A_Car",
                "Characters/C023A/UI/UI_AbilityIcons_C023A_Popcorn",
                "Characters/C023A/UI/UI_AbilityIcons_C023A_Attach",
            ],
            widgets: &["UI_FinnCharge"],
            undrawn: &["UI_icon_finnrun", "ui_icons_search"],
        },
    ),
];

/// The menu bundle's screens; the user widgets inside their trees are added by the closure.
pub const MENU_ROOTS: &[&str] = &[
    "UI/MainMenu/Preferences/UI_PreferencesRework",
    "UI/Prototype/Foundation/UI_Cursor",
    // spawned at run time by UI_SectionBar (the Settings bar's button classes, the drop menus' item class)
    "UI/Prototype/Reskin/UI_SectionBarVertMenusRework",
    "UI/Prototype/Reskin/UI_SectionBarVertSubmenusRework",
    "UI/Prototype/Reskin/UI_SectionBarButtonMenuRework",
    // created at run time by UI_PreferencesPanelRework (the Controls and Graphics pages) and the binding rows
    "UI/MainMenu/Preferences/UI_PreferencesControlsRework",
    "UI/MainMenu/Preferences/UI_PreferencesGraphicsRework",
    // the Interface page (created by UI_PreferencesPanelRework @1347), its preset bar's button class (a class property
    // of the bar's instance) and the swatch class UI_ColorPicker Populate Colors @345 creates
    "UI/MainMenu/Preferences/UI_PreferencesInterfaceRework",
    "UI/Prototype/Reskin/UI_SectionBarHorizontalMenusRework",
    "UI/Prototype/Common/UI_ColorButton",
    "UI/InputBinding/UI_InputBindingSet",
    "UI/InputBinding/UI_InputBinding",
    // the title screen (the WelcomeScreen level Blueprint creates it)
    "UI/UI_WelcomeScreen",
    // the main menu (LobbyGameMode's HUD widget) and the button class its section bar spawns
    "UI/MainMenu/UI_MainMenu",
    "UI/MainMenu/GlobalNav/WBP_MainButtons",
    // spawned at run time by the main menu's section bar (its 'Vertical Button' class)
    "UI/MainMenu/GlobalNav/WBP_MainSectionButton",
    // created at run time by UI_Manager.ShowContextMenu (the drop menus' lists) and its item class
    "UI/Prototype/Common/UI_ContextMenu",
    "UI/Prototype/Common/UI_ContextButton",
    // the Custom Game Lobby (modal customGame) and the widgets its teams create at run time
    "UI/Prototype/Reskin/CustomGame/WBP_CustomGame",
    "UI/Prototype/Reskin/CustomGame/WBP_CustomGamePlayer",
    "UI/Prototype/Reskin/CustomGame/WBP_CustomGameMapThumbnail",
    // the end-of-game screen of a custom game: the step container, its results and NEXT steps,
    // and the podium banner's stat sheet with the item class it creates
    "UI/SteppedProgressionElements/UI_CanvasProgressionContainer",
    "UI/EndOfGame/UI_EOG_MatchResults",
    "UI/EndOfGame/SteppedProgressionElements/UI_SteppedNextButton",
    "UI/EndOfGame/UI_EOG_StatSheet",
    "UI/EndOfGame/UI_EOG_StatItem",
    // the queue's end-of-game steps (the Matchmade list): the rematch vote, PLAY AGAIN and PullToPreMatch (the "eog"
    // tab container is cut. Deviation: NEXT runs PLAY AGAIN)
    "UI/EndOfGame/UI_EOG_RematchStatus",
    // the arcade's end-of-game step (the Arcade list, PGI_GlobalData EndOfGameProgressions "arcadeui")
    "UI/EndOfGame/SteppedProgressionElements/UI_ArcadeEOGContinue",
    "UI/EndOfGame/SteppedProgressionElements/UI_EOG_PlayAgain",
    "UI/EndOfGame/SteppedProgressionElements/UI_EOG_PullToPreMatch",
    // the podium level's 3D winner text (the WidgetComponent of BP_WinningTeam)
    "Maps/Map_PodiumStage/MapBGs/WBP_3dTeam",
    // the podium banner's nametag (BP_Banner_PM's screen-space EOG Nametag component)
    "UI/EndOfGame/UI_EOG_Nametag",
    // the podium banner's toast button (BP_Banner_PM's screen-space ToastButton component; its root UI_ButtonToast)
    "UI/EndOfGame/UI_EOG_ToastButton",
    // the toast notification's extra widget (PandaGameInstance Send Toast Notification: ExtraWidget)
    "UI/Prototype/CustomNotifications/UI_CustomNotificationToasts",
    // the name modal (UI_MainMenu shows it as chooseUsername) with its UI_TextInput and UI_ButtonCartoon
    "UI/MainMenu/UI_SetUsername",
    // created by UI_Manager: notifications (ShowNotification), dialogs (ShowDialog), the waiting cover (SetWaiting);
    // UI spec 17
    "UI/Prototype/Common/UI_Notification",
    // the lobby's Invite Player modal
    "UI/Social/UI_SocialInviteModal",
    // a player's social actions menu (ShowSocialActionsMenu: a social row, a party slot)
    "UI/Social/UI_SocialContextMenu",
    "UI/Prototype/Common/UI_Dialog",
    "UI/Prototype/Common/UI_Waiting",
    // the Lab's menu (UI_Game creates it for a training game mode)
    "UI/TrainingMode/UI_LAB_PauseMenu",
    // the Lab's matchmaking timer (UI_Game adds it beside the pause menu while a search runs)
    "UI/TrainingMode/UI_Lab_MatchmakingTimer",
    // the Lab's hotkeys (UI_Game @2463 adds it to its Root canvas in a training game mode)
    "UI/TrainingMode/UI_LAB_Hotkeys",
    // the Lab menu's character selector (modal labCharacterSelector) and the tile class its grid creates
    "UI/TrainingMode/UI_LAB_CharacterSelector",
    "UI/Collections/UI_Collection_CharacterItem",
    // the Lab menu's skin selector (modal labSkinSelector) and its grid's tile class
    "UI/TrainingMode/UI_LAB_SkinSelector",
    "UI/PreMatch/UI_EquipperItem_V2",
    // the map selector (modal mapSelector of UI_LAB_MapInfo and the custom game lobby) and its tile class
    "UI/TrainingMode/UI_LAB_MapSelector",
    "UI/TrainingMode/UI_LAB_MapItem",
    // the tooltip a UI_TooltipTarget creates (UI_Functions Get Tooltip; the class of every Tooltip Params the
    // ported widgets build)
    "UI/Prototype/Common/UI_Tooltip",
    // the pre-match screen (the PreMatch level's HUD widget) and the tile class its character grid
    // creates (its skin strip's tile class is UI_EquipperItem_V2 above)
    "UI/PreMatch/UI_PreMatch",
    "UI/PreMatch/UI_PM_CharacterItem",
    // the PreMatch level's BP_Banner_PM NametagWidget class (a screen-space WidgetComponent)
    "PreMatch/WBP_3dText",
    // the PreMatch level's PartnerNameplate_PM Nameplate class (a screen-space WidgetComponent: the party partner's
    // name and icon on the main menu)
    "UI/PreMatch/UI_PM_PartnerNameplate",
    // BP_SocialMenu's BannerClick class (a screen-space WidgetComponent: the main menu partner's click target)
    "UI/EndOfGame/WBP_3dClick",
    // the Move List (modal training_movelist of the Lab's MOVE LIST button, TrainingGameState.ToggleMovelist; the
    // pre-match screen shows the same class) and the passive row class its UpdateAppearance creates
    "UI/Collections/UI_CharacterMoveList",
    "UI/Collections/UI_CharacterPassiveMove",
    // the Glossary (modal glossary of the sidebar's section bar), the row class its PreConstruct creates
    // and the button class its horizontal section bar spawns (a class property of the bar's instance)
    "UI/MainMenu/GlobalNav/UI_GlossaryTooltipContainer",
    "UI/MainMenu/GlobalNav/UI_GlossaryTooltipText",
    "UI/Prototype/Reskin/MatchOptions/WBP_MO_HorizontalButton",
    // the screens of the online main menu, shown without their function: the PLAY button's Match
    // Options modal (matchoptions) and the Advanced Tutorials modal (SubOption) with the tile class its grid
    // creates, the ranked help and server settings prompts Match Options opens, the mode suggestion callout
    "UI/Prototype/Reskin/MatchOptions/WBP_MatchOptions",
    "UI/Prototype/Reskin/MatchOptions/WBP_SubOptionModal",
    "UI/Prototype/Reskin/MatchOptions/WBP_TutorialType",
    "UI/Ranked/Common/UI_RankedExplanationModal",
    "UI/MainMenu/Preferences/UI_ServerSettingsPrompt",
    "UI/PreMatch/UI_ModeSuggestion",
    // the large promotions modal (UI_PromoTickerSmall ShowLargePromoAtIndex @4995, modal promoTicker) and the
    // PreDeterminedWidget classes of PROMOTIONS it creates (UI_PromoTickerLarge OnLoaded @5631)
    "UI/MainMenu/PromoTicker/UI_PromoTickerLarge",
    "UI/MainMenu/PromoTicker/Cromulons/UI_Promo_Cromulons",
    "UI/MainMenu/PromoTicker/BatmanDay/UI_Promo_BatmanDay",
    "UI/MainMenu/PromoTicker/PreDeterminedPromoWidgets/UI_ScoobyMansionEventPromo_Shaggy",
    // the home screen's event buttons (UI_HomescreenEvents' entry classes)
    "UI/Events/UI_EventHomescreenButtonBase",
    "UI/Events/DoubleRewardsEvents/UI_EventDoubleBp_HomescreenButton",
    "UI/Events/Event_001/UI_Event001_HomescreenButton",
    "UI/Events/Event_002/UI_Event002_HomescreenButton",
    "UI/Events/Event_003/UI_Event003_HomescreenButton",
    "UI/Events/VotingEvents/UI_Event_Voting_Claim_HomescreenButton",
    "UI/Events/VotingEvents/UI_Event_Voting_HomescreenButton",
    // Local Play (modal localPlay), the Collection (nav collection) with the perk trainer, the profile (modal
    // profile, both classes), the Battle Pass (nav battlepass), the shops (nav shop, the home screen's shop, the
    // currency labels' currencyShop)
    "UI/LocalPlay/UI_LocalPlay",
    // the Local Play confirm step's name row (UI_GlyphEntries) creates it as its DynamicEntryBox's entry class
    "UI/Prototype/Common/UI_GlyphEntry",
    "UI/Collections/UI_CollectionsPanel",
    "UI/Perks/UI_Perks_Trainer",
    "UI/MainMenu/Profile/UI_Profile_v2",
    "UI/MainMenu/Profile/UI_Profile",
    "UI/MainMenu/BattlePass/UI_BattlePass_Rework",
    "UI/MainMenu/WWShop/UI_WWShop",
    "UI/MainMenu/Shop/UI_Shop",
    "UI/MainMenu/CurrencyShop/UI_CurrencyShop",
    // made at run time: the Battle Pass reward track's cards (WBP_RewardTrack AddCard @12087), the
    // shop's tab button class (UI_SectionBar_Shop Populate @4049) and its two page layouts (UI_WWShop CreateTabWidget)
    "UI/MainMenu/BattlePass/WBP_BPass_Card",
    // made at run time: the fighter screen's mastery cards (its WBP_RewardTrack's RewardCards class), the profile's
    // match history entries, the match breakdown popup and its player lines (UI_Profile_v2 PopulateMatchHistory,
    // OpenMatchBreakdown)
    "UI/MainMenu/BattlePass/WBP_CharMastery_Card",
    "UI/MainMenu/Profile/UI_MatchHistoryEntry",
    "UI/MainMenu/Profile/UI_MatchBreakdown",
    "UI/MainMenu/Profile/UI_MatchBreakdownPlayer",
    "UI/MainMenu/WWShop/WBP_MO_HorizontalButton_Shop",
    "UI/MainMenu/WWShop/UI_WWShopPage_Home",
    "UI/MainMenu/WWShop/UI_WWShopPage_Standard",
    // made at run time by the shop's product grids (UI_WWShopProductGridGeneric Create(WW_ShopItemCell_C), the filler
    // UI_WW_EmptyWidget_C) and by the Featured carousel (UI_WWCarousel Add Item (Shop Product) @9427, its pips and
    // promotion cells), and the two details modals a cell opens (WW_ShopItemCellBase @4105, @4846: modals BundleDetails,
    // ItemDetails)
    "UI/MainMenu/WWShop/WW_ShopItemCell",
    "UI/MainMenu/WWShop/UI_WW_EmptyWidget",
    "UI/MainMenu/WWShop/WWCurrencyShop/UI_ContextualUnlockModalCharacter",
    "UI/MainMenu/WWShop/WWCurrencyShop/UI_WWContextualUnlockModalBundle",
    "UI/MainMenu/WW/UI_WWCarousel",
    "UI/MainMenu/WW/UI_WWPaginationPips",
    // made at run time, found by the menu tracks: Local Play's match settings (modal localPlayMatchSettings), the
    // profile's leaderboards (UI_Profile_v2 @1932), the Match Options Style bar's button (SectionBar class), the
    // Collection's "Time to train!" callout (UI_CollectionPerks @903)
    "UI/LocalPlay/UI_QuickMatchSettings",
    "UI/ProfileLeaderboards/UI_Leaderboards",
    "UI/Prototype/Reskin/UI_SectionBarHorizontalMenusRework2",
    "UI/PreMatch/UI_CalloutTooltip",
    // the perk page (appendix B): the sublists' Add button (made at run time, OnListPopulated
    // @8668) and the perk tile's tooltip class (UI_Perks_Item UpdateTooltip @194-422)
    "UI/Perks/UI_Perks_AddButton",
    "UI/Prototype/Reskin/Tooltips/UI_TooltipPerk_v2",
    // the match pause menu and the Local Play controls
    // modal its Submit2 opens (modal inputSettings)
    "UI/UI_PauseMenu",
    "UI/LocalPlay/UI_InputBindingModal",
    // the reward tooltips, children of WBP_RichTooltip made at run time by a UI_TooltipTarget: UI_Functions
    // TooltipClassForRewardType's classes (UI_EquipperItem_V2 GetDefaultTooltipParams @94, WBP_GenericRT_Card Update
    // Tooltip @19458) and WBP_StatBadge SetEquipper's WBP_BadgeTooltip_C
    "UI/MainMenu/GlobalNav/WBP_BadgeTooltip",
    "UI/MainMenu/GlobalNav/WBP_VariantTooltip",
    "UI/MainMenu/GlobalNav/WBP_TauntTooltip",
    "UI/MainMenu/GlobalNav/WBP_ProfileIconTooltip",
    "UI/MainMenu/GlobalNav/WBP_RingoutTooltip",
    "UI/MainMenu/GlobalNav/WBP_EmoteTooltip",
    "UI/MainMenu/GlobalNav/WBP_AnnouncerTooltip",
    "UI/MainMenu/GlobalNav/WBP_CurrencyTooltip",
    "UI/MainMenu/GlobalNav/WBP_PerkTooltip",
    "UI/MainMenu/GlobalNav/WBP_BannerTooltip",
    // the tutorial's widgets over the match, each created by class at run time (TutorialActor: the intro card, the
    // concept card, its task rows and their InputButtonWidget glyphs, the end screen and its score rows)
    "Blueprints/Tutorial/TutorialWidget",
    "Blueprints/Tutorial/TutorialTaskWidget",
    "Blueprints/Tutorial/TutorialIntroScreen",
    "Blueprints/Tutorial/InputButtonWidget",
    "Blueprints/Tutorial/UI_TutorialEndScreen",
    "Blueprints/Tutorial/UI_MinigameScore_Row",
];

/// The Glossary's DataTables, by name under `UI/Prototype/Foundation/Types`.
pub const GLOSSARY_TABLES: &[&str] = &["DT_ModifierGlossary", "DT_GameplayGlossary", "DT_EconomyGlossary"];

/// The Game target's strings the menus' graphs set at run time, by key (empty namespace).
pub const MENU_TEXTS: &[&str] = &[
    "067101DC468CE87E61FD548E33EA0682",
    "72533455428B79A0959C078E3AD7F22E",
    "C12F28E7496A53D0BAD7A1BEBE343EF4",
    "424583CF48092C268CAA03BF14E171AD",
    "B612CFA447B6F0344995D0A6C5C40013",
    "62E22AAC42CD30AD7002C9BC31D14624",
    "CF27B82D4A964E8C9C6DC6A20004BED4",
    "3156D1784EE459BFC2E495985DE37A81",
    "79B311A54CCAD5DDB843D9881B63C6E0",
    "5C8C41C347CB04C9066A9B8D1E29DD15",
    "36924E7E411267B011CEF682C769F7D3",
    "B44373DE4DAF3C90FA88BCB3086BC546",
    "05C8AD994CF4B72D41E9D9B6FA5CC125",
    "8858A4A24785FFFED504B4B1C9DFC2C0",
    "27F02EC640D245F5386C06AC3A7B4E54",
    "5068440D43CDD2424B3DA68254BD36F4",
    "9269A1DC41E486AA85F3BFA95185E98D",
    "ACA55CF84739D150DE1885ABA8C5AAD6",
    "ECEDFE7B4A342008C5A05B810288AC97",
    "C4DA066D460470B56C4A9999AAC08B4F",
    "95BD96174B89406C9AB040AB893025EC",
    "17D20BC74F546886B664438A5B01C40F",
    "DA651E3240F063F57EE9DB8236A9D6AA",
    "9B9B040D48D24A11023730A9F01C8C48",
    "F30CAC6842DC91165C321D844F966F0D",
    "A248EA8A432B4C6CC90740BE4E2F7303",
    "E9C59BD2451E8ABFA7D0D7A82DFCD952",
    "DB9EB7BD42CA912FC3669BB4F06053FA",
    "B5A1EC854483F6512C6838B06C3E65CE",
    "A304F86046D31CC30EEFBFA80895DF75",
    "E056BBBA4FA98A95CEBE4995B8D0D08E",
    "7415534E4361E78502F06A9CFD713EE1",
    "3E0C5B0A43FABC5CD5508080BDD5E165",
    "CF6BBFCA4EE800E6621923A21EE95C86",
    "8775226742714B062EAD3E938465832E",
    "08CED9FA483935E662B86F940A8F02ED",
    "135311634495EF34599536835723DF79",
    "F2A2554A4DD9EDDB676084961C99CC21",
    "59FC3B91409A546CF3D10A90A331295F",
    "7141B64344BFFEDA6B8FE1824639532B",
    "EA8C5FCE460AC663CA742C89CE041DA2",
    "5400E24F4F535368DBF7039656503AFD",
    "8E3C823E4E017B01AEDB3793AFDCE68E",
    "A955A7E741E0BCEB3C250CAD0AE612A1",
    "B17EBE724C8A08E2A14242AE9D7598EF",
    "0FF230AF4730F9CCD16B74A8DE4E5AAF",
    "16565C5D4A5627A300CEFDA9432EEF5F",
    "BDA789E344BA016B9A79328FDBED5EC9",
    "73BD51804BA2EFC8FAD03B8BD0DC2231",
    "681AD7484AAEDE692897EF8A77BD8D8C",
    "3C0EF72945B5E7488210B38F5FC6BB8F",
    "0ABB0A3F43B89BD6A9FB1E8029AF6935",
    "05661FA34147BCA06D1E5EA30398DF7C",
    "45710C094051CDCEA6909C9A280217AC",
    "BC6C2BDA4AEC145828EFC7963CB845CD",
    "B54B64AB48870D59DEE31AB189A15AD5",
    "790DF82C42BDC057F6ED4988AC8F22F0",
    "9634579C48E72A3603A7298D8F99C56A",
    "A86626344C92D1B4F14784A41FE40F6D",
    "C40967F5429C097943763CAE189982D1",
    "623526EF4B240087F8DAAE83E78B2489",
    "4840E50A4EE9795EF959D7B27E417E4D",
    "B89176154112EAA6AB580D968E2BAA80",
    "4E7586DD45A679083CB9649A9F372C52",
    "01E0822548BA24BCBDFBAEAF8997B6D2",
    "32F116FE401FD7FA44F89C8092021B79",
    "EB42CDE54D3512A10657468E2200EF9D",
    "0D4B15CE4FB6F2BC5E5F5FAA9E8B376C",
    "1DC94E7D4A7CF17468D703A175F5D949",
    "E70AAB9C450E8B055BF4EEB687311C4F",
    "D55C2EE74ADE85593595F5AA556BEDB2",
    "236BBA9A4F869C25AEE457BB914D1B39",
    "2EFD913A40EFFF6563B156AE62312528",
    "C51E56ED4E673B922BDE2AAFB938F8F2",
    "1164D2E24901624F6C0BF48A216689B8",
    "37F7D5A94886770D20288BA8D91BCB3D",
    "574B07F54A59F6F281E768B9410B6745",
    "59B333BD487777A7F49F76A22650DDBF",
    "D8CE09314444BBC2ACD4AD81565E0A4F",
    "4D7FCA0C49B918F3716DC6BE217136D7",
    "250CA85D4185D46D58D9FD8A24049621",
    "C1413E164DB8DCE5DC2CC19A2E4AEBA5",
    "720C33654366ACE4376C668A91B54234",
    "1789A04E4D313B3F325D3485EB970175",
    "B2EE21AE4D716D4BC84E558DB0B5FCC8",
    "7ECCDD29495ACF160A8075A6A6E9284F",
    "F8D006C243F227FFB85345BB21684DF8",
    "54C038B2439D0A26FEE5A98B9214FA6C",
    "064EFA804BB3847536ACF99C055B6145",
    "97B4DE964F72922A6EF2F083A2ED65DD",
    "937587C24F6A912C956CD482A38D65E9",
    "C3FAA6344271BFF8CD283E9C1635E7DD",
    "96C3E9AE4DD38E7846EF92B47922414A",
    "95F2CC7145437E299415B0A1BFEBBF98",
    "E332E75C4858C6F577B7B3A7B194585E",
    "B806799F4EB2FFB76314CCBB0A4DE665",
    "F39BDE90481CF97CA8A290859ACF18C5",
    "343B221A43DBCF911B176AB61CC62EE0",
    "70FDF8DB4DE046A48108EB9FC44FD7B9",
    "78EB00F649C85206E1B6288AE019EA6E",
    "FDC14A54445B89024B5ECAA73F106D50",
    "51F00C654C63D0092D603FA302D7579B",
    "24AF51BB4A758D0B7DEF5FBC0F66B1B5",
    "379E05A74F33E66508DD8D9B9BAA7EFA",
    "073A54424B37C6B8B8ACEDACCD1968F5",
    "8E01A7564E427420B5256588B1EAB2AA",
    "6CB98FE246BDE894C3BB44BD328966D5",
    "FBCC9F9342939533CE2551AB7A59CA2B",
    "87D3C6274F15E72F82FD218013075375",
    "3CE05CA54C9809647363DAB7CEDAF071",
    "7CD1B5F2421F206C25899EBC843D938D",
    "82C165DE4C136D80D01ED693EB1B5E30",
    "84F0AC594A2CF293331FED8FD87A401A",
    "D2658F5D410F32AFB27DDFA7D2672AE8",
    "264A07624BBA70A4BE1A52A40E77E427",
    "4F6DE0604D9836A7C81C77955F22288D",
    "2BE3A07642CBCCC640BED784403F2B1D",
    "8A32905041E06FD6F0FF418E2507DED0",
    "232EB89140CC5A45D114598D7C671A8B",
    "A4F1502B49B07E69282BFF8846B4519B",
    "B6C8833543D790302DCD07B9224B6B8D",
    "C22F703C49FD63B21D818EB48DB00937",
    "7F1D1BC742809C58145B67995339C8CA",
    "0DF6CDB94285A1F8D39A3A83C742B63A",
    "06C47FDD407B9D582B75D4A69CF513F3",
    "2E2FF13E415A47A93B81CD9A70CCA05E",
    "9FDC516D47362F6A29B7B4A7165B093D",
    "0724A6ED4241C15CCF062F94DDBFA6F4",
    "1DF69A0840FE482E9FD75A8C6C02AA1B",
    "48DC34EA450122F1251A369932A1CDA2",
    "389237734851535742511C887B34D7E0",
    "676F2E1A422DFBC8A41E1B84A960129B",
    "537228CE4C1DDEA791EF8BB18355165B",
    "909001934351BF1B928EAA904C33B0BB",
    "5FF9EC7A4A0FEEC1942EBA912E1AA555",
    "1AE9D5F64E8F04F61F93AF9DD4B9C9C4",
    "9F73013048B0F6D401ACBE8CEA1A4ACB",
];

/// The same for the HUD.
pub const HUD_TEXTS: &[&str] = &[
    "EC318B5F4DE663B69918359482637629",
    "14A21FC9406DD1FEBD39F087A33446D5",
    "32EACD754554FD12680E2B9CD89715E6",
    "AC63880C4C8F5995638E76A84493CA5B",
    "B52EE17D46FB1781FC8D468618DFB003",
    "4894864A4D2BF5B41DC832B836243CC0",
    "4BF9E9EC4A893C4DE3AC0893643C9B21",
    "EBAB0056453D91F25EBB1D8A51D45523",
    "8F0CC2FB4E111CC752D3679434C276BE",
    "242A429B4D79D9F2094B669C2CB64DE1",
    "73CDF1DC451978B29AAEAB93FCD3BC2C",
];

/// Engine Slate images the menu's native widgets draw by default (below `SLATE_FOLDER`).
pub const SLATE_IMAGES: &[(&str, &str)] = &[
    ("Slate_TextBox", "Common/TextBox"),
    ("Slate_TextBox_Hovered", "Common/TextBox_Hovered"),
    ("Slate_Throbber_Piece", "Common/Throbber_Piece"),
    ("Slate_Scrollbar_Thumb", "Common/Scrollbar_Thumb"),
    ("Slate_Scrollbar_Background_Vertical", "Common/Scrollbar_Background_Vertical"),
];

/// User widgets of the menu trees that the port does not show.
pub const MENU_SKIP: &[&str] = &[
    "UI_Characters",
    "UI_DebugRingOutVFX",
    "WBP_UseCurrencyButton",
    "UI_PM_Cosmetics",
    "UI_PM_BoostInfo",
    "UI_RestedXPBar",
];

/// The engine's Slate brush files (the `image` kind) are below this folder.
pub const SLATE_FOLDER: &str = "/Engine/Content/Slate/";

/// HUD: brush materials set at run time by gameplay Blueprints (literal refs), by package below `PANDA_MAIN`, with their class.
pub const HUD_MATERIALS: &[(&str, &str)] = &[
    ("UI/In-Game/States/MI_State_Armor", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Weak", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Slow", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Dodged", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Thorn", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Ignite", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_ChillRadial", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Confetti", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_GarnetStar", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_FaceSteal", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Fear", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Charm", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Polymorph", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Tornado", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Smell", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Chicken", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Rose", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Music", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_CheeseBuff", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Targeted", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_ArmorBreak", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_ProjectileArmor", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Rocket", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_MarvinFlag", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Attach", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_DodgeBurst", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/MI_State_Silenced", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/In-Game/States/UI_State_Snowman", "/Script/Engine.MaterialInstanceConstant"),
];

/// HUD: textures a widget's Blueprint sets at run time.
pub const HUD_LITERAL_TEXTURES: &[&str] = &["UI/Assets/Icons/ui_icons_midbars", "UI/Assets/Icons/ui_icons_lowbars"];

/// Menu: materials a widget Blueprint makes or sets at run time, with their class.
pub const MENU_MATERIALS: &[(&str, &str)] = &[
    ("Materials/Halftone/testingstuff/MI_FoundersPackShimmer", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/Prototype/HUD/Countdown/M_FontStyling", "/Script/Engine.Material"),
    ("Materials/Halftone/M_RestedIcon_Scroll", "/Script/Engine.Material"),
    ("Materials/Halftone/MI_Quad_RandomChar_Lab", "/Script/Engine.MaterialInstanceConstant"),
    ("Materials/Halftone/MI_Quad_Masked", "/Script/Engine.MaterialInstanceConstant"),
    ("Materials/Halftone/MI_Quad_RandomChar", "/Script/Engine.MaterialInstanceConstant"),
    ("UI/PreMatch/Images/Perks/Icons/MI_PerkContainer", "/Script/Engine.MaterialInstanceConstant"),
    // the shop cell's fallback picture (WW_ShopItemCellBase Load Item Image: `MI_WW_DefaultShopImage` with the texture)
    ("UI/MainMenu/WWShop/MI_WW_DefaultShopImage", "/Script/Engine.MaterialInstanceConstant"),
];

/// Menu: textures the Blueprints set at run time.
pub const MENU_LITERAL_TEXTURES: &[&str] = &[
    "UI/Assets/Icons/ui_icons_featured",
    "UI/Assets/Icons/ui_icons_chest",
    "UI/Assets/Icons/ui_icons_tracker",
    "UI/Prototype/Reskin/MatchOptions/UI_GameMode_1vs1_BG",
    "UI/Prototype/Reskin/MatchOptions/UI_GameMode_FFA_BG",
    "UI/Prototype/Reskin/MatchOptions/UI_ModeSelect_BG_1",
    "UI/Prototype/Reskin/MatchOptions/UI_ModeSelect_BG_2",
    "UI/Prototype/Reskin/MatchOptions/GameMode_AXEasy_BG",
    "UI/Prototype/Reskin/MatchOptions/GameMode_AXMedium_BG",
    "UI/Prototype/Reskin/MatchOptions/GameMode_AXHard_BG",
    "UI/Assets/Icons/Profile_Icons/20210225_CN_AT_Icon_Profile_PButler",
    "UI/Assets/Icons/ui_icons_backarrow",
    "UI/Assets/Icons/ui_icons_kick",
    "UI/Assets/Tutorial/Comma-outline",
];

/// UI_Functions "Get Offline Soft Maps": the MapData lists of its two branches, LocalPlay false (20) and true (16).
pub const OFFLINE_MAPS: &[&str] = &[
    "Map_batcave",
    "map_classic_3_platform_1v1",
    "map_classic_3_platform_2v2",
    "map_scooby_doo_2",
    "map_scooby_doo_2_noroof",
    "Map_TrainingRoomSmall",
    "Map_TrainingRoomLarge",
    "map_m008",
    "map_tree_house_2v2_1_platform",
    "map_m011",
    "map_m011_largenoplat",
    "map_m009",
    "map_m009_1v1",
    "Map_tree_house_1v1",
    "Map_TreeHouse_2v2",
    "map_trophy_room_2_platform",
    "Map_trophy_room_large_platform",
    "map_m011_small",
    "map_m011_smallnoplat",
    "map_M010",
];

pub const OFFLINE_MAPS_LOCAL: &[&str] = &[
    "Map_batcave",
    "map_scooby_doo_2",
    "map_scooby_doo_2_noroof",
    "map_m008",
    "map_tree_house_2v2_1_platform",
    "map_m011",
    "map_m011_largenoplat",
    "map_m009",
    "map_m009_1v1",
    "Map_tree_house_1v1",
    "Map_TreeHouse_2v2",
    "map_trophy_room_2_platform",
    "Map_trophy_room_large_platform",
    "map_m011_small",
    "map_m011_smallnoplat",
    "map_M010",
];

/// The home screen's promotions: a slug of the port's own and the Promotion's widget (`PROMOTIONS`).
// Deviation: the port's own (the backend's promotions need the online service).
pub const PROMOTIONS: &[(&str, &str)] = &[
    ("cromulons", "UI/MainMenu/PromoTicker/Cromulons/UI_Promo_Cromulons"),
    ("batmanday", "UI/MainMenu/PromoTicker/BatmanDay/UI_Promo_BatmanDay"),
    ("scoobymansion", "UI/MainMenu/PromoTicker/PreDeterminedPromoWidgets/UI_ScoobyMansionEventPromo_Shaggy"),
];
