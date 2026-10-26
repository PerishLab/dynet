use super::{settle, widen};
use std::collections::BTreeSet;
use std::io::Result;
use std::net::Ipv4Addr;
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, ERROR_OBJECT_ALREADY_EXISTS};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    ConvertInterfaceAliasToLuid, ConvertInterfaceLuidToIndex, CreateIpForwardEntry2,
    CreateUnicastIpAddressEntry, DeleteIpForwardEntry2, FreeMibTable, GetIpForwardTable2,
    GetIpInterfaceEntry, InitializeIpForwardEntry, InitializeIpInterfaceEntry,
    InitializeUnicastIpAddressEntry, MIB_IPFORWARD_ROW2, MIB_IPFORWARD_TABLE2, MIB_IPINTERFACE_ROW,
    MIB_UNICASTIPADDRESS_ROW,
};
use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, IN_ADDR, IN_ADDR_0, MIB_IPPROTO_NETMGMT, SOCKADDR_IN, SOCKADDR_INET,
};

pub type Prefix = (Ipv4Addr, u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Interface {
    luid: u64,
}

impl Interface {
    pub fn new(luid: u64) -> Self {
        Self { luid }
    }

    pub fn named(alias: &str) -> Result<Self> {
        let wide = widen(alias);
        let mut luid = NET_LUID_LH { Value: 0 };
        settle(unsafe { ConvertInterfaceAliasToLuid(wide.as_ptr(), &mut luid) })?;
        Ok(Self::new(unsafe { luid.Value }))
    }

    pub fn luid(self) -> u64 {
        self.luid
    }

    pub fn index(self) -> Result<u32> {
        let luid = NET_LUID_LH { Value: self.luid };
        let mut index = 0;
        settle(unsafe { ConvertInterfaceLuidToIndex(&luid, &mut index) })?;
        Ok(index)
    }

    pub fn dress(self, address: Ipv4Addr, length: u8) -> Result<()> {
        let mut row = MIB_UNICASTIPADDRESS_ROW::default();
        unsafe { InitializeUnicastIpAddressEntry(&mut row) };
        row.Address = inet(address);
        row.InterfaceLuid = NET_LUID_LH { Value: self.luid };
        row.OnLinkPrefixLength = length;
        row.DadState = windows_sys::Win32::Networking::WinSock::IpDadStatePreferred;
        match unsafe { CreateUnicastIpAddressEntry(&row) } {
            ERROR_OBJECT_ALREADY_EXISTS => Ok(()),
            code => settle(code),
        }
    }

    pub fn routes(self) -> Result<BTreeSet<Prefix>> {
        Ok(table()?
            .into_iter()
            .filter(|row| unsafe { row.InterfaceLuid.Value } == self.luid)
            .filter(|row| row.Protocol == MIB_IPPROTO_NETMGMT)
            .map(|row| prefix(&row))
            .collect())
    }

    pub fn admit(self, prefix: Prefix) -> Result<()> {
        match unsafe { CreateIpForwardEntry2(&self.row(prefix)) } {
            ERROR_OBJECT_ALREADY_EXISTS => Ok(()),
            code => settle(code),
        }
    }

    pub fn evict(self, prefix: Prefix) -> Result<()> {
        match unsafe { DeleteIpForwardEntry2(&self.row(prefix)) } {
            ERROR_NOT_FOUND => Ok(()),
            code => settle(code),
        }
    }

    pub fn uplink(spared: u64) -> Result<Option<Self>> {
        let mut best: Option<(u32, u64)> = None;
        for row in table()? {
            let luid = unsafe { row.InterfaceLuid.Value };
            if row.DestinationPrefix.PrefixLength != 0 || luid == spared {
                continue;
            }
            let metric = row
                .Metric
                .saturating_add(Self::new(luid).metric().unwrap_or(u32::MAX));
            if best.is_none_or(|(least, _)| metric < least) {
                best = Some((metric, luid));
            }
        }
        Ok(best.map(|(_, luid)| Self::new(luid)))
    }

    fn metric(self) -> Result<u32> {
        let mut row = MIB_IPINTERFACE_ROW::default();
        unsafe { InitializeIpInterfaceEntry(&mut row) };
        row.Family = AF_INET;
        row.InterfaceLuid = NET_LUID_LH { Value: self.luid };
        settle(unsafe { GetIpInterfaceEntry(&mut row) })?;
        Ok(row.Metric)
    }

    fn row(self, prefix: Prefix) -> MIB_IPFORWARD_ROW2 {
        let mut row = MIB_IPFORWARD_ROW2::default();
        unsafe { InitializeIpForwardEntry(&mut row) };
        row.InterfaceLuid = NET_LUID_LH { Value: self.luid };
        row.DestinationPrefix.Prefix = inet(prefix.0);
        row.DestinationPrefix.PrefixLength = prefix.1;
        row.NextHop = inet(Ipv4Addr::UNSPECIFIED);
        row.Metric = 0;
        row.Protocol = MIB_IPPROTO_NETMGMT;
        row
    }
}

fn table() -> Result<Vec<MIB_IPFORWARD_ROW2>> {
    let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
    settle(unsafe { GetIpForwardTable2(AF_INET, &mut table) })?;
    let rows = unsafe {
        let count = (*table).NumEntries as usize;
        std::slice::from_raw_parts((*table).Table.as_ptr(), count).to_vec()
    };
    unsafe { FreeMibTable(table.cast()) };
    Ok(rows)
}

fn prefix(row: &MIB_IPFORWARD_ROW2) -> Prefix {
    let address = unsafe { row.DestinationPrefix.Prefix.Ipv4.sin_addr.S_un.S_addr };
    (
        Ipv4Addr::from(address.to_ne_bytes()),
        row.DestinationPrefix.PrefixLength,
    )
}

fn inet(address: Ipv4Addr) -> SOCKADDR_INET {
    SOCKADDR_INET {
        Ipv4: SOCKADDR_IN {
            sin_family: AF_INET,
            sin_port: 0,
            sin_addr: IN_ADDR {
                S_un: IN_ADDR_0 {
                    S_addr: u32::from_ne_bytes(address.octets()),
                },
            },
            sin_zero: [0; 8],
        },
    }
}
