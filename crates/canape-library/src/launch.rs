//! Starting a game. `ShellExecuteW` opens a URI with the program registered
//! for it (Steam, the Epic launcher, Galaxy, the Xbox app through
//! `shell:AppsFolder`) or runs an executable, without a console window. It
//! runs as the caller: from the shell, the user at medium integrity.

use std::io;

use crate::Launch;

#[cfg(windows)]
pub fn launch(launch: &Launch) -> io::Result<()> {
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    let (file, args, cwd) = match launch {
        Launch::Install { .. } => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not installed: see canape_library::install",
            ));
        }
        Launch::Uri { uri } => (wide(uri), None, None),
        Launch::Exe { path, args, cwd } => (
            wide(&path.to_string_lossy()),
            (!args.is_empty()).then(|| wide(args)),
            (!cwd.as_os_str().is_empty()).then(|| wide(&cwd.to_string_lossy())),
        ),
    };
    let ptr = |v: &Option<Vec<u16>>| v.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
    // The documentation asks for COM on the calling thread: some handlers
    // are shell extensions.
    let com = unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    let open = wide("open");
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            open.as_ptr(),
            file.as_ptr(),
            ptr(&args),
            ptr(&cwd),
            SW_SHOWNORMAL,
        )
    } as isize;
    if com >= 0 {
        unsafe { CoUninitialize() };
    }
    // Above 32 means success; below, an SE_ERR_* / ERROR_* code.
    if result > 32 {
        Ok(())
    } else {
        Err(io::Error::other(format!("ShellExecuteW returned {result}")))
    }
}

#[cfg(not(windows))]
pub fn launch(launch: &Launch) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("launching is only implemented on Windows: {launch:?}"),
    ))
}
