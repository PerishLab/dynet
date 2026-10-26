use super::range::Range;
use std::net::{IpAddr, Ipv4Addr};

const RESERVED: [(Ipv4Addr, u8); 10] = [
    (Ipv4Addr::new(0, 0, 0, 0), 8),
    (Ipv4Addr::new(10, 0, 0, 0), 8),
    (Ipv4Addr::new(100, 64, 0, 0), 10),
    (Ipv4Addr::new(127, 0, 0, 0), 8),
    (Ipv4Addr::new(169, 254, 0, 0), 16),
    (Ipv4Addr::new(172, 16, 0, 0), 12),
    (Ipv4Addr::new(192, 168, 0, 0), 16),
    (Ipv4Addr::new(198, 18, 0, 0), 15),
    (Ipv4Addr::new(224, 0, 0, 0), 4),
    (Ipv4Addr::new(240, 0, 0, 0), 4),
];

pub fn reserved() -> Vec<Range> {
    RESERVED
        .iter()
        .filter_map(|(base, prefix)| Range::new(IpAddr::V4(*base), *prefix).ok())
        .collect()
}

pub fn spared(address: IpAddr) -> bool {
    reserved().iter().any(|range| range.holds(address))
}
