use super::widen;
use std::ffi::c_void;
use std::io::{Error, Result};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use windows_sys::Win32::Foundation::NO_ERROR;
use windows_sys::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP,
    SERVICE_CONTROL_INTERROGATE, SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING,
    SERVICE_START_PENDING, SERVICE_STATUS, SERVICE_STATUS_CURRENT_STATE, SERVICE_STOP_PENDING,
    SERVICE_STOPPED, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS, SetServiceStatus,
    StartServiceCtrlDispatcherW,
};
use windows_sys::core::PWSTR;

type Work = Box<dyn Fn() -> u32 + Send + Sync>;

static HALTED: AtomicBool = AtomicBool::new(false);
static CLOSING: AtomicBool = AtomicBool::new(false);
static HANDLE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static NAME: OnceLock<Vec<u16>> = OnceLock::new();
static WORK: OnceLock<Work> = OnceLock::new();

pub fn halted() -> &'static AtomicBool {
    &HALTED
}

pub fn closing() -> bool {
    CLOSING.load(Ordering::SeqCst)
}

pub fn ready() {
    report(SERVICE_RUNNING, 0);
}

pub fn serve(name: &str, work: impl Fn() -> u32 + Send + Sync + 'static) -> Result<()> {
    let name = NAME.get_or_init(|| widen(name));
    if WORK.set(Box::new(work)).is_err() {
        return Err(Error::other("a service is already being served"));
    }
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_ptr().cast_mut(),
            lpServiceProc: Some(main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: std::ptr::null_mut(),
            lpServiceProc: None,
        },
    ];
    match unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } {
        0 => Err(Error::last_os_error()),
        _ => Ok(()),
    }
}

unsafe extern "system" fn main(_: u32, _: *mut PWSTR) {
    let Some(name) = NAME.get() else {
        return;
    };
    let handle =
        unsafe { RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control), std::ptr::null()) };
    if handle.is_null() {
        return;
    }
    HANDLE.store(handle, Ordering::SeqCst);
    report(SERVICE_START_PENDING, 0);
    let code = WORK.get().map_or(1, |work| work());
    report(SERVICE_STOPPED, code);
}

unsafe extern "system" fn control(code: u32, _: u32, _: *mut c_void, _: *mut c_void) -> u32 {
    match code {
        SERVICE_CONTROL_SHUTDOWN => {
            CLOSING.store(true, Ordering::SeqCst);
            HALTED.store(true, Ordering::SeqCst);
            report(SERVICE_STOP_PENDING, 0);
        }
        SERVICE_CONTROL_STOP => {
            HALTED.store(true, Ordering::SeqCst);
            report(SERVICE_STOP_PENDING, 0);
        }
        SERVICE_CONTROL_INTERROGATE => {}
        _ => {}
    }
    NO_ERROR
}

fn report(state: SERVICE_STATUS_CURRENT_STATE, code: u32) {
    let handle = HANDLE.load(Ordering::SeqCst);
    if handle.is_null() {
        return;
    }
    let accepted = match state {
        SERVICE_RUNNING => SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN,
        _ => 0,
    };
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepted,
        dwWin32ExitCode: code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: 30_000,
    };
    unsafe { SetServiceStatus(handle, &status) };
}
