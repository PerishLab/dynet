use super::super::widen;
use std::io::{Error, Result};
use windows_sys::Win32::Foundation::{
    FWP_E_ALREADY_EXISTS, FWP_E_FILTER_NOT_FOUND, FWP_E_PROVIDER_NOT_FOUND,
    FWP_E_SUBLAYER_NOT_FOUND, HANDLE,
};
use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_BYTE_BLOB, FWPM_DISPLAY_DATA0, FWPM_FILTER_ENUM_TEMPLATE0, FWPM_FILTER0,
    FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6,
    FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4, FWPM_PROVIDER_FLAG_PERSISTENT, FWPM_PROVIDER0,
    FWPM_SESSION0, FWPM_SUBLAYER_FLAG_PERSISTENT, FWPM_SUBLAYER0, FwpmEngineClose0,
    FwpmEngineOpen0, FwpmFilterAdd0, FwpmFilterCreateEnumHandle0, FwpmFilterDeleteByKey0,
    FwpmFilterDestroyEnumHandle0, FwpmFilterEnum0, FwpmFreeMemory0, FwpmGetAppIdFromFileName0,
    FwpmProviderAdd0, FwpmProviderDeleteByKey0, FwpmSubLayerAdd0, FwpmSubLayerDeleteByKey0,
    FwpmSubLayerGetByKey0, FwpmTransactionAbort0, FwpmTransactionBegin0, FwpmTransactionCommit0,
};
use windows_sys::core::GUID;

const WINNT: u32 = 10;
const BATCH: u32 = 256;

pub(super) struct Engine {
    handle: HANDLE,
}

pub(super) struct Blob {
    held: *mut FWP_BYTE_BLOB,
}

impl Engine {
    pub(super) fn open() -> Result<Self> {
        let session = FWPM_SESSION0::default();
        let mut handle = std::ptr::null_mut();
        let code = unsafe {
            FwpmEngineOpen0(
                std::ptr::null(),
                WINNT,
                std::ptr::null(),
                &session,
                &mut handle,
            )
        };
        known(code)?;
        Ok(Self { handle })
    }

    pub(super) fn begin(&self) -> Result<()> {
        known(unsafe { FwpmTransactionBegin0(self.handle, 0) })
    }

    pub(super) fn commit(&self) -> Result<()> {
        known(unsafe { FwpmTransactionCommit0(self.handle) })
    }

    pub(super) fn abort(&self) {
        unsafe { FwpmTransactionAbort0(self.handle) };
    }

    pub(super) fn provider(&self, key: GUID, name: &str) -> Result<()> {
        let mut wide = widen(name);
        let provider = FWPM_PROVIDER0 {
            providerKey: key,
            displayData: display(&mut wide),
            flags: FWPM_PROVIDER_FLAG_PERSISTENT,
            ..Default::default()
        };
        let code = unsafe { FwpmProviderAdd0(self.handle, &provider, std::ptr::null_mut()) };
        tolerant(code, FWP_E_ALREADY_EXISTS)
    }

    pub(super) fn sublayer(&self, key: GUID, provider: GUID, name: &str) -> Result<()> {
        let mut wide = widen(name);
        let mut owner = provider;
        let sublayer = FWPM_SUBLAYER0 {
            subLayerKey: key,
            displayData: display(&mut wide),
            flags: FWPM_SUBLAYER_FLAG_PERSISTENT,
            providerKey: &mut owner,
            weight: u16::MAX,
            ..Default::default()
        };
        let code = unsafe { FwpmSubLayerAdd0(self.handle, &sublayer, std::ptr::null_mut()) };
        tolerant(code, FWP_E_ALREADY_EXISTS)
    }

    pub(super) fn holds(&self, key: GUID) -> bool {
        let mut found = std::ptr::null_mut();
        let code = unsafe { FwpmSubLayerGetByKey0(self.handle, &key, &mut found) };
        if code == 0 {
            unsafe { FwpmFreeMemory0((&mut found as *mut *mut FWPM_SUBLAYER0).cast()) };
        }
        code == 0
    }

