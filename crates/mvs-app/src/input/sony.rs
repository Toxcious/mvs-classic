//! The Sony pads over HID: DualShock 4 and DualSense on USB and Bluetooth (WinDualShock: CreateInputDevice
//! 1409c4aa0, SendControllerEvents 1409c4860, the per-pad read 1409c5150 over libScePad's scePadReadState
//! 1435e6ed0; the vibration send 1409c5c20). Each pad is read without blocking (an overlapped read drained once a
//! poll, the newest report kept) and the HID devices are looked through again when one arrives.
//!
//! The build reads libScePad's ScePadData; libScePad's own parse of the HID reports is not read, so the report
//! layouts below are the public ones.

use std::mem::size_of;

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_Device_Interface_List_SizeW, CM_Get_Device_Interface_ListW,
    CR_BUFFER_SMALL, CR_SUCCESS,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData,
    HidP_GetCaps, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    ReadFile, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::CreateEventW;
use windows::core::PCWSTR;

use super::pad::PadState;

/// Sony's USB vendor id.
pub const SONY_VID: u16 = 0x054c;
/// The most reports drained from one pad in a poll (the HID driver queues 32 by default).
const DRAIN: usize = 64;

/// The pads WinDualShock opens (the PIDs it matches: 0x05C4, 0x09CC, 0x0BA0, 0x0CE6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    /// DualShock 4, first and second revision.
    Ds4,
    /// The DualShock 4 USB wireless adapter (a DualShock 4 behind it).
    Ds4Adapter,
    DualSense,
}

impl Model {
    pub fn of(pid: u16) -> Option<Model> {
        match pid {
            0x05c4 | 0x09cc => Some(Model::Ds4),
            0x0ba0 => Some(Model::Ds4Adapter),
            0x0ce6 => Some(Model::DualSense),
            _ => None,
        }
    }
}

/// What one input report said.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Report {
    /// A pad's state.
    Pad(PadState),
    /// The adapter with no pad behind it.
    NoPad,
}

/// A stick byte as WinDualShock's analog event carries it (1409c5150): (b - 128) / 128, up positive for Y (the
/// caller passes 255 - b).
fn stick(b: u8) -> f64 {
    (f64::from(b) - 128.0) * 0.0078125
}

/// The PadState of the report's bytes from the left stick on: the sticks at `s`, the hat and face buttons at `f`,
/// the shoulder, trigger and menu buttons at `f + 1`, the PS and touch pad buttons at `f + 2`.
///
/// The keys (1409c4c40's FGamepadKeyNames per ScePad button bit): Cross Gamepad_FaceButton_Bottom, Circle Right,
/// Square Left, Triangle Top, L1 / R1 the shoulders, L2 / R2 Gamepad_Left / RightTriggerThreshold, Options
/// Gamepad_Special_Right, the touch pad's click Gamepad_Special_Left, L3 / R3 the thumbsticks, the d-pad; Share /
/// Create (bit 0) and PS (bit 16) map to no key. Gamepad_LeftStick_Up is the raw Y byte below 0x40.
// Unverified: the trigger buttons are the report's own L2 / R2 bits and the hat's diagonals press both d-pad
// directions (libScePad's ScePadData button word from the report is not read).
fn pad_of(r: &[u8], s: usize, f: usize) -> PadState {
    let (face, menu, sys) = (r[f], r[f + 1], r[f + 2]);
    let mut buttons = 0u16;
    let mut set = |on: bool, bit: u16| {
        if on {
            buttons |= 1 << bit;
        }
    };
    set(face & 0x20 != 0, 0);
    set(face & 0x40 != 0, 1);
    set(face & 0x10 != 0, 2);
    set(face & 0x80 != 0, 3);
    set(menu & 0x01 != 0, 4);
    set(menu & 0x02 != 0, 5);
    set(menu & 0x04 != 0, 6);
    set(menu & 0x08 != 0, 7);
    set(sys & 0x02 != 0, 8);
    set(menu & 0x20 != 0, 9);
    set(menu & 0x40 != 0, 10);
    set(menu & 0x80 != 0, 11);
    // (the hat: 0 up, clockwise by eighths, 8 centred)
    let hat = face & 0x0f;
    set(matches!(hat, 7 | 0 | 1), 12);
    set(matches!(hat, 3..=5), 13);
    set(matches!(hat, 5..=7), 14);
    set(matches!(hat, 1..=3), 15);
    PadState {
        buttons,
        lx: stick(r[s]),
        ly: stick(255 - r[s + 1]),
        rx: stick(r[s + 2]),
        ry: stick(255 - r[s + 3]),
        stick_up: r[s + 1] < 0x40,
    }
}

