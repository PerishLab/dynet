use dynet_core::Span;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cleared {
    pub veil: bool,
    pub route: bool,
    pub link: bool,
}

impl Cleared {
    pub fn any(self) -> bool {
        self.veil || self.route || self.link
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Standing {
    pub cleared: Cleared,
    pub veiled: bool,
}

pub struct Ground<'a> {
    pub port: u16,
    pub claim: &'a str,
    pub span: Span,
    pub bare: bool,
    pub sources: &'a [String],
    pub transit: u16,
}
