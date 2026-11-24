//! The Direct3D 11, DXGI and Direct3D items the renderer and UI name, by the same paths on both platforms: on
//! Windows the windows crate's own, on Linux bindings generated for DXVK-native (`src/linux/bindings.rs`, written
//! by `tools/d3d-bindings` from `names.txt`), where WCHAR is the 4-byte `wchar_t`.

#[cfg(not(windows))]
mod linux;

#[cfg(windows)]
pub use windows::Win32::Foundation;
#[cfg(windows)]
pub use windows::Win32::Graphics::{Direct3D, Direct3D11, Dxgi};

#[cfg(not(windows))]
pub use linux::bindings::Windows::Win32::Foundation;
#[cfg(not(windows))]
pub use linux::bindings::Windows::Win32::Graphics::{Direct3D, Direct3D11, Dxgi};

/// The COM and result types: `windows::core` on Windows; on Linux `windows_core`, which has no string pointer
/// types there, with the local narrow ones the bindings take.
pub mod core {
    #[cfg(windows)]
    pub use windows::core::*;

    #[cfg(not(windows))]
    pub use crate::linux::strs::{PCSTR, PSTR};
    #[cfg(not(windows))]
    pub use windows_core::*;
}

/// One character of a WCHAR array in a DXGI description: UTF-16 on Windows, DXVK-native's `wchar_t` (UTF-32)
/// on Linux.
#[cfg(windows)]
pub type Wchar = u16;
/// One character of a WCHAR array in a DXGI description: UTF-16 on Windows, DXVK-native's `wchar_t` (UTF-32)
/// on Linux.
#[cfg(not(windows))]
pub type Wchar = u32;

