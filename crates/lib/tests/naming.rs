use dynet_core::Instance;

#[test]
fn prefixed() {
    assert!(Instance::new("dynet0").is_ok());
    for refused in ["tun0", "foo0", "", "dyne"] {
        assert!(
            Instance::new(refused).is_err(),
            "{refused} must refuse so a sweep can recognise what exists"
        );
    }
}

#[test]
fn fits() {
    assert!(
        Instance::new("dynet-lab").is_err(),
        "a dash cannot name a device"
    );
    assert!(
        Instance::new("dynetaaaaaaaaaaaaaa").is_err(),
        "a name longer than an interface allows must refuse"
    );
}

#[test]
fn stable() {
    let first = Instance::new("dynet0").expect("instance");
    let again = Instance::new("dynet0").expect("instance");
    assert_eq!(first.priority(), again.priority());
    assert_eq!(
        first.priority(),
        17000 + (first.priority() - 17000),
        "the priority must sit in the reserved span"
    );
    assert!((17000..18000).contains(&first.priority()));
    let other = Instance::new("dynet1").expect("instance");
    assert_ne!(
        first.priority(),
        other.priority(),
        "two instances must not claim one priority"
    );
}

#[test]
fn tabled() {
    let first = Instance::new("dynet0").expect("instance");
    let again = Instance::new("dynet0").expect("instance");
    assert_eq!(
        first.table(),
        again.table(),
        "a name always claims one table"
    );
    assert!(
        (41000..42000).contains(&first.table()),
        "the table must sit in its own reserved span, clear of the kernel's own"
    );
    let other = Instance::new("dynet1").expect("instance");
    assert_ne!(
        first.table(),
        other.table(),
        "two instances must not claim one routing table, or each would carry the other's routes"
    );
    assert_ne!(
        first.table(),
        first.priority(),
        "the table and the rule priority are different numbers in different namespaces"
    );
}
