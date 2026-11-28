use super::Spec;
use dynet_core::Error;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type Signer = Hmac<Sha256>;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn derive(seed: &[u8], slot: u64) -> String {
    let mut mac = Signer::new_from_slice(seed).expect("HMAC accepts keys of any length");
    mac.update(format!("dynet.project.v1:{slot}").as_bytes());
    let tag = mac.finalize().into_bytes();
    encode(&tag)
}

pub fn active(spec: &Spec, now: u64) -> Vec<(String, String)> {
    let current = now / spec.epoch();
    let mut tokens = Vec::new();
    for recipient in spec.recipients() {
        for offset in 0..=spec.grace() {
            if let Some(slot) = current.checked_sub(offset) {
                tokens.push((recipient.name().to_owned(), derive(recipient.seed(), slot)));
            }
        }
    }
    tokens
}

pub fn same(candidate: &str, expected: &str) -> bool {
    let left = candidate.as_bytes();
    let right = expected.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&a, &b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub(super) fn decode(raw: &str) -> Result<Vec<u8>, Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::new("projection seed must not be empty"));
    }
    if let Some(hex) = trimmed.strip_prefix("hex:") {
        return unhex(hex);
    }
    unbase(trimmed)
}

fn unhex(text: &str) -> Result<Vec<u8>, Error> {
    if !text.len().is_multiple_of(2) {
        return Err(Error::new("hex projection seed must have even length"));
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = nibble(pair[0])?;
        let lo = nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn nibble(byte: u8) -> Result<u8, Error> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(Error::new("hex projection seed contains non-hex byte")),
    }
}

pub(super) fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() * 4).div_ceil(3));
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 0x3f) as usize] as char);
        }
    }
    out
}

fn unbase(text: &str) -> Result<Vec<u8>, Error> {
    let stripped = text.trim_end_matches('=');
    let mut values = Vec::with_capacity(stripped.len());
    for byte in stripped.bytes() {
        let val = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return Err(Error::new("projection seed is not valid base64url")),
        };
        values.push(val);
    }
    if values.len() % 4 == 1 {
        return Err(Error::new("projection seed has invalid base64url length"));
    }
    let mut out = Vec::with_capacity((values.len() * 3) / 4);
    for chunk in values.chunks(4) {
        let v0 = chunk[0] as u32;
        let v1 = chunk[1] as u32;
        out.push(((v0 << 2) | (v1 >> 4)) as u8);
        if chunk.len() > 2 {
            let v2 = chunk[2] as u32;
            out.push((((v1 & 0x0f) << 4) | (v2 >> 2)) as u8);
            if chunk.len() > 3 {
                let v3 = chunk[3] as u32;
                out.push((((v2 & 0x03) << 6) | v3) as u8);
            }
        }
    }
    Ok(out)
}
