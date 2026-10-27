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
    symlink(&home, root.join("redirect")).unwrap();
    assert!(private.read(&root.join("redirect/key"), 10).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(private.read(&path, 10).is_err());
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
