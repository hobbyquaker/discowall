## B-5. discowalld --check does not read the rules file: a broken rules.toml passes ✅

**Created** 2026-10-17.

**Found** 2026-10-17 while wiring discowall into a router's deploy script (task 18): the script validates
`discowall.toml` with `discowalld --check -c <file>` before installing it, and wanted to do the same for `rules.toml`.

**Measured** with the 0.0.2 release binary: a rules file containing only `default = "acept"` (typo), referenced by
`rules_file` in an otherwise valid config, gives `…: ok (5 interfaces)` and exit 0.

**Cause** (`crates/discowalld/src/main.rs`): `--check` runs `Config::load` and prints `ok`; the rules file is first
read when the daemon starts or on `discoctl apply`/SIGHUP. So a broken rule chain only shows up on the live router.

**Fix:**
- `--check` also loads and validates the rules file the config names (same code path as start and apply), prints
  the rule count and the default policy, and exits non-zero with the parse error if it fails;
- a missing rules file is not an error but is said explicitly (`no rules file: default drop, nothing crosses`),
  because that is what the daemon would do;
- tests for: valid file, syntax error, unknown action/kind/rrtype, missing file.

Optional: `--check --rules <file>` to validate a rules file on its own, so that a deploy script can check a new
`rules.toml` without a temporary config.

**Fixed 2026-10-17.** `discowalld --check` also runs `check_rules` (`reflector.rs`): the same `load_rules` +
`Chain::compile` as start and `apply`, printing `<file>: ok (N rules, default <policy>)`, or
`<file>: no rules file: default drop, nothing crosses`, or the error with exit 1. The optional
`--check --rules <file>` checks a rules file alone without reading a configuration. **Measured** with the built
binary: the item's reproduction (`default = "acept"`) now exits 1 with `unknown variant \`acept\``; `contrib/rules.toml`
gives `ok (5 rules, default drop)`; a missing file is reported and exits 0. Unit test: valid, missing, a typo in
default, kind and action, an unknown rrtype, an invalid prefix, a TOML syntax error. ✅ 2026-10-17.
