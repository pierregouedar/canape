//! Reading the clipboard's text, for the GOG sign-in: the address of the
//! page GOG lands on carries the code Canapé needs (see
//! `canape_library::gogdl::parse_code`).

/// The clipboard's text, if it holds some.
#[cfg(windows)]
pub fn text() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    /// `CF_UNICODETEXT`.
    const UNICODE_TEXT: u32 = 13;
    /// Longest text read: a sign-in address is a few hundred characters.
    const MAX: usize = 8 * 1024;

    unsafe {
        if IsClipboardFormatAvailable(UNICODE_TEXT) == 0 || OpenClipboard(std::ptr::null_mut()) == 0
        {
            return None;
        }
        let mut text = None;
        let handle = GetClipboardData(UNICODE_TEXT);
        if !handle.is_null() {
            let ptr = GlobalLock(handle) as *const u16;
            if !ptr.is_null() {
                let mut len = 0;
                while len < MAX && *ptr.add(len) != 0 {
                    len += 1;
                }
                text = Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                    ptr, len,
                )));
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        text
    }
}

#[cfg(not(windows))]
pub fn text() -> Option<String> {
    None
}
