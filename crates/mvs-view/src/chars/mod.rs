//! The characters' render entries: what a
//! character's render module adds to the shared view. One module per character of the sim's registry
//! (mvs-sim chars/), each exporting `RENDER` (its entry) and `RULES` (its classes' mesh rules,
//! crate::actors::MeshRule).
//! A module with no rule of its own yet is its entry's `GENERIC` values: the shared view draws the character.

use mvs_sim::World;
use mvs_sim::events::Event;
use mvs_sim::fighter::FighterId;
use mvs_sim::names::Name;

use crate::actors::{ActorHost, MeshRule};
use crate::anim::{Clips, HeadScaleNode};
use crate::blend::{BlendSpaceTable, BlendSpaces};
use crate::frame::{ActorPose, MeshPose};
use crate::math::Mat4;
use crate::pose::{MeshMap, Rec};
use crate::skins::{FighterDraws, Skin};
use crate::view::{CharView, PoseRules};

pub(crate) mod arya;
pub(crate) mod batman;
pub(crate) mod black_adam;
pub(crate) mod bugs;
pub(crate) mod creature;
pub(crate) mod finn;
pub(crate) mod garnet;
pub(crate) mod gizmo;
pub(crate) mod harley;
pub(crate) mod iron_giant;
pub(crate) mod jake;
pub(crate) mod lebron;
pub(crate) mod marvin;
pub(crate) mod morty;
pub(crate) mod rick;
pub(crate) mod shaggy;
pub(crate) mod steven;
pub(crate) mod stripe;
pub(crate) mod superman;
pub(crate) mod taz;
pub(crate) mod tom_and_jerry;
pub(crate) mod velma;
pub(crate) mod wonder_woman;

pub(crate) use shaggy::SANDWICH;

/// What a character's render module adds to the shared view (its view fields).
pub(crate) struct CharRender {
    /// Its main mesh AnimBP's ModifyBone nodes fed from HeadScale; [] for none.
    pub(crate) head_scale: &'static [HeadScaleNode],
    /// The rage shirt's additive clip (PawnFx.shirtAdd).
    pub(crate) rage_shirt: Option<&'static str>,
    /// Its mirror table leaves the body alone; the VisMesh scale reflects it.
    pub(crate) turns: bool,
    /// With `turns`: its prop meshes are components that copy the pose under the one VisMesh scale (no
    /// setPropMirror); its pose hook places the mirrored prop bones.
    pub(crate) copied_props: bool,
    /// With `turns`: bones its pose hook places on the mirrored side that a socket reads as CharacterMesh0's, with the
    /// bones under them.
    pub(crate) kept_by: &'static [&'static str],
    /// The BlendSpacePlayer nodes its pose hook runs (`blendSpaces`), bound with the view's clips: PoseOf::blend.
    pub(crate) blend_spaces: Option<&'static BlendSpaceTable>,
    /// Its pawn's own mesh components and pose layers on a fighter's view, when the view is made (`view`).
    pub(crate) view: Option<fn(&mut Rec, &mut Clips, &FighterDraws, &MeshMap)>,
    /// Its AnimBP's own nodes on a fighter's view (`pose`), made with the view.
    pub(crate) pose: Option<MakePose>,
    /// The item classes it draws in a holder's hand (HELD_ITEMS: heldItems), on every fighter's view.
    pub(crate) held_items: &'static [HeldItem],
    /// Its pawn's own mesh components with a mesh of their own (CharacterView.addComponentMesh from its CharFx).
    pub(crate) components: Option<PawnComponents>,
    /// Its pawn's own components drawn as actors of their own (a StaticView on a socketFollow: the pawn
    /// props), made per fighter at the view's first present.
    pub(crate) pawn_actors: Option<MakePawnActors>,
    /// `cameraTracker`: the pawn's component whose graph calls PFGCamera.AddOnscreenComponent (its tracker sphere,
    /// MatchCamera's feed), None where it has none.
    pub(crate) camera_tracker: Option<&'static str>,
    /// The minion views its module draws (Steven's minions, Velma's police cars): one per fighter
    /// of the character and per entry, made with the match.
    pub(crate) minions: &'static [MinionView],
    /// `actorStaticMeshes`: meshes its actors' graphs set that no SCS component of its actors holds (loaded with the
    /// character, by their names among the character's mesh paths).
    pub(crate) actor_meshes: &'static [&'static str],
    /// The tags of its skins' static SubMeshes on a socket of the main mesh (a Socket_ tag): a preview's view adds
    /// them as component meshes (BP_CharacterPreview Add Mesh Component), shown by its pose hook's `shown`.
    pub(crate) socket_statics: &'static [&'static str],
}

