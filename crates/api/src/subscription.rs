use dynet_core::Error;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Entry {
    fields: BTreeMap<String, String>,
}

impl Entry {
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.field(name) == Some("true")
    }
}

pub fn read(text: &str) -> Result<Vec<Entry>, Error> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(body) = trimmed
            .strip_prefix("- {")
            .and_then(|rest| rest.strip_suffix('}'))
        else {
            continue;
        };
        entries.push(entry(body)?);
    }
    if entries.is_empty() {
        return Err(Error::new("the subscription carried no proxy entry"));
    }
    Ok(entries)
}

fn entry(body: &str) -> Result<Entry, Error> {
    let mut fields = BTreeMap::new();
    for item in split(body)? {
        let Some((name, value)) = item.split_once(':') else {
            continue;
        };
        fields.insert(name.trim().to_string(), unquote(value.trim()));
    }
    Ok(Entry { fields })
}

fn split(body: &str) -> Result<Vec<String>, Error> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut listed = 0usize;
    for character in body.chars() {
        match character {
            '{' => {
                return Err(Error::new(
                    "a proxy entry carried a nested mapping this reader does not understand",
                ));
            }
            '[' => {
                listed += 1;
                current.push(character);
            }
            ']' => {
                listed = listed.saturating_sub(1);
                current.push(character);
            }
            ',' if listed == 0 => items.push(std::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    if listed != 0 {
        return Err(Error::new("a proxy entry carried an unclosed list"));
    }
    items.push(current);
    Ok(items)
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    for quote in ['\'', '"'] {
        if let Some(inner) = trimmed
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.to_string();
        }
    }
    trimmed.to_string()
}
