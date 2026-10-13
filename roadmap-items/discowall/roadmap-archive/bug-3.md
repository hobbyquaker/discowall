## B-3. discoctl save fails under the shipped unit: /etc/discowall is read-only for the daemon (ProtectSystem=strict, DynamicUser) ✅

**Created** 2026-10-13.

**Found** 2026-10-13 in the Proxmox lab (task 11, cases `scale` and `upgrade`): `discoctl save` answers
`writing /etc/discowall/rules.toml.tmp: Permission denied`. The daemon writes the rules file, and under the shipped
unit it cannot: `ProtectSystem=strict` mounts `/etc` read-only, and even with a `ReadWritePaths=` the directory is
root's — `ConfigurationDirectory=` does not hand it to the `DynamicUser`. The namespace test never saw it because
there the daemon runs as the test's user with a rules file in a temp directory.

**Fix (2026-10-13):** the daemon never writes `/etc`. `discoctl save` asks for `RuleDump` (the chain rendered as
the rules file, plus the path the daemon reads it from), writes the file itself atomically — discoctl runs as the
person at the terminal, root for `/etc/discowall` — and sends `RuleSaved`, which clears the daemon's `unsaved`
flag. `RuleSave` (the daemon writes) stays in the protocol for a client that runs the daemon unsandboxed; the unit
test covers both paths, the namespace test and the lab's `scale`/`upgrade` cases the new one. The unit file is
unchanged: `ProtectSystem=strict` without a `ReadWritePaths=` is exactly right.

**Verified** 2026-10-13 in the lab: `discoctl save` works under the shipped unit (cases `scale` and `upgrade` save
through discoctl); unit test and namespace test cover the dump/write/ack path. ✅
