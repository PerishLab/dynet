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

Publish from the trusted operator's private declaration using the released
Secrets CLI. Its declaration binds target, public recipient, local signer and
remote connection; the recipient is not learned from API inventory.

```sh
secrets publish --declaration /private/publication.json --generation 2 --input -
secrets seal --declaration /private/publication.json --generation 2 --input - --output /private/routing.envelope
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
Application DoH, independently obtained IPv6 and new UDP capabilities are
separate work.

Cutover is an explicit operation, not a compatibility layer. Before first start,
stop the old service while retaining its protection, preserve its issued-address
ledger into the new checkpoint through an offline owned migration, provision
private enrollment and publish the complete encrypted configuration. Verify the
new service and its exact distribution, then delete the former plaintext
manifest and obsolete files. A remaining `<state>/<instance>.ledger` refuses a
new start so a missed migration cannot silently release issued addresses. There
is no old-format reader, historical-directory discovery or plaintext fallback.
