use super::{descriptor, refused};
use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::ptr;
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, INHERIT_ONLY_ACE,
    OWNER_SECURITY_INFORMATION,
};

const INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
const REPLACE: u32 = 0x10000000 | 0x40000000 | 0x000d0040;

pub(super) fn check(file: &File, machine: bool, ancestor: bool) -> io::Result<()> {
    let mut owner = ptr::null_mut();
    let mut acl = ptr::null_mut();
    let mut raw = ptr::null_mut();
    let result = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut raw,
        )
    };
    let owned = descriptor::Descriptor(raw);
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let local = if machine {
        None
    } else {
        Some(descriptor::current()?)
    };
    let name = descriptor::sid(owner)?;
    if !(trusted(&name, local.as_deref()) || ancestor && name == INSTALLER) {
        return Err(refused());
    }
    examine(acl, local.as_deref(), ancestor)?;
    drop(owned);
    Ok(())
}

fn trusted(sid: &str, local: Option<&str>) -> bool {
    matches!(sid, "S-1-5-18" | "S-1-5-32-544") || local == Some(sid)
}

fn examine(acl: *mut ACL, local: Option<&str>, ancestor: bool) -> io::Result<()> {
    if acl.is_null() {
        return Err(refused());
    }
    let count = unsafe { (*acl).AceCount };
    for index in 0..count {
        let mut raw = ptr::null_mut();
        if unsafe { GetAce(acl, u32::from(index), &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        ace(raw, local, ancestor)?;
    }
    Ok(())
}

fn ace(raw: *mut core::ffi::c_void, local: Option<&str>, ancestor: bool) -> io::Result<()> {
    let header = unsafe { &*raw.cast::<ACE_HEADER>() };
    if header.AceFlags & INHERIT_ONLY_ACE as u8 != 0 || matches!(header.AceType, 1 | 6 | 10) {
        return Ok(());
    }
    if header.AceType != 0 || usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>() {
        return Err(refused());
    }
    let allowed = unsafe { &*raw.cast::<ACCESS_ALLOWED_ACE>() };
    let sid = descriptor::sid(ptr::addr_of!(allowed.SidStart).cast_mut().cast())?;
    if trusted(&sid, local) || (ancestor && sid == INSTALLER) {
        return Ok(());
    }
    let dangerous = if ancestor {
        allowed.Mask & REPLACE != 0
    } else {
        allowed.Mask != 0
    };
    if dangerous {
        return Err(refused());
    }
    Ok(())
}
