# Agents

Dynet is the operator's personal VPN management tool. It exists because the
tools it replaces bind a rule to one node at a time, so spreading a rule across
many nodes there is a rewrite rather than a mode. The routing target here is a
pool, not a node.

Traffic enters through a TUN device with real DNS and leaves through commercial
subscription nodes used concurrently. Dynet defines no wire protocol of its own
and runs nothing on the far side; subscription protocols are consumed as they
are.

## Repository

- `crates/lib` owns the shared semantic kernel and contracts as `dynet-core`.
- `crates/api` owns the reusable API surface.
- `crates/cli` owns command grammar and user-facing output.
- Dependency direction is `cli -> api -> lib`.
- Keep the API independent of CLI presentation concerns.
- Add product vocabulary only when its behavior and ownership have been
  explicitly settled.

## Settled Vocabulary

- A `Cluster` is a coarse hand-written bucket, four or five by provider and
  landing. It gives the DNS decision a vantage and gives selection a set to
  choose within; it does not identify nodes and is not where the value lies.
- Because a name is resolved through its cluster, that answer must stay correct
  for every node in it: nodes in one cluster are interchangeable with respect to
  the addresses their vantage returns.
- A `Capability` declares what an outbound can carry. `Carriage` is `Native`,
  `Associate`, `Relay` or `Absent`, because subscription protocols support
  datagrams unevenly. This is a routing input, not an implementation detail: a
  cluster carries datagrams only when every node does, since a connection may
  land on any of them.
- A `Pool` is what a rule resolves to. Spreading is the default and `Pool::wide`
  says so; affinity is the exception and `Pool::bound` requires naming the
  window it holds for.
- There is no fake address space. DNS answers are real, so the rule matches and
  the cluster is chosen when the name is asked for, and the name is resolved
  through that cluster.

Run `plumb doctor .` before and after changing repository shape. Before
landing, run `cargo fmt --all --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo check --locked --workspace --all-targets --release`,
`cargo test --locked --workspace`, and `ectropy .`.

Never work or commit directly in the clean `main` integration checkout after
the initial repository bootstrap. Use a dedicated task branch and land through
the repository guard.
