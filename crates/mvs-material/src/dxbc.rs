//! The declarations of a cooked shader's program the import reads without translating it: which registers a pixel
//! shader reads the custom stencil through (SceneTexture:CustomStencil, a uint 2D texture outside the resource table).
//! Scans for the custom stencil read over the token decode of the SM5 shader
//! (opcode and operand tokens as the D3D11 shader header gives them).

use crate::shader::{Dxbc, ShaderCode};
use crate::{Error, Result};

/// `dcl_resource`.
const DCL_RESOURCE: u32 = 88;
/// `customdata`: its length is the next dword.
const CUSTOMDATA: u32 = 53;
/// The resource dimension texture2d.
const TEXTURE2D: u32 = 3;
/// The return type uint.
const RET_UINT: u32 = 4;
/// The operand type RESOURCE.
const OPERAND_RESOURCE: u32 = 7;

/// The registers `t<n>` a SHEX / SHDR program declares as `dcl_resource_texture2d (uint,uint,uint,uint) t<n>` that are
/// not in `textures` (the resource table's TextureMap registers): the custom stencil reads, in declaration order
/// (`customStencil`'s set). `what` names the shader in errors.
pub fn custom_stencil_registers(shex: &[u8], textures: &[u32], what: &str) -> Result<Vec<u32>> {
    let bad = |why: String| Error::format(what, format!("shex: {why}"));
    let w: Vec<u32> = shex.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect();
    let len = *w.get(1).ok_or_else(|| bad(format!("{} bytes, no length", shex.len())))? as usize;
    if len > w.len() {
        return Err(bad(format!("{len} dwords declared, {} present", w.len())));
    }
    let mut out = Vec::new();
    let mut pos = 2;
    while pos < len {
        let tok = w[pos];
        let op = tok & 0x7ff;
        let n = if op == CUSTOMDATA {
            *w.get(pos + 1).ok_or_else(|| bad(format!("customdata at dword {pos} has no length")))? as usize
        } else {
            ((tok >> 24) & 0x7f) as usize
        };
        if n == 0 {
            return Err(bad(format!("zero-length instruction at dword {pos}")));
        }
        if pos + n > len {
            return Err(bad(format!("opcode {op} at dword {pos} runs past the program's {len} dwords")));
        }
        if op == DCL_RESOURCE
            && let Some(reg) = stencil_decl(&w[pos..pos + n])
            && !textures.contains(&reg)
            && !out.contains(&reg)
        {
            out.push(reg);
        }
        pos += n;
    }
    Ok(out)
}

/// The register of a `dcl_resource` instruction's dwords when it reads `dcl_resource_texture2d
/// (uint,uint,uint,uint) t<n>`: dimension texture2d, a plain resource operand (no components, no extended token, one
/// immediate index), the four return types uint.
fn stencil_decl(ins: &[u32]) -> Option<u32> {
    let tok = ins[0];
    // (an extended opcode token: none on a declaration the compiler writes; a decoder may read it as part of the
    // instruction and its operand after it)
    let mut at = 1;
    let mut ext = tok >> 31 != 0;
    while ext {
        ext = ins.get(at)? >> 31 != 0;
        at += 1;
    }
    if (tok >> 11) & 0x1f != TEXTURE2D {
        return None;
    }
    let o = *ins.get(at)?;
    let plain = o & 3 == 0 && o >> 31 == 0 && (o >> 12) & 0xff == OPERAND_RESOURCE && (o >> 20) & 3 == 1;
    if !plain || (o >> 22) & 7 != 0 {
        return None;
    }
    let reg = *ins.get(at + 1)?;
    let ret = *ins.get(at + 2)?;
    (ins.len() == at + 3 && (0..4).all(|i| (ret >> (4 * i)) & 0xf == RET_UINT)).then_some(reg)
}

/// Whether a shader code reads the custom stencil (a register [`custom_stencil_registers`] finds in its SHEX or SHDR
/// chunk, against its resource table's TextureMap): what a banner context's build binds as the engine texture
/// Whether the shader reads the custom stencil (per pixel stage).
pub fn reads_custom_stencil(code: &ShaderCode, what: &str) -> Result<bool> {
    let dxbc = Dxbc::parse(&code.dxbc)?;
    let Some(shex) = dxbc.chunk(b"SHEX").or_else(|| dxbc.chunk(b"SHDR")) else {
        return Err(Error::format(what, "no SHEX chunk".into()));
    };
    let textures: Vec<u32> =
        code.resource_table.slots(&code.resource_table.texture_map)?.iter().map(|s| u32::from(s.register)).collect();
    Ok(!custom_stencil_registers(shex, &textures, what)?.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `dcl_resource_<dim> (<ret>) t<reg>`: opcode 88, the dimension in bits 11-15, length 4.
    fn decl(dim: u32, reg: u32, ret: u32) -> [u32; 4] {
        [DCL_RESOURCE | dim << 11 | 4 << 24, OPERAND_RESOURCE << 12 | 1 << 20, reg, ret * 0x1111]
    }

    fn program(body: &[u32]) -> Vec<u8> {
        let mut w = vec![0x0000_0050, (body.len() + 2) as u32];
        w.extend_from_slice(body);
        w.iter().flat_map(|d| d.to_le_bytes()).collect()
    }

    #[test]
    fn stencil_registers() {
        let mut body = Vec::new();
        // dcl_globalFlags refactoringAllowed (length 1)
        body.push(106 | 1 << 11 | 1 << 24);
        // customdata of 3 dwords (its length the next dword)
        body.extend_from_slice(&[CUSTOMDATA | 3 << 11, 3, 0]);
        body.extend_from_slice(&decl(TEXTURE2D, 0, 5)); // float: a material texture's
        body.extend_from_slice(&decl(TEXTURE2D, 4, RET_UINT)); // the stencil
        body.extend_from_slice(&decl(TEXTURE2D, 2, RET_UINT)); // uint, but in the TextureMap
        body.extend_from_slice(&decl(1, 7, RET_UINT)); // a uint buffer
        body.extend_from_slice(&decl(TEXTURE2D, 4, RET_UINT)); // again
        let p = program(&body);
        assert_eq!(custom_stencil_registers(&p, &[0, 2], "t").unwrap(), [4]);
        assert_eq!(custom_stencil_registers(&p, &[0, 2, 4], "t").unwrap(), Vec::<u32>::new());
        // mixed returns are no stencil
        let p = program(&[DCL_RESOURCE | TEXTURE2D << 11 | 4 << 24, OPERAND_RESOURCE << 12 | 1 << 20, 3, 0x4445]);
        assert!(custom_stencil_registers(&p, &[], "t").unwrap().is_empty());
        // a zero length fails
        assert!(custom_stencil_registers(&program(&[0]), &[], "t").is_err());
    }
}
