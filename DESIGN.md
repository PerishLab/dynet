# Dynet configuration control

Dynet consumes one complete encrypted configuration. The published `secrets`
Rust library owns age encryption, minisign publisher authentication, the v1
wire protocol, HTTP, TLS, subscription waits and device reports. Dynet owns
configuration interpretation, physical socket binding, durable local state and
host application. There is no subscription importer or converter.

## Local enrollment

Generate each device's age identity on that device, in an administrator-owned
private directory. `dynet identity --output ABSOLUTE_PATH` creates the private
identity without overwriting an existing file and prints only its public age
recipient. The machine identity command requires elevation. Obtain the publisher
public key through the enrollment ceremony, independently of the API.

The local bootstrap is JSON with unknown fields refused:

```json
{
  "version": 1,
  "target": {"device": "windows0", "resource": "routing", "epoch": "enrollment1"},
  "identityRef": "C:\\ProgramData\\dynet\\identity.age",
  "publisher": "MINISIGN_PUBLIC_KEY_BASE64",
  "minimum": 1,
  "source": {
    "kind": "url",
    "baseUrl": "https://secrets.example.com",
    "bootstrap": ["192.0.2.10"],
    "tokenRef": "C:\\ProgramData\\dynet\\access.token"
  }
}
```

The example address is reserved documentation space, not a usable deployment.
`baseUrl` selects the Secrets API origin. `bootstrap` supplies 1–8 physical IP
addresses; it does not disable TLS hostname, SNI or public-root verification.
Control requests never resolve through the captured resolver or use a business
node. SDK transport disables redirects and ambient proxies. Clear HTTP is
limited to literal loopback, and Dynet also requires that origin to connect to
that exact loopback address. Deployment uses HTTPS.

The alternate source is `{"kind":"file","path":"ABSOLUTE_ENVELOPE_PATH"}`.
It reads the same signed encrypted envelope. It has no plaintext manifest mode.
The bootstrap, referenced identity, bearer token and file source use absolute
paths with protected ancestors. References are not secret values in arguments.
Machine facts such as instance, span, port, state root, capture claim and source
scope stay local. DNS upstream, retention and all node/routing material are
inside the encrypted business configuration.

The state root itself must be private: root-owned mode 700 on Unix; an owner and
allow ACL limited to Administrators and LocalSystem for Windows services.
Bootstrap and key files require equivalent private protection. Interactive
checks may use the current user's private custody; service installation and
workers require machine custody. Files must be regular, single-link and not
symbolic links or reparse points. Ancestors cannot grant untrusted replacement
or ownership control. Windows ProgramData's ordinary child-creation permission
is acceptable on an ancestor; the product's own directory is separately private.

## Business configuration

The authenticated decrypted payload has one schema:

```json
{
  "version": 1,
  "upstream": "1.1.1.1",
  "retain": 86400,
  "manifest": {
    "version": 1,
    "generation": "probe",
    "nodes": [],
    "clusters": [],
    "rules": [],
    "fallback": "direct"
  }
}
```

`manifest` retains the existing semantic model. The empty example deliberately
carries no VPN node. Deployment supplies the existing Tunnel -> Private rules
and leaves other destinations direct. `upstream` is an IPv4 literal and
`retain` is a positive retention floor in seconds; changing it affects future
issues, never shortens existing issues. Payloads and envelopes inherit the SDK's
1 MiB and 4 MiB bounds. Errors from decrypted parsing are masked; node material
is absent from `Entry`, `Declared` and enclosing debug output.

### Manifest label versus envelope generation

Two distinct fields share the name `generation` at different layers:

- **Payload `manifest.generation`** (optional non-empty string label, such as
  `"probe"` or `"pixel-full"`): an authoring label inside the decrypted manifest
  JSON (`version: 1`). It carries no anti-replay meaning and must never be set
  to an integer. A non-string or blank `manifest.generation` fails manifest
  parsing immediately with a diagnostic naming `secrets publish/seal --generation`.
- **Control-plane envelope `--generation` / `receipt.generation`** (monotonic
  positive integer): the signed envelope version supplied to
  `secrets publish --generation <N>` or `secrets seal --generation <N>` and
  enforced by the Secrets SDK and Dynet's durable generation floor.

