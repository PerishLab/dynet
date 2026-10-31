use dynet_core::Error;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

pub struct Fence {
    circuits: Mutex<Option<Vec<Weak<Circuit>>>>,
    failed: AtomicBool,
}

pub struct Circuit {
    sockets: Mutex<Option<Vec<TcpStream>>>,
    failed: AtomicBool,
}

impl Default for Fence {
    fn default() -> Self {
        Self::new()
    }
}

impl Fence {
    pub fn new() -> Self {
        Self {
            circuits: Mutex::new(Some(Vec::new())),
            failed: AtomicBool::new(false),
        }
    }

    pub fn closed(&self) -> bool {
        self.circuits
            .lock()
            .map_or(true, |circuits| circuits.is_none())
    }

    pub fn ticket(&self) -> Result<Arc<Circuit>, Error> {
        let mut guard = self.circuits.lock().map_err(|_| refused())?;
        let circuits = guard.as_mut().ok_or_else(refused)?;
        circuits.retain(|circuit| circuit.strong_count() != 0);
        let circuit = Arc::new(Circuit {
            sockets: Mutex::new(Some(Vec::new())),
            failed: AtomicBool::new(false),
        });
        circuits.push(Arc::downgrade(&circuit));
        Ok(circuit)
    }

    pub fn close(&self) -> Result<(), Error> {
        let mut guard = self.circuits.lock().map_err(|_| refused())?;
        let mut failed = false;
        for circuit in guard
            .take()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|circuit| circuit.upgrade())
        {
            failed |= circuit.close().is_err();
        }
        if failed {
            self.failed.store(true, Ordering::Relaxed);
        }
        if self.failed.load(Ordering::Relaxed) {
            return Err(refused());
        }
        Ok(())
    }
}

impl Circuit {
    pub(crate) fn new() -> Self {
        Self {
            sockets: Mutex::new(Some(Vec::new())),
            failed: AtomicBool::new(false),
        }
    }

    pub fn attach(&self, stream: TcpStream) -> Result<TcpStream, Error> {
        let mut guard = self.sockets.lock().map_err(|_| refused())?;
        let Some(sockets) = guard.as_mut() else {
            let _ = stream.shutdown(Shutdown::Both);
            return Err(refused());
        };
        let cloned = stream.try_clone().map_err(|_| refused())?;
        sockets.push(cloned);
        Ok(stream)
    }

    pub fn allow<T>(&self, action: impl FnOnce() -> T) -> Result<T, Error> {
        let guard = self.sockets.lock().map_err(|_| refused())?;
        if guard.is_none() {
            return Err(refused());
        }
        Ok(action())
    }

    pub fn closed(&self) -> bool {
        self.sockets
            .lock()
            .map_or(true, |sockets| sockets.is_none())
    }

    pub(crate) fn close(&self) -> Result<(), Error> {
        let mut guard = self.sockets.lock().map_err(|_| refused())?;
        let mut failed = false;
        for socket in guard.take().unwrap_or_default() {
            if let Err(error) = socket.shutdown(Shutdown::Both) {
                failed |= error.kind() != std::io::ErrorKind::NotConnected;
            }
        }
        if failed {
            self.failed.store(true, Ordering::Relaxed);
        }
        if self.failed.load(Ordering::Relaxed) {
            return Err(refused());
        }
        Ok(())
    }
}

impl Drop for Circuit {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn refused() -> Error {
    Error::new("the configuration circuit is closed")
}

#[cfg(test)]
mod tests {
    use super::Fence;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    #[test]
    fn revocation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fence = Fence::new();
        let circuit = fence.ticket().unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut stream = circuit.attach(stream).unwrap();
        stream.write_all(b"before").unwrap();
        let mut room = [0u8; 6];
        peer.read_exact(&mut room).unwrap();
        assert_eq!(&room, b"before");
        fence.close().unwrap();
        assert!(circuit.closed());
        assert!(fence.ticket().is_err());
        assert!(stream.write_all(b"after").is_err());
        assert_eq!(peer.read(&mut room).unwrap(), 0);
        let late = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        assert!(circuit.attach(late).is_err());
        fence.close().unwrap();
    }
}
