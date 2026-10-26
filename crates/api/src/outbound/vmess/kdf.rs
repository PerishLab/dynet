use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

const LABEL: &[u8] = b"VMess AEAD KDF";
const SEED: u32 = 0x811c_9dc5;
const ODD: u32 = 0x0100_0193;

pub fn derive(key: &[u8], path: &[&[u8]]) -> [u8; 32] {
    let mut chain: Vec<&[u8]> = Vec::with_capacity(path.len() + 1);
    chain.push(LABEL);
    chain.extend_from_slice(path);
    nested(&chain, key)
}

pub fn shorten(key: &[u8], path: &[&[u8]]) -> [u8; 16] {
    let full = derive(key, path);
    let mut short = [0u8; 16];
    short.copy_from_slice(&full[..16]);
    short
}

pub fn digest(value: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(value);
    hasher.finalize().into()
}

pub fn fingerprint(value: &[u8]) -> [u8; 4] {
    let mut state = SEED;
    for byte in value {
        state ^= u32::from(*byte);
        state = state.wrapping_mul(ODD);
    }
    state.to_be_bytes()
}

fn nested(chain: &[&[u8]], message: &[u8]) -> [u8; 32] {
    let Some((outer, inner)) = chain.split_last() else {
        return digest(message);
    };
    sealed(outer, message, inner)
}

fn sealed(key: &[u8], message: &[u8], inner: &[&[u8]]) -> [u8; 32] {
    if inner.is_empty() {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key length");
        mac.update(message);
        return mac.finalize().into_bytes().into();
    }
    let mut block = [0u8; 64];
    let source = match key.len() > 64 {
        true => nested(inner, key).to_vec(),
        false => key.to_vec(),
    };
    block[..source.len()].copy_from_slice(&source);
    let mut first = Vec::with_capacity(64 + message.len());
    first.extend(block.iter().map(|byte| byte ^ 0x36));
    first.extend_from_slice(message);
    let middle = nested(inner, &first);
    let mut second = Vec::with_capacity(96);
    second.extend(block.iter().map(|byte| byte ^ 0x5c));
    second.extend_from_slice(&middle);
    nested(inner, &second)
}