**Schema naming decision:** manifest `version: 1` retains the field name
`manifest.generation` rather than renaming it to `manifest.label` or
`manifest.variant`, preserving wire compatibility with deployed OpenWrt and host
envelopes. Authoring mismatches are prevented through strict string validation
with actionable diagnostics and local pre-seal validation (`dynet check`). A
field rename is deferred unless a breaking manifest `version: 2` schema is
introduced for independent functional requirements.

Validate the plaintext business configuration or standalone manifest locally
before sealing or publishing:

```sh
dynet check --input /private/business-config.json
dynet check --input - < /private/business-config.json
dynet check --kind manifest --input /private/manifest.json
```

`dynet check` enforces the 1 MiB payload bound, business configuration schema,
manifest validation (including string-only `manifest.generation`), and front/exit
node checks with unmasked diagnostics while printing no node credentials.
Runtime decryption (`Payload::read`) retains masked errors.

Publish from the trusted operator's private declaration using the released
Secrets CLI. Its declaration binds target, public recipient, local signer and
remote connection; the recipient is not learned from API inventory.

```sh
dynet check --input /private/business-config.json
secrets publish --declaration /private/publication.json --generation 2 --input - < /private/business-config.json
secrets seal --declaration /private/publication.json --generation 2 --input - --output /private/routing.envelope < /private/business-config.json
```

The first command assigns through the API; the second produces a file source.
The operator supplies the whole JSON document on stdin. The API receives opaque
ciphertext and public metadata only. It possesses neither the recipient private
key nor the publisher signer. Device credentials authorize only their own
assignment, currently assigned bundle and reports. These bearer credentials are
local enrollment material, distinct from encrypted node credentials.

## Confirmed state

Each instance owns `<state>/<instance>.control/`:

- `lock` is the exclusive process lease.
- `initialized` distinguishes a missing checkpoint from a fresh enrollment.
- `checkpoint` atomically commits trust, received/validated/applied receipts,
  the signed revocation barrier, retained configuration pointer and issued
  address ledger.
- `<sha256>.envelope` stores only authenticated ciphertext or a signed revoke.

There is no cleartext node cache or decrypted temporary file. Superseded blobs
are collected after a confirmed checkpoint; current received, applied, retained
and barred envelopes remain. The checkpoint is bounded to 16 MiB. Writes use a
new private file, sync its contents, atomically replace the owned checkpoint and
sync the parent on Unix; Windows replacement requests write-through. Failure
never reports an unconfirmed application. Existing corrupt/missing state or a
changed enrollment trust fingerprint refuses startup. Another instance process
cannot share the lease.

The trust fingerprint binds target, publisher, recipient and local minimum.
API origin or access-token rotation does not change publisher trust. Replacing
the identity, publisher, epoch or minimum is an explicit reenrollment, not an
automatic reset. The device's local generation floor can be advanced by an
authenticated envelope whose payload later refuses; an unauthenticated response
never advances it. Equal-generation different-digest input is a conflict.

A received bad generation 9 can leave applied generation 5 active. Offline
startup reauthenticates that exact applied envelope rather than treating it as a
new rollback. Network or heartbeat failure imposes no lease: the last confirmed
configuration remains valid indefinitely. A validated but unapplied candidate
can be retried without falsely naming it applied.

A signed revocation is a durable barrier from receipt, even before host
application succeeds. It survives a crash and a later invalid configuration.
Only successful application of a higher configuration removes it. Revoked
startup retains the previous known protection and issued addresses, refuses
business DNS and new captured connections, and keeps the control connection
available. It never reopens the old node credentials as an offline fallback.
Expired issues are not swept and flush is refused while revoked.

## Application and reporting

A bounded dedicated control worker owns source I/O, subscription waits and
heartbeats. File inputs are checked once per second; URL subscriptions use the
SDK's ten-second conditional wait. Source receipt and pending application share
one ordering boundary. The packet loop performs no control HTTP request. Stop
cancels SDK transport and joins the worker.

