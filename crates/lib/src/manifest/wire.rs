use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Wire {
    pub(super) version: u32,
    #[serde(default)]
    pub(super) generation: Option<String>,
    pub(super) nodes: Vec<Map<String, Value>>,
    pub(super) clusters: Vec<Pool>,
    pub(super) rules: Vec<Line>,
    pub(super) fallback: String,
    #[serde(default)]
    pub(super) drain: Drain,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pool {
    pub(super) name: String,
    #[serde(default)]
    pub(super) nodes: Option<Vec<String>>,
    #[serde(default)]
    pub(super) members: Option<Vec<Member>>,
    #[serde(default)]
    pub(super) pool: Option<String>,
    #[serde(default)]
    pub(super) via: Option<String>,
    #[serde(default)]
    pub(super) exit: Option<String>,
    #[serde(default)]
    pub(super) bound: Option<String>,
    #[serde(default, rename = "probe_exit")]
    pub(super) probe: Option<String>,
    #[serde(default)]
    pub(super) disabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Member {
    pub(super) provider: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Line {
    #[serde(default)]
    pub(super) exact: Option<String>,
    #[serde(default)]
    pub(super) suffix: Option<String>,
    #[serde(default)]
    pub(super) holds: Option<String>,
    pub(super) cluster: String,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Drain {
    #[serde(default)]
    pub(super) nodes: Vec<String>,
    #[serde(default)]
    pub(super) providers: Vec<String>,
}

pub(super) fn flatten(prefix: &str, value: &Value, into: &mut Vec<(String, String)>) {
    let key = |name: &str| match prefix.is_empty() {
        true => name.to_string(),
        false => format!("{prefix}.{name}"),
    };
    match value {
        Value::Object(fields) => {
            for (name, inner) in fields {
                flatten(&key(name), inner, into);
            }
        }
        Value::Array(items) => {
            let joined: Vec<String> = items.iter().map(scalar).collect();
            into.push((prefix.to_string(), joined.join(",")));
        }
        _ => into.push((prefix.to_string(), scalar(value))),
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
