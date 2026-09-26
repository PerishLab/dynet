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
  firewall table maps it to one fixed seat and the resolver binds that same
  seat, so the reply carries the source connection tracking expects and a strict
  resolver accepts it. Binding to every address lets the kernel choose the
  source instead, which only a tolerant socket will take. The seat is the span's
  own first address rather than loopback, because a caller that is not this host
  cannot reach loopback and reaching it through a redirected local route would
  return an answer from an address the caller never wrote to.
- A query forwarded from elsewhere is captured in the prerouting hook and only
  from a declared source, the same scope that decides what traffic is diverted
  at all. Without a declared source there is no prerouting chain, so nothing
  that is not this host's is answered by accident.
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

- A server name read from a `ClientHello` is the name the caller is actually
  reaching for, and it is read rather than inferred. The parser reassembles a
  hello across TLS records, walks the handshake to the extensions, and asks for
  the exact count of bytes it still needs so a peek can fetch them; a hello it
  cannot use at all is refused rather than guessed at. It reads every extension
  before answering, because whether the `encrypted_client_hello` extension is
  present decides what the name means.
- A name carried beside an encrypted client hello is a provider's public name
  and not the destination's own, and it is reported as such. A rule may name it
  like any other subject, so an operator can route what hides behind one
  provider deliberately; nothing presents it as the destination it conceals.

- A stream on a port where the client speaks TLS first is peeked, never read,
  before a node is chosen; the kernel's own peek leaves the bytes where they
  are, so the carried session needs no replay. The peek is bounded, and a
  destination whose first bytes are repeatedly unreadable stops being peeked at
  all, because a bound paid on every connection to a protocol this cannot read
  is a bound paid forever.
- A name read from the wire decides the cluster and nothing else. It asks the
  table directly, where an unsniffed connection still falls back to what the
  ledger issued the address under, and the target carried through the node is
  unchanged either way. When the two names disagree the disagreement is said
  out loud, because a routing decision nobody can attribute is the shape this
  repository has already paid to fix once.

- What Dynet captures is scoped by where the traffic came from, and the scope is
  configuration rather than an argument because it is a fact about the machine.
  This host's own traffic is always captured; traffic forwarded from elsewhere
  is captured only from a declared source prefix, so a machine sitting on a
  network it does not own diverts nothing by accident. Configuration here means
  the environment, because nothing in this repository passes a file to the
  cascade, so a scope is one comma-separated `DYNET_SOURCES` and never a list
  the cascade cannot read back. The claim stays what it
  always was, the destinations in scope, and the two are separate axes.
- The score is shared across every caller. A node demoted by one client's
  request is demoted for all of them, which is the pool doing what it is for:
  spreading a trusted group's use across the whole set rather than partitioning
  it. Per-caller standing is deliberately not kept.

- Dynet carries no sixth-version address, and it says so only about what it
  carries. A name a rule claims is answered with no sixth address, so a caller
  falls back to the fourth version and leaves through a node; a name no rule
  claims is relayed to the upstream unchanged, sixth-version answer and all, so
  what this product does not carry keeps whatever the network offers it.
  Refusing the sixth version for every name would make a whole network's
  destinations fourth-version-only on the strength of carrying a dozen of them.

- A forwarded connection the kernel hands over already carries its own
  destination: a transparent seat reads it from the socket's own local address,
  so there is no synthetic peer to rewrite onto, no session to remember and no
  packet copied through userspace. Everything after that point is the path the
  device already used, so a name is still read from the hello and a cluster is
  still chosen the same way. The seat is off unless a port is declared, and the
  routing and firewall that steer traffic to it are the host's to arrange, not
  this product's.

- When a transit port is declared alongside a source, this product installs the
  steering itself: a mangle rule that marks a forwarded stream and hands it to
  the transparent seat, a rule of its own ahead of the diversion, and a table
  carrying one local route. Ahead is the whole point. Two capture shapes on one
  machine contend, and the rule with the smaller priority runs; leaving that to
  chance once measured the device twice and read it as the seat being slower.
- The split between the two shapes is by protocol and not by accident. A
  forwarded stream is ushered, because the kernel can hand over its destination;
  a forwarded datagram and everything this host sends itself still go through
  the device. Port fifty three is excluded from the steering so the resolver
  keeps its own capture.
- An instance derives four numbers from its own name and none of them may
  collide: the rule priority, the mark that exempts its own sockets, the table a
  diversion reads, and the table and mark a transit uses.

- This product fails closed. When it stops without running its own teardown its
  rules outlive it, and a caller meets a hole rather than the direct path. That
  is chosen: the destinations routed to the detour must never fall back to
  leaving from this host's own address, which is the exposure routing them there
  exists to prevent. Failing open is right for a router carrying strangers and
  wrong for a host carrying an operator's own accounts.
- Because it fails closed, `doctor` must be able to see it. The device, the
  firewall table and the rule are all still present after the process that
  installed them has gone, so their presence says nothing; what says something
  is whether the resolver's seat is held. A seat nobody holds beside rules that
  are installed is reported as stranded and refuses. A bind that fails for want
  of the address is not evidence of a listener and is read as silence.

- What this host sends itself takes the same seat, by a redirect rather than by
  the kernel handing it over, because a locally produced packet never passes
  the hook that hands anything over. The rule matches the interface the routing
  decision already chose, so it says exactly `this was going to the device` with
  nothing to keep in step; the seat then asks the socket for the destination the
  redirect replaced. Streams therefore no longer touch the device at all, and
  what remains on it is datagrams.

- A `Span` is the address range Dynet occupies on the host, injected through
  configuration rather than a flag because it is a fact about the machine. Its
  first address is what the device wears and the middle of its upper half is the
  synthetic peer a diverted session is rewritten to carry.
- There is no fake address space. DNS answers are real, so the rule matches and
  the cluster is chosen when the name is asked for, and the name is resolved
  through that cluster.

## Release

`plumb.toml` declares the product `dynet`, its authority and the one binary
`dynet` for Linux alone, since the TUN device, socket marks and transparent
seats it drives exist nowhere else; nothing else publishes. The workspace declares
version `0.0.0`, and `crates/cli` carries the release identity region through
`plumb::identity!("DYNET")`, which wharf binds after an unbound build. A
release follows Plumb's lifecycle: `plumb release open` cuts `release/<version>`
from a guarded `main`, `plumb release stamp` marks it, and `plumb ship dispatch`
hands the marker to wharf. A stable's changelog is consigned to the Depot with
`plumb depot consign --kind changelog`; `plumb release owed` lists what is
still owed.

## Operating

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
