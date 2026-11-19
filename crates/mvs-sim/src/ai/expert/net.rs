//! The Expert's network: a small fully connected net (ReLU between layers) that scores every action of the list
//! (`actions`) from the match's numbers (`features`), so the look-ahead tries the best few. The port's own, trained by
//! `tools/expert-train` on the look-ahead's own scores; nothing of the build is in it.
//!
//! Deterministic: float32, each output summed over the inputs in their order, a multiply then an add (never fused),
//! no routine beyond add, multiply and a comparison, so the same weights give the same bits on every PC.
//!
//! The weights file (`expert.net`, little-endian): `MVSX`, the version (1), the inputs, the outputs, the layers
//! (each u32); then per layer its inputs and outputs (u32), the weights input-major (`w[i * outputs + j]`) and the
//! biases (f32). The trainer folds the inputs' normalisation into the first layer.

use std::sync::{Arc, OnceLock};

/// The weights the game is built with.
const WEIGHTS: &[u8] = include_bytes!("expert.net");
const MAGIC: &[u8; 4] = b"MVSX";
const VERSION: u32 = 1;

/// One layer: `outputs` sums of `inputs` products and a bias.
#[derive(Clone, Debug)]
struct Layer {
    inputs: usize,
    outputs: usize,
    w: Vec<f32>,
    b: Vec<f32>,
}

/// A network, read from a weights file.
#[derive(Clone, Debug)]
pub struct Net {
    layers: Vec<Layer>,
}

/// The built-in network, read once.
pub fn built_in() -> Arc<Net> {
    static NET: OnceLock<Arc<Net>> = OnceLock::new();
    NET.get_or_init(|| Arc::new(Net::from_bytes(WEIGHTS).expect("the built-in expert.net is a weights file"))).clone()
}

/// Bytes read in order.
struct Reader<'a> {
    b: &'a [u8],
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.b.len() < n {
            return Err("the weights file ends early".to_owned());
        }
        let (h, t) = self.b.split_at(n);
        self.b = t;
        Ok(h)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn usize(&mut self) -> Result<usize, String> {
        usize::try_from(self.u32()?).map_err(|e| e.to_string())
    }

    fn f32s(&mut self, n: usize) -> Result<Vec<f32>, String> {
        let b = self.take(n.checked_mul(4).ok_or("a layer too large")?)?;
        Ok(b.as_chunks::<4>().0.iter().map(|&c| f32::from_le_bytes(c)).collect())
    }
}

impl Net {
    /// A network from a weights file's bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Net, String> {
        let mut r = Reader { b: bytes };
        if r.take(4)? != MAGIC {
            return Err("not a weights file (no MVSX)".to_owned());
        }
        if r.u32()? != VERSION {
            return Err("a weights file of another version".to_owned());
        }
        let (inputs, outputs, n) = (r.usize()?, r.usize()?, r.usize()?);
        let mut layers = Vec::with_capacity(n);
        let mut want = inputs;
        for _ in 0..n {
            let (i, o) = (r.usize()?, r.usize()?);
            if i != want {
                return Err(format!("a layer of {i} inputs after one of {want} outputs"));
            }
            let w = r.f32s(i.checked_mul(o).ok_or("a layer too large")?)?;
            let b = r.f32s(o)?;
            layers.push(Layer { inputs: i, outputs: o, w, b });
            want = o;
        }
        if want != outputs || !r.b.is_empty() {
            return Err("the weights file's layers do not match its header".to_owned());
        }
        Ok(Net { layers })
    }

    pub fn inputs(&self) -> usize {
        self.layers.first().map_or(0, |l| l.inputs)
    }

    pub fn outputs(&self) -> usize {
        self.layers.last().map_or(0, |l| l.outputs)
    }

    /// The network's outputs for input `x` into `out`; `scratch` holds the layers between (its memory reused).
    pub fn run(&self, x: &[f32], out: &mut Vec<f32>, scratch: &mut Vec<f32>) {
        assert_eq!(x.len(), self.inputs(), "the network's input count");
        out.clear();
        out.extend_from_slice(x);
        let last = self.layers.len().saturating_sub(1);
        for (n, l) in self.layers.iter().enumerate() {
            std::mem::swap(out, scratch);
            out.clear();
            out.extend_from_slice(&l.b);
            for (i, &xi) in scratch.iter().enumerate() {
                // (an input of 0 adds nothing: the one-hots and the ReLU's zeros are skipped)
                if xi == 0.0 {
                    continue;
                }
                let row = &l.w[i * l.outputs..(i + 1) * l.outputs];
                for (o, &wij) in out.iter_mut().zip(row) {
                    *o += xi * wij;
                }
            }
            if n < last {
                for o in out.iter_mut() {
                    *o = if *o > 0.0 { *o } else { 0.0 };
                }
            }
        }
    }
}
