use super::{custody, network::Physical, refused};
use dynet_core::Error;
use dynet_host::Private;
use secrets::{Client, Identity, Publisher, Reader, Target, Token};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Bootstrap {
    version: u8,
    pub target: Target,
    #[serde(rename = "identityRef")]
    identity: PathBuf,
    publisher: String,
    minimum: u64,
    pub source: Source,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Source {
    File {
        path: PathBuf,
    },
    Url {
        #[serde(rename = "baseUrl")]
        base: String,
        bootstrap: Vec<IpAddr>,
        #[serde(rename = "tokenRef")]
        token: PathBuf,
    },
}

impl Bootstrap {
    pub fn read(path: &Path, private: Private) -> Result<Self, Error> {
        let bytes = private.read(path, 16384).map_err(custody)?;
        let bootstrap: Self = serde_json::from_slice(&bytes).map_err(|_| refused())?;
        bootstrap.reader()?;
        Ok(bootstrap)
    }

    pub fn inspect(&self, private: Private, mark: u32) -> Result<(), Error> {
        let identity = self.identity(private)?;
        let reader = self.reader()?;
        self.client(private, mark)?;
        if let Source::File { path } = &self.source {
            let bytes = private.read(path, secrets::LIMIT).map_err(custody)?;
            let verified = reader.verify(&bytes, None).map_err(|_| refused())?;
            verified.open(&identity).map_err(|_| refused())?;
        }
        Ok(())
    }

    pub fn reader(&self) -> Result<Reader, Error> {
        if self.version != 1 {
            return Err(refused());
        }
        let publisher = Publisher::read(&self.publisher).map_err(|_| refused())?;
        Reader::new(self.target.clone(), publisher, self.minimum).map_err(|_| refused())
    }

    pub fn identity(&self, private: Private) -> Result<Identity, Error> {
        let bytes = Zeroizing::new(private.read(&self.identity, 1024).map_err(custody)?);
        let text = std::str::from_utf8(&bytes).map_err(|_| refused())?;
        Identity::read(text).map_err(|_| refused())
    }

    pub fn trust(&self, identity: &Identity) -> Result<String, Error> {
        let target = serde_json::to_vec(&self.target).map_err(|_| refused())?;
        let mut digest = Sha256::new();
        for part in [
            target,
            self.publisher.as_bytes().to_vec(),
            self.minimum.to_be_bytes().to_vec(),
            identity.recipient().to_string().into_bytes(),
        ] {
            digest.update((part.len() as u64).to_be_bytes());
            digest.update(part);
        }
        Ok(format!("{:x}", digest.finalize()))
    }

    pub fn client(&self, private: Private, mark: u32) -> Result<Option<Client>, Error> {
        let Source::Url {
            base,
            bootstrap,
            token,
        } = &self.source
        else {
            return Ok(None);
        };
        let bytes = Zeroizing::new(private.read(token, 512).map_err(custody)?);
        let text = std::str::from_utf8(&bytes).map_err(|_| refused())?;
        let token = Token::read(text.trim()).map_err(|_| refused())?;
        let network = Arc::new(Physical::new(bootstrap, mark)?);
        Client::network(base, token, Some(network))
            .map(Some)
            .map_err(|_| refused())
    }
}
