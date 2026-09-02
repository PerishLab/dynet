use smoltcp::iface::{Config, Interface};
use smoltcp::phy::TunTapInterface;
use smoltcp::time::Instant as Beat;
use smoltcp::wire::{HardwareAddress, IpCidr, Ipv4Address};
use std::time::{Duration, Instant};

pub(super) fn lasting(started: Instant, span: Duration) -> bool {
    span.is_zero() || started.elapsed() < span
}

pub(super) fn beat(started: Instant) -> Beat {
    Beat::from_micros(i64::try_from(started.elapsed().as_micros()).unwrap_or_default())
}

pub(super) fn raise(device: &mut TunTapInterface, started: Instant) -> Interface {
    let config = Config::new(HardwareAddress::Ip);
    let mut iface = Interface::new(config, device, beat(started));
    iface.set_any_ip(true);
    iface.update_ip_addrs(|addresses| {
        let own = IpCidr::new(Ipv4Address::new(198, 51, 100, 1).into(), 24);
        let _ = addresses.push(own);
    });
    let _ = iface
        .routes_mut()
        .add_default_ipv4_route(Ipv4Address::new(198, 51, 100, 2));
    iface
}
