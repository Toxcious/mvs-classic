//! The materials the Blueprints set on the meshes of a character's spawned actors and props (the fighter entry's
//! `actors`), ported from their graphs, written out by hand with the spread rows expanded. Each row is by package below
//! `/Game/` (or `Engine/Content/`); the bind turns each into an object path and checks its slot count against the mesh.
//!
//! Skeletal meshes of spawned actors that are not skin sub-meshes, by actor class: the mesh asset and the material of
//! each slot as the actor's Blueprint sets it (C019_Ship ReceiveBeginPlay: dynamic instances of
//! c019_Props2_Base_MIC_Local on slot 0 and C019_Ship_Glass_MAT on slot 1). A material without the base-pass shader
//! below is written as null, as for the skins.

/// A row: the actor class (or mesh) it is keyed by, the characters whose matches draw it, the mesh package, the
/// material package per mesh slot (the local player's set) and the team sets a class's SetTeamColor puts on instead.
pub struct ActorRow {
    pub cls: &'static str,
    pub owner: &'static [&'static str],
    pub mesh: &'static str,
    pub materials: &'static [&'static str],
    pub variants: Option<&'static [(&'static str, &'static [&'static str])]>,
    pub is_static: bool,
}

/// `{ skinStatics: key }` stands where the table enters the default skin's static SubMeshes of that key
/// (`draws.rs` skin_statics), in the table's order.
pub enum Row {
    Actor(ActorRow),
    SkinStatics(&'static str),
}

pub const ACTOR_TABLE: &[Row] = &[
    Row::Actor(ActorRow {
        cls: "C019_Ship_C",
        owner: &["morty"],
        mesh: "Panda_Main/Characters/C019/VFX/SM_c019_Ship_SkelMesh",
        materials: &[
            "Panda_Main/Characters/C019/VFX/c019_Props2_Base_MIC_Local",
            "Panda_Main/Characters/C019/c019_Ship_Glass_MAT",
        ],
        variants: None,
        is_static: false,
    }),
    // AnimNotify_SubMeshSpawn's meshes (BaseFighter Add Montage Mesh), keyed by the mesh; no material overrides on
    // the notifies, so the meshes' own slot materials.
    Row::Actor(ActorRow {
        cls: "C019_Juice_Carton_SkelMesh",
        owner: &["morty"],
        mesh: "Panda_Main/Characters/C019/C019_Juice_Carton_SkelMesh",
        materials: &["Panda_Main/Characters/C019/C019_JuiceCarton_Base_MIC_Local"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C019_Juice_Stream_SkelMesh",
        owner: &["morty"],
        mesh: "Panda_Main/Characters/C019/C019_Juice_Stream_SkelMesh",
        materials: &["Panda_Main/Characters/C019/VFX/C019_OJ_MAT"],
        variants: None,
        is_static: false,
    }),
    // the defeat montage's skeleton in the vat: the mesh's one slot is the test map's lambert1 material
    Row::Actor(ActorRow {
        cls: "C019_Bones_SkelMesh",
        owner: &["morty"],
        mesh: "Panda_Main/Characters/C019/Animations/Emotes/C019_Bones_SkelMesh",
        materials: &["Panda_Main/Maps/LionwatchTest/lambert1"],
        variants: None,
        is_static: false,
    }),
    // the defeat montage's vat: a static mesh (FLocalVertexFactory shaders, staticMesh below)
    Row::Actor(ActorRow {
        cls: "SM_C019_AcidVat_01",
        owner: &["morty"],
        mesh: "Panda_Main/Characters/C019/VFX/SM_C019_AcidVat_01",
        materials: &[
            "Panda_Main/Characters/C019/VFX/C019_AcidVat_01_MAT",
            "Panda_Main/Characters/C019/VFX/C019_AcidVat_02_MAT",
            "Panda_Main/Characters/C019/VFX/C019_Vat_Acid_Mesh_MAT",
        ],
        variants: None,
        is_static: true,
    }),
    // Shaggy's podium montages. Shaggy_Emote_Victory_Montage: the nunchucks, a skeletal mesh on its own
    // skeleton with its one slot's nunchucks_MIC (0_Character_MAT).
    Row::Actor(ActorRow {
        cls: "nunchucks_SkelMesh",
        owner: &["shaggy"],
        mesh: "Panda_Main/Characters/Shaggy/Mesh/nunchucks_SkelMesh",
        materials: &["Panda_Main/Characters/Shaggy/Mesh/nunchucks_MIC"],
        variants: None,
        is_static: false,
    }),
    // Shaggy_Emote_Defeat_Montage: the sandwich, a static mesh with its one slot's Sandwich_MAT
    Row::Actor(ActorRow {
        cls: "Sandwich",
        owner: &["shaggy"],
        mesh: "Panda_Main/Characters/Shaggy/Props/Sandwich",
        materials: &["Panda_Main/Characters/Shaggy/Props/Sandwich_MAT"],
        variants: None,
        is_static: true,
    }),
    // ScoobySnackBox.ScoobySnackStaticMesh (ScoobySnack, OverrideMaterials [null], no graph sets one): its one slot's
    // lambert2.
    Row::Actor(ActorRow {
        cls: "ScoobySnack",
        owner: &["shaggy"],
        mesh: "Panda_Main/Characters/Shaggy/Props/ScoobySnack",
        materials: &["Panda_Main/Characters/Shaggy/Props/lambert2"],
        variants: None,
        is_static: true,
    }),
    // Iron Giant's spawned actors and items, keyed by the mesh: static meshes. `materials` is what the
    // local player's actor draws with, `variants` what SetTeamColor's SetMaterial(0, ...) picks for colour index 0
    // (red) and 1 (blue) of a player who is not the local one.
    // WeaponModeMissile.StaticMesh (@1839..2067): local C017_Rocket_Blue_Mat_Mat, 0 Red, 1 Blue. Rick's
    // c020_WeaponModeMissile.StaticMesh the same (@511..739: local Blue, 0 Red, 1 Blue).
    Row::Actor(ActorRow {
        cls: "C017_Rocket",
        owner: &["irongiant", "rick"],
        mesh: "Panda_Main/Characters/C017/Submeshes/C017_Rocket",
        materials: &["Panda_Main/Characters/C017/Submeshes/C017_Rocket_Blue_Mat_Mat"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/C017/Submeshes/C017_Rocket_Red_Mat_Mat"]),
            ("blue", &["Panda_Main/Characters/C017/Submeshes/C017_Rocket_Blue_Mat_Mat"]),
        ]),
        is_static: true,
    }),
    // Item_C017_Scrap.ScrapMesh (@1055..1283) and C017_ThrownScrap.Sphere (its OverrideMaterials C017_Submeshes_MIC;
    // no SetMaterial in its graph: the local set for every player). The red instance's package is spelled
    // C017_SUbmeshes_MIC_Red, its object C017_Submeshes_MIC_Red.
    // The pawn's static props: Prop_Scrap = C017_Scrap_Large, Prop_ScrapThrow = C017_Scrap_Base. The
    // skin's static SubMeshes entries (C017_S00 Prop_Scrap, Prop_ScrapThrow) name the same mesh and one Materials
    // entry each: LocalPlayer C017_Submeshes_MIC, Red ..._MIC_Red, Blue ..._MIC_Blue, the set below (buildSkin
    // leaves static entries out: these are drawn as actor meshes).
    Row::Actor(ActorRow {
        cls: "C017_Scrap_Base",
        owner: &["irongiant"],
        mesh: "Panda_Main/Characters/C017/Submeshes/C017_Scrap/C017_Scrap_Base",
        materials: &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_Submeshes_MIC"],
        variants: Some(&[
            (
                "red",
                &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_SUbmeshes_MIC_Red.C017_Submeshes_MIC_Red"],
            ),
            ("blue", &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_Submeshes_MIC_Blue"]),
        ]),
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "C017_Scrap_Switch",
        owner: &["irongiant"],
        mesh: "Panda_Main/Characters/C017/Submeshes/C017_Scrap/C017_Scrap_Switch",
        materials: &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_Submeshes_MIC"],
        variants: Some(&[
            (
                "red",
                &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_SUbmeshes_MIC_Red.C017_Submeshes_MIC_Red"],
            ),
            ("blue", &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_Submeshes_MIC_Blue"]),
        ]),
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "C017_Scrap_Large",
        owner: &["irongiant"],
        mesh: "Panda_Main/Characters/C017/Submeshes/C017_Scrap/C017_Scrap_Large",
        materials: &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_Submeshes_MIC"],
        variants: Some(&[
            (
                "red",
                &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_SUbmeshes_MIC_Red.C017_Submeshes_MIC_Red"],
            ),
            ("blue", &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_Submeshes_MIC_Blue"]),
        ]),
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "C017_Base_Props_Low",
        owner: &["irongiant"],
        mesh: "Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_Base_Props_Low",
        materials: &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_Submeshes_MIC"],
        variants: Some(&[
            (
                "red",
                &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_SUbmeshes_MIC_Red.C017_Submeshes_MIC_Red"],
            ),
            ("blue", &["Panda_Main/Characters/C017/Submeshes/C017_PlantArt/C017_Submeshes_MIC_Blue"]),
        ]),
        is_static: true,
    }),
    // AnimNotify_SubMeshSpawn's C017_DiscArms_SkelMesh (the three C017_WeaponMode_EnergyDisc montages): a skeletal
    // mesh whose one slot (lambert1) has a null material, and the notifies name none (no Materials, SkinOverride or
    // SkinMesdhOverride entry), so BaseFighter's Add Montage Mesh @3965 sets nothing: UMaterial::GetDefaultMaterial
    // (MD_Surface), WorldGridMaterial, as BoltMesh below. Ahead of the static meshes with that material (a material
    // on both kinds is built for the skeletal mesh first, see the loop over ACTORS).
    Row::Actor(ActorRow {
        cls: "C017_DiscArms_SkelMesh",
        owner: &["irongiant"],
        mesh: "Panda_Main/Characters/C017/Submeshes/DiscArms/C017_DiscArms_SkelMesh",
        materials: &["Engine/Content/EngineMaterials/WorldGridMaterial"],
        variants: None,
        is_static: false,
    }),
    // His podium montages' AnimNotify_SubMeshSpawn meshes (C017_Emote_Victory_Montage: Bolt_Emote_Victory_Enter on
    // Root; C017_Emote_Defeat_Montage: PlantArt_Emote_Defeat_Loop2, twice): skeletal meshes whose one slot has a null
    // material, and the notifies name none: WorldGridMaterial, as C017_DiscArms_SkelMesh.
    Row::Actor(ActorRow {
        cls: "Bolt_Emote_Victory_Enter",
        owner: &["irongiant"],
        mesh: "Panda_Main/Characters/C017/Animations/Emotes/Bolt_Emote_Victory_Enter",
        materials: &["Engine/Content/EngineMaterials/WorldGridMaterial"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "PlantArt_Emote_Defeat_Loop2",
        owner: &["irongiant"],
        mesh: "Panda_Main/Characters/C017/Animations/Emotes/PlantArt_Emote_Defeat_Loop2",
        materials: &["Engine/Content/EngineMaterials/WorldGridMaterial"],
        variants: None,
        is_static: false,
    }),
    // C017_AntenaBolts.StaticMesh (AntenaMesh: its slot is the engine's WorldGridMaterial) and the BoltMesh components
    // it adds, also C017_Spit_Bolt.StaticMesh (BoltMesh: a null slot, which draws with UMaterial::GetDefaultMaterial
    // (MD_Surface) = DefaultMaterialName of BaseEngine.ini, WorldGridMaterial). No graph sets a material on either.
    Row::Actor(ActorRow {
        cls: "AntenaMesh",
        owner: &["irongiant"],
        mesh: "Panda_Main/Characters/C017/Submeshes/AntenaMesh",
        materials: &["Engine/Content/EngineMaterials/WorldGridMaterial"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "BoltMesh",
        owner: &["irongiant"],
        mesh: "Panda_Main/Characters/C017/Submeshes/BoltMesh",
        materials: &["Engine/Content/EngineMaterials/WorldGridMaterial"],
        variants: None,
        is_static: true,
    }),
    // Wonder Woman's podium victory (character_wonder_woman DefaultVictoryMontage = WonderWoman_Emote_JumpRope_Montage):
    // AnimNotify_SubMeshSpawn's WW_SkelMesh_JumpRope, a skeletal mesh on its own skeleton; no material on the notify,
    // so the mesh's one slot (lambert1) with WW_Lasso_JumpRope_MAT.
    Row::Actor(ActorRow {
        cls: "WW_SkelMesh_JumpRope",
        owner: &["wonderwoman"],
        mesh: "Panda_Main/Characters/WonderWomanV2/VFX/WW_SkelMesh_JumpRope",
        materials: &["Panda_Main/Characters/WonderWomanV2/VFX/WW_Lasso_JumpRope_MAT"],
        variants: None,
        is_static: false,
    }),
    // Her WeightLifting taunt (WonderWoman_Emote_WeightLifting_Montage, a taunt slot's): AnimNotify_SubMeshSpawn's
    // WonderWoman_Emote_Weight, a skeletal mesh on its own skeleton; no Materials / SkinOverride on the notifies, so
    // the mesh's one slot (WW_Weight_Mat) with WW_Weight_Mic.
    Row::Actor(ActorRow {
        cls: "WonderWoman_Emote_Weight",
        owner: &["wonderwoman"],
        mesh: "Panda_Main/Characters/WonderWomanV2/WonderWoman_Emote_Weight",
        materials: &["Panda_Main/Characters/WonderWomanV2/WW_Weight_Mic"],
        variants: None,
        is_static: false,
    }),
    // Harley Quinn's props that are not skeletal SubMeshes of her skin, keyed by the mesh. Pawn_Harley's components
    // Glove, ConfettiGun, Detonator (StaticMeshComponents) and Pistol (a SkeletalMeshComponent with its own mesh),
    // bVisible false, shown by AnimNotify_Equip's MeshTag / AnimNotify_SetSubmeshVisibility in her montages; the
    // glove is also the StaticMesh of HarleyDownAirProjectile and HarleySideAirProjectile2.
    // The glove: skin HarleyWithJacket's static SubMeshes entry (key Glove) has one Materials entry, LocalPlayer
    // HarleyQuinnProps_Yellow_MIC, Red HarleyQuinnProps_MIC, Blue HarleyQuinnProps_Blue_MIC (buildSkin leaves static
    // entries out). HarleyDownAirProjectile's class default Materials set is the same three (HarleySideAirProjectile2
    // inherits it), which SpawnedActorParent SetTeamColor (@849 calls it first) puts on the StaticMesh's slot 0.
    Row::Actor(ActorRow {
        cls: "HarleyQuinn_Glove",
        owner: &["harley"],
        mesh: "Panda_Main/Characters/HarleyQuinn/HarleyQuinn_Glove",
        materials: &["Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_Yellow_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_MIC"]),
            ("blue", &["Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_Blue_MIC"]),
        ]),
        is_static: true,
    }),
    // No SubMeshes entry of the skin names these three: the meshes' own slot (HarleyPropMat = HarleyQuinnProps_MIC).
    // Unverified: whether the skin manager recolours a component without an entry is not read.
    Row::Actor(ActorRow {
        cls: "HarleyQuinn_ConfettiGun",
        owner: &["harley"],
        mesh: "Panda_Main/Characters/HarleyQuinn/HarleyQuinn_ConfettiGun",
        materials: &["Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "HarleyQuinn_Detonator",
        owner: &["harley"],
        mesh: "Panda_Main/Characters/HarleyQuinn/HarleyQuinn_Detonator",
        materials: &["Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "HarleyQuinn_Pistol_SkelMesh",
        owner: &["harley"],
        mesh: "Panda_Main/Characters/HarleyQuinn/HarleyQuinn_Pistol_SkelMesh",
        materials: &["Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_MIC"],
        variants: None,
        is_static: false,
    }),
    // HarleyQuinn_Special_JackInTheBox_Montage / _Air_Montage: AnimNotify_SubMeshSpawn's mesh (the box in her hand
    // before the actor is placed), keyed by the mesh. The notify's Materials entry: MaterialName HarleyPropMat,
    // LocalPlayer HarleyQuinnProps_Yellow_MIC, Red HarleyQuinnProps_MIC, Blue HarleyQuinnProps_Blue_MIC (the sets of
    // the skin's JackInTheBox SubMeshes entry, which the placed actor draws with).
    Row::Actor(ActorRow {
        cls: "HarleyQuinn_JackInTheBox_SkelMesh",
        owner: &["harley"],
        mesh: "Panda_Main/Characters/HarleyQuinn/HarleyQuinn_JackInTheBox_SkelMesh",
        materials: &["Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_Yellow_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_MIC"]),
            ("blue", &["Panda_Main/Characters/HarleyQuinn/HarleyQuinnProps_Blue_MIC"]),
        ]),
        is_static: false,
    }),
    // The gum bubble of her alt idle and character select (HarleyQuinn_Nav_IdleAlt_Loop, HarleyQuinn_CharacterSelect_Idle
    // / _Intro: AnimNotify_SubMeshSpawn on fml_un_C_prop3, in those sequences and the montages over them): a static
    // mesh, no Materials on the notify, so its one slot's (HarleyPropMat) BubbleGum_MAT.
    Row::Actor(ActorRow {
        cls: "HarleyQuinn_Bubble",
        owner: &["harley"],
        mesh: "Panda_Main/Characters/HarleyQuinn/HarleyQuinn_Bubble",
        materials: &["Panda_Main/Characters/HarleyQuinn/VFX/BubbleGum_MAT"],
        variants: None,
        is_static: true,
    }),
    // LeBron's hoop, keyed by the mesh as his data names it (the package is c016_Hoop_SkelMesh, its object
    // C016_Hoop_SkelMesh): Pawn_C016's Hoop component (a SkeletalMeshComponent on HoopSocket, scale 0.6, tag Hoop,
    // shown by AnimNotify_SetSubmeshVisibility of his Wall_Enter and Wall_Idle sequences and hidden by the one of Wall_Jump, Jump, DoubleJump, Fall and
    // FastFall) and the Mesh of AnimNotify_SubMeshSpawn in C016_Emote_Victory_Montage. Neither
    // names a material and no SubMeshes entry of the skin names the mesh: its own slot, C016_Hoop_MIC.
    // C016_Hoop_MIC is 0_Character_MAT with BlendMode overridden to BLEND_Additive (DefaultLit): the translucent base
    // pass (buildMaterial's effect path).
    Row::Actor(ActorRow {
        cls: "C016_Hoop_SkelMesh",
        owner: &["lebron"],
        mesh: "Panda_Main/Characters/c016/c016_Hoop_SkelMesh.C016_Hoop_SkelMesh",
        materials: &["Panda_Main/Characters/c016/c016_Hoop_MIC"],
        variants: None,
        is_static: false,
    }),
    // AryaKillPie's ScoobySnackStaticMesh (BugsBunny_Prop_Pie of Characters/BugsBunnyV2), keyed by the mesh: the
    // component's OverrideMaterials is [null] and no graph of the class sets a material, so the mesh's own slot
    // (BugsBunnyPropsMat) for every team: SpawnedActorParent SetTeamColor's set (Materials) is empty in its class
    // default, so ColoredMaterial is not valid (@20975). The same for Projectile_Neptr_Pie (OverrideMaterials none,
    // ParentProjectilePhysics' empty set): owners arya and bugs. Bugs Bunny's PieProjectile has a set: its own entry below.
    Row::Actor(ActorRow {
        cls: "BugsBunny_Prop_Pie",
        owner: &["arya", "bugs"],
        mesh: "Panda_Main/Characters/BugsBunnyV2/BugsBunny_Prop_Pie",
        materials: &["Panda_Main/Characters/BugsBunnyV2/BugsBunnyPropsMat"],
        variants: None,
        is_static: true,
    }),
    // Batman's actor meshes, keyed by the mesh. Item_BatarangV2's Geo (SM_BatarangV2): OverrideMaterials [null], the
    // mesh's slot BatmanBatarangV1_MESH_MIC, which ExecuteUbergraph also makes the dynamic instance of (@8866) and
    // gives the scalar Team (@7601, @9163; the renderer's job). GrappleHookProjectileV2's StaticMesh
    // (SM_BatmanGrappleHook): no override and no graph sets one, the mesh's slot BatmanPropsSimple_MAT. SmokeGrenade's
    // MineMesh (SM_BatmanBomb01): ReceiveBeginPlay makes a dynamic instance of C020_Satchel_MAT on slot 0 (@1521) and
    // sets its vector Color by the instigator's team (@1899: team 0 (20,0,0,1), team 1 (0,0,20,1)), so that material,
    // not the mesh's own BatmanPropsSimple_MAT.
    Row::Actor(ActorRow {
        cls: "SM_BatarangV2",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/BatmanV2/Mesh/SM_BatarangV2",
        materials: &["Panda_Main/Characters/BatmanV2/Mesh/BatmanBatarangV1_MESH_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "SM_BatmanGrappleHook",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/Batman/VFX/SM_BatmanGrappleHook",
        materials: &["Panda_Main/Characters/Batman/VFX/BatmanPropsSimple_MAT"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "SM_BatmanBomb01",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/Batman/VFX/SM_BatmanBomb01",
        materials: &["Panda_Main/Characters/C020/VFX/C020_Satchel_MAT"],
        variants: None,
        is_static: true,
    }),
    // The pawn's SM_DebrisBallProjectile (SM_Bombarang_Bomb, tag SatchelCharge, the held bomb): its slot is
    // Bombarang_MAT, and OnMeshActivated makes a dynamic instance of BombarangMesh_MIC on slot 0 (Pawn_BatmanV2 @5388)
    // with the scalar Team (@5603; the renderer's job).
    Row::Actor(ActorRow {
        cls: "SM_Bombarang_Bomb",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/Batman/VFX/SM_Bombarang_Bomb",
        materials: &["Panda_Main/Characters/Batman/VFX/BombarangMesh_MIC"],
        variants: None,
        is_static: true,
    }),
    // Batman's podium defeat (Character_Batman DefaultDefeatMontage = Batman_GunBreak_Taunt_Montage):
    // AnimNotify_SubMeshSpawn's three StaticMeshes, no Materials / SkinOverride on the notifies, so each mesh's one
    // slot (Ak_MAT) with Batman_AK_Color_Mat (unlit).
    Row::Actor(ActorRow {
        cls: "BatmanGun",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/BatmanV2/SubMeshes/BatmanGun",
        materials: &["Panda_Main/Characters/BatmanV2/SubMeshes/Batman_AK_Color_Mat"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "BatmanGun_Broken_Front",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/BatmanV2/SubMeshes/BatmanGun_Broken_Front",
        materials: &["Panda_Main/Characters/BatmanV2/SubMeshes/Batman_AK_Color_Mat"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "BatmanGun_Broken_Back",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/BatmanV2/SubMeshes/BatmanGun_Broken_Back",
        materials: &["Panda_Main/Characters/BatmanV2/SubMeshes/Batman_AK_Color_Mat"],
        variants: None,
        is_static: true,
    }),
    // His UtilityBelt taunt (Batman_Taunt_UtilityBelt_Montage, a taunt slot's): AnimNotify_SubMeshSpawn's three
    // StaticMeshes, no Materials / SkinOverride on the notifies, so each mesh's one slot (MAT_BatItem) with
    // Batman_Belt_Items_Color_Mat.
    Row::Actor(ActorRow {
        cls: "Batman_Radio",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/BatmanV2/SubMeshes/Batman_Radio",
        materials: &["Panda_Main/Characters/BatmanV2/SubMeshes/Batman_Belt_Items_Color_Mat"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Batman_Remote",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/BatmanV2/SubMeshes/Batman_Remote",
        materials: &["Panda_Main/Characters/BatmanV2/SubMeshes/Batman_Belt_Items_Color_Mat"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Batman_SharkRepelent",
        owner: &["batman"],
        mesh: "Panda_Main/Characters/BatmanV2/SubMeshes/Batman_SharkRepelent",
        materials: &["Panda_Main/Characters/BatmanV2/SubMeshes/Batman_Belt_Items_Color_Mat"],
        variants: None,
        is_static: true,
    }),
    // Reindog: TetherProjectile's TetherObject, SM_heart with the template's OverrideMaterials Creature_Mat
    // (Characters/Common/Materials); no graph of the tether sets a material on it.
    Row::Actor(ActorRow {
        cls: "SM_heart",
        owner: &["creature"],
        mesh: "Panda_Main/Characters/Common/SM_heart",
        materials: &["Panda_Main/Characters/Common/Materials/Creature_Mat"],
        variants: None,
        is_static: true,
    }),
    // Black Adam's podium victory (C021_S00 VictoryMontageOverride = C021_Emote_Victory_Montage): AnimNotify_SubMeshSpawn's
    // StaticMesh C021_Throne, no Materials / SkinOverride on the notify, so the mesh's one slot (C021_Throne_M) with
    // C021_Throne_01_MAT.
    Row::Actor(ActorRow {
        cls: "C021_Throne",
        owner: &["blackadam"],
        mesh: "Panda_Main/Characters/C021/C021_Throne",
        materials: &["Panda_Main/Characters/C021/C021_Throne_01_MAT"],
        variants: None,
        is_static: true,
    }),
    // Taz's podium victory (C015_Emote_Victory_Montage): AnimNotify_SubMeshSpawn's C015_Bouquet_SkelMesh and StaticMesh
    // C015_BouquetPetal_Mesh, no Materials on the notifies, so each mesh's one slot, C015_Flowers_MIC_Local.
    Row::Actor(ActorRow {
        cls: "C015_Bouquet_SkelMesh",
        owner: &["taz"],
        mesh: "Panda_Main/Characters/C015/C015_Bouquet_SkelMesh",
        materials: &["Panda_Main/Characters/C015/C015_Flowers_MIC_Local"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C015_BouquetPetal_Mesh",
        owner: &["taz"],
        mesh: "Panda_Main/Characters/C015/C015_BouquetPetal_Mesh",
        materials: &["Panda_Main/Characters/C015/C015_Flowers_MIC_Local"],
        variants: None,
        is_static: true,
    }),
    // Arya's podium defeat (Arya_Emote_Defeat_Montage): AnimNotify_SubMeshSpawn's Arya_Cloth_SkelMesh, no Materials on
    // the notify, so the mesh's one slot, AryaCloth_MIC.
    Row::Actor(ActorRow {
        cls: "Arya_Cloth_SkelMesh",
        owner: &["arya"],
        mesh: "Panda_Main/Characters/Arya/Arya_Cloth_SkelMesh",
        materials: &["Panda_Main/Characters/Arya/AryaCloth_MIC"],
        variants: None,
        is_static: false,
    }),
    // Rick's C020_Emote_Schwifty_Montage: its sequence's AnimNotify_SubMeshSpawn (C020_Emote_Schwifty
    // AnimNotify_SubMeshSpawn_C_0) spawns StaticMesh C020_MicProp with no Materials / SkinOverride, so BaseFighter Add
    // Montage Mesh's SetMaterial loop (@3965-4137) runs over an empty list and the mesh keeps its one slot (lambert2):
    // the parent Material C020_MicProp_MAT, not the skin's SubMeshes "Mic" C020_MicProp_MAT_Inst (the C020_S00.Mic entry).
    Row::Actor(ActorRow {
        cls: "C020_MicProp",
        owner: &["rick"],
        mesh: "Panda_Main/Characters/C020/C020_MicProp",
        materials: &["Panda_Main/Characters/C020/C020_MicProp_MAT"],
        variants: None,
        is_static: true,
    }),
    // Bugs Bunny's skin's static SubMeshes (Pawn_BugsBunny's StaticMeshComponents Rocket, AcmeBox, Breifcase, Target,
    // Pie, Brush, PieProp1): skinStatics below, keyed '<skin>.<SubMeshes key>' (Pie and PieProp1 share a mesh, and
    // AryaKillPie's entry above has the pie's own slot).
    Row::SkinStatics("bugs"),
    // His actors' StaticMesh components, keyed by the mesh. Each class is a
    // SpawnedActorParent whose SetTeamColor (BeginPlay @8857 -> @13150; the children's overrides call it first:
    // PieProjectile @1224, BugsBunnySafe @10702) puts SelectMaterialColor(the class default's Materials set) on slot 0 of
    // the actor's one StaticMeshComponent (@20946-21922), over the template's OverrideMaterials: `materials` the set's
    // LocalPlayerMaterialOverride, `variants` its Red / Blue. The sets: BugsBunnySafe Damaged_MIC /
    // Damaged_Red_MIC / Damaged_Blue_MIC (the Visual's BugsBunnyProps_Damaged_MAT_Inst is never seen); BugsBunny_SafeDrop
    // (the target) Props_Local_MIC / Props_Red_MIC / BugsBunnyProps_Damaged_Blue_MIC (as the class default names them);
    // BugsBunnyRocketF / UpProjectile Props_Local / Red / Blue_MIC; PieProjectile Props_Local / Red / Blue_MIC on its own
    // entry, keyed 'PieProjectile_C.StaticMesh' and drawn with BugsBunny_Prop_Pie's glTF (AryaKillPie and
    // Projectile_Neptr_Pie have an empty set and keep the mesh's slot: the BugsBunny_Prop_Pie entry above).
    Row::Actor(ActorRow {
        cls: "BugsBunny_Prop_Safe",
        owner: &["bugs"],
        mesh: "Panda_Main/Characters/BugsBunnyV2/BugsBunny_Prop_Safe",
        materials: &["Panda_Main/Characters/BugsBunnyV2/VFX/BugsBunnyProps_Damaged_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/BugsBunnyV2/VFX/BugsBunnyProps_Damaged_Red_MIC"]),
            ("blue", &["Panda_Main/Characters/BugsBunnyV2/VFX/BugsBunnyProps_Damaged_Blue_MIC"]),
        ]),
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "BugsBunny_Prop_Target",
        owner: &["bugs"],
        mesh: "Panda_Main/Characters/BugsBunnyV2/BugsBunny_Prop_Target",
        materials: &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Local_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Red_MIC"]),
            ("blue", &["Panda_Main/Characters/BugsBunnyV2/VFX/BugsBunnyProps_Damaged_Blue_MIC"]),
        ]),
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "BugsBunny_Prop_Rocket_02",
        owner: &["bugs"],
        mesh: "Panda_Main/Characters/BugsBunnyV2/BugsBunny_Prop_Rocket_02",
        materials: &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Local_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Red_MIC"]),
            ("blue", &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Blue_MIC"]),
        ]),
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "PieProjectile_C.StaticMesh",
        owner: &["bugs"],
        mesh: "Panda_Main/Characters/BugsBunnyV2/BugsBunny_Prop_Pie",
        materials: &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Local_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Red_MIC"]),
            ("blue", &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Blue_MIC"]),
        ]),
        is_static: true,
    }),
    // Item_Dynamite's Geo (OverrideMaterials [null]: the mesh's slot BugsBunnyPropsMat until SetTeamColor):
    // SetTeamColor @2669-2809 Geo.SetMaterial(0, IsLocalOnline ? BugsBunny_Props_Local_MIC : Round(the owner's colour
    // value) 0 ? BugsBunny_Props_Red_MIC : 1 ? BugsBunny_Props_Blue_MIC).
    Row::Actor(ActorRow {
        cls: "BugsBunny_Prop_Dynamite",
        owner: &["bugs"],
        mesh: "Panda_Main/Characters/BugsBunnyV2/BugsBunny_Prop_Dynamite",
        materials: &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Local_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Red_MIC"]),
            ("blue", &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Blue_MIC"]),
        ]),
        is_static: true,
    }),
    // BugsBunny_Emote_RabbitOutOfTheHat_Montage (Pawn_BugsBunny's IntroAnimation): AnimNotify_SubMeshSpawn's StaticMesh
    // BugsBunny_Prop_TopHat on fml_un_C_prop1, no Materials / SkinOverride on the notify: the mesh's slot,
    // BugsBunny_Props_Local_MIC.
    Row::Actor(ActorRow {
        cls: "BugsBunny_Prop_TopHat",
        owner: &["bugs"],
        mesh: "Panda_Main/Characters/BugsBunnyV2/BugsBunny_Prop_TopHat",
        materials: &["Panda_Main/Characters/BugsBunnyV2/BugsBunny_Props_Local_MIC"],
        variants: None,
        is_static: true,
    }),
    // The ACME box's helpers' skeletal meshes, keyed by the mesh.
    // ItemHelper_Nepter's Neptr_Skelmesh: no override on the template; BeginPlay @2017-2141 makes a dynamic instance of
    // slot 0's own material (Neptr_MAT) and sets its scalars TredAlpha and Light (the view's job). ItemHelper_BananaGuard's
    // CharacterMesh0 (BananaGuard_Skelmesh): no override and no graph sets one, the mesh's slot BananaGuard_MAT.
    // SelfActivate_Meeseeks_Run's Meeseeks_SkelMesh: the slot's Meeseeks_MAT until Meeseeks_Base SetTeamColor @3355-3420
    // sets slot 0 by FTrunc(the owner's BPI_GetColorValue): 0 Meeseeks_Red_MIC, 1 Meeseeks_Blue_MIC (the view picks).
    Row::Actor(ActorRow {
        cls: "Neptr_Skelmesh",
        owner: &["bugs"],
        mesh: "Panda_Main/Characters/ItemHelpers/Neptr/Neptr_Skelmesh",
        materials: &["Panda_Main/Characters/ItemHelpers/Neptr/Neptr_MAT"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "BananaGuard_Skelmesh",
        owner: &["bugs"],
        mesh: "Panda_Main/Characters/ItemHelpers/BananaGuard/BananaGuard_Skelmesh",
        materials: &["Panda_Main/Characters/ItemHelpers/BananaGuard/BananaGuard_MAT"],
        variants: None,
        is_static: false,
    }),
    // Item_Meeseek's MeeseekBox (SM_MeeseeksBox): OverrideMaterials [null] and no graph sets one, the mesh's slot
    // 0_MeeseeksProp_MIC. Rick's too: his Meeseeks emote's AnimNotify_SubMeshSpawn and his prop of that tag draw the
    // box (render/chars/rick montageMeshes, PROPS.meseeks).
    Row::Actor(ActorRow {
        cls: "SM_MeeseeksBox",
        owner: &["bugs", "rick"],
        mesh: "Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/SM_MeeseeksBox",
        materials: &["Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/0_MeeseeksProp_MIC"],
        variants: None,
        is_static: true,
    }),
    // Item_Helper's Item_TV_Mesh (the TV, the ACME box's item): OverrideMaterials none and no graph sets one, the mesh's
    // two slots: Tv_Mat (TV_Color_MAT, the set) and TV_Static_MAT (the screen, unlit).
    Row::Actor(ActorRow {
        cls: "Item_TV_Mesh",
        owner: &["bugs"],
        mesh: "Panda_Main/Blueprints/Items/Item_TV/Item_TV_Mesh",
        materials: &["Panda_Main/Blueprints/Items/Item_TV/Tv_Mat", "Panda_Main/Blueprints/Items/Item_TV/TV_Static_MAT"],
        variants: None,
        is_static: true,
    }),
    // Rick's Meeseeks actors (Meeseeks_Base children, Meeseeks_SpawnedActor, MeeSeeks_AirFlap) draw the same mesh:
    // Meeseeks_SpawnedActor SetTeamColor @4900 Meeseeks_SkelMesh.SetMaterial(0, by the colour index) as Meeseeks_Base's.
    Row::Actor(ActorRow {
        cls: "Meeseeks_SkelMesh",
        owner: &["bugs", "rick"],
        mesh: "Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/Meeseeks_SkelMesh",
        materials: &["Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/Meeseeks_MAT"],
        variants: Some(&[
            ("red", &["Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/Meeseeks_Red_MIC"]),
            ("blue", &["Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/Meeseeks_Blue_MIC"]),
        ]),
        is_static: false,
    }),
    // Tutorial_Meeseeks's SM_RickJetBootleft / Right (SM_JetBoot on CharacterMesh0's fml_un_L / R_lolegTwist_att,
    // shown by ConvertToFlyingMeeseeks): no override on the templates and no graph sets one, the mesh's slot JetBoot_MAT.
    // The tutorial draws its Meeseeks with Rick's assets.
    Row::Actor(ActorRow {
        cls: "SM_JetBoot",
        owner: &["rick"],
        mesh: "Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/SM_JetBoot",
        materials: &["Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/JetBoot_MAT"],
        variants: None,
        is_static: true,
    }),
    // Rick's skin's static SubMeshes (Pawn_C020's StaticMeshComponents Blaster, MeeseeksBox, PortalGun, PolymorphGun,
    // Mic, Megaseed, shown by his montages' AnimNotify_Equip MeshTag), keyed '<skin>.<SubMeshes key>'.
    Row::SkinStatics("rick"),
    // His actors' meshes, keyed by the mesh. Meeseeks_SpawnedActor's golfClub
    // (no override: the slot's C020_prop2_MIC); FartBomb_SpawnedActor's Mesh (no override: C020_prop2_MIC);
    // the portals' PortalMesh (OverrideMaterials [C020Portal00_Mesh_MAT_Inst] over the mesh's WorldGridMaterial);
    // PolymorphGrenade's MineMesh (OverrideMaterials [null, null]: the slot's C020_Satchel_MAT). No graph of theirs
    // sets a material.
    Row::Actor(ActorRow {
        cls: "MrMeeseeks_GolfClub_SkelMesh",
        owner: &["rick"],
        mesh: "Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/MrMeeseeks_GolfClub_SkelMesh",
        materials: &["Panda_Main/Characters/C020/C020_prop2_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C020_fartbomb_SkelMesh",
        owner: &["rick"],
        mesh: "Panda_Main/Characters/C020/C020_fartbomb_SkelMesh",
        materials: &["Panda_Main/Characters/C020/C020_prop2_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "SM_C020Portal",
        owner: &["rick"],
        mesh: "Panda_Main/Characters/C020/VFX/SM_C020Portal",
        materials: &["Panda_Main/Characters/C020/VFX/C020Portal00_Mesh_MAT_Inst"],
        variants: None,
        is_static: true,
    }),
    // C020PortalAimer's own gun: a StaticMeshComponent it adds @6596, SetStaticMesh(SM_C020PortalGun) @6698,
    // attached to the fighter's Mesh at L_Hand @6874 / @9657, filed with MeshManager.AddMesh @7148. No graph sets a
    // material, so the mesh's own slots draw: Rick_PortalGun_Mat 0_MeeseeksProp_MIC and gas_mat the base Material
    // C020PortalGunGlass_MAT (not the skin's _Inst of its PortalGun SubMesh).
    Row::Actor(ActorRow {
        cls: "SM_C020PortalGun",
        owner: &["rick"],
        mesh: "Panda_Main/Characters/C020/VFX/SM_C020PortalGun",
        materials: &[
            "Panda_Main/Blueprints/Tutorial/TempMeeseeksMove/0_MeeseeksProp_MIC",
            "Panda_Main/Characters/C020/VFX/C020PortalGunGlass_MAT",
        ],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "SM_C020Satchel",
        owner: &["rick"],
        mesh: "Panda_Main/Characters/C020/VFX/SM_C020Satchel",
        materials: &["Panda_Main/Characters/C020/VFX/C020_Satchel_MAT"],
        variants: None,
        is_static: true,
    }),
    // Taz's skin's static SubMeshes (Pawn_C015's StaticMeshComponents SoupPot, PepperShaker, Salad, SaltShaker).
    Row::SkinStatics("taz"),
    // C015Trash.StaticMesh (Finn's Apple): ExecuteUbergraph @304 SetMaterial(0, ...): IsLocalOnline Finn_Apple_D_Mat,
    // else colour index 0 Finn_Apple_Red_D_Mat, 1 Finn_Apple_Blue_D_Mat. Finn's FinnDashThrownTarget the same: its weak
    // throw's SetStaticMesh(Apple) @3508 and TargetedMulticast @374-477 SetMaterial(0, ...) with the same three.
    Row::Actor(ActorRow {
        cls: "Apple",
        owner: &["taz", "finn"],
        mesh: "Panda_Main/Characters/Finn/Submeshes/Apple",
        materials: &["Panda_Main/Characters/Finn/Submeshes/Finn_Apple_D_Mat"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/Finn/Submeshes/Finn_Apple_Red_D_Mat"]),
            ("blue", &["Panda_Main/Characters/Finn/Submeshes/Finn_Apple_Blue_D_Mat"]),
        ]),
        is_static: true,
    }),
    // Buff_Chicken's Polymorph Mesh: its Materials (Red / Blue / LocalPlayer) are all Chicken_MIC_Local. Ahead of
    // the drumstick, whose static mesh draws the same material (built again under its #static key).
    Row::Actor(ActorRow {
        cls: "Chicken_SkelMesh",
        owner: &["taz"],
        mesh: "Panda_Main/Characters/C015/Chicken/Chicken_SkelMesh",
        materials: &["Panda_Main/Characters/C015/Chicken/Chicken_MIC_Local"],
        variants: None,
        is_static: false,
    }),
    // C015_HealPickup.Chicken_Drumstick (no override: the slot's Chicken_MIC_Local); no graph of its sets a material.
    Row::Actor(ActorRow {
        cls: "Chicken_Drumstick",
        owner: &["taz"],
        mesh: "Panda_Main/Characters/C015/Chicken/Chicken_Drumstick",
        materials: &["Panda_Main/Characters/C015/Chicken/Chicken_MIC_Local"],
        variants: None,
        is_static: true,
    }),
    // C015Anvil.Visual (the EatProjectiles perk's anvil; its class data is the perk part's, the taz perk part):
    // OverrideMaterials C015_Props_MIC_Local; SpawnedActorParent SetTeamColor (@22710 -> @21872, its first
    // StaticMeshComponent, slot 0) puts the instigator's pick of Materials (Red C015_Props_MIC_Red, Blue _Blue, Local _Local).
    Row::Actor(ActorRow {
        cls: "C015_Anvil_Mesh_C015_anvil",
        owner: &["taz"],
        mesh: "Panda_Main/Characters/C015/C015_Anvil_Mesh_C015_anvil",
        materials: &["Panda_Main/Characters/C015/C015_Props_MIC_Local"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/C015/C015_Props_MIC_Red"]),
            ("blue", &["Panda_Main/Characters/C015/C015_Props_MIC_Blue"]),
        ]),
        is_static: true,
    }),
    // Steven: Pawn_StevenUniverse's StaticMeshComponents PinkShield / PinkShield2, PinkBubble and PinkBarrier (shown by
    // AnimNotify_Equip MeshTag; Comp_Pawn_MeshManager files them by ComponentTags), keyed
    // by the mesh: no OverrideMaterials on the components, no skin SubMeshes entry and no graph sets a material, so
    // each mesh's one slot, Steven_PropBase_MIC.
    Row::Actor(ActorRow {
        cls: "Steven_Shield_SkelMesh",
        owner: &["steven"],
        mesh: "Panda_Main/Characters/Steven/Steven_Shield_SkelMesh",
        materials: &["Panda_Main/Characters/Steven/Steven_PropBase_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Steven_Bubble_SkelMesh",
        owner: &["steven"],
        mesh: "Panda_Main/Characters/Steven/Steven_Bubble_SkelMesh",
        materials: &["Panda_Main/Characters/Steven/Steven_PropBase_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Steven_Barrier_SkelMesh",
        owner: &["steven"],
        mesh: "Panda_Main/Characters/Steven/Steven_Barrier_SkelMesh",
        materials: &["Panda_Main/Characters/Steven/Steven_PropBase_MIC"],
        variants: None,
        is_static: true,
    }),
    // Steven_ShieldActor's SkeletalMesh (Steven_ShieldActor_Up is its child with the same component), keyed by the mesh:
    // slot 0 the mesh's MI_Shield_Cascade_MESH; slot 1 SetTeamColor (ubergraph @2381-2484) SetMaterial(1, ...): the
    // instigator's IsLocalOnline StevenShield_MESH_MAT_Inst_Local, else colour index 0 _Red, 1 _Blue.
    Row::Actor(ActorRow {
        cls: "Steven_SeparateShield_SkelMesh",
        owner: &["steven"],
        mesh: "Panda_Main/Characters/Steven/Steven_SeparateShield_SkelMesh",
        materials: &[
            "Panda_Main/Characters/Steven/VFX/MI_Shield_Cascade_MESH",
            "Panda_Main/Characters/Steven/VFX/StevenShield_MESH_MAT_Inst_Local",
        ],
        variants: Some(&[
            (
                "red",
                &[
                    "Panda_Main/Characters/Steven/VFX/MI_Shield_Cascade_MESH",
                    "Panda_Main/Characters/Steven/VFX/StevenShield_MESH_MAT_Inst_Red",
                ],
            ),
            (
                "blue",
                &[
                    "Panda_Main/Characters/Steven/VFX/MI_Shield_Cascade_MESH",
                    "Panda_Main/Characters/Steven/VFX/StevenShield_MESH_MAT_Inst_Blue",
                ],
            ),
        ]),
        is_static: false,
    }),
    // AnimNotify_SubMeshSpawn's Steven_Cookie_Static (Steven_Emote_DefaultTaunt_Montage, Steven_Emote_EatCookieCat_Montage):
    // no material on the notifies, the mesh's one slot Steven_Cookie_MIC.
    Row::Actor(ActorRow {
        cls: "Steven_Cookie_Static",
        owner: &["steven"],
        mesh: "Panda_Main/Characters/Steven/Steven_Cookie_Static",
        materials: &["Panda_Main/Characters/Steven/Steven_Cookie_MIC"],
        variants: None,
        is_static: true,
    }),
    // Garnet: AnimNotify_SubMeshSpawn's meshes of her emotes, each on a skeleton of its
    // own; no Materials / SkinOverride on the notifies, so each mesh's own slots: Garnet_Emote_Intro_Gems (the intro)
    // GarnetGem_Triangle_MIC and GarnetGem_Square_MIC, Butterfly_Skelmesh (the defeat, the podium) Butterfly_MAT,
    // Garnet_TinyCar_SkelMesh (the TinyCar taunt) Garnet_TinyCar_MIC.
    Row::Actor(ActorRow {
        cls: "Garnet_Emote_Intro_Gems",
        owner: &["garnet"],
        mesh: "Panda_Main/Characters/Garnet/Garnet_Emote_Intro_Gems",
        materials: &[
            "Panda_Main/Characters/Garnet/VFX/GarnetGem_Triangle_MIC",
            "Panda_Main/Characters/Garnet/VFX/GarnetGem_Square_MIC",
        ],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Butterfly_Skelmesh",
        owner: &["garnet"],
        mesh: "Panda_Main/Characters/Garnet/Animations/Emotes/Butterfly_Skelmesh",
        materials: &["Panda_Main/Characters/Garnet/Animations/Emotes/Butterfly_MAT"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Garnet_TinyCar_SkelMesh",
        owner: &["garnet"],
        mesh: "Panda_Main/Characters/Garnet/Garnet_TinyCar_SkelMesh",
        materials: &["Panda_Main/Characters/Garnet/Materials/Garnet_TinyCar_MIC"],
        variants: None,
        is_static: false,
    }),
    // Finn: his actors' meshes. No OverrideMaterials on their components and no
    // SetMaterial in BMO_FallowActor's, Finn_Shop's or FinnTreasure's graphs (FinnTreasure's SetTeamColor @2745-3096
    // sets its sparkle's Team parameter only): each mesh's own slot, Bmo_Material, ChooseGoose_MAT, FinnCoin_MAT_Inst.
    Row::Actor(ActorRow {
        cls: "Bmo_SkeletalMesh",
        owner: &["finn"],
        mesh: "Panda_Main/Characters/Finn/Submeshes/Bmo/Bmo_SkeletalMesh",
        materials: &["Panda_Main/Characters/Finn/Submeshes/Bmo/Bmo_Material"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "ChooseGoose_SkelMesh",
        owner: &["finn"],
        mesh: "Panda_Main/Characters/Finn/Submeshes/ChooseGoose/ChooseGoose_SkelMesh",
        materials: &["Panda_Main/Characters/Finn/Submeshes/ChooseGoose/ChooseGoose_MAT"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "SkelMesh_FinnCoin",
        owner: &["finn"],
        mesh: "Panda_Main/Characters/Finn/Submeshes/SkelMesh_FinnCoin",
        materials: &["Panda_Main/Characters/Finn/VFX/FinnCoin_MAT_Inst"],
        variants: None,
        is_static: false,
    }),
    // Tom and Jerry's dynamite (Dynamite_SpawnedActor and its CantSplitDynamite child, Projectiles/TomAndJerry):
    // DynamiteMesh (SkeletalMesh Dynamite, OverrideMaterials [Red]) @5507 SetMaterial(0, ...): IsLocalOnline Yellow
    // (Characters/Common/Materials), else colour index 0 Tom_and_Jerry_Prop_Red_MIC, 1 _Blue_MIC. Here, ahead of Finn's
    // Gem: Yellow is drawn on this skeletal mesh and on that static one, and the skeletal entry has to come first (the
    // static one then takes Yellow#static). The Dynamite montage's prop (AnimNotify_SubMeshSpawn) is the entry
    // 'Dynamite' with the Tom and Jerry entries at the end.
    Row::Actor(ActorRow {
        cls: "Dynamite_SpawnedActor_C",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Dynamite/Dynamite",
        materials: &["Panda_Main/Characters/Common/Materials/Yellow"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_Red_MIC"]),
            ("blue", &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_Blue_MIC"]),
        ]),
        is_static: false,
    }),
    // FinnDashThrownTarget.StaticMesh (Gem, slot Red): TargetedMulticast @3740-3968 SetMaterial(0, ...): IsLocalOnline
    // Yellow, else colour index 0 Red, 1 Blue (Characters/Common/Materials).
    Row::Actor(ActorRow {
        cls: "Gem",
        owner: &["finn"],
        mesh: "Panda_Main/Characters/Finn/Submeshes/Gem",
        materials: &["Panda_Main/Characters/Common/Materials/Yellow"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/Common/Materials/Red"]),
            ("blue", &["Panda_Main/Characters/Common/Materials/Blue"]),
        ]),
        is_static: true,
    }),
    // Pawn_Finn's TopTooth1-8 / BotTooth1-8 (StaticMeshComponents, no OverrideMaterials): Finn_Tooth's slot FinnBase_MIC.
    Row::Actor(ActorRow {
        cls: "Finn_Tooth",
        owner: &["finn"],
        mesh: "Panda_Main/Characters/Finn/Submeshes/Finn_Tooth",
        materials: &["Panda_Main/Characters/Finn/FinnBase_MIC"],
        variants: None,
        is_static: true,
    }),
    // Jake's stretch (JakeStretch, Projectiles/): the lower body Jake_BotHalf_SkelMesh takes the pawn mesh's slot 0
    // (@5898-5971 SetMaterial(0, Mesh.GetMaterial(0))), each spline body piece a dynamic instance of
    // Comp_Pawn_SkinManager.GetMaterialForSkin(0) (@2976-3048): both the skin's Materials[0], JakeBase_MIC for the local
    // player, Jake_Base_RED_MIC / _BLUE_MIC by colour index.
    Row::Actor(ActorRow {
        cls: "Jake_BotHalf_SkelMesh",
        owner: &["jake"],
        mesh: "Panda_Main/Characters/Jake/Mesh/Jake_BotHalf_SkelMesh",
        materials: &["Panda_Main/Characters/Jake/Mesh/JakeBase_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/Jake/Jake_Base_RED_MIC"]),
            ("blue", &["Panda_Main/Characters/Jake/Jake_Base_BLUE_MIC"]),
        ]),
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Jake_StretchMesh",
        owner: &["jake"],
        mesh: "Panda_Main/Characters/Jake/Mesh/Jake_StretchMesh",
        materials: &["Panda_Main/Characters/Jake/Mesh/JakeBase_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/Jake/Jake_Base_RED_MIC"]),
            ("blue", &["Panda_Main/Characters/Jake/Jake_Base_BLUE_MIC"]),
        ]),
        is_static: true,
    }),
    // The spit aimers' AimSpline (ProjectileAimer_SpitJake: a dynamic instance of JakeSpitAim_MAT @6446, its Team
    // parameter set @7151; JakeEatAlly_Aim: no SetMaterial, the mesh's own slot, the same JakeSpitAim_MAT).
    Row::Actor(ActorRow {
        cls: "SpitAimer",
        owner: &["jake"],
        mesh: "Panda_Main/Characters/Jake/SubMeshes/SpitAimer",
        materials: &["Panda_Main/Characters/Jake/SubMeshes/JakeSpitAim_MAT"],
        variants: None,
        is_static: true,
    }),
    // Tom and Jerry (key tomandjerry, Characters/TomAndJerry/SubMeshes): the meshes of
    // the montages' AnimNotify_SubMeshSpawn (no Materials / SkinOverride on any of them: each mesh's own slots) and of
    // his actors' components. The skeletal ones first: Tom_and_Jerry_Prop_MIC, Champagne_MAT, Cheese_MAT and
    // Tom_andJerry_PropTemp_Mat are drawn on both kinds.
    // JerryRocket (JerryRocket / JerryKickedRocket RocketMesh; slots lambert1, Tom_and_Jerry_Prop_MAT): @10289 / @7671
    // SetMaterial(0, ...): IsLocalOnline Tom_and_Jerry_Prop_MIC, else colour index 0 _Prop_Red_MIC, 1 _Prop_Blue_MIC;
    // slot 1 keeps its Prop_MIC. JerryRocket_NoRope (TomAimRocket1 RocketMesh, @4812 the same select; Jerry's hidden
    // Rocket component, no SetMaterial). MouseTrap (MouseTrap_SpawnedActor MouseTrapMesh @3692, the same select; the
    // MouseTrap montage's SubMeshSpawn prop: the mesh's own Prop_MIC, the local set).
    Row::Actor(ActorRow {
        cls: "JerryRocket",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Rocket/JerryRocket",
        materials: &[
            "Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC",
            "Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC",
        ],
        variants: Some(&[
            (
                "red",
                &[
                    "Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_Red_MIC",
                    "Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC",
                ],
            ),
            (
                "blue",
                &[
                    "Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_Blue_MIC",
                    "Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC",
                ],
            ),
        ]),
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "JerryRocket_NoRope",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Rocket/JerryRocket_NoRope",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_Red_MIC"]),
            ("blue", &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_Blue_MIC"]),
        ]),
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "MouseTrap",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/MouseTrap/MouseTrap",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_Red_MIC"]),
            ("blue", &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_Blue_MIC"]),
        ]),
        is_static: false,
    }),
    // The skeletal props in their own slots: Dynamite (the Dynamite montage), Tom_FishingPole (Fishing_Ground / _Air),
    // Trash_Lids (Air_Attack_TrashLids), Tennis_Racket (Attack_Tennis / _Air), Tom_Mallet (Combo_01-04, Emote_Hurt_Toe),
    // PaddleBall and PaddleBall_NoBall (PaddleBall_No_Jerry, PaddleBall), SlingShot (the four SlingShot montages):
    // Tom_and_Jerry_Prop_MIC; DefeatFlag (Defeat): Tom_andJerry_PropTemp_Mat; Milk_Bottle (Victory): BottleGlass_MAT,
    // Milk_MAT; Champagne_Mesh (Jerry_SpawnedActor's hidden Champagne): Champagne_MAT; Cheese (MouseTrap_SpawnedActor's
    // Cheese, Cheese_SpawnedActor): Cheese_MAT.
    Row::Actor(ActorRow {
        cls: "Dynamite",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Dynamite/Dynamite",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Tom_FishingPole",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/FishingPole/Tom_FishingPole",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Trash_Lids",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/TrashLids/Trash_Lids",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Tennis_Racket",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/TenisRacket/Tennis_Racket",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Tom_Mallet",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/TomMallet/Tom_Mallet",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "PaddleBall",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/PaddleBall/PaddleBall",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "PaddleBall_NoBall",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/PaddleBall/PaddleBall_NoBall",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "SlingShot",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/SlingShot/SlingShot",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "DefeatFlag",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/DefeatFlag/DefeatFlag",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_andJerry_PropTemp_Mat"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Milk_Bottle",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Milk/Milk_Bottle",
        materials: &[
            "Panda_Main/Characters/TomAndJerry/SubMeshes/Milk/BottleGlass_MAT",
            "Panda_Main/Characters/TomAndJerry/SubMeshes/Milk/Milk_MAT",
        ],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Champagne_Mesh",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Champagne/Champagne_Mesh",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Champagne/Champagne_MAT"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Cheese",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Cheese/Cheese",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Cheese/Cheese_MAT"],
        variants: None,
        is_static: false,
    }),
    // The static ones, each in its own slot (no OverrideMaterials, no SetMaterial): FryingPan (Air_Attack_DownSpike),
    // JerryBomb and TomClub (CharacterSelect, Emote_DefaultTaunt), Lure and LureBase (FishingLure's Lure / LureNoRope):
    // Tom_and_Jerry_Prop_MIC; JoyStick and JoyStick_Pad (Rocket_Ground): Tom_andJerry_PropTemp_Mat; Cheese_Clump and
    // Cheese_SM (Victory): Cheese_MAT; Champagne_Cork (JerryCork): Champagne_MAT; TennisBall (Jerry's hidden TennisBall,
    // the tennis balls): MAT_TennisBall; Tennis_Rachet_Mesh (Jerry's hidden Tenis_Rachet_Mesh): its slot's
    // WorldGridMaterial.
    Row::Actor(ActorRow {
        cls: "FryingPan",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/FyingPan/FryingPan",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "JerryBomb",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/JerryBomb/JerryBomb",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "TomClub",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/TomClub/TomClub",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Lure",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Lure/Lure",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "LureBase",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Lure/LureBase",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_and_Jerry_Prop_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "JoyStick",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/JoyStick/JoyStick",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_andJerry_PropTemp_Mat"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "JoyStick_Pad",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/JoyStick/JoyStick_Pad",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Tom_andJerry_PropTemp_Mat"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Cheese_Clump",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Cheese/Cheese_Clump",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Cheese/Cheese_MAT"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Cheese_SM",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Cheese/Cheese_SM",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Cheese/Cheese_MAT"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Champagne_Cork",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/Champagne/Champagne_Cork",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/Champagne/Champagne_MAT"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "TennisBall",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/TennisBall",
        materials: &["Panda_Main/Characters/TomAndJerry/SubMeshes/MAT_TennisBall"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Tennis_Rachet_Mesh",
        owner: &["tomandjerry"],
        mesh: "Panda_Main/Characters/TomAndJerry/SubMeshes/TenisRacket/Tennis_Rachet_Mesh",
        materials: &["Engine/Content/EngineMaterials/WorldGridMaterial"],
        variants: None,
        is_static: true,
    }),
    // Velma's police (velmapolice): VelmaJenkinsPin's Mesh, no OverrideMaterials: its two slots' own. Ahead of
    // Velma's static props: Velma_JenkinsPin_MIC is the skin's Evidence SubMesh material too (its #static copy).
    Row::Actor(ActorRow {
        cls: "Velma_JenkinsPin_SkelMesh",
        owner: &["velmapolice"],
        mesh: "Panda_Main/Characters/Velma/Velma_JenkinsPin_SkelMesh",
        materials: &[
            "Panda_Main/Characters/Velma/Velma_JenkinsPin_MIC",
            "Panda_Main/Characters/Velma/Velma_JenkinsProfile_Mat",
        ],
        variants: None,
        is_static: false,
    }),
    // Velma: the skin's static SubMeshes (Pawn_Velma's prop StaticMeshComponents: MagnifyingGlass, Flashlight, Megaphone,
    // Vial, Camera, Calculator, Evidence, Lightbulb, Tablet), keyed '<skin>.<SubMeshes key>'.
    Row::SkinStatics("velma"),
    // VelmaEvidence / VelmaEvidenceNoGrav's EvidenceStaticMesh (SM_VelmaClue), keyed by the mesh: the component's
    // OverrideMaterials, its one slot's own material too.
    Row::Actor(ActorRow {
        cls: "SM_VelmaClue",
        owner: &["velma"],
        mesh: "Panda_Main/Characters/Velma/VFX/SM_VelmaClue",
        materials: &["Panda_Main/Characters/Velma/VFX/Velma_Clues_3x3_MESH_MAT_Inst"],
        variants: None,
        is_static: true,
    }),
    // Velma_Emote_DefaultTaunt_Montage's and Velma_Emote_Victory_Montage's AnimNotify_SubMeshSpawn StaticMesh
    // Velma_Flashlight_SkelMesh, keyed by the mesh (the skin's Flashlight SubMesh above is keyed by its tag): no
    // Materials on the notifies, so the mesh's one slot, Velma_Base_Props1_MIC.
    Row::Actor(ActorRow {
        cls: "Velma_Flashlight_SkelMesh",
        owner: &["velma"],
        mesh: "Panda_Main/Characters/Velma/Velma_Flashlight_SkelMesh",
        materials: &["Panda_Main/Characters/Velma/Velma_Base_Props1_MIC"],
        variants: None,
        is_static: true,
    }),
    // Velma_Emote_Defeat_Montage's AnimNotify_SubMeshSpawn meshes (no Materials on the notify: the slot's own).
    Row::Actor(ActorRow {
        cls: "Velma_Broken_Glasses_L_Mesh",
        owner: &["velma"],
        mesh: "Panda_Main/Characters/Velma/Velma_Broken_Glasses_L_Mesh",
        materials: &["Panda_Main/Characters/Velma/Velma_Base_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "Velma_Broken_Glasses_R_Mesh",
        owner: &["velma"],
        mesh: "Panda_Main/Characters/Velma/Velma_Broken_Glasses_R_Mesh",
        materials: &["Panda_Main/Characters/Velma/Velma_Base_MIC"],
        variants: None,
        is_static: true,
    }),
    // Marvin's UFO (C018_Platform, C018_PerkPlatform): ShipMesh and RocketMesh SetMaterial(0, ...) @10615 / @10761, local
    // C018_Props_MIC_Local, colour 0 C018_Props_MIC_Red, 1 C018_Props_MIC_Blue (the SCS override C018_Ship_MIC_Local on
    // the ship's slot 0 is replaced there); the ship's slot 1 keeps its own C018_Props_Transparent_MIC_Local_MIC.
    // C018CannonBlast's rocket mesh the same (@1877). Emote_Intro's SubMeshSpawn ship has no override: the mesh's slots.
    Row::Actor(ActorRow {
        cls: "C018_Spaceship_SkelMesh",
        owner: &["marvin"],
        mesh: "Panda_Main/Characters/C018/C018_Spaceship_SkelMesh",
        materials: &[
            "Panda_Main/Characters/C018/C018_Props_MIC_Local",
            "Panda_Main/Characters/C018/C018_Props_Transparent_MIC_Local_MIC",
        ],
        variants: Some(&[
            (
                "red",
                &[
                    "Panda_Main/Characters/C018/C018_Props_MIC_Red",
                    "Panda_Main/Characters/C018/C018_Props_Transparent_MIC_Local_MIC",
                ],
            ),
            (
                "blue",
                &[
                    "Panda_Main/Characters/C018/C018_Props_MIC_Blue",
                    "Panda_Main/Characters/C018/C018_Props_Transparent_MIC_Local_MIC",
                ],
            ),
        ]),
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C018_SpaceshipRocket_SkelMesh",
        owner: &["marvin"],
        mesh: "Panda_Main/Characters/C018/C018_SpaceshipRocket_SkelMesh",
        materials: &["Panda_Main/Characters/C018/C018_Props_MIC_Local"],
        variants: Some(&[
            ("red", &["Panda_Main/Characters/C018/C018_Props_MIC_Red"]),
            ("blue", &["Panda_Main/Characters/C018/C018_Props_MIC_Blue"]),
        ]),
        is_static: false,
    }),
    // Pawn_C018's hidden props (SkeletalMeshComponents with no OverrideMaterials, shown by tag: C018_AnimBP
    // SetSubmeshVisibility): each mesh's own slots, C018_Props_MIC_Local (the remote's second slot
    // C018_Props_Transparent_MIC_Local_MIC). No team variant: no graph sets their materials.
    Row::Actor(ActorRow {
        cls: "C018_Blaster_gun_SkelMesh",
        owner: &["marvin"],
        mesh: "Panda_Main/Characters/C018/C018_Blaster_gun_SkelMesh",
        materials: &["Panda_Main/Characters/C018/C018_Props_MIC_Local"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C018_Dynamite_stick_SkelMesh",
        owner: &["marvin"],
        mesh: "Panda_Main/Characters/C018/C018_Dynamite_stick_SkelMesh",
        materials: &["Panda_Main/Characters/C018/C018_Props_MIC_Local"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C018_RemoteButton_SkelMesh",
        owner: &["marvin"],
        mesh: "Panda_Main/Characters/C018/C018_RemoteButton_SkelMesh",
        materials: &[
            "Panda_Main/Characters/C018/C018_Props_MIC_Local",
            "Panda_Main/Characters/C018/C018_Props_Transparent_MIC_Local_MIC",
        ],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C018_TimeSpaceGun_SkelMesh",
        owner: &["marvin"],
        mesh: "Panda_Main/Characters/C018/C018_TimeSpaceGun_SkelMesh",
        materials: &["Panda_Main/Characters/C018/C018_Props_MIC_Local"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C018_Trumpet_SkelMesh",
        owner: &["marvin"],
        mesh: "Panda_Main/Characters/C018/C018_Trumpet_SkelMesh",
        materials: &["Panda_Main/Characters/C018/C018_Props_MIC_Local"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C018_UltimatumAnswerer_SkelMesh",
        owner: &["marvin"],
        mesh: "Panda_Main/Characters/C018/C018_UltimatumAnswerer_SkelMesh",
        materials: &["Panda_Main/Characters/C018/C018_Props_MIC_Local"],
        variants: None,
        is_static: false,
    }),
    // Stripe. The saws' StaticMesh Sawblade: no OverrideMaterials on the components and
    // no SetMaterial in their graphs (C023B_SawBlade_EdgeTrace SetTeamColor @10598-10871 sets the Sparks' Team
    // parameter only), the mesh's own C023B_Props_A_MIC. BoomBox_SpawnedActor @1564-1656: a dynamic instance of
    // C023B_Props_BoomboxCrossbow_Base_MIC on slot 0 (the mesh's own; its BaseColor parameter set @29 is the render
    // track's). The emote props (no Materials on the notifies): each mesh's own slots.
    Row::Actor(ActorRow {
        cls: "Sawblade",
        owner: &["stripe"],
        mesh: "Panda_Main/Characters/C023B/SubMeshes/SawBlade/Sawblade",
        materials: &["Panda_Main/Characters/C023B/SubMeshes/C023B_Props_A_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "C023B_Boombox_SkelMesh",
        owner: &["stripe"],
        mesh: "Panda_Main/Characters/C023B/SubMeshes/Boombox/C023B_Boombox_SkelMesh",
        materials: &["Panda_Main/Characters/C023B/SubMeshes/C023B_Props_BoomboxCrossbow_Base_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C023B_Tricycle_SkelMesh",
        owner: &["stripe"],
        mesh: "Panda_Main/Characters/C023B/SubMeshes/Tricycle/C023B_Tricycle_SkelMesh",
        materials: &["Panda_Main/Characters/C023B/SubMeshes/C023B_Props_A_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "Toys_Emote_Taunt",
        owner: &["stripe"],
        mesh: "Panda_Main/Characters/C023B/SubMeshes/Toys/Toys_Emote_Taunt",
        materials: &[
            "Panda_Main/Characters/C023B/SubMeshes/Toys/fml_un_rig_v001_Stipe_Base_STRIPE_Mat",
            "Panda_Main/Characters/C023B/SubMeshes/Toys/lambert2",
            "Panda_Main/Characters/C023B/SubMeshes/Toys/lambert3",
            "Panda_Main/Characters/C023B/SubMeshes/Toys/lambert4",
            "Panda_Main/Characters/C023B/SubMeshes/Toys/M_StripeFace",
            "Panda_Main/Characters/C023B/SubMeshes/Toys/M_Reindog",
        ],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C023B_Eggshell_SkelMesh",
        owner: &["stripe"],
        mesh: "Panda_Main/Characters/C023B/SubMeshes/Eggshell/C023B_Eggshell_SkelMesh",
        materials: &[
            "Panda_Main/Characters/C023B/SubMeshes/Eggshell/C023B_Webbing_MIC",
            "Panda_Main/Characters/C023B/SubMeshes/Eggshell/C023B_Eggshell_MIC",
        ],
        variants: None,
        is_static: false,
    }),
    // Gizmo's car (C023A_Car_SpawnedActor): ReceiveBeginPlay @6369-@6557 SetMaterial(0, a dynamic instance of a select on
    // the owner's GetColorIndex: a local online player C023A_Corvette_Overheat_MIC (also the SCS OverrideMaterials[0]),
    // colour 0 C023A_Props_A_MIC_Red, 1 C023A_Props_A_MIC_Blue, both of SubMeshes/Corvette); slot 1 keeps the mesh's
    // own Windshield_MAT.
    Row::Actor(ActorRow {
        cls: "C023A_Car_SpawnedActor_C",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/Corvette/C023A_Corvette_SkelMesh",
        materials: &[
            "Panda_Main/Characters/C023A/SubMeshes/Corvette/C023A_Corvette_Overheat_MIC",
            "Panda_Main/Characters/C023A/SubMeshes/Corvette/Windshield_MAT",
        ],
        variants: Some(&[
            (
                "red",
                &[
                    "Panda_Main/Characters/C023A/SubMeshes/Corvette/C023A_Props_A_MIC_Red",
                    "Panda_Main/Characters/C023A/SubMeshes/Corvette/Windshield_MAT",
                ],
            ),
            (
                "blue",
                &[
                    "Panda_Main/Characters/C023A/SubMeshes/Corvette/C023A_Props_A_MIC_Blue",
                    "Panda_Main/Characters/C023A/SubMeshes/Corvette/Windshield_MAT",
                ],
            ),
        ]),
        is_static: false,
    }),
    // C023A_Emote_Victory_Montage's AnimNotify_SubMeshSpawn props, keyed by the mesh (no Materials on the notifies: the
    // meshes' own slots). The skeletal ones first: C023A_Props_B_MIC is the TV's too.
    // C023A_Popcorn_SkelMesh is the skin's Popcorn SubMesh too (keyed by the skin there); here by the mesh.
    Row::Actor(ActorRow {
        cls: "C023A_Popcorn_SkelMesh",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/C023A_Popcorn_SkelMesh",
        materials: &["Panda_Main/Characters/C023A/SubMeshes/C023A_Props_B_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C023A_PopcornKernel_SkelMesh",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/C023A_PopcornKernel_SkelMesh",
        materials: &["Panda_Main/Characters/C023A/SubMeshes/C023A_Props_B_MIC"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C023A_WallClock",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/C023A_WallClock",
        materials: &[
            "Panda_Main/Characters/C023A/SubMeshes/C023A_Props_B_MIC",
            "Panda_Main/Characters/C023A/SubMeshes/ClockGlass_MAT",
        ],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C023A_Glasses_SkelMesh",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/C023A_Glasses_SkelMesh",
        materials: &["Panda_Main/Characters/lambert1", "Panda_Main/Characters/C023A/SubMeshes/C023A_3DGlassesLens_MAT"],
        variants: None,
        is_static: false,
    }),
    Row::Actor(ActorRow {
        cls: "C023A_Beanbag",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/C023A_Beanbag",
        materials: &["Panda_Main/Characters/C023A/SubMeshes/C023A_Props_A_MIC"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "C023A_TV",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/C023A_TV",
        materials: &["Panda_Main/Characters/C023A/SubMeshes/C023A_Props_B_MIC"],
        variants: None,
        is_static: true,
    }),
    // The arrow: C023A_Arrow_Projectile's StaticMesh (SCS OverrideMaterials[0] C023A_B_Props_EOG_MIC, a Skin01 material,
    // slot 1 its own C023A_Props_A_MIC; no graph sets one), and Pawn_C023A's ArrowCharge (no override: the mesh's slots),
    // keyed by the mesh.
    Row::Actor(ActorRow {
        cls: "C023A_Arrow_Projectile_C",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/Arrow/C023A_Arrow_Mesh",
        materials: &[
            "Panda_Main/Characters/C023A/Skins/Skin01/Materials/C023A_B_Props_EOG_MIC",
            "Panda_Main/Characters/C023A/SubMeshes/C023A_Props_A_MIC",
        ],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "C023A_Arrow_Mesh",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/Arrow/C023A_Arrow_Mesh",
        materials: &[
            "Panda_Main/Characters/C023A/Skins/Skin01/Materials/C023A_B_Props_EOG_MIC",
            "Panda_Main/Characters/C023A/SubMeshes/C023A_Props_A_MIC",
        ],
        variants: None,
        is_static: true,
    }),
    // The music StaticMeshes: C023A_MusicPickup's MusicMesh, C023A_Music_Projectile's and C023A_PianoNote's StaticMesh,
    // each with SCS OverrideMaterials blackMat (Characters/BugsBunnyV2); no graph sets one.
    Row::Actor(ActorRow {
        cls: "C023A_MusicPickup_C",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/MusicNotes/MusicNote_01",
        materials: &["Panda_Main/Characters/BugsBunnyV2/blackMat"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "C023A_Music_Projectile_C",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/MusicNotes/MusicNote_02",
        materials: &["Panda_Main/Characters/BugsBunnyV2/blackMat"],
        variants: None,
        is_static: true,
    }),
    Row::Actor(ActorRow {
        cls: "C023A_PianoNote_C",
        owner: &["gizmo"],
        mesh: "Panda_Main/Characters/C023A/SubMeshes/MusicNotes/MusicNote_TrebleClef",
        materials: &["Panda_Main/Characters/BugsBunnyV2/blackMat"],
        variants: None,
        is_static: true,
    }),
];