/// A minion's view its creator's module draws (a Fighter of World.minions has no view of the match's): made with its
/// creator's view, shown while a minion of `key` that the creator made lives, drawn as an actor of the frame
/// (ActorPose, actor the minion), its events' effects the effects' minion modules'.
pub(crate) struct MinionView {
    /// The minion's character (its data, clips and draws).
    pub(crate) key: &'static str,
    /// The skin it is drawn with, from its data.
    pub(crate) skin: fn(&mvs_data::fighter::FighterData) -> crate::Result<Skin>,
    /// The mesh component's scale; None: its data's.
    pub(crate) mesh_scale: Option<f64>,
    /// The creator's local Team (0.6) draws the skin's Local set (the car's SelectMaterialColor on IsLocalOnline);
    /// false: the creator's red or blue set.
    pub(crate) local_set: bool,
    /// A new minion while one is shown is a new pawn on the view (PoseHook::reset).
    pub(crate) renew: bool,
    /// Its AnimBP's HeadScale follows the minion fighter's (its pawn's BPI HeadScale event writes it); false: 1.
    pub(crate) head_scale: bool,
    /// Its AnimBP's nodes (as CharRender.pose).
    pub(crate) pose: Option<MakePose>,
    /// The source clip its AnimBP's state machine stands in for (the pose hook's `sample`): with no montage the view
    /// samples that clip at 0, under a montage it is the source pose under the slot, and the view's loco key is one
    /// constant, so no crossfade runs between the machine's poses (the 'nav' loco key). None: the fighter's anim.
    pub(crate) nav: Option<&'static str>,
    /// After the pose hook's set, each tick it is shown: what its pawn's graph writes on the view (the car's
    /// DamageAmount).
    pub(crate) place: Option<fn(&mut Rec, &World, FighterId)>,
}

impl std::fmt::Debug for MinionView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MinionView").field("key", &self.key).finish_non_exhaustive()
    }
}

/// A pawn's own mesh components on its view (the character modules' add, attach and
/// set component mesh calls): `make` puts them on the view when it is made
/// (Rec::add_component_mesh), `place` runs once a presentation tick after the montage meshes (their CharFx.place:
/// Rec::set_component_mesh, attach_component_mesh), `reset` at the Lab's reset (their CharFx.reset's part on the
/// view's own state, Rec::own).
#[derive(Clone, Copy)]
pub(crate) struct PawnComponents {
    pub(crate) make: fn(&mut Rec, &World, &Clips, &FighterDraws, &MeshMap),
    pub(crate) place: fn(&mut Rec, &World),
    pub(crate) reset: Option<fn(&mut Rec)>,
}

impl CharRender {
    /// A character with no rule of its own: the shared view's drawing.
    pub(crate) const GENERIC: CharRender = CharRender {
        head_scale: &[],
        rage_shirt: None,
        turns: false,
        copied_props: false,
        kept_by: &[],
        blend_spaces: None,
        view: None,
        pose: None,
        held_items: &[],
        components: None,
        pawn_actors: None,
        camera_tracker: None,
        minions: &[],
        actor_meshes: &[],
        socket_statics: &[],
    };
}

