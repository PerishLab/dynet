use dynet_host::Private;

#[test]
fn durability() {
    let root = temporary();
    let root = root.path().canonicalize().unwrap();
    let private = Private::current();
    let home = root.join("private");
    private.folder(&home).unwrap();
    let path = home.join("key");
    private.create(&path, b"first").unwrap();
    assert!(private.create(&path, b"overwrite").is_err());
    assert_eq!(private.read(&path, 5).unwrap(), b"first");
    assert!(private.read(&path, 4).is_err());
    private.replace(&path, b"second").unwrap();
    assert_eq!(private.read(&path, 6).unwrap(), b"second");
    let locked = private.lock(&home.join("lock")).unwrap();
    assert!(private.lock(&home.join("lock")).is_err());
    drop(locked);
    private.lock(&home.join("lock")).unwrap();
    std::fs::hard_link(&path, home.join("alias")).unwrap();
    assert!(private.read(&path, 6).is_err());
}

#[cfg(unix)]
#[test]
fn ownership() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = temporary();
    let root = root.path().canonicalize().unwrap();
    let private = Private::current();
    let home = root.join("private");
    private.folder(&home).unwrap();
    let path = home.join("key");
    private.create(&path, b"key").unwrap();
    symlink(&path, home.join("alias")).unwrap();
    assert!(private.read(&home.join("alias"), 10).is_err());
    assert!(private.lock(&home.join("alias")).is_err());
    symlink(&home, root.join("redirect")).unwrap();
    assert!(private.read(&root.join("redirect/key"), 10).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(private.read(&path, 10).is_err());
    assert!(private.lock(&path).is_err());
}

#[cfg(unix)]
#[test]
fn ancestors() {
    use std::os::unix::fs::PermissionsExt;
    let root = temporary();
    let root = root.path().canonicalize().unwrap();
    let private = Private::current();
    let home = root.join("private");
    private.folder(&home).unwrap();
    let path = home.join("key");
    private.create(&path, b"key").unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(private.read(&path, 10).is_err());
    assert!(private.lock(&path).is_err());
}

fn temporary() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    builder.prefix("dynet-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(windows)]
    {
        builder
            .tempdir_in(dynet_host::windows::data().unwrap())
            .unwrap()
    }
    #[cfg(unix)]
    builder.tempdir().unwrap()
}

#[cfg(windows)]
#[test]
fn acl() {
    let root = temporary();
    let root = root.path().canonicalize().unwrap();
    let private = Private::current();
    let home = root.join("private");
    private.folder(&home).unwrap();
    let path = home.join("key");
    private.create(&path, b"fixture").unwrap();
    let output = std::process::Command::new("icacls.exe")
        .arg(&path)
        .args(["/grant", "*S-1-1-0:(R)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(private.read(&path, 16).is_err());
    let output = std::process::Command::new("icacls.exe")
        .arg(&home)
        .args(["/grant", "*S-1-1-0:(DC)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(private.create(&home.join("another"), b"fixture").is_err());
}

#[test]
#[ignore = "requires elevated machine custody; exercised on native validation hosts"]
fn machine() {
    let mut builder = tempfile::Builder::new();
    builder.prefix("dynet-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(windows)]
    let root = builder
        .tempdir_in(dynet_host::windows::data().unwrap())
        .unwrap();
    #[cfg(unix)]
    let root = builder.tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let private = Private::machine();
    let home = root.join("private");
    private.folder(&home).unwrap();
    let path = home.join("identity");
    private.create(&path, b"fixture").unwrap();
    assert_eq!(private.read(&path, 16).unwrap(), b"fixture");
    private.replace(&path, b"updated").unwrap();
    assert_eq!(private.read(&path, 16).unwrap(), b"updated");
    let locked = private.lock(&home.join("lock")).unwrap();
    assert!(private.lock(&home.join("lock")).is_err());
    drop(locked);
    private.lock(&home.join("lock")).unwrap();
}

#[test]
fn exclusive() {
    let root = temporary();
    let root = root.path().canonicalize().unwrap();
    let private = Private::current();
    let home = root.join("private");
    private.folder(&home).unwrap();
    let path = home.join("lock");
    private.create(&path, b"caller bytes").unwrap();
    let held = private.lock(&path).unwrap();
    assert_eq!(probe(&path), 75);
    drop(held);
    assert_eq!(probe(&path), 0);
    assert_eq!(private.read(&path, 32).unwrap(), b"caller bytes");
    let held = private.lock(&path).unwrap();
    assert_eq!(probe(&path), 75);
    drop(held);
    assert_eq!(probe(&path), 0);
    assert!(path.is_file());
    assert!(private.lock(&home.join("absent/lock")).is_err());
    assert!(private.lock(std::path::Path::new("relative")).is_err());
}

fn probe(path: &std::path::Path) -> i32 {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker"])
        .env("DYNET_LOCK_PROBE", path)
        .status()
        .unwrap()
        .code()
        .unwrap()
}

#[test]
fn worker() {
    let Some(path) = std::env::var_os("DYNET_LOCK_PROBE") else {
        return;
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    match file.try_lock() {
        Ok(()) => std::process::exit(0),
        Err(std::fs::TryLockError::WouldBlock) => std::process::exit(75),
        Err(error) => panic!("independent lock probe failed: {error}"),
    }
}
