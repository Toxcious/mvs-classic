//! FByteBulkData (TBulkData<T>) as cue4dump writes it: the header only (FByteBulkDataHeaderConverter). An inline
//! payload is skipped, its place kept; a payload in the .ubulk or at the end of the file is not read here. Also an
//! array's BulkSerialize ([`read_bulk_array`]), which the instanced mesh components and the mesh readers share.

use crate::archive::{Archive, ReadError, Result};
use crate::flags::{BULK_DATA_FLAGS, bitfield};
use crate::objects::ExportArchive;
use crate::value::{Props, Value};

pub const PAYLOAD_AT_END_OF_FILE: u32 = 0x1;
pub const UNUSED: u32 = 0x20;
const FORCE_INLINE_PAYLOAD: u32 = 0x40;
/// BULKDATA_PayloadInSeperateFile: the payload is in the package's .ubulk at `offset_in_file`.
pub const PAYLOAD_IN_SEPERATE_FILE: u32 = 0x100;
const SIZE_64_BIT: u32 = 0x2000;
const DUPLICATE_NON_OPTIONAL_PAYLOAD: u32 = 0x4000;
const BAD_DATA_VERSION: u32 = 0x8000;
const NO_OFFSET_FIX_UP: u32 = 0x10000;
const LAZY_LOADABLE: u32 = 0x40000;

#[derive(Clone, Copy, Debug)]
pub struct BulkHeader {
    pub flags: u32,
    pub element_count: i32,
    pub size_on_disk: u32,
    pub offset_in_file: i64,
}

/// A bulk read: the header and an inline payload's place in the export data (`ExportArchive::ar.buf`), its length
/// `size_on_disk`; `None` when the payload is not inline.
#[derive(Clone, Copy, Debug)]
pub struct BulkRead {
    pub header: BulkHeader,
    pub inline_at: Option<usize>,
}

/// FByteBulkDataHeader(FAssetArchive) for a versioned UE 4 package.
pub fn read_bulk_header(ar: &mut ExportArchive<'_>) -> Result<BulkHeader> {
    let mut flags = ar.u32()?;
    // a 64-bit count and size keep their low 32 bits, as JavaScript's `| 0` and asUintN(32) do
    let element_count = if flags & SIZE_64_BIT != 0 { ar.i64()? as i32 } else { ar.i32()? };
    let size_on_disk = if flags & SIZE_64_BIT != 0 { ar.i64()? as u32 } else { ar.u32()? };
    let mut offset_in_file = ar.i64()?;
    if flags & NO_OFFSET_FIX_UP == 0 {
        offset_in_file = offset_in_file.wrapping_add(ar.pkg.summary.bulk_data_start_offset);
    }
    if flags & BAD_DATA_VERSION != 0 {
        ar.skip(2)?;
        flags &= !BAD_DATA_VERSION;
    }
    if flags & DUPLICATE_NON_OPTIONAL_PAYLOAD != 0 {
        ar.skip(4 + if flags & SIZE_64_BIT != 0 { 8 } else { 4 } + 8)?;
    }
    Ok(BulkHeader { flags, element_count, size_on_disk, offset_in_file })
}

/// TBulkData(FAssetArchive): the header, then past an inline payload.
pub fn read_bulk_payload(ar: &mut ExportArchive<'_>) -> Result<BulkRead> {
    let header = read_bulk_header(ar)?;
    let mut inline_at = None;
    let f = header.flags;
    if !(header.size_on_disk == 0 || f & UNUSED != 0) && (f & FORCE_INLINE_PAYLOAD != 0 || f == 0 || f == LAZY_LOADABLE)
    {
        inline_at = Some(ar.pos);
        ar.skip(header.size_on_disk as usize)?;
    }
    Ok(BulkRead { header, inline_at })
}

/// TBulkData(FAssetArchive) as cue4dump writes it: the header's JSON.
pub fn read_bulk_data(ar: &mut ExportArchive<'_>) -> Result<Props> {
    Ok(bulk_json(&read_bulk_payload(ar)?.header))
}

/// The header as cue4dump writes it: BulkDataFlags, ElementCount, SizeOnDisk, OffsetInFile ("0x..." hex).
pub fn bulk_json(h: &BulkHeader) -> Props {
    let mut p = Props::new();
    p.insert("BulkDataFlags".into(), Value::Str(bitfield(u64::from(h.flags), BULK_DATA_FLAGS)));
    p.insert("ElementCount".into(), Value::Int(i64::from(h.element_count)));
    p.insert("SizeOnDisk".into(), Value::Int(i64::from(h.size_on_disk)));
    p.insert("OffsetInFile".into(), Value::Str(format!("0x{:X}", h.offset_in_file as u64)));
    p
}

/// BulkSerialize of an array: its element size, its count, then the elements;
/// `size` the element size expected. The count and the elements' bytes.
pub fn read_bulk_array<'a>(ar: &mut Archive<'a>, size: usize, what: &str) -> Result<(usize, &'a [u8])> {
    let elem = ar.i32()?;
    let count = ar.i32()?;
    let Ok(n) = usize::try_from(count) else {
        return Err(ReadError::parse(format!("{}: {what}: bulk array count {count}", ar.name)));
    };
    if n > 0 && usize::try_from(elem).ok() != Some(size) {
        return Err(ReadError::parse(format!("{}: {what}: element size {elem}, {size} expected", ar.name)));
    }
    let bytes = ar.bytes(if n > 0 { n * size } else { 0 })?;
    Ok((n, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bulk_array(elem: i32, count: i32, payload: usize) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&elem.to_le_bytes());
        b.extend_from_slice(&count.to_le_bytes());
        b.extend((0..payload).map(|i| i as u8));
        b
    }

    #[test]
    fn bulk_array_sizes() {
        let b = bulk_array(64, 2, 128);
        let mut ar = Archive::new(&b, "t");
        let (n, bytes) = read_bulk_array(&mut ar, 64, "x").unwrap();
        assert_eq!((n, bytes.len(), ar.pos), (2, 128, 136));
        // an empty array reads no elements whatever its element size
        let b = bulk_array(12, 0, 4);
        let mut ar = Archive::new(&b, "t");
        assert_eq!(read_bulk_array(&mut ar, 64, "x").unwrap().0, 0);
        assert_eq!(ar.pos, 8);
        // a wrong element size, a negative count, a short payload
        assert!(read_bulk_array(&mut Archive::new(&bulk_array(4, 1, 64), "t"), 64, "x").is_err());
        assert!(read_bulk_array(&mut Archive::new(&bulk_array(64, -1, 0), "t"), 64, "x").is_err());
        assert!(read_bulk_array(&mut Archive::new(&bulk_array(4, 3, 8), "t"), 4, "x").is_err());
    }
}
