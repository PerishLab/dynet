use crate::error::Error;
use crate::{Affinity, Bearing, Capability, Label};

pub const DIRECT: &str = "direct";

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Name(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    label: Label,
    capability: Capability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Form {
    Pool(Vec<Node>),
    Alias(Name),
    Via(Name, Label),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cluster {
    name: Name,
    form: Form,
    bound: Option<Affinity>,
    probe: Option<Label>,
    disabled: bool,
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
    pub fn new(name: Name, form: Form) -> Result<Self, Error> {
        if name.get() == DIRECT {
            return Err(Error::new(format!(
                "{DIRECT} names leaving from the host itself and cannot name a cluster"
            )));
        }
        let looped = match &form {
            Form::Alias(target) | Form::Via(target, _) => target == &name,
            Form::Pool(_) => false,
        };
        if looped {
            return Err(Error::new(format!(
                "cluster {} cannot lead to itself",
                name.get()
            )));
        }
        Ok(Self {
            name,
            form,
            bound: None,
            probe: None,
            disabled: false,
        })
    }

    pub fn pool(name: Name, nodes: Vec<Node>) -> Result<Self, Error> {
        Self::new(name, Form::Pool(nodes))
    }

    pub fn bounded(mut self, affinity: Affinity) -> Self {
        self.bound = Some(affinity);
        self
    }

    pub fn probing(mut self, exit: Label) -> Self {
        self.probe = Some(exit);
        self
    }

    pub fn disable(mut self) -> Self {
        self.disabled = true;
        self
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn form(&self) -> &Form {
        &self.form
    }

    pub fn bound(&self) -> Option<Affinity> {
        self.bound
    }

    pub fn probe(&self) -> Option<&Label> {
        self.probe.as_ref()
    }

    pub fn disabled(&self) -> bool {
        self.disabled
    }

    pub fn nodes(&self) -> &[Node] {
        match &self.form {
            Form::Pool(nodes) => nodes,
            Form::Alias(_) | Form::Via(..) => &[],
        }
    }

    pub fn datagrams(&self) -> bool {
        self.nodes()
            .iter()
            .any(|node| node.capability().datagrams())
    }

    pub fn bearers(&self, bearing: Bearing) -> Vec<&Node> {
        self.nodes()
            .iter()
            .filter(|node| node.capability().bears(bearing))
            .collect()
    }
}
