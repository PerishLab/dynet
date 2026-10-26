use std::ffi::{CStr, c_void};
use std::io::{Error, Result};
use std::path::Path;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{HANDLE, HMODULE};
use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::core::GUID;

pub(super) type Adapter = *mut c_void;
pub(super) type Session = *mut c_void;

pub(super) struct Library {
    pub(super) create: unsafe extern "system" fn(*const u16, *const u16, *const GUID) -> Adapter,
    pub(super) close: unsafe extern "system" fn(Adapter),
    pub(super) luid: unsafe extern "system" fn(Adapter, *mut NET_LUID_LH),
    pub(super) start: unsafe extern "system" fn(Adapter, u32) -> Session,
    pub(super) end: unsafe extern "system" fn(Session),
    pub(super) event: unsafe extern "system" fn(Session) -> HANDLE,
    pub(super) receive: unsafe extern "system" fn(Session, *mut u32) -> *mut u8,
    pub(super) release: unsafe extern "system" fn(Session, *const u8),
    pub(super) allocate: unsafe extern "system" fn(Session, u32) -> *mut u8,
    pub(super) send: unsafe extern "system" fn(Session, *const u8),
}

static LOADED: OnceLock<Library> = OnceLock::new();

pub(super) fn library() -> Result<&'static Library> {
    if let Some(library) = LOADED.get() {
        return Ok(library);
    }
    let library = Library::load(&super::resource::place()?)?;
    Ok(LOADED.get_or_init(|| library))
}

impl Library {
    fn load(path: &Path) -> Result<Self> {
        let wide = super::widen(path.as_os_str());
        let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
        let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), flags) };
        if module.is_null() {
            return Err(Error::last_os_error());
        }
        unsafe {
            Ok(Self {
                create: bind(module, c"WintunCreateAdapter")?,
                close: bind(module, c"WintunCloseAdapter")?,
                luid: bind(module, c"WintunGetAdapterLUID")?,
                start: bind(module, c"WintunStartSession")?,
                end: bind(module, c"WintunEndSession")?,
                event: bind(module, c"WintunGetReadWaitEvent")?,
                receive: bind(module, c"WintunReceivePacket")?,
                release: bind(module, c"WintunReleaseReceivePacket")?,
                allocate: bind(module, c"WintunAllocateSendPacket")?,
                send: bind(module, c"WintunSendPacket")?,
            })
        }
    }
}

unsafe fn bind<T: Copy>(module: HMODULE, name: &CStr) -> Result<T> {
    const { assert!(size_of::<T>() == size_of::<usize>()) };
    let found = unsafe { GetProcAddress(module, name.as_ptr().cast()) }
        .ok_or_else(|| Error::other(format!("wintun.dll exports no {}", name.to_string_lossy())))?;
    Ok(unsafe { std::mem::transmute_copy(&found) })
}