    pub(super) fn add(&self, filter: &FWPM_FILTER0) -> Result<()> {
        let code = unsafe {
            FwpmFilterAdd0(
                self.handle,
                filter,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        known(code)
    }

    pub(super) fn delete(&self, key: GUID) -> Result<bool> {
        let code = unsafe { FwpmFilterDeleteByKey0(self.handle, &key) };
        match code == FWP_E_FILTER_NOT_FOUND as u32 {
            true => Ok(false),
            false => known(code).map(|()| true),
        }
    }

    pub(super) fn forget(&self, sublayer: GUID, provider: GUID) -> Result<()> {
        tolerant(
            unsafe { FwpmSubLayerDeleteByKey0(self.handle, &sublayer) },
            FWP_E_SUBLAYER_NOT_FOUND,
        )?;
        tolerant(
            unsafe { FwpmProviderDeleteByKey0(self.handle, &provider) },
            FWP_E_PROVIDER_NOT_FOUND,
        )
    }

    pub(super) fn listed(&self, provider: GUID) -> Result<Vec<(GUID, String)>> {
        let mut found = Vec::new();
        for layer in [
            FWPM_LAYER_ALE_AUTH_CONNECT_V4,
            FWPM_LAYER_ALE_AUTH_CONNECT_V6,
            FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
        ] {
            self.layer(provider, layer, &mut found)?;
        }
        Ok(found)
    }

    fn layer(&self, provider: GUID, layer: GUID, found: &mut Vec<(GUID, String)>) -> Result<()> {
        let mut owner = provider;
        let template = FWPM_FILTER_ENUM_TEMPLATE0 {
            providerKey: &mut owner,
            layerKey: layer,
            actionMask: u32::MAX,
            ..Default::default()
        };
        let mut handle = std::ptr::null_mut();
        known(unsafe { FwpmFilterCreateEnumHandle0(self.handle, &template, &mut handle) })?;
        let walked = self.walk(handle, found);
        unsafe { FwpmFilterDestroyEnumHandle0(self.handle, handle) };
        walked
    }

    fn walk(&self, handle: HANDLE, found: &mut Vec<(GUID, String)>) -> Result<()> {
        loop {
            let mut entries: *mut *mut FWPM_FILTER0 = std::ptr::null_mut();
            let mut count = 0;
            known(unsafe {
                FwpmFilterEnum0(self.handle, handle, BATCH, &mut entries, &mut count)
            })?;
            for index in 0..count as usize {
                let filter = unsafe { &**entries.add(index) };
                found.push((filter.filterKey, spoken(filter.displayData.name)));
            }
            if !entries.is_null() {
                unsafe { FwpmFreeMemory0((&mut entries as *mut *mut *mut FWPM_FILTER0).cast()) };
            }
            if count < BATCH {
                return Ok(());
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { FwpmEngineClose0(self.handle) };
    }
}

impl Blob {
    pub(super) fn program(path: &str) -> Result<Self> {
        let wide = widen(path);
        let mut held = std::ptr::null_mut();
        known(unsafe { FwpmGetAppIdFromFileName0(wide.as_ptr(), &mut held) })?;
        Ok(Self { held })
    }

    pub(super) fn get(&self) -> *mut FWP_BYTE_BLOB {
        self.held
    }
}

impl Drop for Blob {
    fn drop(&mut self) {
        unsafe { FwpmFreeMemory0((&mut self.held as *mut *mut FWP_BYTE_BLOB).cast()) };
    }
}

pub(super) fn display(wide: &mut [u16]) -> FWPM_DISPLAY_DATA0 {
    FWPM_DISPLAY_DATA0 {
        name: wide.as_mut_ptr(),
        description: std::ptr::null_mut(),
    }
}

fn spoken(wide: *const u16) -> String {
    if wide.is_null() {
        return String::new();
    }
    let mut length = 0;
    while unsafe { *wide.add(length) } != 0 {
        length += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(wide, length) })
}

fn tolerant(code: u32, spared: i32) -> Result<()> {
    match code == spared as u32 {
        true => Ok(()),
        false => known(code),
    }
}

fn known(code: u32) -> Result<()> {
    match code {
        0 => Ok(()),
        code => Err(Error::other(format!(
            "the filtering engine refused: 0x{code:08x}"
        ))),
    }
}