/// The text of a WCHAR array up to its first zero (all of it when there is none), with what does not decode as
/// U+FFFD.
pub fn wide_str(w: &[Wchar]) -> String {
    let n = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    #[cfg(windows)]
    {
        String::from_utf16_lossy(&w[..n])
    }
    #[cfg(not(windows))]
    {
        w[..n].iter().map(|&c| char::from_u32(c).unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
    }
}

/// What DXVK-native's `DXVK_CONFIG` is to hold so a swap chain queues one frame, as on Windows: the player's
/// value with `dxgi.maxFrameLatency = 1` put first, or theirs unchanged when it already sets that option. First,
/// because a `[name.exe]` entry in the value scopes the entries after it to that program.
#[cfg_attr(windows, allow(dead_code))]
fn dxvk_config(old: Option<&str>) -> String {
    const LATENCY: &str = "dxgi.maxFrameLatency";
    let Some(old) = old.filter(|o| !o.trim().is_empty()) else {
        return format!("{LATENCY} = 1");
    };
    let sets_latency = old.split(';').any(|entry| {
        let key = entry.trim_start();
        key.strip_prefix(LATENCY).is_some_and(|rest| rest.trim_start().starts_with('='))
    });
    if sets_latency { old.to_string() } else { format!("{LATENCY} = 1;{old}") }
}

/// Sets up DXVK-native before the first device: its window system is SDL3 (`DXVK_WSI_DRIVER`, when the player has
/// not chosen one) and its swap chains queue one frame (`DXVK_CONFIG`, see [`dxvk_config`]). Does nothing on
/// Windows.
pub fn set_linux_wsi() {
    #[cfg(not(windows))]
    {
        let wsi = std::env::var_os("DXVK_WSI_DRIVER").is_none();
        // A value that is not Unicode is the player's and is left as it is.
        let config = match std::env::var("DXVK_CONFIG") {
            Ok(old) => Some(dxvk_config(Some(&old))),
            Err(std::env::VarError::NotPresent) => Some(dxvk_config(None)),
            Err(std::env::VarError::NotUnicode(_)) => None,
        };
        // SAFETY: called once at startup, before the device and any thread that reads the environment.
        unsafe {
            if wsi {
                std::env::set_var("DXVK_WSI_DRIVER", "SDL3");
            }
            if let Some(config) = config {
                std::env::set_var("DXVK_CONFIG", config);
            }
        }
    }
}

/// The key a compiled shader is found by on Linux, where there is no `D3DCompile`: FNV-1a 64 over the HLSL source,
/// a 0 byte, the entry point, a 0 byte and the target profile.
pub fn dxbc_key(src: &str, entry: &str, target: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in [src.as_bytes(), &[0], entry.as_bytes(), &[0], target.as_bytes()] {
        for &b in part {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    #[test]
    fn dxbc_key_is_fnv1a_64_of_the_three_parts() {
        let key = crate::dxbc_key("float4 Main() : SV_Target { return 1; }", "Main", "ps_5_0");
        assert_eq!(key, 0x7ba5_3528_f594_f2b3);
    }

    #[test]
    fn wide_str_stops_at_zero() {
        let w: Vec<crate::Wchar> = "RTX\0junk".chars().map(|c| c as crate::Wchar).collect();
        assert_eq!(crate::wide_str(&w), "RTX");
    }

    #[test]
    fn wide_str_takes_all_without_a_zero() {
        let w: Vec<crate::Wchar> = "GeForce".chars().map(|c| c as crate::Wchar).collect();
        assert_eq!(crate::wide_str(&w), "GeForce");
    }

    #[test]
    fn dxvk_config_unset_queues_one_frame() {
        assert_eq!(crate::dxvk_config(None), "dxgi.maxFrameLatency = 1");
        assert_eq!(crate::dxvk_config(Some("")), "dxgi.maxFrameLatency = 1");
    }

    #[test]
    fn dxvk_config_keeps_someone_elses_setting() {
        assert_eq!(crate::dxvk_config(Some("dxgi.syncInterval = 0")), "dxgi.maxFrameLatency = 1;dxgi.syncInterval = 0");
    }

    #[test]
    fn dxvk_config_keeps_the_players_own_latency() {
        let theirs = "dxgi.syncInterval = 0; dxgi.maxFrameLatency=3";
        assert_eq!(crate::dxvk_config(Some(theirs)), theirs);
    }

    // DXVK-native's WCHAR is wchar_t, 4 bytes (include/native/windows/windows_base.h), so the DXGI descriptions
    // with WCHAR arrays are laid out differently from Windows'. One test per struct the generator rewrites; the
    // offsets and sizes are what a C compiler gives for DXVK-native's own dxgi.h.
    #[cfg(not(windows))]
    #[test]
    fn adapter_desc_has_dxvk_native_layout() {
        use crate::Dxgi::DXGI_ADAPTER_DESC;
        use std::mem::{offset_of, size_of};
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC, Description), 0);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC, VendorId), 512);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC, DeviceId), 516);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC, SubSysId), 520);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC, Revision), 524);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC, DedicatedVideoMemory), 528);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC, DedicatedSystemMemory), 536);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC, SharedSystemMemory), 544);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC, AdapterLuid), 552);
        assert_eq!(size_of::<DXGI_ADAPTER_DESC>(), 560);
    }

    #[cfg(not(windows))]
    #[test]
    fn adapter_desc1_has_dxvk_native_layout() {
        use crate::Dxgi::DXGI_ADAPTER_DESC1;
        use std::mem::{offset_of, size_of};
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC1, VendorId), 512);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC1, DedicatedVideoMemory), 528);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC1, AdapterLuid), 552);
        assert_eq!(offset_of!(DXGI_ADAPTER_DESC1, Flags), 560);
        assert_eq!(size_of::<DXGI_ADAPTER_DESC1>(), 568);
    }

    #[cfg(not(windows))]
    #[test]
    fn output_desc_has_dxvk_native_layout() {
        use crate::Dxgi::DXGI_OUTPUT_DESC;
        use std::mem::{offset_of, size_of};
        assert_eq!(offset_of!(DXGI_OUTPUT_DESC, DeviceName), 0);
        assert_eq!(offset_of!(DXGI_OUTPUT_DESC, DesktopCoordinates), 128);
        assert_eq!(offset_of!(DXGI_OUTPUT_DESC, AttachedToDesktop), 144);
        assert_eq!(offset_of!(DXGI_OUTPUT_DESC, Rotation), 148);
        assert_eq!(offset_of!(DXGI_OUTPUT_DESC, Monitor), 152);
        assert_eq!(size_of::<DXGI_OUTPUT_DESC>(), 160);
    }
}