At startup, the device and capture projection and resolver/listener sockets must
succeed before committing initial application or notifying Windows SCM ready.
During update, Dynet prepares a new semantic policy and rejudged ledger, then
projects and verifies the union of old and new protected destinations. Only
after that succeeds does one checkpoint commit configuration and ledger. The
in-memory policy is replaced under the same ledger/policy ordering. Removing
obsolete extra protection follows the commit; failure to prune keeps the extra
protection. An application failure retains the confirmed policy. A durability
failure closes its connection circuits and stops the worker with host protection
retained.

Pool standing survives unchanged pool membership. Existing flows keep their
accepted policy across ordinary updates. Every flow's physical socket is
registered before it can send protocol credentials. Revocation closes both
outbound and captured client sockets, bars new circuit tickets and prevents late
DNS/datagram replies before acknowledging revoked. Recovery creates fresh
circuits; an old revoked flow cannot revive. DNS responses from a replaced
policy are withheld. DNS address records are durable before an answer is sent,
including answers served from memory.

Reports distinguish received, validated, applied, failed and revoked. `applied`
means the device confirmed its protected local application; a server report is
a client claim, not independent hardware proof. Denying an API token stops
control access but is not a claim that an offline device stopped. Failures do
not release the confirmed cache or turn a claimed destination direct.

## Commands and cutover

`dynet --instance dynet0 --state ABSOLUTE_STATE seed --bootstrap ABSOLUTE_BOOTSTRAP
--issued ABSOLUTE_LEDGER` explicitly initializes an absent encrypted checkpoint.
The administrator supplies a frozen private ledger in the current native ledger
serialization, also used inside checkpoints. Unknown or silently normalized
records refuse. The existing SDK authenticates and decrypts the complete
configuration before initialization. Its first checkpoint commits received,
validated and every issued record together; applied remains absent until host
readiness. Existing or interrupted destinations refuse, and the durable
initialization marker prevents a partial seed from becoming an empty enrollment.
The command changes no host protection, service or source files. It does not
discover historical paths or read a legacy format. The operator quiesces the
source while retaining protection, supplies its exact snapshot and promotes the
completed state only after checking the receipt and issued records.

All node-using diagnostics (`carry`, `resolve`, `reach`, `spread`) accept
`--bootstrap`. They use the same authenticated, generation-aware source and
private instance lease. Run them against an offline instance; a running service
owns that lease. Diagnostics validate input without reporting host application.
The raw `exit --secret` and plaintext `--manifest`, `--upstream`, `--retain` and
`--ledger` paths have been removed.

Service installation captures only the executable, resolved absolute bootstrap,
state root and machine facts. Neither the private key nor any node credential
appears in the registration. The existing macOS and Windows capture boundaries,
explicit teardown and automatic startup remain as documented in AGENTS.md.
Application DoH and independently obtained IPv6 are separate work.

Cutover is an explicit operation, not a compatibility layer. Before first start,
stop the old service while retaining its protection, preserve its issued-address
ledger into the new checkpoint through an offline owned migration, provision
private enrollment and publish the complete encrypted configuration. Verify the
new service and its exact distribution, then delete the former plaintext
manifest and obsolete files. A remaining `<state>/<instance>.ledger` refuses a
new start so a missed migration cannot silently release issued addresses. There
is no old-format reader, historical-directory discovery or plaintext fallback.

## Two-hop UDP

A Shadowsocks 2022 exit with `"uot": 2` in the encrypted manifest enables
IPv4 UDP on its `via` cluster. The front opens its ordinary authenticated
stream to the exit. The exit then opens a sing-box UoT v2 connected stream
for the original destination. A plain VLESS front still carries streams only.
An absent declaration leaves the cluster's UDP path refused. A disabled,
drained or failed path cannot fall back to direct UDP.

The diverter keys TCP and UDP mappings separately and rejects fragmented IPv4
UDP. Each UDP association holds one destination and uses the shared connection
budget, bounded channels and circuit cancellation. UoT responses are delivered
only as complete datagrams within the IPv4 payload limit. A relay cluster
records its UDP results separately from the front pool's stream results.

Dynet #149 owns formal release qualification and Windows consumption.
OpenWrt traffic admission also needs a candidate APK, gateway forwarding
checks and an independent WAN observation during proxy failure.

## Refusal reporting

