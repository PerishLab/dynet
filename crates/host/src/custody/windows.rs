use super::{acl, descriptor::Descriptor, refused};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateDirectoryW, CreateFileW,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    GetFileInformationByHandle, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub(super) struct Place<'a>(pub &'a Path);

impl Place<'_> {
    pub(super) fn open(&self, directory: bool) -> io::Result<File> {
        let flags = FILE_FLAG_OPEN_REPARSE_POINT
            | if directory {
                FILE_FLAG_BACKUP_SEMANTICS
            } else {
                0
            };
        OpenOptions::new()
            .access_mode(0x80020000)
            .share_mode(7)
            .custom_flags(flags)
            .open(self.0)
    }

    pub(super) fn create(&self, machine: bool) -> io::Result<File> {
        let descriptor = Descriptor::new(machine)?;
        let attributes = descriptor.attributes();
        let handle = unsafe {
            CreateFileW(
                wide(self.0).as_ptr(),
                0xc0020000,
                7,
                &attributes,
                CREATE_NEW,
                FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    pub(super) fn folder(&self, machine: bool) -> io::Result<()> {
        let descriptor = Descriptor::new(machine)?;
        let attributes = descriptor.attributes();
        if unsafe { CreateDirectoryW(wide(self.0).as_ptr(), &attributes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn sync(&self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn check(file: &File, machine: bool, ancestor: bool) -> io::Result<()> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let meta = file.metadata()?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(refused());
    }
    match (meta.is_dir(), meta.is_file(), info.nNumberOfLinks) {
        (true, _, _) => (),
        (false, true, 1) if !ancestor => (),
        _ => return Err(refused()),
    }
    acl::check(file, machine, ancestor)
}

pub(super) fn replace(from: &Path, to: &Path) -> io::Result<()> {
    if unsafe {
        MoveFileExW(
            wide(from).as_ptr(),
            wide(to).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
