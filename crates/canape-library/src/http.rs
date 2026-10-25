//! HTTPS GET and POST with WinHTTP, which ships with Windows: no TLS stack to bundle.
//! Follows redirects and the system proxy settings.

use std::io;

use windows_sys::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts,
};

use crate::online::Http;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Closes a WinHTTP handle when dropped.
struct Handle(*mut core::ffi::c_void);

impl Handle {
    fn new(raw: *mut core::ffi::c_void) -> io::Result<Self> {
        if raw.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn check(ok: windows_sys::core::BOOL) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// One WinHTTP session for a batch of downloads.
pub struct WinHttp {
    session: Handle,
}

impl WinHttp {
    pub fn new() -> io::Result<Self> {
        let agent = wide(concat!("Canape/", env!("CARGO_PKG_VERSION")));
        let session = Handle::new(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        })?;
        // Resolve, connect, send, receive: a slow server is skipped.
        check(unsafe { WinHttpSetTimeouts(session.0, 5_000, 5_000, 10_000, 15_000) })?;
        Ok(Self { session })
    }
}

/// `https://host/path?query` -> (`host`, `/path?query`).
fn split(url: &str) -> io::Result<(&str, &str)> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "HTTPS only"))?;
    Ok(match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, "/"),
    })
}

/// The headers of a request, `Name: value` lines, each ending in CRLF.
fn header_lines(bearer: Option<&str>, headers: &[(&str, &str)]) -> String {
    let mut lines = String::new();
    if let Some(key) = bearer {
        lines.push_str(&format!("Authorization: Bearer {key}\r\n"));
    }
    for (name, value) in headers {
        lines.push_str(&format!("{name}: {value}\r\n"));
    }
    lines
}

/// The error for a status other than 200: `PermissionDenied` for 401, so
/// a caller can renew an expired token.
fn status_error(status: u32) -> io::Error {
    let kind = if status == 401 {
        io::ErrorKind::PermissionDenied
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(kind, format!("HTTP {status}"))
}

impl WinHttp {
    fn request(
        &self,
        verb: &str,
        url: &str,
        headers: &str,
        body: &[u8],
        max: usize,
    ) -> io::Result<Vec<u8>> {
        let (host, path) = split(url)?;
        let (host, path, verb) = (wide(host), wide(path), wide(verb));
        let connect = Handle::new(unsafe {
            WinHttpConnect(
                self.session.0,
                host.as_ptr(),
                INTERNET_DEFAULT_HTTPS_PORT,
                0,
            )
        })?;
        let request = Handle::new(unsafe {
            WinHttpOpenRequest(
                connect.0,
                verb.as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        let headers = (!headers.is_empty()).then(|| wide(headers));
        let (headers_ptr, headers_len) = match &headers {
            // The length excludes the terminating null.
            Some(h) => (h.as_ptr(), (h.len() - 1) as u32),
            None => (std::ptr::null(), 0),
        };
        let (body_ptr, body_len) = if body.is_empty() {
            (std::ptr::null(), 0)
        } else {
            (body.as_ptr().cast(), body.len() as u32)
        };
        check(unsafe {
            WinHttpSendRequest(
                request.0,
                headers_ptr,
                headers_len,
                body_ptr,
                body_len,
                body_len,
                0,
            )
        })?;
        check(unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) })?;

        let mut status = 0u32;
        let mut size = size_of::<u32>() as u32;
        check(unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                (&mut status as *mut u32).cast(),
                &mut size,
                std::ptr::null_mut(),
            )
        })?;
        if status != 200 {
            return Err(status_error(status));
        }

        let mut body = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            check(unsafe {
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut read,
                )
            })?;
            if read == 0 {
                return Ok(body);
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > max {
                return Err(io::Error::other("answer too large"));
            }
        }
    }
}

impl Http for WinHttp {
    fn get_limited(&self, url: &str, bearer: Option<&str>, max: usize) -> io::Result<Vec<u8>> {
        self.request("GET", url, &header_lines(bearer, &[]), &[], max)
    }

    fn post(&self, url: &str, headers: &[(&str, &str)], body: &[u8]) -> io::Result<Vec<u8>> {
        self.request(
            "POST",
            url,
            &header_lines(None, headers),
            body,
            crate::online::MAX_BYTES,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{header_lines, split, status_error};

    #[test]
    fn urls_split_into_host_and_path() {
        assert_eq!(
            split("https://www.steamgriddb.com/api/v2/grids/steam/400?a=b").unwrap(),
            ("www.steamgriddb.com", "/api/v2/grids/steam/400?a=b")
        );
        assert_eq!(split("https://example.com").unwrap(), ("example.com", "/"));
        assert!(split("http://example.com/").is_err());
    }

    #[test]
    fn headers_are_crlf_lines() {
        assert_eq!(header_lines(None, &[]), "");
        assert_eq!(
            header_lines(Some("k"), &[("Client-ID", "id")]),
            "Authorization: Bearer k\r\nClient-ID: id\r\n"
        );
        assert_eq!(
            status_error(401).kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(status_error(500).to_string(), "HTTP 500");
    }
}
