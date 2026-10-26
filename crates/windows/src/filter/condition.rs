use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_BYTE_BLOB, FWP_BYTE_BLOB_TYPE, FWP_CONDITION_VALUE0, FWP_CONDITION_VALUE0_0,
    FWP_MATCH_EQUAL, FWP_UINT16, FWP_UINT64, FWP_V4_ADDR_AND_MASK, FWP_V4_ADDR_MASK,
    FWPM_CONDITION_ALE_APP_ID, FWPM_CONDITION_IP_LOCAL_INTERFACE, FWPM_CONDITION_IP_REMOTE_ADDRESS,
    FWPM_CONDITION_IP_REMOTE_PORT, FWPM_FILTER_CONDITION0,
};
use windows_sys::core::GUID;

pub(super) fn port(value: u16) -> FWPM_FILTER_CONDITION0 {
    condition(
        FWPM_CONDITION_IP_REMOTE_PORT,
        FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT16,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint16: value },
        },
    )
}

pub(super) fn remote(mask: &mut FWP_V4_ADDR_AND_MASK) -> FWPM_FILTER_CONDITION0 {
    condition(
        FWPM_CONDITION_IP_REMOTE_ADDRESS,
        FWP_CONDITION_VALUE0 {
            r#type: FWP_V4_ADDR_MASK,
            Anonymous: FWP_CONDITION_VALUE0_0 { v4AddrMask: mask },
        },
    )
}

pub(super) fn interface(luid: &mut u64) -> FWPM_FILTER_CONDITION0 {
    condition(
        FWPM_CONDITION_IP_LOCAL_INTERFACE,
        FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT64,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint64: luid },
        },
    )
}

pub(super) fn program(blob: *mut FWP_BYTE_BLOB) -> FWPM_FILTER_CONDITION0 {
    condition(
        FWPM_CONDITION_ALE_APP_ID,
        FWP_CONDITION_VALUE0 {
            r#type: FWP_BYTE_BLOB_TYPE,
            Anonymous: FWP_CONDITION_VALUE0_0 { byteBlob: blob },
        },
    )
}

fn condition(field: GUID, value: FWP_CONDITION_VALUE0) -> FWPM_FILTER_CONDITION0 {
    FWPM_FILTER_CONDITION0 {
        fieldKey: field,
        matchType: FWP_MATCH_EQUAL,
        conditionValue: value,
    }
}
