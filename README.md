# Dynet

Dynet is the operator's personal VPN management tool. Traffic enters through a
TUN device with real DNS and leaves through commercial subscription nodes used
concurrently, routed to a pool rather than a single node. It defines no wire
protocol of its own and runs nothing on the far side. `AGENTS.md` carries the
product boundary.

Configuration is a complete signed, age-encrypted document loaded from a file
or the Secrets API. [DESIGN.md](DESIGN.md) describes enrollment, bootstrap,
publication, offline recovery and explicit state migration. Existing plaintext
configuration is not accepted.

On Windows, run elevated with a machine-private bootstrap and key references:

```powershell
dynet --instance dynet0 service install --bootstrap C:\ProgramData\dynet\bootstrap.json
Start-Service dynet0
dynet --instance dynet0 doctor
dynet --instance dynet0 standing
dynet --instance dynet0 down
```

The service starts at boot and restarts after failure. `down` stops it and
restores direct connectivity; `service remove` also removes its registration.
A killed process retains DNS and destination protection until restart or
explicit teardown. Its encrypted configuration checkpoint, issued-address ledger and log live in
`%ProgramData%\dynet`. Keep that directory and the bootstrap/key ACL private to
Administrators and LocalSystem.

Windows supports this host's IPv4 traffic. Claimed names receive no AAAA or
HTTPS/SVCB hints; unclaimed names retain their upstream answers. Applications
must use system DNS: arbitrary application DoH and externally acquired IPv6
addresses are outside the current capture boundary.

On macOS Apple Silicon, install machine binaries with the manager's explicit
version root `/Library/Dynet/versions` and bin root `/Library/Dynet/bin`.
Keep the bootstrap and referenced key root-owned and mode 600, with a private
state directory, then use the installed executable:

```sh
sudo /Library/Dynet/bin/dynet service install --bootstrap "/Library/Application Support/Dynet/bootstrap.json"
sudo /Library/Dynet/bin/dynet doctor
sudo /Library/Dynet/bin/dynet standing
sudo /Library/Dynet/bin/dynet service restart
sudo /Library/Dynet/bin/dynet service stop
sudo /Library/Dynet/bin/dynet service start
sudo /Library/Dynet/bin/dynet service uninstall
```

The LaunchDaemon starts at boot and restarts failures. State and the service
log live under `/Library/Application Support/Dynet`; `--state` selects one
absolute alternative. Explicit stop/down restores owned DNS settings and
removes capture protection. A killed worker retains PF protection for ordinary
user applications until recovery or explicit teardown. Root-owned sockets
are exempt, because macOS PF identifies users rather than executable paths.
Chained UDP and arbitrary application DoH are outside this capture boundary.
