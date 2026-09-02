use dynet_core::Error;
use std::fs::File;
use std::io::Read;

pub fn checksum(value: &[u8]) -> [u8; 4] {
    let mut state = 0xffff_ffffu32;
    for byte in value {
        state ^= u32::from(*byte);
        for _ in 0..8 {
            let carry = state & 1;
            state >>= 1;
            if carry != 0 {
                state ^= 0xedb8_8320;
            }
        }
    }
    (!state).to_be_bytes()
}

pub(super) fn random(size: usize) -> Result<Vec<u8>, Error> {
    let mut buffer = vec![0u8; size];
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut buffer))
        .map_err(|error| Error::new(format!("cannot draw randomness: {error}")))?;
    Ok(buffer)
}

pub fn parse(uuid: &str) -> Result<[u8; 16], Error> {
    let digits: String = uuid.chars().filter(|item| *item != '-').collect();
    if digits.len() != 32 {
        return Err(Error::new("a node identity must carry sixteen bytes"));
    }
    let mut identity = [0u8; 16];
    for (slot, pair) in identity.iter_mut().zip(digits.as_bytes().chunks(2)) {
        let text =
            std::str::from_utf8(pair).map_err(|_| Error::new("a node identity is not text"))?;
        *slot = u8::from_str_radix(text, 16)
            .map_err(|_| Error::new("a node identity is not hexadecimal"))?;
    }
    Ok(identity)
}
