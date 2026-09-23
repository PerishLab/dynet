use dynet_api::host::{Shape, read};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

fn ground(name: &str) -> PathBuf {
    let seat = std::env::temp_dir().join(format!("dynet-shape-{name}"));
    let _ = fs::remove_dir_all(&seat);
    fs::create_dir_all(&seat).expect("seat");
    seat
}

fn wired(seat: &Path, uplink: &str) -> (PathBuf, PathBuf) {
    let stub = seat.join("stub-resolv.conf");
    fs::write(&stub, "nameserver 127.0.0.53\n").expect("stub");
    let told = seat.join("resolv.conf");
    fs::write(&told, uplink).expect("uplink");
    let resolv = seat.join("etc-resolv.conf");
    symlink(&stub, &resolv).expect("symlink");
    (resolv, told)
}

#[test]
fn carries() {
    let seat = ground("carries");
    let (resolv, uplink) = wired(&seat, "nameserver 223.5.5.5\nnameserver 1.1.1.1\n");
    let shape = read(&resolv, &uplink).expect("shape");
    assert_eq!(shape, Shape::Resolved { upstream: true });
}

#[test]
fn empty() {
    let seat = ground("empty");
    let (resolv, uplink) = wired(&seat, "search example.invalid\n");
    let shape = read(&resolv, &uplink).expect("shape");
    assert_eq!(
        shape,
        Shape::Resolved { upstream: false },
        "a stub that names only itself must not pass for an upstream"
    );
    assert!(
        !shape.sound(),
        "every lookup the host makes already fails, so dynet has nothing to stand on"
    );
}

#[test]
fn plain() {
    let seat = ground("plain");
    let resolv = seat.join("etc-resolv.conf");
    fs::write(&resolv, "nameserver 8.8.8.8\n").expect("plain");
    let shape = read(&resolv, &seat.join("resolv.conf")).expect("shape");
    assert_eq!(shape, Shape::Plain);
    assert!(
        shape.sound(),
        "a file another manager keeps is not dynet's to own, and dynet never writes it"
    );
}

#[test]
fn gone() {
    let seat = ground("gone");
    let shape = read(&seat.join("nothing"), &seat.join("resolv.conf")).expect("shape");
    assert_eq!(shape, Shape::Missing);
    assert!(
        !shape.sound(),
        "unreadable resolution is not the same as healthy"
    );
}

#[test]
fn absent() {
    let seat = ground("absent");
    let stub = seat.join("stub-resolv.conf");
    fs::write(&stub, "nameserver 127.0.0.53\n").expect("stub");
    let resolv = seat.join("etc-resolv.conf");
    symlink(&stub, &resolv).expect("symlink");
    let shape = read(&resolv, &seat.join("resolv.conf")).expect("shape");
    assert_eq!(
        shape,
        Shape::Plain,
        "resolved that is not running leaves no uplink to read"
    );
}
