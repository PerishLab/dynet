use super::filter::{DATA, Traffic, whole};
use crate::outbound::raw::random;
use dynet_core::Error;

pub const CONTINUE: u8 = 0;
pub const END: u8 = 1;
pub const DIRECT: u8 = 2;
const SIZE: usize = 8192;
const ROOM: usize = SIZE - 21;

pub struct Shaped {
    pub blocks: Vec<Vec<u8>>,
    pub switch: bool,
}

pub fn block(
    body: &[u8],
    command: u8,
    long: bool,
    once: &mut Option<[u8; 16]>,
) -> Result<Vec<u8>, Error> {
    let drawn = random(2)?;
    let roll = usize::from(u16::from_be_bytes([drawn[0], drawn[1]]));
    let size = body.len();
    let mut pad = match size < 900 && long {
        true => roll % 500 + 900 - size,
        false => roll % 256,
    };
    pad = pad.min(SIZE.saturating_sub(21 + size));
    let mut out = Vec::with_capacity(21 + size + pad);
    if let Some(identity) = once.take() {
        out.extend_from_slice(&identity);
    }
    out.push(command);
    out.extend_from_slice(&u16::try_from(size).unwrap_or_default().to_be_bytes());
    out.extend_from_slice(&u16::try_from(pad).unwrap_or_default().to_be_bytes());
    out.extend_from_slice(body);
    out.resize(out.len() + pad, 0);
    Ok(out)
}

pub fn reshape(body: &[u8]) -> Vec<&[u8]> {
    let mut pieces = Vec::new();
    for chunk in body.chunks(SIZE) {
        if chunk.len() < ROOM {
            pieces.push(chunk);
            continue;
        }
        let found = chunk.windows(DATA.len()).rposition(|held| held == DATA);
        let cut = match found {
            Some(place) if (21..=ROOM).contains(&place) => place,
            _ => SIZE / 2,
        };
        pieces.push(&chunk[..cut]);
        pieces.push(&chunk[cut..]);
    }
    pieces
}

pub fn shape(traffic: &mut Traffic, body: &[u8]) -> Result<Shaped, Error> {
    if traffic.count > 0 && !body.is_empty() {
        let chunks: Vec<&[u8]> = body.chunks(SIZE).collect();
        traffic.filter(&chunks);
    }
    let pieces = reshape(body);
    if !traffic.padding {
        return Ok(Shaped {
            blocks: pieces.iter().map(|piece| piece.to_vec()).collect(),
            switch: false,
        });
    }
    if body.is_empty() {
        let lone = block(&[], CONTINUE, true, &mut traffic.once)?;
        return Ok(Shaped {
            blocks: vec![lone],
            switch: false,
        });
    }
    pad(&pieces, whole(body), traffic)
}

fn pad(pieces: &[&[u8]], complete: bool, traffic: &mut Traffic) -> Result<Shaped, Error> {
    let mut shaped = Shaped {
        blocks: Vec::new(),
        switch: false,
    };
    let mut long = traffic.tls;
    let last = pieces.len() - 1;
    let closing = |traffic: &Traffic| match traffic.xtls {
        true => DIRECT,
        false => END,
    };
    for (place, piece) in pieces.iter().enumerate() {
        if traffic.tls && opens(piece) && complete {
            shaped.switch |= traffic.xtls;
            let command = match place == last {
                true => closing(traffic),
                false => CONTINUE,
            };
            shaped
                .blocks
                .push(block(piece, command, true, &mut traffic.once)?);
            traffic.padding = false;
            long = false;
            continue;
        }
        if !traffic.modern && traffic.count <= 1 {
            traffic.padding = false;
            shaped
                .blocks
                .push(block(piece, END, long, &mut traffic.once)?);
            shaped
                .blocks
                .extend(pieces[place + 1..].iter().map(|rest| rest.to_vec()));
            return Ok(shaped);
        }
        let command = match place == last && !traffic.padding {
            true => closing(traffic),
            false => CONTINUE,
        };
        shaped
            .blocks
            .push(block(piece, command, long, &mut traffic.once)?);
    }
    Ok(shaped)
}

pub fn unshape(traffic: &mut Traffic, body: &[u8]) -> Vec<u8> {
    let mut rest = body;
    if traffic.command == -1 && traffic.content == -1 && traffic.pad == -1 {
        if rest.len() < 21 || rest[..16] != traffic.identity {
            return rest.to_vec();
        }
        rest = &rest[16..];
        traffic.command = 5;
    }
    let mut out = Vec::new();
    while !rest.is_empty() {
        rest = step(rest, traffic, &mut out);
        if traffic.command > 0 || traffic.content > 0 || traffic.pad > 0 {
            continue;
        }
        if traffic.current == i32::from(CONTINUE) {
            traffic.command = 5;
            continue;
        }
        traffic.command = -1;
        traffic.content = -1;
        traffic.pad = -1;
        out.extend_from_slice(rest);
        break;
    }
    out
}

fn step<'a>(rest: &'a [u8], traffic: &mut Traffic, out: &mut Vec<u8>) -> &'a [u8] {
    if traffic.command > 0 {
        let byte = i32::from(rest[0]);
        match traffic.command {
            5 => traffic.current = byte,
            4 => traffic.content = byte << 8,
            3 => traffic.content |= byte,
            2 => traffic.pad = byte << 8,
            _ => traffic.pad |= byte,
        }
        traffic.command -= 1;
        return &rest[1..];
    }
    if traffic.content > 0 {
        let size = usize::try_from(traffic.content)
            .unwrap_or_default()
            .min(rest.len());
        out.extend_from_slice(&rest[..size]);
        traffic.content -= i32::try_from(size).unwrap_or_default();
        return &rest[size..];
    }
    let size = usize::try_from(traffic.pad)
        .unwrap_or_default()
        .min(rest.len());
    traffic.pad -= i32::try_from(size).unwrap_or_default();
    &rest[size..]
}

pub fn settle(traffic: &mut Traffic) {
    if traffic.content > 0 || traffic.pad > 0 {
        traffic.within = true;
        return;
    }
    match u8::try_from(traffic.current) {
        Ok(CONTINUE) => traffic.within = true,
        Ok(END) => traffic.within = false,
        Ok(DIRECT) => {
            traffic.within = false;
            traffic.direct = true;
        }
        _ => {}
    }
}

fn opens(piece: &[u8]) -> bool {
    piece.len() >= 6 && piece[..3] == DATA
}