/// An input report of `model` (`bt` over Bluetooth); None for a report it does not read.
// Unverified: the public report layouts (DualShock 4: 0x01 on USB and Bluetooth's basic report, 0x11 Bluetooth;
// DualSense: 0x01 on USB, 0x31 Bluetooth, Bluetooth's basic 0x01 as the DualShock 4's) and the adapter's no-pad bit
// (byte 31, 0x04).
pub fn parse(model: Model, bt: bool, r: &[u8]) -> Option<Report> {
    let ds4 = |o: usize| (r.len() >= o + 7).then(|| Report::Pad(pad_of(r, o, o + 4)));
    let ds5 = |o: usize| (r.len() >= o + 10).then(|| Report::Pad(pad_of(r, o, o + 7)));
    match (model, *r.first()?) {
        (Model::DualSense, 0x01) if !bt => ds5(1),
        (Model::DualSense, 0x31) => ds5(2),
        (Model::Ds4Adapter, 0x01) if r.len() > 31 && r[31] & 0x04 != 0 => Some(Report::NoPad),
        (_, 0x01) => ds4(1),
        (Model::Ds4 | Model::Ds4Adapter, 0x11) => ds4(3),
        _ => None,
    }
}

/// The CRC-32 (IEEE, reflected).
fn crc32(data: impl IntoIterator<Item = u8>) -> u32 {
    let mut c = !0u32;
    for b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// The CRC-32 a Bluetooth output report ends with: over the 0xA2 header byte and the report.
pub fn bt_crc(data: &[u8]) -> u32 {
    crc32(std::iter::once(0xa2).chain(data.iter().copied()))
}

/// The output report that sets the motors (`large` the low-frequency one, `small` the high-frequency one, as
/// scePadSetVibration's ScePadVibrationParam), `len` bytes at least (the device's output report length).
// Unverified: the public layouts (DualShock 4: 0x05 USB, 0x11 Bluetooth; DualSense: 0x02 USB, 0x31 Bluetooth; the
// Bluetooth ones 78 bytes with the CRC in their last four). Only the motors' enable bits are set: the light bar and
// the rest keep theirs.
pub fn rumble_report(model: Model, bt: bool, len: usize, large: u8, small: u8) -> Vec<u8> {
    let (id, size, at) = match (model, bt) {
        (Model::DualSense, false) => (0x02, 48, 3),
        (Model::DualSense, true) => (0x31, 78, 4),
        (_, false) => (0x05, 32, 4),
        (_, true) => (0x11, 78, 6),
    };
    let mut r = vec![0u8; len.max(size)];
    r[0] = id;
    match (model, bt) {
        // (valid flags: compatible vibration, haptics select)
        (Model::DualSense, false) => r[1] = 0x03,
        (Model::DualSense, true) => {
            r[1] = 0x02;
            r[2] = 0x03;
        }
        // (flags: the motors)
        (_, false) => r[1] = 0x01,
        (_, true) => {
            // (HID and CRC on, 4 ms report interval)
            r[1] = 0xc4;
            r[3] = 0x01;
        }
    }
    r[at] = small;
    r[at + 1] = large;
    if bt {
        let crc = bt_crc(&r[..74]);
        r[74..78].copy_from_slice(&crc.to_le_bytes());
    }
    r
}

/// An overlapped read or write: its OVERLAPPED and its buffer at a fixed address while the I/O is pending.
struct Io {
    ov: OVERLAPPED,
    buf: Vec<u8>,
    pending: bool,
}

impl std::fmt::Debug for Io {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Io").field("len", &self.buf.len()).field("pending", &self.pending).finish()
    }
}

