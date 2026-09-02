use crate::Capability;
use crate::error::Error;
use crate::label::Label;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Name(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    label: Label,
    capability: Capability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cluster {
    name: Name,
    nodes: Vec<Node>,
}

impl Name {
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        if value.trim().is_empty() || value.chars().any(char::is_whitespace) {
            return Err(Error::new(
                "a name must be non-empty and carry no whitespace",
            ));
        }
        Ok(Self(value))
    }

    pub fn get(&self) -> &str {
        &self.0
    }
}

impl Node {
    pub fn new(label: Label, capability: Capability) -> Self {
        Self { label, capability }
    }

    pub fn label(&self) -> &Label {
        &self.label
    }

    pub fn capability(&self) -> Capability {
        self.capability
    }
}

impl Cluster {
    pub fn new(name: Name, nodes: Vec<Node>) -> Result<Self, Error> {
        if nodes.is_empty() {
            return Err(Error::new(format!(
                "cluster {} carries no node and can serve no rule",
                name.get()
            )));
        }
        Ok(Self { name, nodes })
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn datagrams(&self) -> bool {
        self.nodes.iter().all(|node| node.capability().datagrams())
    }
}
