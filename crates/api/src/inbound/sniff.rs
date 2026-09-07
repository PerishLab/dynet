const HANDSHAKE: u8 = 0x16;
const MAJOR: u8 = 3;
const HEAD: usize = 5;
const PLAINTEXT: usize = 1 << 14;
const HELLO: usize = 0x01;
const RANDOM: usize = 32;
const SERVER: u16 = 0x0000;
const SPOKEN: u16 = 0x0010;
const ENCRYPTED: u16 = 0xfe0d;
const HOST: usize = 0x00;
const SPOKE: [u16; 1] = [443];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Sniffed {
    pub name: String,
    pub outer: bool,
    pub alpn: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refused {
    Need(usize),
    Foreign,
}

struct Reader<'a> {
    body: &'a [u8],
    at: usize,
}

pub fn worth(port: u16) -> bool {
    SPOKE.contains(&port)
}

pub fn read(body: &[u8]) -> Result<Sniffed, Refused> {
    let mut held: Vec<u8> = Vec::new();
    let mut at = 0;
    loop {
        let want = at + HEAD;
        let head = body.get(at..want).ok_or(Refused::Need(want))?;
        let width = framed(head)?;
        let end = want + width;
        held.extend_from_slice(body.get(want..end).ok_or(Refused::Need(end))?);
        at = end;
        let told = Reader::new(&held).hello();
        if !matches!(told, Err(Refused::Need(_))) {
            return told;
        }
    }
}

fn framed(head: &[u8]) -> Result<usize, Refused> {
    if head[0] != HANDSHAKE || head[1] != MAJOR {
        return Err(Refused::Foreign);
    }
    let width = usize::from(u16::from_be_bytes([head[3], head[4]]));
    match width == 0 || width > PLAINTEXT {
        true => Err(Refused::Foreign),
        false => Ok(width),
    }
}

impl Sniffed {
    fn note(&mut self, kind: u16, held: &[u8]) -> Result<(), Refused> {
        match kind {
            SERVER => self.name = Reader::new(held).server()?,
            SPOKEN => self.alpn = Reader::new(held).spoken()?,
            ENCRYPTED => self.outer = true,
            _ => return Ok(()),
        }
        Ok(())
    }
}

impl<'a> Reader<'a> {
    fn new(body: &'a [u8]) -> Self {
        Self { body, at: 0 }
    }

    fn hello(&mut self) -> Result<Sniffed, Refused> {
        if self.byte()? != HELLO {
            return Err(Refused::Foreign);
        }
        self.skip(3)?;
        self.skip(2)?;
        self.skip(RANDOM)?;
        let session = self.byte()?;
        self.skip(session)?;
        let suites = self.pair()?;
        self.skip(suites)?;
        let methods = self.byte()?;
        self.skip(methods)?;
        let width = self.pair()?;
        let held = self.take(width)?;
        Reader::new(held).extensions()
    }

    fn extensions(&mut self) -> Result<Sniffed, Refused> {
        let mut sniffed = Sniffed::default();
        while self.spare() {
            let kind = self.wide()?;
            let width = self.pair()?;
            let held = self.take(width)?;
            sniffed.note(kind, held)?;
        }
        match sniffed.name.is_empty() {
            true => Err(Refused::Foreign),
            false => Ok(sniffed),
        }
    }

    fn server(&mut self) -> Result<String, Refused> {
        self.skip(2)?;
        if self.byte()? != HOST {
            return Err(Refused::Foreign);
        }
        let width = self.pair()?;
        let taken = self.take(width)?;
        let name = std::str::from_utf8(taken).map_err(|_| Refused::Foreign)?;
        match name.is_empty() || name.ends_with('.') {
            true => Err(Refused::Foreign),
            false => Ok(name.to_ascii_lowercase()),
        }
    }

    fn spoken(&mut self) -> Result<Vec<String>, Refused> {
        self.skip(2)?;
        let mut told = Vec::new();
        while self.spare() {
            let width = self.byte()?;
            let taken = self.take(width)?;
            told.push(String::from_utf8_lossy(taken).into_owned());
        }
        Ok(told)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], Refused> {
        let end = self.at.checked_add(count).ok_or(Refused::Foreign)?;
        let taken = self.body.get(self.at..end).ok_or(Refused::Need(end))?;
        self.at = end;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<usize, Refused> {
        Ok(usize::from(self.take(1)?[0]))
    }

    fn wide(&mut self) -> Result<u16, Refused> {
        let taken = self.take(2)?;
        Ok(u16::from_be_bytes([taken[0], taken[1]]))
    }

    fn pair(&mut self) -> Result<usize, Refused> {
        Ok(usize::from(self.wide()?))
    }

    fn skip(&mut self, count: usize) -> Result<(), Refused> {
        self.take(count).map(|_| ())
    }

    fn spare(&self) -> bool {
        self.at < self.body.len()
    }
}
