//! The heart skins: the nine skins whose main mesh slot 0 material is a child of
//! M_Character_Hearts (C002_S05, WonderWoman_S06, C008_S05, C003_S06, C004_S06, C021_S03, C012_S06, C014_S05,
//! C018_S04). The view's half: the material scalar the pawn's HeartSkin_SetOpacity writes (a pose tape records it); the
//! petals are mvs-render's fx/shared/heart_skin.rs.
//!
//! Presentation of a skin, so the render's alone: the sim keeps the default skin and carries none of this.
//!
//! Nine pawns define HeartSkin_Initialize and HeartSkin_SetOpacity, each its own copy (the [`RULES`] table, one entry
//! per pawn class). HeartSkin_Initialize (OnSkinApplied; Pawn_Velma @5-2420):
//!   - the gate @5-184: GetObjectName(Mesh.GetMaterial(0).GetBaseMaterial()) == "M_Character_Hearts", else nothing. The
//!     port reads it as "slot 0's instance has the scalar Skin BaseColor Opacity Max": among the built character
//!     materials exactly the M_Character_Hearts children list it.
//!   - @194-477: HeartSkinOpacityMax / Min = slot 0's K2_GetScalarParameterValue("Skin BaseColor Opacity Max" / "Min").
//!   - the petals (Spawn Attached Particles into Character Particles) and their Team @1547 are fx's.
//!
//! ReceiveTick (Pawn_Velma @15-83: skipped while HeartSkinOpacityMax == 0) -> HeartSkin_SetOpacity (@0-1889):
//!   - moving or falling @191: SetScalarParameterValueOnMaterials("Skin BaseColor Opacity Max", HeartSkinOpacityMin) on
//!     the entry's components, and the petals stop and hide (fx);
//!   - else @562: the same scalar = HeartSkinOpacityMax, and the petals start over (fx).
//!     The scalar is set only when the state changes (the material's preshaders run again only then).
//!
//! The moving / falling reads: most pawns read their AnimBP's Moving and IsFalling, which BlueprintUpdateAnimation
//! writes as CharacterMovement Velocity.Y != 0 and IsFalling() (Shaggy_AnimBP @296-441, Batman_AnimBP @11893-12038,
//! C018_AnimBP @937-1082, C021_AnimBP @1431-1576, Garnet_AnimBP @997-1142, HarleyQuinn_AnimBP @3594-3739,
//! Superman_AnimBP @4275-4420). Pawn_Velma reads the movement component itself (@5-153: the same two).
//! Pawn_WonderWomanV3 reads AnimBP.IsMoving: VSize(Velocity) >= 50 (WonderWoman_AnimBPV2 @1981-2074).
//! Deviation: an AnimBP's values are those of its last update, which in the build may lag the pawn's tick by one
//! frame; the port reads the fighter's state of the tick. A dead pawn's AnimBP is not updated by the port.

use mvs_data::fighter::FighterData;
use mvs_sim::World;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::moves::is_falling;

use crate::skins::Skin;

/// The scalar the hearts' opacity is written on, and its two bounds on slot 0.
pub(crate) const MAX: &str = "Skin BaseColor Opacity Max";
pub(crate) const MIN: &str = "Skin BaseColor Opacity Min";

/// How a pawn's "moving" is read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MovingRead {
    /// Velocity.Y != 0 or IsFalling.
    VelocityY,
    /// VSize(Velocity) >= 50 or IsFalling.
    Speed50,
}

/// One of the petal systems a heart pawn spawns (SpawnEmitterAttached to its mesh).
#[derive(Clone, Copy, Debug)]
pub struct Petal {
    pub ps: &'static str,
    pub socket: &'static str,
    pub loc: [f64; 3],
}

/// A pawn class's HeartSkin functions.
#[derive(Debug)]
pub struct HeartRule {
    pub moving: MovingRead,
    /// HeartSkin_SetOpacity's SetScalarParameterValueOnMaterials targets: "Mesh" (CharacterMesh0) or a pawn component by
    /// its variable name (the skin's SubMeshes entry of the component's tag, when the skin has one).
    pub components: &'static [&'static str],
    pub petals: &'static [Petal],
}

const VDAY: &str = "PS_VDay_Petals";
const Z: [f64; 3] = [0.0; 3];
const fn p(ps: &'static str, socket: &'static str, loc: [f64; 3]) -> Petal {
    Petal { ps, socket, loc }
}

const HANDS: &[Petal] = &[p(VDAY, "L_Hand", Z), p(VDAY, "R_Hand", Z)];

