//! Small platform adapter for invitation text. No rendering/network dependency.

use std::io;

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::WindowsAndMessaging::GetForegroundWindow,
    };
    const TEXT: u32 = 13; // CF_UNICODETEXT
    struct Clipboard;
    impl Clipboard {
        fn open() -> io::Result<Self> {
            // SAFETY: The app calls this on its window thread; the guard closes a successful open.
            if unsafe { OpenClipboard(GetForegroundWindow()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self)
        }
    }
    impl Drop for Clipboard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    pub fn read() -> io::Result<String> {
        let _guard = Clipboard::open()?;
        // SAFETY: The clipboard stays open while its bounded global-memory block is locked/read.
        unsafe {
            let handle = GetClipboardData(TEXT);
            if handle.is_null() {
                return Err(io::Error::other("Clipboard has no text"));
            }
            let count = GlobalSize(handle) / 2;
            if count == 0 || count > 65536 {
                return Err(io::Error::other("Clipboard text exceeds limit"));
            }
            let memory = GlobalLock(handle) as *const u16;
            if memory.is_null() {
                return Err(io::Error::last_os_error());
            }
            let units = std::slice::from_raw_parts(memory, count);
            let end = units.iter().position(|c| *c == 0).unwrap_or(count);
            let result = String::from_utf16(&units[..end])
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
            GlobalUnlock(handle);
            result
        }
    }
    pub fn write(text: &str) -> io::Result<()> {
        let _guard = Clipboard::open()?;
        let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        if units.len() > 65536 {
            return Err(io::Error::other("Clipboard text exceeds limit"));
        }
        // SAFETY: Allocate enough UTF-16 storage; ownership transfers to Windows only on success.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2);
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let memory = GlobalLock(handle) as *mut u16;
            if memory.is_null() {
                GlobalFree(handle);
                return Err(io::Error::last_os_error());
            }
            ptr::copy_nonoverlapping(units.as_ptr(), memory, units.len());
            GlobalUnlock(handle);
            if EmptyClipboard() == 0 || SetClipboardData(TEXT, handle).is_null() {
                GlobalFree(handle);
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub fn read() -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Clipboard adapter is not available on this platform; use --connect",
        ))
    }
    pub fn write(_: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Clipboard adapter is not available on this platform",
        ))
    }
}
pub use platform::{read, write};
