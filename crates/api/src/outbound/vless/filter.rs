const SERVER: [u8; 3] = [0x16, 0x03, 0x03];
const CLIENT: [u8; 2] = [0x16, 0x03];
pub const DATA: [u8; 3] = [0x17, 0x03, 0x03];
const MODERN: [u8; 6] = [0x00, 0x2b, 0x00, 0x02, 0x03, 0x04];
const NARROW: u16 = 0x1305;
const KNOWN: [u16; 5] = [0x1301, 0x1302, 0x1303, 0x1304, 0x1305];

pub struct Traffic {
    pub identity: [u8; 16],
    pub count: i32,
    pub tls: bool,
    pub modern: bool,
    pub xtls: bool,
    cipher: u16,
    remaining: i32,
    pub padding: bool,
    pub once: Option<[u8; 16]>,
    pub within: bool,
    pub command: i32,
    pub content: i32,
    pub pad: i32,
    pub current: i32,
    pub direct: bool,
}

impl Traffic {
    pub fn new(identity: [u8; 16]) -> Self {
        Self {
            identity,
            count: 8,
            tls: false,
            modern: false,
            xtls: false,
            cipher: 0,
            remaining: 0,
            padding: true,
            once: Some(identity),
            within: true,
            command: -1,
            content: -1,
            pad: -1,
            current: 0,
            direct: false,
        }
    }

    pub fn filter(&mut self, pieces: &[&[u8]]) {
        for piece in pieces {
            self.count -= 1;
            self.sniff(piece);
            if self.remaining > 0 && self.settle(piece) {
                return;
            }
        }
    }

    fn sniff(&mut self, piece: &[u8]) {
        if piece.len() < 6 {
            return;
        }
        if piece[..3] == SERVER && piece[5] == 2 {
            self.remaining = i32::from(u16::from_be_bytes([piece[3], piece[4]])) + 5;
            self.modern = true;
            self.tls = true;
            if piece.len() >= 79 && self.remaining >= 79 {
                let skip = usize::from(piece[43]);
                if let Some(code) = piece.get(44 + skip..46 + skip) {
                    self.cipher = u16::from_be_bytes([code[0], code[1]]);
                }
            }
        } else if piece[..2] == CLIENT && piece[5] == 1 {
            self.tls = true;
        }
    }

    fn settle(&mut self, piece: &[u8]) -> bool {
        let end = usize::try_from(self.remaining)
            .unwrap_or_default()
            .min(piece.len());
        self.remaining -= i32::try_from(piece.len()).unwrap_or(i32::MAX);
        if piece[..end]
            .windows(MODERN.len())
            .any(|held| held == MODERN)
        {
            if KNOWN.contains(&self.cipher) && self.cipher != NARROW {
                self.xtls = true;
            }
            self.count = 0;
            return true;
        }
        if self.remaining <= 0 {
            self.count = 0;
            return true;
        }
        false
    }
}

pub fn whole(body: &[u8]) -> bool {
    let mut cursor = 0;
    while cursor < body.len() {
        let Some(header) = body.get(cursor..cursor + 5) else {
            return false;
        };
        if header[..3] != DATA {
            return false;
        }
        let size = usize::from(u16::from_be_bytes([header[3], header[4]]));
        if size == 0 || cursor + 5 + size > body.len() {
            return false;
        }
        cursor += 5 + size;
    }
    cursor > 0
}
