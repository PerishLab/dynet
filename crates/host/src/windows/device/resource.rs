use sha2::{Digest, Sha256};
use std::io::{Error, Result};
use std::path::PathBuf;

const DIGEST: [u8; 32] = [
    0xe5, 0xda, 0x84, 0x47, 0xdc, 0x2c, 0x32, 0x0e, 0xdc, 0x0f, 0xc5, 0x2f, 0xa0, 0x18, 0x85, 0xc1,
    0x03, 0xde, 0x8c, 0x11, 0x84, 0x81, 0xf6, 0x83, 0x64, 0x3c, 0xac, 0xc3, 0x22, 0x0d, 0xaf, 0xce,
];

const BINARY: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/wintun.dll"));

pub(super) fn place() -> Result<PathBuf> {
    if Sha256::digest(BINARY).as_slice() != DIGEST {
        return Err(Error::other(
            "the carried wintun.dll does not match its pinned digest; refusing it",
        ));
    }
    let path = std::env::current_exe()?.with_file_name("wintun.dll");
    if std::fs::read(&path).is_ok_and(|found| found == BINARY) {
        return Ok(path);
    }
    let part = path.with_extension("part");
    std::fs::write(&part, BINARY)?;
    std::fs::rename(&part, &path)?;
    Ok(path)
}
