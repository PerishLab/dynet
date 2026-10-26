use std::fs::{File, OpenOptions};
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;

pub(super) struct Device {
    file: File,
    name: String,
}

impl Device {
    pub(super) fn open(name: &str) -> Result<Self> {
        let bytes = name.as_bytes();
        if bytes.is_empty() || bytes.len() >= libc::IFNAMSIZ || bytes.contains(&0) {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("{name:?} cannot name a device"),
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open("/dev/net/tun")?;
        let mut request: libc::ifreq = unsafe { std::mem::zeroed() };
        for (slot, byte) in request.ifr_name.iter_mut().zip(bytes) {
            *slot = libc::c_char::from_ne_bytes([*byte]);
        }
        request.ifr_ifru.ifru_flags = libc::c_short::try_from(libc::IFF_TUN | libc::IFF_NO_PI)
            .map_err(|_| Error::other("the device flags overflow"))?;
        let code = unsafe { libc::ioctl(file.as_raw_fd(), libc::TUNSETIFF, &mut request) };
        if code < 0 {
            return Err(Error::last_os_error());
        }
        Ok(Self {
            file,
            name: name.to_owned(),
        })
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn index(&self) -> Result<u32> {
        self.read("ifindex")
    }

    pub(super) fn mtu(&self) -> Result<u32> {
        self.read("mtu")
    }

    pub(super) fn receive(&self, room: &mut [u8]) -> Result<Option<usize>> {
        match (&self.file).read(room) {
            Ok(size) => Ok(Some(size)),
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(super) fn send(&self, packet: &[u8]) -> Result<()> {
        let written = (&self.file).write(packet)?;
        match written == packet.len() {
            true => Ok(()),
            false => Err(Error::other("the device took part of a packet")),
        }
    }

    pub(super) fn wait(&self, span: Duration) -> Result<()> {
        let mut watch = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = libc::c_int::try_from(span.as_millis()).unwrap_or(libc::c_int::MAX);
        let code = unsafe { libc::poll(&mut watch, 1, millis) };
        let error = Error::last_os_error();
        match code < 0 && error.kind() != ErrorKind::Interrupted {
            true => Err(error),
            false => Ok(()),
        }
    }

    fn read(&self, key: &str) -> Result<u32> {
        let path = format!("/sys/class/net/{}/{key}", self.name);
        std::fs::read_to_string(path)?
            .trim()
            .parse()
            .map_err(|_| Error::other(format!("the device {key} is not a number")))
    }
}
