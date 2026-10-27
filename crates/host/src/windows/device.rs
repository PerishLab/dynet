mod library;
mod resource;

use library::{Adapter, Library, Session, library};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::io::{Error, ErrorKind, Result};
use std::os::windows::ffi::OsStrExt;
use std::time::Duration;
use windows_sys::Win32::Foundation::{ERROR_NO_MORE_ITEMS, HANDLE, WAIT_FAILED};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    ConvertInterfaceLuidToIndex, GetIpInterfaceEntry, InitializeIpInterfaceEntry,
    MIB_IPINTERFACE_ROW,
};
use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows_sys::Win32::Networking::WinSock::AF_INET;
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use windows_sys::core::GUID;

const CAPACITY: u32 = 0x40_0000;

pub(crate) struct Device {
    library: &'static Library,
    adapter: Adapter,
    session: Session,
    event: HANDLE,
    name: String,
}

unsafe impl Send for Device {}
unsafe impl Sync for Device {}

impl Device {
    pub(crate) fn open(name: &str) -> Result<Self> {
        if name.is_empty() || name.len() >= 128 || name.contains('\0') {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("{name:?} cannot name a device"),
            ));
        }
        let library = library()?;
        let wide = widen(OsStr::new(name));
        let kind = widen(OsStr::new("Dynet"));
        let guid = identity(name);
        let adapter = unsafe { (library.create)(wide.as_ptr(), kind.as_ptr(), &guid) };
        if adapter.is_null() {
            return Err(Error::last_os_error());
        }
        let session = unsafe { (library.start)(adapter, CAPACITY) };
        if session.is_null() {
            let error = Error::last_os_error();
            unsafe { (library.close)(adapter) };
            return Err(error);
        }
        let event = unsafe { (library.event)(session) };
        Ok(Self {
            library,
            adapter,
            session,
            event,
            name: name.to_owned(),
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn luid(&self) -> u64 {
        let mut luid = NET_LUID_LH { Value: 0 };
        unsafe {
            (self.library.luid)(self.adapter, &mut luid);
            luid.Value
        }
    }

    pub(crate) fn index(&self) -> Result<u32> {
        let luid = NET_LUID_LH { Value: self.luid() };
        let mut index = 0;
        let code = unsafe { ConvertInterfaceLuidToIndex(&luid, &mut index) };
        settle(code).map(|()| index)
    }

    pub(crate) fn mtu(&self) -> Result<u32> {
        let mut row: MIB_IPINTERFACE_ROW = unsafe { std::mem::zeroed() };
        unsafe { InitializeIpInterfaceEntry(&mut row) };
        row.Family = AF_INET;
        row.InterfaceLuid = NET_LUID_LH { Value: self.luid() };
        let code = unsafe { GetIpInterfaceEntry(&mut row) };
        settle(code).map(|()| row.NlMtu)
    }

    pub(crate) fn receive(&self, room: &mut [u8]) -> Result<Option<usize>> {
        let mut size = 0;
        let packet = unsafe { (self.library.receive)(self.session, &mut size) };
        if packet.is_null() {
            let error = Error::last_os_error();
            return match error.raw_os_error() == i32::try_from(ERROR_NO_MORE_ITEMS).ok() {
                true => Ok(None),
                false => Err(error),
            };
        }
        let length = usize::try_from(size).unwrap_or(usize::MAX);
        let fits = length <= room.len();
        if fits {
            unsafe { std::ptr::copy_nonoverlapping(packet, room.as_mut_ptr(), length) };
        }
        unsafe { (self.library.release)(self.session, packet) };
        match fits {
            true => Ok(Some(length)),
            false => Err(Error::other("a packet outgrew the room it was read into")),
        }
    }

    pub(crate) fn send(&self, packet: &[u8]) -> Result<()> {
        let size = u32::try_from(packet.len())
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "a packet outgrew the device"))?;
        let slot = unsafe { (self.library.allocate)(self.session, size) };
        if slot.is_null() {
            return Err(Error::last_os_error());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(packet.as_ptr(), slot, packet.len());
            (self.library.send)(self.session, slot);
        }
        Ok(())
    }

    pub(crate) fn wait(&self, span: Duration) -> Result<()> {
        let millis = u32::try_from(span.as_millis()).unwrap_or(u32::MAX - 1);
        match unsafe { WaitForSingleObject(self.event, millis) } {
            WAIT_FAILED => Err(Error::last_os_error()),
            _ => Ok(()),
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            (self.library.end)(self.session);
            (self.library.close)(self.adapter);
        }
    }
}

pub(crate) fn widen(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

fn identity(name: &str) -> GUID {
    let digest = Sha256::digest(format!("dynet/{name}").as_bytes());
    let mut tail = [0; 8];
    tail.copy_from_slice(&digest[8..16]);
    GUID {
        data1: u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]),
        data2: u16::from_be_bytes([digest[4], digest[5]]),
        data3: u16::from_be_bytes([digest[6], digest[7]]),
        data4: tail,
    }
}

fn settle(code: u32) -> Result<()> {
    match code {
        0 => Ok(()),
        code => Err(Error::from_raw_os_error(
            i32::try_from(code).unwrap_or(i32::MAX),
        )),
    }
}
