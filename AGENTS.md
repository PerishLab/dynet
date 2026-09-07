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
- A `Capability` declares what an outbound can carry, and `Carriage` names the
  protocol the outbound speaks rather than the flag its vendor set: `Relay` is
  datagrams wrapped in the node's own stream, which is what this repository
  speaks, and `Absent` is a protocol reached by stream alone. `Native` and
  `Associate` keep their meaning and have no producer yet.
- `Bearing` is what a piece of traffic needs, `Stream` or `Datagram`. A cluster
  carries datagrams when any of its nodes does, and datagram selection chooses
  only within those, so one node that cannot carry them costs that node and not
  the cluster.
- A node's standing is kept per bearing. A node silent on datagrams loses its
  datagram weight and keeps its stream weight, because a vendor flag is a claim
  and the score is where that claim is checked. Nothing probes carriage ahead of
  time; the first real request through a node is the measurement.
- A `Pool` is what a rule resolves to. Spreading is the default and `Pool::wide`
  says so; affinity is the exception and `Pool::bound` requires naming the
  window it holds for.
- An instance derives everything it must not share from its own name: the rule
  priority it installs, the mark it stamps, and the numeric routing table it
  claims. A constant table number would make two instances two names for one
  table, each silently carrying the other's routes.
- Dynet's own traffic is named by a mark it stamps on every socket it opens,
  derived from the instance name. A rule ahead of the instance's own sends
  marked traffic to the main table, and the firewall table accepts it before
  the redirect, so the exemption names the traffic and not the account it runs
  under.
- The captured query is answered from the address the caller wrote to. The
  firewall table maps it to one fixed local seat and the resolver binds that
  same seat, so the reply carries the source connection tracking expects and a
  strict resolver accepts it. Binding to every address lets the kernel choose
  the source instead, which only a tolerant socket will take.
- A `Table` is the routing policy, declared in its own file and named by
  `--table`. It carries a rule per line of policy, each naming exactly one of
  `exact`, `suffix` or `holds` and the cluster that carries what it matches, and
  a `fallback` that is `direct` unless it names a declared cluster. It is a
  separate file from the cluster declaration because the two change on different
  clocks: topology changes when the subscription or an exit does, policy changes
  whenever a destination is added. The running service re-reads it on SIGHUP and
  keeps the standing table when the new one refuses, so a bad edit costs nothing;
  the ledger of issued addresses survives a reload, so sessions already carried
  are not disturbed.

- What the running service thinks of each node is recorded to
  `/run/dynet/<instance>.standing` on every sweep and read back by
  `dynet standing`. It is a file and not a socket because this product declares
  nothing to listen on. The first line carries the moment it was taken and how
  long the service has stood, so a reader can tell a snapshot from now; each
  line after it names the cluster, the bearing, the weight, the count answered
  and the count charged, and the node's label last, because a label carries
  spaces and nothing else may follow it.

- An answer never reaches the caller before the route that carries it exists.
  The store serves a remembered answer past its own life while refreshing it,
  and the ledger releases an issue on its own clock, so a remembered answer can
  outlive the route it was issued under; before speaking from memory the
  resolver checks the ledger still holds every address it is about to name and
  reissues them when it does not. Without that the caller receives an address
  the kernel routes by the main interface, and the session is lost before this
  product ever sees it.
- Every use of a node names the node. Carriage prints its verdict, a fresh
  lookup prints the node it chose, and a refreshed one prints the node that
  refreshed it, because a node scored by an event nobody can name cannot be
  argued about later.

- A `Span` is the address range Dynet occupies on the host, injected through
  configuration rather than a flag because it is a fact about the machine. Its
  first address is what the device wears and the middle of its upper half is the
  synthetic peer a diverted session is rewritten to carry.
- There is no fake address space. DNS answers are real, so the rule matches and
  the cluster is chosen when the name is asked for, and the name is resolved
  through that cluster.

Run `plumb doctor .` before and after changing repository shape. Before
landing, run `cargo fmt --all --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo check --locked --workspace --all-targets --release`,
`cargo test --locked --workspace`, and `ectropy .`.

A test that reaches the network is `#[ignore]` and is driven on a host, never
here. The workstation's own tunnel client answers for servers that do not
exist, so a check made against it passes or fails for reasons that are not the
code's.

Never work or commit directly in the clean `main` integration checkout after
the initial repository bootstrap. Use a dedicated task branch and land through
the repository guard.
