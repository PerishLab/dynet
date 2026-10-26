use super::{base, read};
use dynet_core::{
    Bearing, Course, Decision, Domain, Ground, Ledger, Manifest, Name, Refusal, Sight, Told, decide,
};
use serde_json::{Value, json};
use std::net::IpAddr;

const HELD: &str = "157.240.1.1";
const ISSUED: &str = "203.0.113.5";
const PLAIN: &str = "198.51.100.7";

struct Case {
    manifest: Manifest,
    ledger: Ledger,
}

fn name(value: &str) -> Name {
    Name::new(value).expect("name")
}

fn address(value: &str) -> IpAddr {
    value.parse().expect("address")
}

fn case(value: &Value) -> Case {
    let mut ledger = Ledger::default();
    let domain = Domain::new("cdn.example.com").expect("domain");
    ledger.record(address(ISSUED), (&domain, &name("shared")), (300, 0));
    Case {
        manifest: read(value).expect("manifest"),
        ledger,
    }
}

fn variants() -> Vec<Value> {
    let mut drained = base();
    drained["drain"] = json!({"providers": ["zx.us-01", "jm"]});
    let mut disabled = base();
    disabled["clusters"][1]["disabled"] = json!(true);
    let mut fallen = base();
    fallen["fallback"] = json!("shared");
    vec![base(), drained, disabled, fallen]
}

fn tolds() -> Vec<Option<Told>> {
    [
        ("news.example.com", false),
        ("cdn.example.com", false),
        ("elsewhere.net", false),
        ("cdn.example.com", true),
        ("public.ech.example.net", true),
        ("192.0.2.1", false),
    ]
    .into_iter()
    .map(|(text, outer)| Told::read(text, outer))
    .chain([None])
    .collect()
}

fn sights() -> Vec<Sight> {
    let mut sights = Vec::new();
    for told in tolds() {
        for target in [HELD, ISSUED, PLAIN] {
            sights.extend([Bearing::Stream, Bearing::Datagram].map(|bearing| Sight {
                told: told.clone(),
                address: address(target),
                bearing,
            }));
        }
    }
    sights
}

fn each(mut check: impl FnMut(&Case, &Sight, &Decision)) {
    for value in variants() {
        let case = case(&value);
        for sight in sights() {
            let decision = decide(&case.manifest, &case.ledger, &sight);
            check(&case, &sight, &decision);
        }
    }
}

fn claimed<'a>(case: &'a Case, told: &Told) -> Option<&'a Name> {
    case.manifest.table().claimed(told.domain())
}

#[test]
fn held() {
    each(|case, sight, decision| {
        if case.manifest.table().held(sight.address).is_some() {
            assert_ne!(decision.course(), &Course::Direct, "I1: {sight:?}");
        }
    });
}

#[test]
fn own() {
    each(|case, sight, decision| {
        let Some(told @ Told::Own(_)) = &sight.told else {
            return;
        };
        let Some(cluster) = claimed(case, told) else {
            return;
        };
        assert_eq!(decision.ground(), Ground::Named, "I2: {sight:?}");
        assert_eq!(decision.cluster(), Some(cluster), "I2: {sight:?}");
        assert_ne!(decision.course(), &Course::Direct, "I2: {sight:?}");
    });
}

#[test]
fn borrowed() {
    each(|case, sight, decision| {
        let unclaimed = sight
            .told
            .as_ref()
            .is_none_or(|told| claimed(case, told).is_none());
        let silent = sight
            .told
            .as_ref()
            .is_none_or(|told| matches!(told, Told::Outer(_)));
        if !(unclaimed && silent) {
            return;
        }
        let bare = Sight {
            told: None,
            ..sight.clone()
        };
        let alone = decide(&case.manifest, &case.ledger, &bare);
        assert_eq!(decision, &alone, "I3: {sight:?}");
    });
}

#[test]
fn refuses() {
    each(|case, sight, decision| {
        let named = sight.told.as_ref().and_then(|told| claimed(case, told));
        let Some(cluster) = named.or_else(|| case.manifest.table().held(sight.address)) else {
            return;
        };
        match decision.course() {
            Course::Through(held) | Course::Refused(held, _) => {
                assert_eq!(held, cluster, "I4: {sight:?}");
            }
            Course::Direct => panic!("I4: a claimed destination fell back: {sight:?}"),
        }
    });
}

#[test]
fn shared() {
    each(|case, sight, decision| {
        let Some(told @ Told::Own(_)) = &sight.told else {
            return;
        };
        let issued = case.ledger.lookup(sight.address).is_some();
        let free =
            claimed(case, told).is_none() && case.manifest.table().held(sight.address).is_none();
        if issued && free {
            assert_eq!(decision.course(), &Course::Direct, "I5: {sight:?}");
            assert_eq!(decision.ground(), Ground::Unclaimed);
        }
    });
}

fn decided(value: &Value, told: Option<Told>, target: &str, bearing: Bearing) -> Decision {
    let case = case(value);
    let sight = Sight {
        told,
        address: address(target),
        bearing,
    };
    decide(&case.manifest, &case.ledger, &sight)
}

#[test]
fn aims() {
    let news = Told::read("news.example.com", false);
    let rescued = decided(&base(), news.clone(), PLAIN, Bearing::Stream);
    assert_eq!(rescued.aim().map(Domain::get), Some("news.example.com"));
    let kept = decided(&base(), news, ISSUED, Bearing::Stream);
    assert_eq!(
        kept.aim(),
        None,
        "an issued address keeps its address, whoever it was issued for"
    );
    let outer = decided(
        &base(),
        Told::read("cdn.example.com", true),
        PLAIN,
        Bearing::Stream,
    );
    assert_eq!(outer.aim().map(Domain::get), Some("cdn.example.com"));
}

#[test]
fn rows() {
    let value = base();
    let issued = decided(&value, None, ISSUED, Bearing::Stream);
    assert_eq!(
        (issued.ground(), issued.course()),
        (Ground::Issued, &Course::Through(name("shared")))
    );
    let blind = decided(&value, None, PLAIN, Bearing::Stream);
    assert_eq!(
        (blind.ground(), blind.course()),
        (Ground::Fallback, &Course::Direct)
    );
    let held = decided(&value, None, HELD, Bearing::Stream);
    assert_eq!(
        (held.ground(), held.course()),
        (Ground::Held, &Course::Through(name("veiled")))
    );
}

#[test]
fn refusals() {
    let value = base();
    let unborne = decided(&value, None, HELD, Bearing::Datagram);
    assert_eq!(
        unborne.course(),
        &Course::Refused(name("veiled"), Refusal::Unborne)
    );
    let borne = decided(
        &value,
        Told::read("cdn.example.com", false),
        PLAIN,
        Bearing::Datagram,
    );
    assert_eq!(borne.course(), &Course::Through(name("shared")));
    let off = decided(
        &value,
        Told::read("a.off.test", false),
        PLAIN,
        Bearing::Stream,
    );
    assert_eq!(
        off.course(),
        &Course::Refused(name("off"), Refusal::Disabled)
    );
    let mut drained = base();
    drained["drain"] = json!({"nodes": ["lisa.us-02/exit"]});
    let exitless = decided(&drained, None, HELD, Bearing::Stream);
    assert_eq!(
        exitless.course(),
        &Course::Refused(name("veiled"), Refusal::Empty)
    );
}
