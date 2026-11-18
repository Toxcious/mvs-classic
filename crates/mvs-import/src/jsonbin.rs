//! The container of the binary kinds whose header is structured (object entries with a body, mesh, animation,
//! lighting, shadermap, shader): a 4-byte magic, u32 header length, the header as UTF-8 JSON, padding to 16, then the
//! body, every section of it 16-byte aligned and named in the header by its offset from the body's start. This is the
//! writer (its reader is the game's: `mvs_material::jsonbin`, the render and anim crates).

use serde::Serialize;

use crate::json::{self, JsonError};

/// A section of the body: offset from the body's start, length in bytes (written in a header as
/// `{"offset":..,"length":..}`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BodyRef {
    pub offset: usize,
    pub length: usize,
}

const fn align16(n: usize) -> usize {
    (n + 15) & !15
}

/// Lays `sections` out back to back, each at a 16-byte boundary: their refs and the body's length (`layOutBody`).
pub fn lay_out_body<S: AsRef<[u8]>>(sections: &[S]) -> (Vec<BodyRef>, usize) {
    let mut refs = Vec::with_capacity(sections.len());
    let mut at = 0;
    for s in sections {
        let length = s.as_ref().len();
        refs.push(BodyRef { offset: at, length });
        at = align16(at + length);
    }
    (refs, at)
}

/// The entry (`writeJsonBin`): `magic`, the header's JSON (made after the layout, so it can name the refs) and the
/// sections at their refs.
pub fn write_json_bin<S: AsRef<[u8]>>(magic: &[u8; 4], header: &[u8], sections: &[S], refs: &[BodyRef]) -> Vec<u8> {
    let body_at = align16(8 + header.len());
    let end = refs.last().map_or(0, |r| r.offset + r.length);
    let mut out = vec![0u8; body_at + end];
    out[..4].copy_from_slice(magic);
    out[4..8].copy_from_slice(&u32::try_from(header.len()).expect("a header under 4 GB").to_le_bytes());
    out[8..8 + header.len()].copy_from_slice(header);
    for (s, r) in sections.iter().zip(refs) {
        out[body_at + r.offset..body_at + r.offset + r.length].copy_from_slice(s.as_ref());
    }
    out
}

/// [`write_json_bin`] with the header given as a value, printed as JSON.stringify prints it.
pub fn json_bin<H: Serialize + ?Sized, S: AsRef<[u8]>>(
    magic: &[u8; 4],
    header: &H,
    sections: &[S],
    refs: &[BodyRef],
) -> Result<Vec<u8>, JsonError> {
    Ok(write_json_bin(magic, &json::to_vec(header)?, sections, refs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_container() {
        let sections: [&[u8]; 3] = [&[1, 2, 3], &[], &[9; 17]];
        let (refs, len) = lay_out_body(&sections);
        let at = |o, l| BodyRef { offset: o, length: l };
        assert_eq!(refs, [at(0, 3), at(16, 0), at(16, 17)]);
        assert_eq!(len, 48);
        #[derive(Serialize)]
        struct H {
            parts: Vec<BodyRef>,
        }
        let e = json_bin(b"MVXX", &H { parts: refs.clone() }, &sections, &refs).unwrap();
        let head = br#"{"parts":[{"offset":0,"length":3},{"offset":16,"length":0},{"offset":16,"length":17}]}"#;
        assert_eq!(&e[..4], b"MVXX");
        assert_eq!(u32::from_le_bytes(e[4..8].try_into().unwrap()) as usize, head.len());
        assert_eq!(&e[8..8 + head.len()], head);
        let body = align16(8 + head.len());
        assert_eq!(e.len(), body + 33);
        assert_eq!(&e[body..body + 3], &[1, 2, 3]);
        assert_eq!(&e[body + 16..], &[9; 17]);
    }
}
