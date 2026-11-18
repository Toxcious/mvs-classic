//! A plain HTTP(S) GET through WinHTTP: Windows' own TLS and the system's proxy settings, redirects followed (a
//! GitHub release's download answers with one). The body comes in chunks to a callback.

use std::ffi::c_void;

use windows::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_REFRESH, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
    WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR};

use super::UpdateError;

/// A URL's parts WinHTTP is given.
#[derive(Debug, PartialEq, Eq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    /// From its `/` on, the query included.
    pub path: String,
}

/// `https://host[:port]/path`, or `http://...` to this computer only (127.0.0.1, ::1, localhost: the release
/// check's own feed); an IPv6 host in brackets.
pub fn parse_url(url: &str) -> Result<Url, UpdateError> {
    let bad = || UpdateError::BadUrl(url.to_owned());
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(bad());
    };
    let (authority, path) = rest.find('/').map_or((rest, "/"), |k| (&rest[..k], &rest[k..]));
    if authority.is_empty() || authority.contains('@') {
        return Err(bad());
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let end = v6.find(']').ok_or_else(bad)?;
        (&v6[..end], v6[end + 1..].strip_prefix(':'))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    let port = match port {
        Some(p) => p.parse().ok().filter(|p| *p != 0).ok_or_else(bad)?,
        None if secure => 443,
        None => 80,
    };
    // (a download over plain http could be swapped on the way, its hash with it)
    let local = ["127.0.0.1", "::1"].contains(&host) || host.eq_ignore_ascii_case("localhost");
    if host.is_empty() || !(secure || local) {
        return Err(bad());
    }
    Ok(Url { secure, host: host.to_owned(), port, path: path.to_owned() })
}

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle WinHTTP returned and nothing else closes.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn handle(h: *mut c_void, what: &str) -> Result<Handle, UpdateError> {
    if h.is_null() {
        Err(UpdateError::Http(format!("{what}: {}", windows::core::Error::from_thread())))
    } else {
        Ok(Handle(h))
    }
}

fn os(what: &'static str) -> impl Fn(windows::core::Error) -> UpdateError {
    move |e| UpdateError::Http(format!("{what}: {}", e.message()))
}

/// GETs `url`: the status code once the headers are in (`on_status`: refused, the body is not read), then each chunk
/// of the body to `on_chunk`, which stops the read with an error of its own.
pub fn get(
    url: &str,
    mut on_status: impl FnMut(u32) -> Result<(), UpdateError>,
    mut on_chunk: impl FnMut(&[u8]) -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    let u = parse_url(url)?;
    let agent = HSTRING::from(format!("MVS-Classic/{}", mvs_release::VERSION));
    // SAFETY: WinHTTP calls on handles this function owns; the strings and the buffer outlive the calls that read
    // them.
    unsafe {
        let session = handle(
            WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
            "WinHttpOpen",
        )?;
        // Unverified: the timeouts are the port's own (Electron's net.fetch defaults were not read).
        // (a name, a connection and a request's send: 15 s each, and 30 s for each read, before it gives up)
        WinHttpSetTimeouts(session.0, 15_000, 15_000, 15_000, 30_000).map_err(os("WinHttpSetTimeouts"))?;
        let conn = handle(WinHttpConnect(session.0, &HSTRING::from(u.host.as_str()), u.port, 0), "WinHttpConnect")?;
        let flags =
            WINHTTP_OPEN_REQUEST_FLAGS(WINHTTP_FLAG_REFRESH.0 | if u.secure { WINHTTP_FLAG_SECURE.0 } else { 0 });
        let req = handle(
            WinHttpOpenRequest(
                conn.0,
                &HSTRING::from("GET"),
                &HSTRING::from(u.path.as_str()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                flags,
            ),
            "WinHttpOpenRequest",
        )?;
        // (no cache between the feed and this copy: the latest release changes in place)
        let headers: Vec<u16> = "Cache-Control: no-cache\r\nPragma: no-cache\r\n".encode_utf16().collect();
        WinHttpSendRequest(req.0, Some(&headers), None, 0, 0, 0).map_err(os("WinHttpSendRequest"))?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(os("WinHttpReceiveResponse"))?;
        let mut status = 0u32;
        let mut len = 4u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&raw mut status).cast()),
            &mut len,
            std::ptr::null_mut(),
        )
        .map_err(os("WinHttpQueryHeaders"))?;
        on_status(status)?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut got = 0u32;
            WinHttpReadData(req.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut got)
                .map_err(os("WinHttpReadData"))?;
            if got == 0 {
                return Ok(());
            }
            on_chunk(&buf[..got as usize])?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_split() {
        let u = parse_url("https://github.com/a/b/releases/latest/download/latest.json").unwrap();
        assert_eq!(
            u,
            Url {
                secure: true,
                host: "github.com".into(),
                port: 443,
                path: "/a/b/releases/latest/download/latest.json".into()
            }
        );
        let u = parse_url("http://127.0.0.1:8123/feed/").unwrap();
        assert_eq!((u.secure, u.host.as_str(), u.port, u.path.as_str()), (false, "127.0.0.1", 8123, "/feed/"));
        let u = parse_url("http://[::1]:9000").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.path.as_str()), ("::1", 9000, "/"));
        assert!(parse_url("ftp://x/").is_err());
        assert!(parse_url("http://:80/").is_err());
        assert!(parse_url("http://x:0/").is_err());
        assert!(parse_url("http://user@x/").is_err());
        assert!(parse_url("http://example.com/feed/").is_err());
        assert!(parse_url("http://LocalHost:8000/").is_ok());
    }
}