/// By character key, one entry per pawn class that defines the functions. A heart skin on a character without an
/// entry warns and draws the material's own values, no petals.
static RULES: &[(&str, HeartRule)] = &[
    // Pawn_Shaggy: Initialize @607 / @683, SetOpacity @101-566 (Mesh, Shirt)
    ("shaggy", HeartRule { moving: MovingRead::VelocityY, components: &["Mesh", "Shirt"], petals: HANDS }),
    // Pawn_WonderWomanV3: Initialize @504 / @580, SetOpacity @101-890
    (
        "wonderwoman",
        HeartRule {
            moving: MovingRead::Speed50,
            components: &["Mesh", "BodyC", "BodyB", "HairInvertedMesh", "HairBaseMesh"],
            petals: &[p(VDAY, "L_Hand", [5.0, -5.0, 0.0]), p(VDAY, "R_Hand", [5.0, 5.0, 0.0])],
        },
    ),
    // Pawn_Harley: Initialize @504-732 (R_Hand, L_Hand, R_Foot, L_Foot), SetOpacity @208 / @565 (Mesh)
    (
        "harley",
        HeartRule {
            moving: MovingRead::VelocityY,
            components: &["Mesh"],
            petals: &[
                p("C008_S05_Petals", "R_Hand", Z),
                p("C008_S05_Petals", "L_Hand", Z),
                p("C008_S05_Petals", "R_Foot", [0.0, -15.0, 25.0]),
                p("C008_S05_Petals", "L_Foot", [0.0, 15.0, 25.0]),
            ],
        },
    ),
    // Pawn_SuperMan: Initialize @504 / @580, SetOpacity @101 / @458 (Mesh)
    (
        "superman",
        HeartRule {
            moving: MovingRead::VelocityY,
            components: &["Mesh"],
            petals: &[p(VDAY, "L_Hand", [5.0, 10.0, 0.0]), p(VDAY, "R_Hand", [5.0, -10.0, 0.0])],
        },
    ),
    // Pawn_BatmanV2: Initialize @504 / @580, SetOpacity @101-1214
    (
        "batman",
        HeartRule {
            moving: MovingRead::VelocityY,
            components: &[
                "Mesh",
                "CapeDetailedB",
                "CapeFullA",
                "CapeFullB",
                "CapeFullC",
                "BodyC",
                "BodyB",
                "CapeDetailedA",
            ],
            petals: HANDS,
        },
    ),
    // Pawn_C021: Initialize @504 / @580 (the feet), SetOpacity @101-566 (BodyMesh, BodyMI: not Mesh)
    (
        "blackadam",
        HeartRule {
            moving: MovingRead::VelocityY,
            components: &["BodyMesh", "BodyMI"],
            petals: &[p("C021_S03_Petals", "fml_un_R_foot", Z), p("C021_S03_Petals", "fml_un_L_foot", Z)],
        },
    ),
    // Pawn_Garnet: Initialize @504 / @580, SetOpacity @101-782
    (
        "garnet",
        HeartRule {
            moving: MovingRead::VelocityY,
            components: &["Mesh", "RightGauntlet", "LeftGauntlet", "Visor"],
            petals: HANDS,
        },
    ),
    // Pawn_Velma: Initialize @504 / @580, SetOpacity @205 / @562 (Body: the BodyMesh component, not Mesh)
    ("velma", HeartRule { moving: MovingRead::VelocityY, components: &["Body"], petals: HANDS }),
    // Pawn_C018: Initialize @504 (one, on the helmet plume bone), SetOpacity @101-782
    (
        "marvin",
        HeartRule {
            moving: MovingRead::VelocityY,
            components: &["Mesh", "ShoesMesh", "BodyMesh", "HeadMesh"],
            petals: &[p("C018_S04_Petals", "fml_un_helmet_plumes_c", Z)],
        },
    ),
];

/// The character's rule (None: no entry).
pub fn rule_of(key: &str) -> Option<&'static HeartRule> {
    RULES.iter().find(|(k, _)| *k == key).map(|(_, r)| r)
}

const ROLE_TAGS: [&str; 4] = ["RecieveArmor", "IgnoreArmor", "VisMesh", "PrimaryMesh"];

/// A pawn component's Meshes key: the first of its tags that is not a role tag.
pub(crate) fn component_tag(d: &FighterData, name: &str) -> Option<String> {
    let var = format!("{name}_GEN_VARIABLE");
    let c = d.components.pawn.get(&var).or_else(|| d.components.base.get(&var))?;
    let tags = c.p.as_obj()?.get("ComponentTags")?.as_arr()?;
    tags.iter().filter_map(|t| t.as_str()).find(|t| !ROLE_TAGS.contains(t)).map(str::to_owned)
}

/// The skin's part index of a component tag (skinSubMeshKey: a SubMeshes entry of that key in any case; a template
/// mesh the skin has no entry of is not a heart material).
pub(crate) fn part_of(skin: &Skin, tag: &str) -> Option<usize> {
    skin.parts.iter().position(|p| !p.own && p.key.eq_ignore_ascii_case(tag))
}

/// `moving`: IsFalling, or by the rule's read the speed or Velocity.Y (the sim's vx).
pub fn moving(w: &World, f: FighterId, rule: &HeartRule) -> bool {
    if is_falling(w, f) {
        return true;
    }
    let b = &w.fighter(f).body;
    match rule.moving {
        MovingRead::Speed50 => b.vx.hypot(b.vy) >= 50.0,
        MovingRead::VelocityY => b.vx != 0.0,
    }
}