The diverter aggregates only multicast datagrams decided direct and datagrams
refused as Unborne. Their first event is retained with its original reason;
60-second monotonic windows summarize seen and suppressed counts, relative first
and last times, and a process-local window number. At most 128 cluster/port
buckets and two class overflow buckets exist. Overflow remains explicitly
classified, and saturated counters report lower bounds. Single-event and empty
windows emit no redundant summary. The one-second worker tick and orderly exit
flush pending summaries; process abort may lose the unfinished window. These
statistics are not a durable audit. A counter failure reports unknown counts
once and does not alter refusal. Formatting sinks run outside the data lock.
Other refusals, stream failures, startup, control and protection events retain
their existing reporting.

The diverter returns ICMPv4 Port Unreachable for a new, captured, eligible
unicast UDP flow when the route is explicitly unborne or direct UDP is
unsupported. It addresses the reply to the caller and quotes the original
IPv4 header with options and the first eight bytes of UDP. Multicast,
broadcast-looking addresses, fragments, malformed headers and unassociated UDP
ports receive no feedback. At most 16 replies are emitted per diverter window
of one second. Further requests still follow the refusal policy. The ICMP
message is local feedback through the device and never opens a route.

## Resolver transport

The fixed DNS seat binds UDP and TCP together before confirming the applied
configuration. Both transports use the same request snapshot, circuit fence,
store, durable address ledger and independent sixteen-request direct/proxy
budgets. TCP has a separate ceiling of32 connections; an idle frame waits30s,
a frame in progress2s, complete-frame writes2s, and reads poll at200ms for halt. Lengths cannot exceed
65535 bytes. A connection accepts successive and pipelined questions in order.
Malformed, stalled or failed requests close the connection; policy-authorized
lookup failures return SERVFAIL. A direct TCP question uses a marked TCP
upstream with a2s connect bound,8s first-answer bound and2s frame bound, and
attaches that socket to the request circuit. UDP continues to relay UDP; an
upstream truncation reaches its client, whose TCP retry now has a real seat.
Claimed names retain the same suppression, persistence and capture-before-answer
rules. This repair originates in#152; native whole-source admission still needs
its separate operator window.

## Linux diagnostics

`DYNET_RECORD` opts continuous diversion into bounded asynchronous diagnostics.
It names an absolute private directory owned by the worker user, mode 0700.
Without this setting, output is unchanged. Procd unit generation carries the
setting and enables stdout capture only when selected. Other platforms refuse
this option. Unit rendering does not access the directory.

The producer performs no file or sink IO. It offers at most 256 records to a
nonblocking queue, clips escaped diagnostic text to 512 ASCII characters, and
adds instance, PID, sequence, monotonic uptime, classification, detail and
truncation fields.
A detached writer owns stdout; blocked output cannot stall forwarding or mode
expiry. Full queues and failed record writes increment a saturating cumulative
loss count. The writer reports that count at startup and after changes when the
sink accepts it. This count covers producer and writer losses, not downstream
syslog, collector, rotation or power losses; sequence gaps and restart identity
are observable but do not prove complete capture. Shutdown does not drain or
join a blocked writer.

Brief mode retains recognized failures and unknown text, suppressing recognized
routine success messages. This compatibility filter is conservative text
classification, not a typed event severity contract. A separate watcher polls
`detail.until` every 250 ms. The file must be owned by the worker user, mode
0600, regular, unlinked elsewhere and not a symlink. Its integer deadline uses
Linux boot uptime and may be no more than 300 seconds ahead. Missing, invalid,
expired or overlong controls select brief mode. Enabling detail needs no worker
restart. Directory custody failure refuses diagnostic initialization; subsequent
control or output failures preserve forwarding. Operators provision the private
RAM directory before starting a worker.

An active Linux recording directory also enables structured DNS and route
evidence in both brief and detail modes. The existing diag envelope carries
`class=evidence truncated=false` followed by one compact JSON object of up to
768 bytes. Each record has `v=1`, a positive assignment `generation` and the
event's Unix wall-clock time in `unix_ms`. The snapshot that accepted the
question or selected the route supplies the generation. Sequence and loss
counters cover this channel too. Invalid clocks or generations, an over-budget
record and queue or writer failures contribute to loss. Usual text diagnostics
retain their 512-character budget and classification.

