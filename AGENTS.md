# Agents

Read the canonical [PerishLab delivery governance](https://github.com/PerishLab/.github/blob/main/GOVERNANCE.md)
at work start and again before delivery or Issue closure. That document owns
organization-wide Issue, pull-request and acceptance policy; this file keeps
repository-specific constraints without copying that policy.

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
- `crates/tun` owns the packet device on each platform as `dynet-tun`: open
  a device by name, read its identity and MTU, read and write one IP packet,
  wait. Linux opens `/dev/net/tun`; Windows binds Wintun by hand. It is the
  only crate that holds unsafe code; routes, DNS, exemption, firewall and
  service stay in `api`'s host layer.
- Dependency direction is `cli -> api -> lib`, and `api -> tun`.
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
- A `Front` is the node a stream boards first, and it speaks either VMess or
  VLESS over REALITY. An exit is reached through any front, because what the
  exit sees is a stream and not the protocol that carried it there. A VLESS
  front bears streams alone until datagrams are spoken over it, so its
  `Carriage` is `Absent` whatever the vendor flag claims.
- A REALITY front is accepted only when the server proves it holds the key the
  node was declared with: the session id carries the client's seal over the
  whole hello, and the certificate that comes back must be the server's own
  HMAC over its Ed25519 key under the shared secret, signed for and finished
  under it. A server that answers with any other certificate is relaying the
  handshake target, so the session is refused and never used to carry
  anything. The hello is shaped like a current Chrome: GREASE, shuffled
  extensions, and a hybrid ML-KEM and X25519 share beside a plain X25519 share
  drawn from the same X25519 key, so whichever share the server picks seals
  the same secret.
- The only flow spoken is `xtls-rprx-vision`, and it follows Xray's framing
  exactly: the first packets travel padded under the node's identity, and once
  the carried session is TLS 1.3 both directions switch to copying the inner
  records raw beside the outer session rather than sealing them twice. A
  record the server sends with no content is padding and is skipped.
- `dynet carry` dials one node, optionally through one exit, and either fetches
  the egress address or relays one local connection to a target; it is the
  probe a front is checked with on a host, because the workstation's own
  tunnel client rewrites destinations by name and a check made through it
  reaches the handshake target instead of the node.
- The `Manifest` is the one input that says what Dynet routes and through
  what: `nodes` with their protocol material, `clusters`, ordered `rules`, a
  `fallback` and a `drain`, as one JSON document (`version` 1) read by
  `dynet-core` and refused whole when any part of it is wrong. A node's nested
  material is read as dotted names (`tls.reality.public_key`), and `dynet-api`
  checks at load that every pool node can front a stream and every exit speaks
  Shadowsocks 2022, so a manifest that names an unusable node never stands.
  Machine facts (instance, seats, sources) stay in configuration, not here.
- A cluster takes exactly one of three forms: a pool that lists `nodes` or takes
  every node of a provider through `members`; an alias whose `pool` names a
  pool and carries nothing else; or `via` a pool to one fixed `exit` node. An
  alias cannot name an alias and a front cannot be reached through one, so
  every path is one level. Only a pool may carry `bound` (an affinity window
  such as `10m`) or `probe_exit`, which must be the exit of a cluster fronted by
  that pool. `disabled` keeps a cluster declared while every decision for it is
  refused. Clusters are declared, never derived from how a vendor labels nodes.
- `drain` removes nodes, or every node of a provider, from the pools that list
  them; a provider is a node's own `provider`, or the host part of its label
  before `/`. It is the only operator intervention: there is no pin.

- `Bearing` is what a piece of traffic needs, `Stream` or `Datagram`. A cluster
  carries datagrams when any of its nodes does, and datagram selection chooses
  only within those, so one node that cannot carry them costs that node and not
  the cluster.
- A node's standing is kept per bearing. A node silent on datagrams loses its
  datagram weight and keeps its stream weight, because a vendor flag is a claim
  and the score is where that claim is checked. Nothing probes carriage ahead of
  time; the first real request through a node is the measurement.
- Spreading across a pool is the default; affinity is the exception and a pool
  states it with `bound` and the window it holds for.
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
- Rules are matched in order and the first one that matches wins; each names
  exactly one of `exact`, `suffix` or `holds` and the cluster that carries what
  it matches. One name or prefix belongs to one rule only. The `fallback` is
  `direct` or any declared cluster; that choice is the deployment's, not the
  product's. The running service re-reads the whole manifest on SIGHUP and
  keeps the standing one when the new one refuses, so a bad edit costs nothing;
  the ledger of issued addresses and each unchanged pool's standing survive a
  replacement.
- A destination is decided by one pure function over the manifest, the ledger,
  the name the caller gave if any, the address and the bearing, taking the
  first of five rows that holds: a name that is the destination's own and is
  claimed by a rule, or a provider's public name a rule claims explicitly, goes
  through that rule's cluster; an address inside a `holds` goes through its
  cluster; a destination's own name that no rule claims leaves directly; an
  address the ledger issued goes through the cluster it was issued under;
  anything else takes the fallback. Through a cluster that is disabled, holds
  no usable node or cannot bear a datagram, the decision is a refusal and never
  falls back. Tests drive every combination and pin that a held address is
  never direct, a claimed own name is never direct, a provider's public name or
  an address literal alone never yields direct, a claimed destination refuses
  rather than falls back, and an unclaimed name on an address issued to another
  name leaves directly.
- A destination's own name that a rule claims, on an address the ledger never
  issued, is dialled by name through the node and logged with `aim=`: the
  caller's address came from some other resolver and may be poisoned, so the
  node resolves the name where it stands. An issued address keeps its address,
  whoever it was issued for.
- `direct` means Dynet dials the destination itself under its own mark, which
  is how a captured stream leaves from the host. Datagrams are never sent
  directly from here, so a datagram decided `direct` is refused.
- Reserved ranges (`0/8`, `10/8`, `100.64/10`, `127/8`, `169.254/16`,
  `172.16/12`, `192.168/16`, `198.18/15`, `224/4`, `240/4`) are never claimed,
  issued or answered: a `holds` overlapping one refuses the manifest, the ledger
  never records one, and an answer carrying one has it left out, failing the
  name when nothing else remains.

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
- A name read from the wire enters the decision as the destination's own name,
  or as a provider's public name when it rode beside an encrypted client hello;
  an address literal is no name at all. When it disagrees with the name the
  ledger issued the address for, the disagreement is said out loud, because a
  routing decision nobody can attribute is the shape this repository has
  already paid to fix once.

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

## Wintun

`crates/tun/resources/wintun.dll` is the amd64 binary from the official
Wintun 0.14.1 release at `https://www.wintun.net/builds/wintun-0.14.1.zip`
(archive SHA256
`07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51`, the DLL
signed by WireGuard LLC), DLL SHA256 `e5da8447dc2c320edc0fc52fa01885c103de8c118481f683643cacc3220dafce`,
carried beside its `LICENSE.txt`. The license allows redistribution alongside
software that uses it only through `wintun.h`, which is all `crates/tun` does.
The digest is pinned in `crates/tun/src/windows/resource.rs`; the binary is
embedded at compile time and written beside `dynet.exe` only after it matches,
then loaded from there. Nothing is fetched at build or run time. Replacing it
means replacing both files and the pin together.

The adapter is created under the instance's name with tunnel type `Dynet` and
a GUID derived from that name, so it keeps one identity across restarts and
never collides with another product's adapter; dropping the device removes
it.

Where a platform lacks a mechanism the Linux path uses, the call is compiled
for Linux alone and refuses elsewhere rather than pretending: socket marks and
transparent seats are Linux's; the keepalive retry count is set where the
platform exposes it.

## Release

`plumb.toml` declares the product `dynet`, its authority and the one binary
`dynet` for Linux alone, since the TUN device, socket marks and transparent
seats it drives exist nowhere else; nothing else publishes. The workspace declares
version `0.0.0`, and `crates/cli` carries the release identity region through
`plumb::identity!("DYNET")`, which wharf binds after an unbound build. A
release follows Plumb's lifecycle (`plumb release --help`); wharf publishes it.
A stable's changelog goes to the Depot.

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
the initial repository bootstrap. Use a dedicated topic branch and land through
the repository guard.
