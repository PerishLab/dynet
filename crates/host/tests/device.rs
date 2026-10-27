use dynet_host::Device;
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

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires root and native utun support"]
fn metadata() {
    let instance = format!("dt{}", std::process::id());
    let device = Device::open(&instance).expect("native device");
    assert!(device.index().expect("interface index") > 0);
    assert!(device.mtu().expect("initial MTU") > 0);
    let status = std::process::Command::new("/sbin/ifconfig")
        .args([device.name(), "mtu", "1300"])
        .status()
        .expect("set native MTU");
    assert!(status.success());
    assert_eq!(device.mtu().expect("changed MTU"), 1300);
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("1")
        .spawn()
        .expect("unrelated child");
    let name = device.name().to_string();
    drop(device);
    let absent = dynet_host::macos::index(&name).is_err();
    child.wait().expect("child exits");
    assert!(absent, "an unrelated child must not keep the device alive");
}
