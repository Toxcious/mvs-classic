//! The bloom (render/post bloom): the downsample chain (FDownsamplePS) to six levels, then from the smallest level
//! up each level's Gaussian blur along X and along Y (FFilterVS + FFilterPS), the Y pass tinted and adding the level
//! below's result. This build's FPostProcessSettings defaults.

use std::collections::HashMap;

use mvs_material::Frequency;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT;

use super::pass::{
    Binds, Global, Quad, Rows, Stage, States, bilinear_range, constant_buffer, draw_rect, screen_position_vs,
    set_target, target, unbind, write,
};
use crate::Result;
use crate::assets::Assets;
use crate::gpu::{Buffer, Gpu, ShaderId, Texture};

/// FPostProcessSettings BloomIntensity's default (exe constructor FUN_142d3fd30, offsets 0x21c..0x290).
pub(super) const BLOOM_INTENSITY: f32 = 0.675;
const BLOOM_SIZE_SCALE: f64 = 4.0;
/// Bloom1Size .. Bloom6Size.
const BLOOM_SIZE: [f64; STAGES] = [0.3, 1.0, 2.0, 10.0, 30.0, 64.0];
/// Bloom1Tint .. Bloom6Tint (grey).
const BLOOM_TINT: [f32; STAGES] = [0.3465, 0.138, 0.1176, 0.066, 0.066, 0.061];
/// BloomQualityToSceneDownsampleStage[r.BloomQuality 5].
const STAGES: usize = 6;
/// FFilterPS's largest tap count.
const FILTER_SAMPLES: usize = 32;
/// FFilterPS's permutations of a tap count: + 33 with the additive input.
const ADDITIVE_PERMUTATION: i32 = 33;

/// Compute1DGaussianFilterKernel (FUN_141bf4170): offsets in texels and normalised weights, as bilinear tap pairs;
/// r.Filter.SizeScale 1, r.Bloom.Cross 0.
fn gauss_kernel(radius: f64) -> (Vec<f64>, Vec<f64>) {
    let max = (FILTER_SAMPLES - 1) as f64;
    let sigma = radius.clamp(1e-5, max);
    let r = sigma.ceil().min(max) as i32;
    let nd = |x: f64| (-16.7 * (x / sigma) * (x / sigma)).exp();
    let (mut off, mut w) = (Vec::new(), Vec::new());
    let mut sum = 0.0;
    let mut i = -r;
    while i <= r {
        let w0 = nd(i as f64);
        let w1 = if i != r { nd(i as f64 + 1.0) } else { 0.0 };
        let t = w0 + w1;
        off.push(i as f64 + w1 / t);
        w.push(t);
        sum += t;
        i += 2;
    }
    (off, w.into_iter().map(|x| x / sum).collect())
}

/// A bloom level's targets: the downsample, its X blur and its result.
#[derive(Debug)]
struct Level {
    size: [u32; 2],
    down: Texture,
    blur_x: Texture,
    bloom: Texture,
}

#[derive(Debug)]
pub(super) struct Bloom {
    levels: Vec<Level>,
    /// The downsample's vertex shader (pass.rs screen_position_vs).
    screen_vs: ShaderId,
    down_ps: Global,
    /// FFilterVS by tap count.
    filter_vs: HashMap<i32, Global>,
    /// FFilterPS by permutation.
    filter_ps: HashMap<i32, Global>,
    vs_globals: Buffer,
    ps_globals: Buffer,
    rect: Buffer,
}

impl Bloom {
    pub fn new(gpu: &Gpu, assets: &Assets, size: [u32; 2]) -> Result<Bloom> {
        let mut levels = Vec::with_capacity(STAGES);
        let mut s = size;
        for _ in 0..STAGES {
            s = s.map(|x| x.div_ceil(2).max(1));
            let t = || target(gpu, s, DXGI_FORMAT_R16G16B16A16_FLOAT);
            levels.push(Level { size: s, down: t()?, blur_x: t()?, bloom: t()? });
        }
        Ok(Bloom {
            levels,
            screen_vs: screen_position_vs(gpu)?,
            // The high-quality downsample (four taps).
            // Unverified: permutation 1 (r.Downsample.Quality's high).
            down_ps: Global::new(gpu, assets, "FDownsamplePS", 1, Frequency::Pixel)?,
            filter_vs: HashMap::new(),
            filter_ps: HashMap::new(),
            vs_globals: constant_buffer(gpu)?,
            ps_globals: constant_buffer(gpu)?,
            rect: constant_buffer(gpu)?,
        })
    }

