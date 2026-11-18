//! The console of the two programs, which are built as desktop programs (no console window of their own): started
//! from a terminal, their lines go to that terminal; started from Explorer or a shortcut, nowhere.

use std::os::windows::io::IntoRawHandle;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
};

/// Points stdout and stderr, each that was not handed over (a pipe or a file), at the console of the program that
/// started this one, when it has one. Called first thing in `main`.
pub fn attach_parent() {
    // SAFETY: reads this process's standard handle.
    let none = |h: STD_HANDLE| unsafe { GetStdHandle(h) }.is_ok_and(|h| h.is_invalid());
    let want: Vec<STD_HANDLE> = [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE].into_iter().filter(|&h| none(h)).collect();
    // SAFETY: attaches this process, which has no console, to its parent's; fails when the parent has none.
    if want.is_empty() || unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_err() {
        return;
    }
    for h in want {
        let Ok(out) = std::fs::OpenOptions::new().write(true).open("CONOUT$") else { return };
        // SAFETY: the handle is left open for the process's life as its standard handle.
        let _ = unsafe { SetStdHandle(h, HANDLE(out.into_raw_handle())) };
    }
}