impl Io {
    fn new(len: usize) -> Option<Box<Io>> {
        // SAFETY: a plain manual-reset event, closed in SonyPad's drop.
        let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.ok()?;
        Some(Box::new(Io { ov: OVERLAPPED { hEvent: event, ..Default::default() }, buf: vec![0; len], pending: false }))
    }
}

/// One open Sony pad.
#[derive(Debug)]
struct SonyPad {
    serial: u32,
    path: Vec<u16>,
    handle: HANDLE,
    model: Model,
    bt: bool,
    out_len: usize,
    read: Box<Io>,
    write: Box<Io>,
    /// The newest report's state; None before the first or with no pad behind the adapter.
    state: Option<PadState>,
    /// The motors (large, small) to send, and those last sent.
    want: (u8, u8),
    sent: (u8, u8),
    /// The last output report was refused.
    write_failed: bool,
    dead: bool,
}

impl SonyPad {
    /// `path` (NUL-terminated) opened for overlapped reads and writes, if it is a Sony pad WinDualShock reads.
    fn open(path: &[u16], serial: u32) -> Option<SonyPad> {
        let p = PCWSTR(path.as_ptr());
        let share = FILE_SHARE_READ | FILE_SHARE_WRITE;
        // SAFETY: `path` is NUL-terminated; the handle is closed before return. No access rights: any HID device
        // opens this way, its attributes readable.
        let model = unsafe {
            let h = CreateFileW(p, 0, share, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None).ok()?;
            let mut a = HIDD_ATTRIBUTES { Size: size_of::<HIDD_ATTRIBUTES>() as u32, ..Default::default() };
            let ok = HidD_GetAttributes(h, &mut a);
            let _ = CloseHandle(h);
            if !ok || a.VendorID != SONY_VID {
                return None;
            }
            Model::of(a.ProductID)?
        };
        let rw = GENERIC_READ.0 | GENERIC_WRITE.0;
        // SAFETY: as above; the handle is owned by the SonyPad (closed in its drop) or closed here on a failure.
        let handle = unsafe { CreateFileW(p, rw, share, None, OPEN_EXISTING, FILE_FLAG_OVERLAPPED, None) }.ok()?;
        let mut caps = HIDP_CAPS::default();
        let mut pre = PHIDP_PREPARSED_DATA::default();
        // SAFETY: a valid HID handle and out-pointers; the preparsed data is freed right after.
        let ok = unsafe {
            HidD_GetPreparsedData(handle, &mut pre) && {
                let ok = HidP_GetCaps(pre, &mut caps).is_ok();
                let _ = HidD_FreePreparsedData(pre);
                ok
            }
        };
        let ios = ok.then(|| Some((Io::new(usize::from(caps.InputReportByteLength))?, Io::new(0)?))).flatten();
        let Some((read, write)) = ios else {
            // SAFETY: the handle opened above.
            unsafe {
                let _ = CloseHandle(handle);
            }
            return None;
        };
        // Unverified: the transport by the input report's length (64 on USB for both pads).
        let bt = caps.InputReportByteLength != 64;
        Some(SonyPad {
            serial,
            path: path.to_vec(),
            handle,
            model,
            bt,
            out_len: usize::from(caps.OutputReportByteLength),
            read,
            write,
            state: None,
            want: (0, 0),
            sent: (0, 0),
            write_failed: false,
            dead: false,
        })
    }

    /// Every report the driver holds read, the newest kept; a failed read marks the pad gone.
    fn drain(&mut self) {
        for _ in 0..DRAIN {
            let io = &mut *self.read;
            if !io.pending {
                // SAFETY: the buffer and the OVERLAPPED are boxed and stay put until the read completes or the drop
                // has cancelled it and waited.
                let r = unsafe { ReadFile(self.handle, Some(&mut io.buf), None, Some(&mut io.ov)) };
                match r {
                    Ok(()) => {}
                    Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {}
                    Err(_) => {
                        self.dead = true;
                        return;
                    }
                }
                io.pending = true;
            }
            let mut n = 0u32;
            // SAFETY: the pending read's OVERLAPPED; no wait.
            match unsafe { GetOverlappedResult(self.handle, &io.ov, &mut n, false) } {
                Ok(()) => {
                    io.pending = false;
                    let len = (n as usize).min(io.buf.len());
                    match parse(self.model, self.bt, &io.buf[..len]) {
                        Some(Report::Pad(s)) => self.state = Some(s),
                        Some(Report::NoPad) => self.state = None,
                        None => {}
                    }
                }
                Err(e) if e.code() == ERROR_IO_INCOMPLETE.to_hresult() => return,
                Err(_) => {
                    io.pending = false;
                    self.dead = true;
                    return;
                }
            }
        }
    }

