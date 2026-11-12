// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Small synchronous HTTPS client shared by background services.

use windows::{
    Win32::Networking::WinHttp::{
        INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
        WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    },
    core::{Error, HSTRING, PCWSTR, Result, w},
};

pub fn get(host: &str, path: &str, headers: &str, max_body: usize) -> Result<String> {
    // SAFETY: every handle is checked for null and owned by a `Handle` guard that closes it once;
    // string arguments outlive their calls, and each read writes at most `avail` bytes into the
    // space just reserved in `body`.
    unsafe {
        let session = WinHttpOpen(w!("Polynya"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
        if session.is_null() {
            return Err(Error::from_thread());
        }
        let _s = Handle(session);
        WinHttpSetTimeouts(session, 10_000, 10_000, 10_000, 15_000)?;
        let conn = WinHttpConnect(session, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0);
        if conn.is_null() {
            return Err(Error::from_thread());
        }
        let _c = Handle(conn);
        let req = WinHttpOpenRequest(conn, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE);
        if req.is_null() {
            return Err(Error::from_thread());
        }
        let _r = Handle(req);
        let has_headers = !headers.is_empty();
        let headers: Vec<u16> = headers.encode_utf16().collect();
        WinHttpSendRequest(req, has_headers.then_some(headers.as_slice()), None, 0, 0, 0)?;
        WinHttpReceiveResponse(req, std::ptr::null_mut())?;

        let mut status = 0u32;
        let mut status_len = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut _ as *mut core::ffi::c_void),
            &mut status_len,
            std::ptr::null_mut(),
        )?;
        if status != 200 {
            return Err(invalid(format!("HTTP {status}")));
        }

        let mut body = Vec::new();
        loop {
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(req, &mut avail)?;
            if avail == 0 {
                break;
            }
            if body.len().saturating_add(avail as usize) > max_body {
                return Err(invalid(format!("response exceeds {max_body} bytes")));
            }
            let start = body.len();
            body.resize(start + avail as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(req, body[start..].as_mut_ptr() as _, avail, &mut read)?;
            body.truncate(start + read as usize);
        }
        String::from_utf8(body).map_err(|e| invalid(e.to_string()))
    }
}

fn invalid(message: String) -> Error {
    Error::new(windows::core::HRESULT(-1), message)
}

struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a non-null WinHTTP handle owned by this guard and closed only here.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}
