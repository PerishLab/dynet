use super::Fixture;
use crate::control::Scope;
use crate::resolver::Chain;
use dynet_core::{Domain, Name, Rule, Table};
use std::sync::Barrier;

fn table() -> Table {
    let proxy = Name::new("proxy").unwrap();
    Table::new(
        vec![Rule::new(
            dynet_core::Subject::Suffix(Domain::new("protected.test").unwrap()),
            proxy,
        )],
        None,
    )
    .unwrap()
}

fn question<'a>(root: &'a Domain, table: &'a Table) -> Scope<'a> {
    Scope {
        root,
        cluster: table.claimed(root),
        generation: "one",
        table,
    }
}

fn chain(alias: &str, until: u64) -> Chain {
    let mut chain = Chain::default();
    chain.links.insert(Domain::new(alias).unwrap(), until);
    chain
}

#[test]
fn restart() {
    let rig = Fixture::new();
    let table = table();
    let one = Domain::new("a.protected.test").unwrap();
    let two = Domain::new("b.protected.test").unwrap();
    let plain = Domain::new("unclaimed.test").unwrap();
    let shared = Domain::new("shared.test").unwrap();
    let link = chain(shared.get(), 80);
    let cache = rig.cache();
    cache.save(&dynet_core::Ledger::default()).unwrap();
    for root in [&one, &two] {
        assert!(
            cache
                .origins(&question(root, &table), &link, 3)
                .unwrap()
                .is_ok()
        );
    }
    assert!(
        cache
            .origins(&question(&shared, &table), &Chain::default(), 4)
            .unwrap()
            .is_err()
    );
    assert!(
        cache
            .origins(&question(&plain, &table), &link, 5)
            .unwrap()
            .is_err()
    );
    drop(cache);
    let cache = rig.cache();
    assert!(
        cache
            .origins(&question(&one, &table), &link, 6)
            .unwrap()
            .is_err()
    );
    assert!(
        cache
            .origins(&question(&shared, &table), &Chain::default(), 81)
            .unwrap()
            .is_ok()
    );
}

#[test]
fn simultaneous() {
    let rig = Fixture::new();
    let cache = rig.cache();
    let table = table();
    let barrier = Barrier::new(2);
    let owners = [
        Domain::new("protected.test").unwrap(),
        Domain::new("direct.test").unwrap(),
    ];
    let link = chain("same.test", 80);
    std::thread::scope(|scope| {
        let attempts = owners
            .iter()
            .map(|root| {
                scope.spawn(|| {
                    barrier.wait();
                    cache
                        .origins(&question(root, &table), &link, 7)
                        .unwrap()
                        .is_ok()
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            attempts
                .into_iter()
                .map(|job| job.join().unwrap())
                .filter(|ok| *ok)
                .count(),
            1
        );
    });
    for root in &owners {
        assert!(
            cache
                .origins(&question(root, &table), &link, 8)
                .unwrap()
                .is_err()
        );
    }
}

#[test]
fn generation() {
    let rig = Fixture::new();
    let cache = rig.cache();
    cache.save(&dynet_core::Ledger::default()).unwrap();
    let before = table();
    let after = Table::new(Vec::new(), None).unwrap();
    let root = Domain::new("managed.protected.test").unwrap();
    let alias = Domain::new("edge.test").unwrap();
    let path = chain(alias.get(), 20);
    assert!(
        cache
            .origins(&question(&root, &before), &path, 10)
            .unwrap()
            .is_ok()
    );
    assert!(
        cache
            .origins(&question(&alias, &after), &Chain::default(), 11)
            .unwrap()
            .is_err()
    );
    assert!(
        cache
            .origins(&question(&alias, &after), &Chain::default(), 21)
            .unwrap()
            .is_ok()
    );
    drop(cache);
    let bindings = rig.root.join("cache/origins/bindings");
    std::fs::remove_file(bindings).unwrap();
    assert!(
        crate::control::Cache::open(
            &rig.root.join("cache"),
            "trust".to_owned(),
            dynet_host::Private::current()
        )
        .is_err()
    );
}