    /// The motors sent when they changed and the last write is done.
    fn send(&mut self) {
        let io = &mut *self.write;
        if io.pending {
            let mut n = 0u32;
            // SAFETY: the pending write's OVERLAPPED; no wait.
            match unsafe { GetOverlappedResult(self.handle, &io.ov, &mut n, false) } {
                Err(e) if e.code() == ERROR_IO_INCOMPLETE.to_hresult() => return,
                _ => io.pending = false,
            }
        }
        if self.want == self.sent || self.out_len == 0 {
            return;
        }
        io.buf = rumble_report(self.model, self.bt, self.out_len, self.want.0, self.want.1);
        // SAFETY: as the read's: the boxed buffer and OVERLAPPED stay put until the write completes or is cancelled.
        let r = unsafe { WriteFile(self.handle, Some(&io.buf), None, Some(&mut io.ov)) };
        // (a pad that takes no output report keeps its motors as they are)
        self.write_failed = false;
        match r {
            Ok(()) => io.pending = true,
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => io.pending = true,
            Err(_) => self.write_failed = true,
        }
        self.sent = self.want;
    }
}

impl Drop for SonyPad {
    fn drop(&mut self) {
        // SAFETY: the pad's own handle and events; the pending I/O is cancelled and waited for before its buffers
        // go.
        unsafe {
            let _ = CancelIoEx(self.handle, None);
            for io in [&mut *self.read, &mut *self.write] {
                if io.pending {
                    let mut n = 0u32;
                    let _ = GetOverlappedResult(self.handle, &io.ov, &mut n, true);
                }
                let _ = CloseHandle(io.ov.hEvent);
            }
            let _ = CloseHandle(self.handle);
        }
    }
}

