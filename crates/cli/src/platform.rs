#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::Boundary;
#[cfg(target_os = "macos")]
pub(crate) use macos::{Boundary, Service, serve, unit};
#[cfg(windows)]
pub(crate) use windows::{Boundary, Service, serve};
