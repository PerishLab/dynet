use dynet_core::{
    Blame, Capability, Carriage, Cluster, Fault, Label, Name, Node, Policy, Selector, Verdict,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn cluster(count: usize) -> Cluster {
    let nodes = (0..count)
        .map(|index| {
            Node::new(
                Label::new(format!("node-{index}")).expect("label"),
                Capability::new(Carriage::Native),
            )
        })
        .collect();
    Cluster::new(Name::new("provider").expect("name"), nodes).expect("cluster")
}

fn policy() -> Policy {
    Policy::new(Duration::from_secs(600), 0.05).expect("policy")
}

fn tally(selector: &mut Selector, rounds: usize, now: Instant) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for _ in 0..rounds {
        let label = selector.choose(now).expect("a node");
        *counts.entry(label.get().to_string()).or_default() += 1;
    }
    counts
}

#[test]
fn attributes() {
    assert_eq!(Fault::Reach.blame(), Blame::Node);
    assert_eq!(Fault::Handshake.blame(), Blame::Node);
    assert_eq!(Fault::Refused.blame(), Blame::Target);
    assert_eq!(Fault::Silent.blame(), Blame::Unclear);
    assert_eq!(Fault::Severed.blame(), Blame::Unclear);
    assert!(
        Blame::Unclear.charges(),
        "an unattributed failure falls to the node by default"
    );
    assert!(!Blame::Target.charges());
}

#[test]
fn answered() {
    assert!(Verdict::Answered.working());
    assert!(
        Verdict::Blocked.working(),
        "a request the destination refused was still carried by the node"
    );
    assert!(Verdict::Faulted(Fault::Refused).working());
    assert!(!Verdict::Faulted(Fault::Silent).working());
    assert!(Verdict::Blocked.blame().is_none());
}

#[test]
fn spreads() {
    let now = Instant::now();
    let mut selector = Selector::new(&cluster(4), policy(), now);
    let counts = tally(&mut selector, 40, now);
    assert_eq!(counts.len(), 4, "every node must carry traffic: {counts:?}");
    for (label, count) in &counts {
        assert_eq!(*count, 10, "{label} took {count} of forty");
    }
}

#[test]
fn demotes() {
    let now = Instant::now();
    let mut selector = Selector::new(&cluster(4), policy(), now);
    let bad = Label::new("node-0").expect("label");
    for _ in 0..6 {
        selector.observed(&bad, Verdict::Faulted(Fault::Reach), now);
    }
    let counts = tally(&mut selector, 200, now);
    let share = counts.get("node-0").copied().expect("the demoted node");
    assert!(
        share > 0,
        "an exploration floor must keep the node reachable"
    );
    assert!(share < 20, "a failing node took {share} of two hundred");
}

#[test]
fn spares() {
    let now = Instant::now();
    let mut selector = Selector::new(&cluster(2), policy(), now);
    let held = Label::new("node-0").expect("label");
    let before = selector.weight(&held, now).expect("weight");
    for _ in 0..8 {
        selector.observed(&held, Verdict::Faulted(Fault::Refused), now);
    }
    let after = selector.weight(&held, now).expect("weight");
    assert!(
        after >= before,
        "a destination that refuses must not cost the node that reached it"
    );
}

#[test]
fn recovers() {
    let now = Instant::now();
    let mut selector = Selector::new(&cluster(2), policy(), now);
    let bad = Label::new("node-0").expect("label");
    for _ in 0..8 {
        selector.observed(&bad, Verdict::Faulted(Fault::Silent), now);
    }
    let sunk = selector.weight(&bad, now).expect("weight");
    let later = now + Duration::from_secs(6000);
    let healed = selector.weight(&bad, later).expect("weight");
    assert!(sunk < 0.2, "eight faults must sink the standing: {sunk}");
    assert!(
        healed > 0.9,
        "a penalty must decay so a misattribution expires: {healed}"
    );
}

#[test]
fn refuses() {
    assert!(
        Policy::new(Duration::ZERO, 0.05).is_err(),
        "a penalty that never decays must refuse"
    );
    for floor in [0.0, 1.0, -0.1, f64::NAN] {
        assert!(
            Policy::new(Duration::from_secs(600), floor).is_err(),
            "floor {floor} must refuse"
        );
    }
}
