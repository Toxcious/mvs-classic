//! The upscale pass (AddUpscalePass 141bed0b0): the view drawn to the full frame when r.ScreenPercentage is below
//! 100, FScreenPassVS and FUpscalePS of permutation clamp(r.Upscale.Quality, 0, 5) = 3 (`[PostProcessQuality@3]`),
//! Catmull-Rom from five bilinear taps, each clamped half a texel inside the view.

use mvs_material::Frequency;

use super::pass::{
    Binds, Global, Quad, Rows, Stage, States, bilinear_range, constant_buffer, draw_rect, set_target, unbind, write,
};
use crate::Result;
use crate::assets::Assets;
use crate::gpu::{Buffer, Gpu, Texture};

/// r.Upscale.Quality's start value, the FUpscalePS permutation (PostProcessQuality 3's; the cvar default).
const UPSCALE_QUALITY: i32 = 3;

/// The pass's shaders and buffers, made at the first frame that needs them.
#[derive(Debug, Default)]
pub(super) struct Upscale {
    made: Option<Made>,
}

#[derive(Debug)]
struct Made {
    vs: Global,
    ps: Global,
    ps_globals: Buffer,
    rect: Buffer,
}

impl Upscale {
    /// \`input\` (the view, filling its texture) drawn over all of \`out\`.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        assets: &Assets,
        quad: &Quad,
        states: &States,
        input: &Texture,
        out: &Texture,
    ) -> Result<()> {
        if self.made.is_none() {
            self.made = Some(Made {
                vs: Global::new(gpu, assets, "FScreenPassVS", 0, Frequency::Vertex)?,
                ps: Global::new(gpu, assets, "FUpscalePS", UPSCALE_QUALITY, Frequency::Pixel)?,
                ps_globals: constant_buffer(gpu)?,
                rect: constant_buffer(gpu)?,
            });
        }
        let m = self.made.as_ref().expect("made above");
        let size = [input.desc.size[0], input.desc.size[1]];
        let out_size = [out.desc.size[0], out.desc.size[1]];
        let mut g = Rows::new(40);
        let r = bilinear_range(size);
        g.put(m.ps.at(16), &[size[0] as f32, size[1] as f32]);
        g.put(m.ps.at(24), &[1.0 / size[0] as f32, 1.0 / size[1] as f32]);
        g.put(m.ps.at(112), &r[..2]);
        g.put(m.ps.at(120), &r[2..]);
        write(gpu, &m.ps_globals, &g);
        write(gpu, &m.rect, &draw_rect(out_size, size));
        gpu.set_shaders(m.vs.id, Some(m.ps.id));
        gpu.set_states(states.opaque, states.cull_none, states.no_depth, 0);
        Binds { cbs: vec![Some(&m.rect)], ..Default::default() }.set(gpu, Stage::Vertex);
        // (the build's sampler is bilinear over a border; every tap is clamped inside the view)
        Binds { cbs: vec![Some(&m.ps_globals)], srvs: vec![input.srv.clone()], samplers: vec![states.bilinear_clamp] }
            .set(gpu, Stage::Pixel);
        set_target(gpu, out, out_size);
        quad.draw(gpu, m.vs.id, false, 1)?;
        unbind(gpu);
        Ok(())
    }
}
