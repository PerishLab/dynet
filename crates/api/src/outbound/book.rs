use crate::resolver;
use dynet_core::Error;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct Book {
    known: Mutex<HashMap<String, String>>,
}

impl Book {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn find(&self, host: &str, upstream: &str) -> Result<String, Error> {
        if host.parse::<std::net::IpAddr>().is_ok() {
            return Ok(host.to_string());
        }
        if let Some(held) = self.recall(host) {
            return Ok(held);
        }
        let found = resolver::locate(host, upstream)?.to_string();
        if let Ok(mut known) = self.known.lock() {
            known.insert(host.to_string(), found.clone());
        }
        Ok(found)
    }

    fn recall(&self, host: &str) -> Option<String> {
        self.known.lock().ok()?.get(host).cloned()
    }
}
