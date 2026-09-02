use crate::error::Error;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Label(String);

impl Label {
    pub fn new(value: impl AsRef<str>) -> Result<Self, Error> {
        let value = value.as_ref().trim().to_string();
        if value.is_empty() {
            return Err(Error::new("a label must carry visible text"));
        }
        if value.chars().any(char::is_control) {
            return Err(Error::new("a label must carry no control character"));
        }
        Ok(Self(value))
    }

    pub fn get(&self) -> &str {
        &self.0
    }
}
