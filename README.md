# Dynet

Dynet is the operator's personal VPN management tool. Traffic enters through a
TUN device with real DNS and leaves through commercial subscription nodes used
concurrently, routed to a pool rather than a single node. It defines no wire
protocol of its own and runs nothing on the far side. `AGENTS.md` carries the
product boundary.

On Windows, run elevated with a private manifest file readable by SYSTEM:

```powershell
dynet --instance dynet0 service install --manifest C:\ProgramData\dynet\manifest.json
Start-Service dynet0
dynet --instance dynet0 doctor
dynet --instance dynet0 standing
dynet --instance dynet0 down
```

The service starts at boot and restarts after failure. `down` stops it and
restores direct connectivity; `service remove` also removes its registration.
A killed process retains DNS and destination protection until restart or
explicit teardown. Its ledger and log are in `%ProgramData%\dynet`.
Protect the manifest's ACL because it contains node credentials.

Windows supports this host's IPv4 traffic. Claimed names receive no AAAA or
HTTPS/SVCB hints; unclaimed names retain their upstream answers. Applications
must use system DNS: arbitrary application DoH and externally acquired IPv6
addresses are outside the current capture boundary.
