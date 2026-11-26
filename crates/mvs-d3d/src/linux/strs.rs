//! The narrow string pointers the Linux bindings take: `windows_core` defines its string types only on Windows
//! (`windows-strings` is a Windows-only crate). Same layout as Windows', a `char` being one byte under DXVK-native.

/// A pointer to a constant, zero-terminated narrow string (`LPCSTR`): an input layout's semantic name.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PCSTR(pub *const u8);

impl PCSTR {
    /// The null pointer.
    pub const fn null() -> PCSTR {
        PCSTR(std::ptr::null())
    }

    /// Whether the pointer is null.
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

impl Default for PCSTR {
    fn default() -> Self {
        PCSTR::null()
    }
}

impl windows_core::TypeKind for PCSTR {
    type TypeKind = windows_core::CopyType;
}

/// A pointer to a writable narrow string (`LPSTR`): the bindings' `ID3D11ClassInstance` names take one.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PSTR(pub *mut u8);

impl PSTR {
    /// The null pointer.
    pub const fn null() -> PSTR {
        PSTR(std::ptr::null_mut())
    }

    /// Whether the pointer is null.
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

impl Default for PSTR {
    fn default() -> Self {
        PSTR::null()
    }
}

impl windows_core::TypeKind for PSTR {
    type TypeKind = windows_core::CopyType;
}
