use std::num::NonZeroU32;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static UPLINK: Mutex<Option<(Instant, u32)>> = Mutex::new(None);

pub fn uplink() -> std::io::Result<NonZeroU32> {
    let mut held = UPLINK
        .lock()
        .map_err(|_| std::io::Error::other("the uplink lock is poisoned"))?;
    if let Some((when, index)) = *held
        && when.elapsed() < Duration::from_secs(5)
    {
        return NonZeroU32::new(index).ok_or_else(|| std::io::Error::other("no physical uplink"));
    }
    let output = route()?;
    let name = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("interface: "))
        .ok_or_else(|| std::io::Error::other("the default route names no interface"))?;
    if name.starts_with("utun") || name == "lo0" {
        return Err(std::io::Error::other(
            "the default route is not a physical uplink",
        ));
    }
    let index = super::interface::index(name)?;
    *held = Some((Instant::now(), index));
    NonZeroU32::new(index).ok_or_else(|| std::io::Error::other("no physical uplink"))
}

fn route() -> std::io::Result<String> {
    let output = std::process::Command::new("/sbin/route")
        .args(["-n", "get", "default"])
        .output()
        .map_err(|error| std::io::Error::other(format!("cannot run /sbin/route: {error}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(format!(
            "/sbin/route -n get default refused: {}{}",
            stdout.trim(),
            stderr.trim()
        )));
    }
    Ok(stdout.into_owned())
}
