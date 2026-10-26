use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath};

struct Folder(*mut u16);

impl Drop for Folder {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(self.0.cast()) };
    }
}

pub fn data() -> io::Result<PathBuf> {
    let mut raw = std::ptr::null_mut();
    let code =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, std::ptr::null_mut(), &mut raw) };
    let folder = Folder(raw);
    if code < 0 || folder.0.is_null() {
        return Err(io::Error::other(format!(
            "Windows did not resolve ProgramData: HRESULT {code:#010x}"
        )));
    }
    let mut length = 0;
    while unsafe { *folder.0.add(length) } != 0 {
        length += 1;
    }
    let wide = unsafe { std::slice::from_raw_parts(folder.0, length) };
    let path = PathBuf::from(OsString::from_wide(wide));
    if !path.is_absolute() {
        return Err(io::Error::other(
            "Windows returned no absolute ProgramData folder",
        ));
    }
    Ok(path)
}
