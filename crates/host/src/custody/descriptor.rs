use super::refused;
use std::io;
use std::ptr;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub(super) struct Descriptor(pub *mut core::ffi::c_void);

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

impl Descriptor {
    pub fn new(machine: bool) -> io::Result<Self> {
        let owner = if machine { "BA".to_owned() } else { current()? };
        let sddl = format!("O:{owner}D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{owner})");
        let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut raw = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                1,
                &mut raw,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(raw))
    }

    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}

pub(super) fn current() -> io::Result<String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut length = 0;
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length) };
    let mut room = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            room.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    };
    unsafe { CloseHandle(token) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*room.as_ptr().cast::<TOKEN_USER>() };
    sid(user.User.Sid)
}

pub(super) fn sid(raw: *mut core::ffi::c_void) -> io::Result<String> {
    if raw.is_null() {
        return Err(refused());
    }
    let mut wide = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(raw, &mut wide) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let owned = Descriptor(wide.cast());
    let mut length = 0;
    while unsafe { *wide.add(length) } != 0 {
        length += 1;
    }
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(wide, length) })
        .map_err(|_| refused());
    drop(owned);
    text
}