- A `dns` header carries the normalized question `domain`, numeric `qtype`,
  `dns_id`, `policy` (direct or cluster), and the matched `rule` (default, exact
  or suffix). For a suffix match, `suffix_labels` counts its rightmost labels
  in the question. `source` distinguishes upstream, cluster, cache and local
  answers. `result` distinguishes answered, empty, truncated, negative
  (NXDOMAIN), refused, failed (SERVFAIL), other DNS errors and unanswered.
  A delivered reply includes its numeric `rcode`. Unanswered denotes a failed
  local response attempt. Malformed input before question admission has no
  question identity.
- Each `binding` carries the question's policy, domain, address and `ttl_s`.
  Join it to the header by boot identity, instance, PID and `dns_id`. All rows
  of one answer share its event time and generation. A and AAAA queries yield
  only reachable answer-section addresses, with TTL limited by alias lifetimes.
  A cached answer uses its remaining wire TTL. Validity runs from `unix_ms`
  up to, but excluding, `unix_ms + 1000 * ttl_s`. Downstream DNS caches may
  apply their own lifetimes. Background refresh emits no client answer. The
  header gives the total `binding_count` and number `omitted` beyond the first
  32. An unrepresentable answer sets `bindings_unknown`.
- A `route` carries `transport`, destination `address` and `port`, the decision
  `ground` and `policy`. A selected cluster records the chosen `front` and,
  for a two-hop passage, `exit`. The `selected` result records selection before
  connection establishment. A refused decision carries `refused`. Ordinary
  selection uses `scope=flow`. The existing rate-limited ICMP refusal path for
  individual UDP packets uses `scope=packet_sample`. Each `observed_name`
  comes from the destination name explicitly presented on that TCP connection.
  ECH outer names and ledger guesses cannot identify the connection's domain.
  Direct traffic outside Dynet's capture is unobserved.
- Cluster and node labels of up to 64 UTF-8 bytes containing letters, digits,
  space, underscore, dot, slash or hyphen can be projected. Otherwise
  `labels_omitted=true` marks omitted labels. Structured records never copy
  packet bodies, URLs, client addresses, query strings, keys or outbound
  connection parameters. Distinct domain bindings on a shared address remain
  separate time-qualified candidates.

The deployed gateway collector is an independent operator service. It retains
two native streams and two DNS frontend streams in bounded private RAM files.
Frontend query serials and original clients belong to dnsmasq; Dynet observes
the forwarder. Matching a name and time across those streams is an inference,
not an exact client-to-flow identifier. Collector restarts may replay the shared
syslog ring, and live exports may have rotation gaps or duplicates. Runtime
files survive worker restarts but disappear at router reboot.

## DNS alias ownership

A managed address lookup follows reachable CNAME and DNAME edges under the
original cluster. The resolver uses complete answers directly and asks for a
missing hop through that same cluster. It bounds chains to sixteen edges.
The published real IPv4 RRset belongs to the original question name and its
lifetime includes every alias on the path. Only its reachable addresses enter
the route ledger. Managed AAAA, HTTPS and SVCB retain the empty result.
Explicit alias inspection and unrepresentable metadata receive REFUSED.
Locally constructed answers clear AD; callers that require signed original
RRsets or canonical-name visibility can observe the projection.

Direct and named clusters contribute evidence at exact alias targets. Each
binding records the origin, observed cluster, generation and expiry. New
conflicting evidence is durably recorded before refusal. Recheck before
publishing an answer, including a memory hit or refresh, under the ledger
and policy locks that order configuration changes. Live historical origins are
also checked against the current rule table. New evidence or expiry can change
the decision. Independently obtained addresses and unobserved private DNS
caches remain outside this contract.

The private control directory retains origins/bindings and origins.enabled
beside the existing checkpoint. New installations and upgrades initialize
them on first evidence. An unreadable or missing initialized binding file
refuses startup. Preserve the entire directory for backup and recovery.
Existing address checkpoints retain their format for rollback. Older binaries
retain address custody but do not enforce alias ownership. New runtimes can
resume enforcement from unexpired bindings on return.
