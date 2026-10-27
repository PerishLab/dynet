use super::refused;
use dynet_core::Error;
use secrets::{Network, Origin};
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

#[derive(Debug)]
pub(super) struct Physical {
    addresses: Vec<IpAddr>,
    mark: u32,
}

impl Physical {
    pub fn new(addresses: &[IpAddr], mark: u32) -> Result<Self, Error> {
        if addresses.is_empty() || addresses.len() > 8 {
            return Err(refused());
        }
        if addresses
            .iter()
            .any(|address| address.is_unspecified() || address.is_multicast())
        {
            return Err(refused());
        }
        Ok(Self {
            addresses: addresses.to_vec(),
            mark,
        })
    }
}

impl Network for Physical {
    fn resolve(&self, origin: &Origin, _: Duration) -> io::Result<Vec<SocketAddr>> {
        let host = origin.host.trim_matches(['[', ']']);
        let literal = host.parse::<IpAddr>().ok();
        if literal.is_some_and(|address| address.is_loopback()) {
            if self
                .addresses
                .iter()
                .any(|address| Some(*address) != literal)
            {
                return Err(io::Error::other("control origin refused"));
            }
        } else if self.addresses.iter().any(IpAddr::is_loopback) {
            return Err(io::Error::other("control origin refused"));
        }
        Ok(self
            .addresses
            .iter()
            .map(|address| SocketAddr::new(*address, origin.port))
            .collect())
    }

    fn connect(&self, address: SocketAddr, budget: Duration) -> io::Result<TcpStream> {
        if !self.addresses.contains(&address.ip()) {
            return Err(io::Error::other("control origin refused"));
        }
        let budget = budget.min(Duration::from_secs(5));
        if address.ip().is_loopback() {
            return TcpStream::connect_timeout(&address, budget);
        }
        crate::outbound::reach(address, self.mark, budget)
            .map_err(|_| io::Error::other("physical control connection failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::Physical;
    use secrets::{Network, Origin};
    use std::time::Duration;

    #[test]
    fn physical() {
        let network = Physical::new(&["192.0.2.8".parse().unwrap()], 0).unwrap();
        let origin = Origin {
            host: "nonexistent.invalid".to_owned(),
            port: 443,
        };
        let found = network.resolve(&origin, Duration::from_secs(1)).unwrap();
        assert_eq!(found, vec!["192.0.2.8:443".parse().unwrap()]);
        let local = Origin {
            host: "127.0.0.1".to_owned(),
            port: 8080,
        };
        assert!(network.resolve(&local, Duration::from_secs(1)).is_err());
        let network = Physical::new(&["127.0.0.1".parse().unwrap()], 0).unwrap();
        assert!(network.resolve(&origin, Duration::from_secs(1)).is_err());
        assert_eq!(
            network.resolve(&local, Duration::from_secs(1)).unwrap(),
            vec!["127.0.0.1:8080".parse().unwrap()]
        );
        assert!(
            network
                .connect("192.0.2.8:443".parse().unwrap(), Duration::from_secs(1))
                .is_err()
        );
        assert!(Physical::new(&[], 0).is_err());
        assert!(Physical::new(&["0.0.0.0".parse().unwrap()], 0).is_err());
    }
}
