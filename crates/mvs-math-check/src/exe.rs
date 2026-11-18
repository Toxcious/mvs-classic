//! The beta's shipping exe mapped into this process without running it, so its leaf math routines can be called by
//! address: the sections copied to their virtual addresses (at the preferred base when it is free, else relocated),
//! the C runtime imports (`api-ms-win-crt-*`) bound to the system's ucrtbase, every other import bound to a trap, and
//! the per-file initializers that only copy a 16-byte vector constant (`movaps xmm0, [c]; movaps [g], xmm0; ret`, or
//! its movdqa form) done by copying. Nothing else of the exe's start-up runs, so only routines that read no other
//! runtime state can be called; `check_globals` confirms the ones the checks use.
//!
//! Addresses are the exe's own (preferred base 0x140000000).

use std::ffi::{CString, c_void};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAlloc, VirtualFree,
};
use windows::core::PCSTR;

pub const EXE: &str = r"C:\Program Files (x86)\Steam\steamapps\content\app_1818750\depot_1818751\MultiVersus\Binaries\Win64\MultiVersus-Win64-Shipping.exe";

pub struct Image {
    base: *mut u8,
    preferred: u64,
    size: usize,
    /// How many vector-constant initializers were copied.
    pub initializers: usize,
}

// SAFETY: the mapping is written only while `load` builds it; afterwards it is read and its code called, which the
// called routines do without shared mutable state (they are leaf math routines).
unsafe impl Send for Image {}
unsafe impl Sync for Image {}

struct Section {
    name: String,
    va: u32,
    vsize: u32,
    raw: u32,
    raw_size: u32,
}

fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(d[o..o + 2].try_into().unwrap())
}

fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

fn u64_at(d: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(d[o..o + 8].try_into().unwrap())
}

extern "C" fn unbound_import() {
    eprintln!("the mapped exe called an import that is not bound (only the C runtime's are)");
    std::process::abort();
}

