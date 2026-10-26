use crate::outbound::raw::random;
use dynet_core::Error;

pub const HYBRID: u16 = 0x11ec;
pub const CURVE: u16 = 0x001d;
const SUITES: [u16; 15] = [
    0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014, 0x009c,
    0x009d, 0x002f, 0x0035,
];
const SIGNATURES: [u16; 8] = [
    0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601,
];
const LENGTHS: [usize; 4] = [144, 176, 208, 240];

pub struct Shares {
    pub curve: [u8; 32],
    pub lattice: Vec<u8>,
}

struct Grease {
    values: Vec<u16>,
}

impl Grease {
    fn draw() -> Result<Self, Error> {
        let drawn = random(8)?;
        let mut values: Vec<u16> = drawn
            .iter()
            .map(|byte| u16::from(byte & 0xf0 | 0x0a) * 0x0101)
            .collect();
        for (left, right) in [(2, 3), (1, 4)] {
            if values[left] == values[right] {
                values[right] ^= 0x1010;
            }
        }
        Ok(Self { values })
    }

    fn get(&self, place: usize) -> u16 {
        self.values[place]
    }
}

pub fn build(seed: &[u8; 32], name: &str, shares: &Shares) -> Result<Vec<u8>, Error> {
    let grease = Grease::draw()?;
    let mut body = vec![3, 3];
    body.extend_from_slice(seed);
    body.push(32);
    body.extend_from_slice(&[0u8; 32]);
    let mut suites = vec![grease.get(0)];
    suites.extend_from_slice(&SUITES);
    extend(&mut body, &listed(&suites));
    body.extend_from_slice(&[1, 0]);
    extend(&mut body, &extensions(&grease, name, shares)?);
    let size = u32::try_from(body.len()).map_err(|_| Error::new("a hello is too long"))?;
    let mut message = vec![1];
    message.extend_from_slice(&size.to_be_bytes()[1..]);
    message.extend_from_slice(&body);
    Ok(message)
}

fn extensions(grease: &Grease, name: &str, shares: &Shares) -> Result<Vec<u8>, Error> {
    let mut middle = vec![
        (0x0000, named(name)),
        (0x0017, Vec::new()),
        (0xff01, vec![0]),
        (
            0x000a,
            framed(&listed(&[grease.get(2), HYBRID, CURVE, 0x0017, 0x0018])),
        ),
        (0x000b, vec![1, 0]),
        (0x0023, Vec::new()),
        (0x0010, framed(&spoken(&["h2", "http/1.1"]))),
        (0x0005, vec![1, 0, 0, 0, 0]),
        (0x000d, framed(&listed(&SIGNATURES))),
        (0x0012, Vec::new()),
        (0x0033, framed(&keyed(grease.get(2), shares))),
        (0x002d, vec![1, 1]),
        (0x002b, versions(grease.get(3))),
        (0x001b, vec![2, 0, 2]),
        (0x44cd, framed(&spoken(&["h2"]))),
        (0xfe0d, encrypted()?),
    ];
    shuffle(&mut middle)?;
    let mut out = Vec::new();
    push(&mut out, grease.get(1), &[]);
    for (kind, body) in &middle {
        push(&mut out, *kind, body);
    }
    push(&mut out, grease.get(4), &[0]);
    Ok(out)
}

fn shuffle(items: &mut [(u16, Vec<u8>)]) -> Result<(), Error> {
    let drawn = random(items.len())?;
    for place in (1..items.len()).rev() {
        let other = usize::from(drawn[place]) % (place + 1);
        items.swap(place, other);
    }
    Ok(())
}

fn named(name: &str) -> Vec<u8> {
    let mut entry = vec![0];
    extend(&mut entry, name.as_bytes());
    framed(&entry)
}

fn keyed(grease: u16, shares: &Shares) -> Vec<u8> {
    let mut hybrid = shares.lattice.clone();
    hybrid.extend_from_slice(&shares.curve);
    let mut out = Vec::new();
    for (group, data) in [
        (grease, vec![0u8]),
        (HYBRID, hybrid),
        (CURVE, shares.curve.to_vec()),
    ] {
        out.extend_from_slice(&group.to_be_bytes());
        extend(&mut out, &data);
    }
    out
}

fn versions(grease: u16) -> Vec<u8> {
    let mut out = vec![6];
    for version in [grease, 0x0304, 0x0303] {
        out.extend_from_slice(&version.to_be_bytes());
    }
    out
}

fn encrypted() -> Result<Vec<u8>, Error> {
    let drawn = random(34)?;
    let length = LENGTHS[usize::from(drawn[0]) % LENGTHS.len()];
    let mut out = vec![0, 0, 1, 0, 1, drawn[1]];
    extend(&mut out, &drawn[2..34]);
    extend(&mut out, &random(length)?);
    Ok(out)
}

fn listed(values: &[u16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

fn spoken(names: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for name in names {
        out.push(u8::try_from(name.len()).unwrap_or_default());
        out.extend_from_slice(name.as_bytes());
    }
    out
}

fn framed(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    extend(&mut out, body);
    out
}

pub fn extend(out: &mut Vec<u8>, body: &[u8]) {
    out.extend_from_slice(&u16::try_from(body.len()).unwrap_or_default().to_be_bytes());
    out.extend_from_slice(body);
}

fn push(out: &mut Vec<u8>, kind: u16, body: &[u8]) {
    out.extend_from_slice(&kind.to_be_bytes());
    extend(out, body);
}
