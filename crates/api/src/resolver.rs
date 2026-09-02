use dynet_core::Error;
use std::net::Ipv4Addr;

const ADDRESS: u16 = 1;

pub fn ask(name: &str, mark: u16) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    body.extend_from_slice(&mark.to_be_bytes());
    body.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        let size = u8::try_from(label.len()).map_err(|_| Error::new("a label is too long"))?;
        body.push(size);
        body.extend_from_slice(label.as_bytes());
    }
    body.push(0);
    body.extend_from_slice(&ADDRESS.to_be_bytes());
    body.extend_from_slice(&[0, 1]);
    let length = u16::try_from(body.len()).map_err(|_| Error::new("the query is too long"))?;
    let mut framed = length.to_be_bytes().to_vec();
    framed.extend_from_slice(&body);
    Ok(framed)
}

pub fn read(answer: &[u8]) -> Result<Vec<Ipv4Addr>, Error> {
    if answer.len() < 14 {
        return Err(Error::new("the answer is too short to carry a header"));
    }
    let body = &answer[2..];
    let count = usize::from(u16::from_be_bytes([body[6], body[7]]));
    let mut cursor = 12;
    cursor = skip(body, cursor)? + 4;
    let mut found = Vec::new();
    for _ in 0..count {
        cursor = skip(body, cursor)?;
        if cursor + 10 > body.len() {
            break;
        }
        let kind = u16::from_be_bytes([body[cursor], body[cursor + 1]]);
        let size = usize::from(u16::from_be_bytes([body[cursor + 8], body[cursor + 9]]));
        cursor += 10;
        if kind == ADDRESS && size == 4 && cursor + 4 <= body.len() {
            found.push(Ipv4Addr::new(
                body[cursor],
                body[cursor + 1],
                body[cursor + 2],
                body[cursor + 3],
            ));
        }
        cursor += size;
    }
    Ok(found)
}

fn skip(body: &[u8], mut cursor: usize) -> Result<usize, Error> {
    loop {
        let Some(size) = body.get(cursor) else {
            return Err(Error::new("a name ran past the answer"));
        };
        if size & 0xc0 == 0xc0 {
            return Ok(cursor + 2);
        }
        if *size == 0 {
            return Ok(cursor + 1);
        }
        cursor += usize::from(*size) + 1;
    }
}