/// What a pose hook is made with (the `of` of CharRender.pose): the skin drawn, its sub-mesh keys in the view's part
/// order, and the entry's blend space players bound (CharAssets.blend; empty without `blend_spaces`).
#[derive(Debug)]
#[allow(dead_code, reason = "read by the characters' pose hooks")]
pub(crate) struct PoseOf<'a> {
    pub(crate) skin: &'a Skin,
    pub(crate) part_keys: &'a [String],
    pub(crate) blend: &'a BlendSpaces,
    /// The view's sub-meshes follow its main mesh as a master pose, with no AnimBP of their own (`slaved`: a Disguise's
    /// copy).
    pub(crate) slaved: bool,
    /// The hook reads a stand-in of its fighter without the fighter's character state (no character state: a
    /// Polymorph mesh's view on its victim). A hook that reads its character's state reads none.
    pub(crate) stand_in: bool,
}

/// CharRender.pose: makes a fighter's pose hook with its view.
pub(crate) type MakePose = fn(&mut CharView, &Clips, &PoseOf<'_>) -> Box<dyn PoseHook>;
/// PoseHook::vectors's writer: (part or None for the main mesh, parameter, value).
pub(crate) type PutVector<'a> = dyn FnMut(Option<usize>, &str, [f64; 4]) + 'a;

/// A character's AnimBP nodes on one fighter's view (the object CharRender.pose returns, with the view's rules it
/// installs: PoseRules, called by the view's pose()).
pub(crate) trait PoseHook: PoseRules + std::fmt::Debug + Send {
    /// `set(f)`: the fighter's state, before each pose of the view.
    fn set(&mut self, _w: &World, _f: FighterId, _view: &mut CharView, _clips: &Clips) {}
    /// `anim(f)`: the sequence the view draws in place of the fighter's anim at its anim time (a skin's own slot over
    /// DefaultSlot); None: the fighter's.
    fn anim(&self, _w: &World, _f: FighterId) -> Option<Name> {
        None
    }

    /// An event of the World's step (the effects' reach into the rule: ARYA_POSES and the like), for the view of
    /// fighter `id`; `part_scalar(part, name, value)` writes a scalar on every material of a part.
    fn event(&mut self, _w: &World, _id: i32, _e: &Event, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {}

    /// After `set`: the scalars its pawn's graph writes on the view's materials this tick (a timeline's, as Batman's
    /// idle fade): `put(None, ..)` on every material of the main mesh, `put(Some(k), ..)` on every material of part k.
    /// `color_index`: the fighter's GetColorIndex.
    fn scalars(&mut self, _color_index: u8, _put: &mut dyn FnMut(Option<usize>, &str, f64)) {}
    /// Each frame, on the pose just presented: the vector parameters its pawn's graph writes on the view's materials
    /// from the mesh's sockets (LeBron's ball stretch): `world(bone)` is the bone's UE world matrix (column form);
    /// `put` as in [`PoseHook::scalars`].
    fn vectors(&self, _world: &dyn Fn(usize) -> Mat4, _put: &mut PutVector<'_>) {}
    /// `reset()`: a new pawn on the view (the Lab's reset): the AnimBP's state anew;
    /// `part_scalar` as in [`PoseHook::event`].
    fn reset(&mut self, _part_scalar: &mut dyn FnMut(usize, &str, f64)) {}

    /// `shown(tag, simShown)`: whether the AnimBP's own writes show the sub-mesh `tag` (lower case), the sim's
    /// `sim` where it has none (a pawn actor's visibility, PawnCx::shown).
    fn shown(&self, _tag: &str, sim: bool) -> bool {
        sim
    }

    /// `preview()`: the view has no fighter (a preview actor, a podium fighter): the AnimBP's state as a fresh
    /// preview's, after the view's parts are shown or hidden by tag.
    fn preview(&mut self, _view: &mut CharView, _of: &PreviewOf<'_>) {}
    /// `previewStep(anim, time, facingLeft)`: the preview's AnimBP update of the frame, before the view's pose, from
    /// the montage segment it shows (Gizmo, LeBron, Stripe, Taz and Velma; Wonder Woman's
    /// preview sampleRule, which reads the montage's name).
    fn preview_step(&mut self, _view: &mut CharView, _clips: &Clips, _step: &PreviewStep<'_>) {}
}

/// What a preview's hook reads of its view's making (`preview(partKeys, montageOf)`): the character's
/// data (its montages by name). The sub-mesh keys are the hook's own (PoseOf::part_keys).
#[derive(Debug)]
pub(crate) struct PreviewOf<'a> {
    pub(crate) fighter: &'a mvs_data::fighter::FighterData,
}

/// The frame a preview shows (PoseHook::preview_step): the montage playing, its segment's sequence and time, and the
/// actor's FacingLeft.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewStep<'a> {
    pub(crate) montage: &'a str,
    pub(crate) anim: &'a str,
    pub(crate) time: f64,
    pub(crate) facing_left: bool,
}

/// CharRender.pawn_actors: fighter `f`'s pawn actors.
pub(crate) type MakePawnActors = fn(&ActorHost<'_>, &World, FighterId) -> Vec<Box<dyn PawnActor>>;

/// One of a pawn's components drawn as an actor of its own (a StaticView on host.socketFollow).
pub(crate) trait PawnActor: std::fmt::Debug + Send {
    /// Once a frame, after the views are presented at `alpha` (ActorHost::socket reads the drawn bones): its meshes
    /// into `out` (nothing when hidden).
    fn present(&mut self, host: &ActorHost<'_>, w: &World, cx: &PawnCx<'_>, alpha: f64, out: &mut Vec<ActorPose>);
}

/// What a pawn actor reads of its fighter's view.
pub(crate) struct PawnCx<'a> {
    pub(crate) fid: FighterId,
    /// meshHidden: dead or under AddInvisibility (the smoke's occlusion is not ported: place_view).
    pub(crate) hidden: bool,
    pub(crate) pose: Option<&'a dyn PoseHook>,
    /// The custom depth stencil the pawn's tagged mesh components draw with: the fighter's, 0 with its outlines off
    /// (Comp_Pawn_MeshManager AddMesh @1291-1332, SetStencilValue, SetOutlinesOn).
    pub(crate) stencil: u8,
}

impl PawnCx<'_> {
    /// A pawn mesh component's meshes on the fighter's custom depth stencil (Comp_Pawn_MeshManager AddMesh gives each
    /// tagged MeshComponent child of the main mesh SetCustomDepthStencilValue and SetRenderCustomDepth, static ones
    /// included); `outlined` false (a NoOutline tag, `outlined`): none.
    pub(crate) fn outline(&self, outlined: bool, meshes: &mut [MeshPose]) {
        let s = if outlined { self.stencil } else { 0 };
        for m in meshes {
            m.custom_stencil = s;
        }
    }

    /// Whether the sub-mesh `tag` (lower case) is shown: the sim's (`!f.subHidden.has(tag)`) through the pose hook's
    /// own writes (PoseHook::shown), and never when the fighter's meshes are hidden.
    pub(crate) fn shown(&self, w: &World, tag: &str) -> bool {
        let sim = !w.names.get(tag).is_some_and(|n| w.fighter(self.fid).pose.sub_hidden.contains(&n));
        !self.hidden && self.pose.map_or(sim, |p| p.shown(tag, sim))
    }
}

/// Whether a pawn mesh component takes the fighter's stencil: not tagged NoOutline (Comp_Pawn_MeshManager
/// SetStencilValue @523 skips those; Velma's Lightbulb alone).
// Unverified: a NoOutline component is taken as drawing no custom depth; AddMesh's own write (@1291-1332) gives it the
// manager's StencilValue at SetupMeshes, and whether SetupMeshes runs before the first SetStencilValue is not read.
pub(crate) fn outlined(c: &mvs_data::fighter::Component) -> bool {
    !c.p.as_obj()
        .and_then(|p| p.get("ComponentTags"))
        .and_then(mvs_data::Value::as_arr)
        .is_some_and(|t| t.iter().any(|v| v.as_str() == Some("NoOutline")))
}

/// An item class drawn as a prop in its holder's hand: `prop` puts the prop on a
/// fighter's view, with the draws of the first character whose data has the class (None: no loaded one has).
pub(crate) struct HeldItem {
    pub(crate) cls: &'static str,
    pub(crate) prop: fn(&mut Rec, &World, Option<&FighterDraws>, &MeshMap),
}

/// Every character's entry by its key, in this order (the order the held items are put on a view).
static ENTRIES: [(&str, &CharRender); 24] = [
    (mvs_sim::chars::shaggy::KEY, &shaggy::RENDER),
    (mvs_sim::chars::morty::KEY, &morty::RENDER),
    (mvs_sim::chars::iron_giant::KEY, &iron_giant::RENDER),
    (mvs_sim::chars::wonder_woman::KEY, &wonder_woman::RENDER),
    (mvs_sim::chars::harley::KEY, &harley::RENDER),
    (mvs_sim::chars::superman::KEY, &superman::RENDER),
    (mvs_sim::chars::lebron::KEY, &lebron::RENDER),
    (mvs_sim::chars::arya::KEY, &arya::RENDER),
    (mvs_sim::chars::batman::KEY, &batman::RENDER),
    (mvs_sim::chars::creature::KEY, &creature::RENDER),
    (mvs_sim::chars::black_adam::KEY, &black_adam::RENDER),
    (mvs_sim::chars::bugs::KEY, &bugs::RENDER),
    (mvs_sim::chars::rick::KEY, &rick::RENDER),
    (mvs_sim::chars::taz::KEY, &taz::RENDER),
    (mvs_sim::chars::steven::KEY, &steven::RENDER),
    (mvs_sim::chars::garnet::KEY, &garnet::RENDER),
    (mvs_sim::chars::finn::KEY, &finn::RENDER),
    (mvs_sim::chars::jake::KEY, &jake::RENDER),
    (mvs_sim::chars::tom_and_jerry::KEY, &tom_and_jerry::RENDER),
    (mvs_sim::chars::velma::KEY, &velma::RENDER),
    (mvs_sim::chars::velma::police::POLICE_DATA, &velma::POLICE_RENDER),
    (mvs_sim::chars::marvin::KEY, &marvin::RENDER),
    (mvs_sim::chars::stripe::KEY, &stripe::RENDER),
    (mvs_sim::chars::gizmo::KEY, &gizmo::RENDER),
];

/// Every character module's mesh rules, in the same order (resolved once into crate::actors::mesh_rule's table).
pub(crate) static RULES: [&[(&str, MeshRule)]; 23] = [
    shaggy::RULES,
    morty::RULES,
    iron_giant::RULES,
    wonder_woman::RULES,
    harley::RULES,
    superman::RULES,
    lebron::RULES,
    arya::RULES,
    batman::RULES,
    creature::RULES,
    black_adam::RULES,
    bugs::RULES,
    rick::RULES,
    taz::RULES,
    steven::RULES,
    garnet::RULES,
    finn::RULES,
    jake::RULES,
    tom_and_jerry::RULES,
    velma::RULES,
    marvin::RULES,
    stripe::RULES,
    gizmo::RULES,
];

/// `renderOf`: a character's entry. A key with no module stops here.
pub(crate) fn render_of(key: &str) -> &'static CharRender {
    match ENTRIES.iter().find(|(k, _)| *k == key) {
        Some((_, r)) => r,
        None => panic!("no render entry for the character {key} (mvs-view chars/)"),
    }
}

/// HELD_ITEMS: every entry's held item classes, in the entries' order.
pub(crate) fn held_items() -> impl Iterator<Item = &'static HeldItem> {
    ENTRIES.iter().flat_map(|(_, r)| r.held_items)
}

/// Whether `cls` is a class some entry draws in a holder's hand (HELD_ITEMS.has).
pub(crate) fn is_held_item(cls: &str) -> bool {
    held_items().any(|h| h.cls == cls)
}
