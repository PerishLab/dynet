use std::ffi::CString;
use std::fs::File;
use std::io::{Error, ErrorKind, IoSlice, Read, Result, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::time::Duration;

const CONTROL: i32 = 2;

pub fn name(instance: &str) -> String {
    let hash = instance.bytes().fold(2166136261u32, |held, byte| {
        (held ^ u32::from(byte)).wrapping_mul(16777619)
    });
    format!("utun{}", 100 + hash % 30000)
}

pub fn index(name: &str) -> Result<u32> {
    let name = CString::new(name).map_err(|_| Error::other("an interface name contains zero"))?;
    match unsafe { libc::if_nametoindex(name.as_ptr()) } {
        0 => Err(Error::last_os_error()),
        index => Ok(index),
    }
}

pub(super) struct Device {
    file: File,
    name: String,
}

impl Device {
    pub(super) fn open(instance: &str) -> Result<Self> {
        let bytes = instance.as_bytes();
        if bytes.is_empty() || bytes.len() >= libc::IFNAMSIZ || bytes.contains(&0) {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "invalid device instance name",
            ));
        }
        let name = name(instance);
        let unit: u32 = name[4..]
            .parse()
            .map_err(|_| Error::other("invalid utun unit"))?;
        let fd = unsafe { libc::socket(libc::PF_SYSTEM, libc::SOCK_DGRAM, CONTROL) };
        if fd < 0 {
            return Err(Error::last_os_error());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let mut info: libc::ctl_info = unsafe { std::mem::zeroed() };
        for (slot, byte) in info.ctl_name.iter_mut().zip(b"com.apple.net.utun_control") {
            *slot = *byte as libc::c_char;
        }
        if unsafe { libc::ioctl(fd, libc::CTLIOCGINFO, &mut info) } < 0 {
            return Err(Error::last_os_error());
        }
        let address = libc::sockaddr_ctl {
            sc_len: std::mem::size_of::<libc::sockaddr_ctl>() as u8,
            sc_family: libc::AF_SYSTEM as u8,
            ss_sysaddr: 2,
            sc_id: info.ctl_id,
            sc_unit: unit + 1,
            sc_reserved: [0; 5],
        };
        let code = unsafe {
            libc::connect(
                fd,
                (&address as *const libc::sockaddr_ctl).cast(),
                u32::from(address.sc_len),
            )
        };
        if code < 0 {
            return Err(Error::last_os_error());
        }
        let mut returned = [0u8; libc::IFNAMSIZ];
        let mut size = returned.len() as libc::socklen_t;
        let code = unsafe {
            libc::getsockopt(
                fd,
                CONTROL,
                libc::UTUN_OPT_IFNAME,
                returned.as_mut_ptr().cast(),
                &mut size,
            )
        };
        let end = returned
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(returned.len());
        if code < 0 || returned[..end] != *name.as_bytes() {
            return Err(Error::other("utun did not return the requested interface"));
        }
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(Error::last_os_error());
        }
        Ok(Self { file, name })
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn index(&self) -> Result<u32> {
        index(&self.name)
    }

    pub(super) fn mtu(&self) -> Result<u32> {
        Ok(1400)
    }

    pub(super) fn receive(&self, room: &mut [u8]) -> Result<Option<usize>> {
        let mut framed = [0u8; super::ROOM + 4];
        let size = match (&self.file).read(&mut framed) {
            Ok(size) => size,
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(error),
        };
        if size < 4 || size - 4 > room.len() {
            return Err(Error::other("utun returned a truncated frame"));
        }
        room[..size - 4].copy_from_slice(&framed[4..size]);
        Ok(Some(size - 4))
    }

    pub(super) fn send(&self, packet: &[u8]) -> Result<()> {
        let family = match packet.first().map(|byte| byte >> 4) {
            Some(4) => libc::AF_INET,
            Some(6) => libc::AF_INET6,
            _ => return Err(Error::other("utun requires an IP packet")),
        };
        let header = (family as u32).to_be_bytes();
        let size = (&self.file).write_vectored(&[IoSlice::new(&header), IoSlice::new(packet)])?;
        match size == packet.len() + header.len() {
            true => Ok(()),
            false => Err(Error::other("utun took part of a packet")),
        }
    }

    pub(super) fn wait(&self, span: Duration) -> Result<()> {
        let mut watch = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = i32::try_from(span.as_millis()).unwrap_or(i32::MAX);
        let result = unsafe { libc::poll(&mut watch, 1, millis) };
        let error = Error::last_os_error();
        match result < 0 && error.kind() != ErrorKind::Interrupted {
            true => Err(error),
            false => Ok(()),
        }
    }
}
