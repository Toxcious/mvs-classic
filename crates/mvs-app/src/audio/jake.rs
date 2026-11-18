//! Jake's stretch's head pitch RTPC from the drawn pose (JakeStretch's Tick @9568-10102): the sim samples socket
//! places only, so the glue reads the head bone's rotation from the view.

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::world_actor::WorldActorState;

use super::{Calls, Views};

const STRETCH_CLS: &str = "JakeStretch_C";
const HEAD: &str = "fml_un_C_head";
const RTPC: &str = "cha_jake_pitch";
/// SetRTPCValue's InterpolationTimeMs @9853.
const INTERP_MS: f64 = 10.0;

/// SFX_Pitch per live stretch actor (by its sequence number): the head's |pitch| at its last tick (the class default 0).
#[derive(Debug, Default)]
pub(super) struct HeadPitch {
    last: IndexMap<i32, f64>,
}

impl HeadPitch {
    /// Each stretch whose actor tick still runs (it ends at ActivateMulticast's SetActorTickEnabled(false), which
    /// sends 2 itself): cha_jake_pitch = FClamp(|the head socket's world Pitch| - SFX_Pitch, 0, 2) on the Instigator
    /// over 10 ms, then SFX_Pitch = that |Pitch|. A stretch whose owner has no drawn body sends nothing.
    // Unverified: the build reads the socket in the actor's tick, against the mesh's pose of its last evaluation; here
    // the pose the view drew for the tick just stepped.
    // Deviation: the build's (a sim) value posted from the view: the sim samples socket places only, not rotations.
    pub(super) fn tick(&mut self, world: &World, views: &dyn Views, c: &mut Calls) {
        let mut seen = Vec::new();
        for (_, x) in world.actors.actors.iter() {
            let WorldActorState::Stretch(s) = &x.state else { continue };
            if x.dead || world.names.str(x.cls) != STRETCH_CLS {
                continue;
            }
            seen.push(x.seq);
            if !s.ticking {
                continue;
            }
            let id = world.fighter(x.owner).id;
            // (LocalBaseFighter is set only for a Pawn_Jake Instigator: another's Mesh read is None's, a zero rotator)
            let pitch = if s.jake {
                let Some(r) = views.socket_rows(world, id, HEAD) else { continue };
                // FRotator's Pitch of the X axis (the first row), in degrees
                r[2].atan2(r[0].hypot(r[1])).to_degrees().abs()
            } else {
                0.0
            };
            let last = self.last.entry(x.seq).or_insert(0.0);
            c.rtpc(RTPC, (pitch - *last).clamp(0.0, 2.0), id, INTERP_MS);
            *last = pitch;
        }
        self.last.retain(|k, _| seen.contains(k));
    }
}
