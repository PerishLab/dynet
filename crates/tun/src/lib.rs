#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as native;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(target_os = "macos")]
pub use macos::{index, name};
#[cfg(windows)]
use windows as native;

use std::io::Result;
use std::time::Duration;

pub const ROOM: usize = 0xFFFF;

pub struct Device {
    inner: native::Device,
}

impl Device {
    pub fn open(name: &str) -> Result<Self> {
        native::Device::open(name).map(|inner| Self { inner })
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub fn index(&self) -> Result<u32> {
        self.inner.index()
    }

    pub fn mtu(&self) -> Result<u32> {
        self.inner.mtu()
    }

    pub fn receive(&self, room: &mut [u8]) -> Result<Option<usize>> {
        self.inner.receive(room)
    }

    pub fn send(&self, packet: &[u8]) -> Result<()> {
        self.inner.send(packet)
    }

    pub fn wait(&self, span: Duration) -> Result<()> {
        self.inner.wait(span)
    }

    #[cfg(windows)]
    pub fn luid(&self) -> u64 {
        self.inner.luid()
    }
}