/// The present HID device interfaces' paths, each NUL-terminated.
fn hid_paths() -> Vec<Vec<u16>> {
    // SAFETY: plain cfgmgr32 calls with a valid GUID and a buffer of the size they asked for.
    let list = unsafe {
        let guid = HidD_GetHidGuid();
        loop {
            let mut len = 0u32;
            if CM_Get_Device_Interface_List_SizeW(&mut len, &guid, PCWSTR::null(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT)
                != CR_SUCCESS
            {
                return Vec::new();
            }
            let mut buf = vec![0u16; len as usize];
            match CM_Get_Device_Interface_ListW(&guid, PCWSTR::null(), &mut buf, CM_GET_DEVICE_INTERFACE_LIST_PRESENT) {
                CR_SUCCESS => break buf,
                CR_BUFFER_SMALL => continue,
                _ => return Vec::new(),
            }
        }
    };
    list.split(|&c| c == 0).filter(|p| !p.is_empty()).map(|p| p.iter().copied().chain([0]).collect()).collect()
}

/// A pad as `describe` gives it.
#[derive(Clone, Debug)]
pub struct PadInfo {
    pub serial: u32,
    pub model: Model,
    pub bt: bool,
    /// The input and output reports' lengths.
    pub lens: (usize, usize),
    pub write_failed: bool,
    pub path: String,
    pub state: Option<PadState>,
}

/// The open Sony pads.
#[derive(Debug)]
pub struct SonyPads {
    pads: Vec<SonyPad>,
    /// The HID devices are looked through at the next poll (at the start, and when one arrived).
    rescan: bool,
    next_serial: u32,
}

impl Default for SonyPads {
    fn default() -> SonyPads {
        SonyPads { pads: Vec::new(), rescan: true, next_serial: 0 }
    }
}

impl SonyPads {
    /// A HID device arrived: look again at the next poll.
    pub fn rescan(&mut self) {
        self.rescan = true;
    }

    /// New pads opened (after an arrival), every pad's reports drained, the gone ones closed.
    pub fn poll(&mut self) {
        if std::mem::take(&mut self.rescan) {
            for path in hid_paths() {
                if self.pads.iter().any(|p| p.path == path) {
                    continue;
                }
                if let Some(p) = SonyPad::open(&path, self.next_serial) {
                    self.next_serial += 1;
                    self.pads.push(p);
                }
            }
        }
        for p in &mut self.pads {
            p.drain();
        }
        self.pads.retain(|p| !p.dead);
    }

    /// Each open pad's serial and state (None: no report yet, or no pad behind the adapter).
    pub fn states(&self) -> impl Iterator<Item = (u32, Option<PadState>)> + '_ {
        self.pads.iter().map(|p| (p.serial, p.state))
    }

    /// Whether the pad of `serial` is still open.
    pub fn open(&self, serial: u32) -> bool {
        self.pads.iter().any(|p| p.serial == serial)
    }

    /// The motors of the pad of `serial` (ScePadVibrationParam's large and small motor), sent now or once its last
    /// write is done.
    pub fn rumble(&mut self, serial: u32, large: u8, small: u8) {
        if let Some(p) = self.pads.iter_mut().find(|p| p.serial == serial) {
            p.want = (large, small);
            p.send();
        }
    }

    /// The writes left waiting sent.
    pub fn flush(&mut self) {
        for p in &mut self.pads {
            p.send();
        }
    }

    /// The open pads (a probe's listing).
    pub fn describe(&self) -> Vec<PadInfo> {
        self.pads
            .iter()
            .map(|p| PadInfo {
                serial: p.serial,
                model: p.model,
                bt: p.bt,
                lens: (p.read.buf.len(), p.out_len),
                write_failed: p.write_failed,
                path: String::from_utf16_lossy(&p.path[..p.path.len() - 1]),
                state: p.state,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ds4_usb_report_reads() {
        let mut r = [0u8; 64];
        r[0] = 0x01;
        r[1..5].copy_from_slice(&[128, 0, 255, 128]);
        // (hat up-right, Cross; L1, Options; touch pad)
        r[5] = 0x01 | 0x20;
        r[6] = 0x01 | 0x20;
        r[7] = 0x02;
        let Some(Report::Pad(p)) = parse(Model::Ds4, false, &r) else { panic!("no pad") };
        assert_eq!((p.lx, p.ly, p.rx, p.ry), (0.0, 127.0 / 128.0, 127.0 / 128.0, -1.0 / 128.0));
        assert!(p.stick_up);
        assert_eq!(p.buttons, 1 | 1 << 4 | 1 << 8 | 1 << 9 | 1 << 12 | 1 << 15);
    }

    #[test]
    fn bluetooth_and_dualsense_layouts_read() {
        let mut r = [0u8; 78];
        r[0] = 0x11;
        r[3..7].copy_from_slice(&[0, 128, 128, 255]);
        r[7] = 0x08 | 0x40;
        let Some(Report::Pad(p)) = parse(Model::Ds4, true, &r) else { panic!("no pad") };
        assert_eq!((p.lx, p.ry), (-1.0, -1.0));
        assert_eq!(p.buttons, 1 << 1);
        let mut r = [0u8; 64];
        r[0] = 0x01;
        r[1..5].copy_from_slice(&[128; 4]);
        r[8] = 0x04 | 0x80;
        r[9] = 0x04 | 0x10;
        let Some(Report::Pad(p)) = parse(Model::DualSense, false, &r) else { panic!("no pad") };
        // (Create maps to no key)
        assert_eq!(p.buttons, 1 << 3 | 1 << 6 | 1 << 13);
        assert!(!p.stick_up);
    }

    #[test]
    fn a_bluetooth_output_report_carries_its_crc() {
        let r = rumble_report(Model::Ds4, true, 78, 255, 10);
        assert_eq!((r[6], r[7]), (10, 255));
        assert_eq!(u32::from_le_bytes(r[74..78].try_into().unwrap()), bt_crc(&r[..74]));
        // (the CRC-32 check value)
        assert_eq!(crc32(*b"123456789"), 0xcbf4_3926);
    }
}
