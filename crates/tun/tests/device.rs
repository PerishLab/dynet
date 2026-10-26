use dynet_tun::Device;
use std::io::ErrorKind;

#[test]
fn nameless() {
    for name in ["", "dy\0net"] {
        let refused = Device::open(name).err().map(|error| error.kind());
        assert_eq!(refused, Some(ErrorKind::InvalidInput));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn overlong() {
    let refused = Device::open("dynet-overlong-name")
        .err()
        .map(|error| error.kind());
    assert_eq!(refused, Some(ErrorKind::InvalidInput));
}