    /// The bloom of `scene`: its result, the first level's.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        assets: &Assets,
        quad: &Quad,
        states: &States,
        scene: &Texture,
    ) -> Result<&Texture> {
        gpu.set_states(states.opaque, states.cull_none, states.no_depth, 0);
        for i in 0..STAGES {
            let (src, src_size) = match i {
                0 => (scene, [scene.desc.size[0], scene.desc.size[1]]),
                _ => (&self.levels[i - 1].down, self.levels[i - 1].size),
            };
            self.downsample(gpu, quad, states, src, src_size, i)?;
        }
        for i in (0..STAGES).rev() {
            let size = self.levels[i].size;
            let (off, w) = gauss_kernel(size[0] as f64 * BLOOM_SIZE[i] * BLOOM_SIZE_SCALE * 0.005);
            for pass in 0..2 {
                let add = pass == 1 && i + 1 < STAGES;
                let tint = if pass == 1 { BLOOM_TINT[i] / STAGES as f32 } else { 1.0 };
                let offsets: Vec<[f32; 2]> = off
                    .iter()
                    .map(|o| match pass {
                        0 => [(o / size[0] as f64) as f32, 0.0],
                        _ => [0.0, (o / size[1] as f64) as f32],
                    })
                    .collect();
                let weights: Vec<f32> = w.iter().map(|x| *x as f32 * tint).collect();
                self.filter(gpu, assets, quad, states, i, pass, add, &offsets, &weights)?;
            }
        }
        Ok(&self.levels[0].bloom)
    }

    fn downsample(
        &self,
        gpu: &Gpu,
        quad: &Quad,
        states: &States,
        src: &Texture,
        src_size: [u32; 2],
        i: usize,
    ) -> Result<()> {
        let dst = &self.levels[i];
        let ps = &self.down_ps;
        let mut g = Rows::new(35);
        let r = bilinear_range(src_size);
        g.put(ps.at(24), &[1.0 / src_size[0] as f32, 1.0 / src_size[1] as f32]);
        g.put(ps.at(112), &r[..2]);
        g.put(ps.at(120), &r[2..]);
        g.put(ps.at(136), &[1.0 / dst.size[0] as f32, 1.0 / dst.size[1] as f32]);
        write(gpu, &self.ps_globals, &g);
        write(gpu, &self.rect, &draw_rect(dst.size, src_size));
        gpu.set_shaders(self.screen_vs, Some(ps.id));
        Binds { cbs: vec![Some(&self.rect)], ..Default::default() }.set(gpu, Stage::Vertex);
        Binds { cbs: vec![Some(&self.ps_globals)], srvs: vec![src.srv.clone()], samplers: vec![states.bilinear_clamp] }
            .set(gpu, Stage::Pixel);
        set_target(gpu, &dst.down, dst.size);
        quad.draw(gpu, self.screen_vs, false, 1)?;
        unbind(gpu);
        Ok(())
    }

    /// One blur pass of level `i`: X (pass 0) from the downsample into blur_x, Y (pass 1) from blur_x into the
    /// level's result, adding the level below's when `add`.
    #[allow(clippy::too_many_arguments)]
    fn filter(
        &mut self,
        gpu: &Gpu,
        assets: &Assets,
        quad: &Quad,
        states: &States,
        i: usize,
        pass: u32,
        add: bool,
        offsets: &[[f32; 2]],
        weights: &[f32],
    ) -> Result<()> {
        let n = offsets.len() as i32;
        let perm = n + if add { ADDITIVE_PERMUTATION } else { 0 };
        if let std::collections::hash_map::Entry::Vacant(e) = self.filter_vs.entry(n) {
            e.insert(Global::new(gpu, assets, "FFilterVS", n, Frequency::Vertex)?);
        }
        if let std::collections::hash_map::Entry::Vacant(e) = self.filter_ps.entry(perm) {
            e.insert(Global::new(gpu, assets, "FFilterPS", perm, Frequency::Pixel)?);
        }
        let (vs, ps) = (&self.filter_vs[&n], &self.filter_ps[&perm]);
        // SampleOffsets: two taps' UV offsets a row; SampleWeights: a float4 a tap.
        let mut v = Rows::new(FILTER_SAMPLES / 2);
        let flat: Vec<f32> = offsets.iter().flatten().copied().collect();
        v.put(vs.at(256), &flat);
        let mut p = Rows::new(super::pass::CB_ROWS);
        let wv: Vec<f32> = weights.iter().flat_map(|w| [*w; 4]).collect();
        p.put(ps.at(1280), &wv);
        write(gpu, &self.vs_globals, &v);
        write(gpu, &self.ps_globals, &p);
        let level = &self.levels[i];
        write(gpu, &self.rect, &draw_rect(level.size, level.size));
        gpu.set_shaders(vs.id, Some(ps.id));
        Binds { cbs: vec![Some(&self.vs_globals), Some(&self.rect)], ..Default::default() }.set(gpu, Stage::Vertex);
        let (src, dst) = if pass == 0 { (&level.down, &level.blur_x) } else { (&level.blur_x, &level.bloom) };
        let mut srvs = vec![src.srv.clone()];
        let mut samplers = vec![states.bilinear_clamp];
        if add {
            srvs.push(self.levels[i + 1].bloom.srv.clone());
            samplers.push(states.bilinear_clamp);
        }
        Binds { cbs: vec![Some(&self.ps_globals)], srvs, samplers }.set(gpu, Stage::Pixel);
        set_target(gpu, dst, level.size);
        quad.draw(gpu, vs.id, false, 1)?;
        unbind(gpu);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_pairs_taps() {
        let (off, w) = gauss_kernel(2.4);
        // sigma 2.4: R 3, taps at -3, -1, 1, 3 paired with their next.
        assert_eq!(off.len(), 4);
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert_eq!(off[3], 3.0);
        assert!(off[0] > -3.0 && off[0] < -2.0);
    }
}
