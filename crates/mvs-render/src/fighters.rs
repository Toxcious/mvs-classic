//! The fighters' and actors' meshes drawn from their poses (skins): GPUSkin bone
//! buffers, the poses' material parameters (team variants, armor, flash), parts, props, montage meshes and the
//! actors' static models. What a mesh shows and which parameters its instances take is the poser's
//! (mvs_view::ViewPoser); here each pose becomes its draw items.

use mvs_view::FrameInput;

use crate::Result;
use crate::renderer::{Owner, Renderer};

/// FLightingChannels' default: channel 0 alone.
const CHANNEL_0: u8 = 1;

impl Renderer {
    /// The frame's fighter and actor meshes into the draw list. A fighter's meshes keep their materials and bone
    /// buffer by the fighter and the mesh's place in its pose; an actor's by its place in the frame (a slot another
    /// actor takes over rebinds its materials).
    // Unverified: BaseFighter's and Pawn_Shaggy's mesh components and the sandwich actors' set no LightingChannels
    // (no SetLightingChannels in their Blueprints): channel 0 alone, the component default; a native change at run
    // time is not ruled out.
    // Unverified: a skinned mesh's bounds (the Primitive rows' bounds) are the imported mesh bounds moved by its
    // transform; the build's USkinnedMeshComponent::CalcBounds takes the PhysicsAsset bodies' posed boxes, which
    // the store does not hold.
    // Unverified: a fighter's meshes receive no decals (bReceivesDecals false on Pawn_Shaggy's CharacterMesh0 and
    // body meshes, and on the other pawns that set it); a pawn that leaves the component default (true) is not told
    // apart. An actor's meshes take the default unless the view says the component turns it off (`no_decals`).
    pub(crate) fn add_fighters(&mut self, f: &FrameInput) -> Result<()> {
        // (a hidden view is posed for the effects alone)
        for fp in f.fighters.iter().filter(|fp| !fp.hidden) {
            for (k, p) in fp.meshes.iter().enumerate() {
                let first = self.list.opaque.len();
                self.add_pose(Owner::Fighter(fp.fighter, k as u32), p, None, CHANNEL_0, false, None)?;
                // Capture N shows the priority stack's indexes below N (PFGCamera ComputeBoundingBox's show-only
                // list); a mesh of another actor held by the fighter is left out (capture_skip).
                if let Some(s) = fp.priority.filter(|&s| s < 3 && !p.capture_skip) {
                    let added = first as u32..self.list.opaque.len() as u32;
                    self.list.captures.extend(added.map(|i| (s, i)));
                }
            }
        }
        // (the port's own Ghosts) a local player's hidden fighter: its meshes into custom depth and stencil alone, so
        // the team outline's blendable draws its outline around nothing; no capture, no base or translucent pass.
        for fp in f.fighters.iter().filter(|fp| fp.hidden && fp.outline) {
            for (k, p) in fp.meshes.iter().enumerate() {
                let (opaque, translucent) = (self.list.opaque.len(), self.list.translucent.len());
                self.add_pose(Owner::Fighter(fp.fighter, k as u32), p, None, CHANNEL_0, false, None)?;
                self.list.opaque.truncate(opaque);
                self.list.translucent.truncate(translucent);
            }
        }
        for (a, ap) in f.actors.iter().enumerate() {
            for (k, p) in ap.meshes.iter().enumerate() {
                self.add_pose(Owner::Actor(a as u32, k as u32), p, None, CHANNEL_0, !p.no_decals, None)?;
            }
        }
        Ok(())
    }
}