impl Image {
    pub fn load(path: &str) -> Result<Image, String> {
        let d = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let pe = u32_at(&d, 0x3c) as usize;
        if &d[pe..pe + 4] != b"PE\0\0" {
            return Err("not a PE file".into());
        }
        let coff = pe + 4;
        let sections_n = u16_at(&d, coff + 2) as usize;
        let opt = coff + 20;
        let opt_size = u16_at(&d, coff + 16) as usize;
        if u16_at(&d, opt) != 0x20b {
            return Err("not PE32+".into());
        }
        let preferred = u64_at(&d, opt + 24);
        let size = u32_at(&d, opt + 56) as usize;
        let headers = u32_at(&d, opt + 60) as usize;
        let dir = |i: usize| (u32_at(&d, opt + 112 + 8 * i) as usize, u32_at(&d, opt + 116 + 8 * i) as usize);
        let sections: Vec<Section> = (0..sections_n)
            .map(|i| {
                let s = opt + opt_size + 40 * i;
                Section {
                    name: String::from_utf8_lossy(&d[s..s + 8]).trim_end_matches('\0').to_string(),
                    vsize: u32_at(&d, s + 8),
                    va: u32_at(&d, s + 12),
                    raw_size: u32_at(&d, s + 16),
                    raw: u32_at(&d, s + 20),
                }
            })
            .collect();

        // SAFETY: a fresh reservation; null is checked.
        let mut base = unsafe {
            VirtualAlloc(Some(preferred as *const c_void), size, MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE)
        } as *mut u8;
        if base.is_null() {
            // SAFETY: as above, anywhere.
            base = unsafe { VirtualAlloc(None, size, MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE) } as *mut u8;
        }
        if base.is_null() {
            return Err(format!("could not reserve {size} bytes"));
        }
        let mut image = Image { base, preferred, size, initializers: 0 };
        // SAFETY: `base` holds `size` committed bytes; every write below is bounds-checked against it through `mem`.
        let mem = unsafe { std::slice::from_raw_parts_mut(base, size) };
        mem[..headers].copy_from_slice(&d[..headers]);
        for s in &sections {
            let n = s.raw_size.min(s.vsize) as usize;
            mem[s.va as usize..s.va as usize + n].copy_from_slice(&d[s.raw as usize..s.raw as usize + n]);
        }

        // Base relocations: only DIR64 entries in a PE32+ image.
        let delta = (base as u64).wrapping_sub(preferred);
        let (reloc, reloc_size) = dir(5);
        if delta != 0 {
            let mut o = reloc;
            while o < reloc + reloc_size {
                let page = u32_at(mem, o) as usize;
                let block = u32_at(mem, o + 4) as usize;
                if block == 0 {
                    break;
                }
                for k in (8..block).step_by(2) {
                    let e = u16_at(mem, o + k);
                    match e >> 12 {
                        0 => {}
                        10 => {
                            let at = page + (e & 0xfff) as usize;
                            let v = u64_at(mem, at).wrapping_add(delta);
                            mem[at..at + 8].copy_from_slice(&v.to_le_bytes());
                        }
                        t => return Err(format!("relocation type {t}")),
                    }
                }
                o += block;
            }
        }

        // Imports.
        // SAFETY: loading a system DLL by name.
        let ucrt = unsafe { LoadLibraryA(PCSTR(c"ucrtbase.dll".as_ptr() as *const u8)) }
            .map_err(|e| format!("ucrtbase.dll: {e}"))?;
        let trap = unbound_import as *const () as u64;
        let (imports, _) = dir(1);
        let mut o = imports;
        loop {
            let lookup = u32_at(mem, o) as usize;
            let name = u32_at(mem, o + 12) as usize;
            let thunks = u32_at(mem, o + 16) as usize;
            if name == 0 {
                break;
            }
            let dll = cstr(mem, name).to_ascii_lowercase();
            let crt = dll.starts_with("api-ms-win-crt-") || dll == "ucrtbase.dll";
            let lookup = if lookup != 0 { lookup } else { thunks };
            let mut k = 0;
            loop {
                let entry = u64_at(mem, lookup + 8 * k);
                if entry == 0 {
                    break;
                }
                let mut target = trap;
                if crt && entry >> 63 == 0 {
                    let symbol = CString::new(cstr(mem, entry as usize + 2)).unwrap();
                    // SAFETY: a lookup by name in a loaded module.
                    if let Some(f) = unsafe { GetProcAddress(ucrt, PCSTR(symbol.as_ptr() as *const u8)) } {
                        target = f as usize as u64;
                    }
                }
                let at = thunks + 8 * k;
                mem[at..at + 8].copy_from_slice(&target.to_le_bytes());
                k += 1;
            }
            o += 20;
        }

        // The vector-constant initializers: copy rdata's 16 bytes to the data global.
        let section = |n: &str| sections.iter().find(|s| s.name == n).map(|s| (s.va as usize, s.vsize as usize));
        let (text, text_size) = section(".text").ok_or("no .text")?;
        let (rdata, rdata_size) = section(".rdata").ok_or("no .rdata")?;
        let (data, data_size) = section(".data").ok_or("no .data")?;
        let mut copies = Vec::new();
        for i in text..text + text_size - 16 {
            let (load, store, len) = if mem[i..i + 3] == [0x0f, 0x28, 0x05] && mem[i + 7..i + 10] == [0x0f, 0x29, 0x05]
            {
                (i + 3, i + 10, 15)
            } else if mem[i..i + 4] == [0x66, 0x0f, 0x6f, 0x05] && mem[i + 8..i + 12] == [0x66, 0x0f, 0x7f, 0x05] {
                (i + 4, i + 12, 17)
            } else {
                continue;
            };
            if mem[i + len - 1] != 0xc3 {
                continue;
            }
            let from = (load + 4).wrapping_add_signed(u32_at(mem, load) as i32 as isize);
            let to = (store + 4).wrapping_add_signed(u32_at(mem, store) as i32 as isize);
            if (rdata..rdata + rdata_size).contains(&from) && (data..data + data_size).contains(&to) {
                copies.push((from, to));
            }
        }
        for (from, to) in &copies {
            mem.copy_within(*from..*from + 16, *to);
        }
        image.initializers = copies.len();
        Ok(image)
    }

    fn rva(&self, va: u64) -> usize {
        let r = va.checked_sub(self.preferred).expect("address below the image") as usize;
        assert!(r < self.size, "address {va:#x} outside the image");
        r
    }

    pub fn u32_at(&self, va: u64) -> u32 {
        let r = self.rva(va);
        // SAFETY: in bounds of the mapping (checked by `rva`); unaligned read.
        unsafe { (self.base.add(r) as *const u32).read_unaligned() }
    }

    /// The routine at `va` as a function pointer of type `F`.
    ///
    /// # Safety
    /// `F` must be an `extern "C"` function pointer type matching the routine's arguments and result.
    pub unsafe fn func<F: Copy>(&self, va: u64) -> F {
        assert_eq!(size_of::<F>(), size_of::<*const u8>());
        let p = self.base.wrapping_add(self.rva(va));
        // SAFETY: the caller promises F is a function pointer type of the routine at `va`.
        unsafe { std::mem::transmute_copy(&p) }
    }

    /// Fails unless every (address, first lane) pair holds its value: the globals a check relies on were set.
    pub fn check_globals(&self, globals: &[(u64, u32)]) -> Result<(), String> {
        for &(va, want) in globals {
            let got = self.u32_at(va);
            if got != want {
                return Err(format!("global {va:#x} holds {got:#010x}, expected {want:#010x}"));
            }
        }
        Ok(())
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        // SAFETY: the reservation made in `load`, released once.
        let _ = unsafe { VirtualFree(self.base as *mut c_void, 0, MEM_RELEASE) };
    }
}

fn cstr(mem: &[u8], at: usize) -> String {
    let end = mem[at..].iter().position(|&b| b == 0).unwrap_or(0);
    String::from_utf8_lossy(&mem[at..at + end]).into_owned()
}
