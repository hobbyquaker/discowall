# Agent instructions for discowall

discowall (until the Q&A of 2026-10-10: mdns-router; the directory is still `~/repos/mdns-router` until the
maintainer renames it) is a **discovery firewall**: a tool for a router that forwards mDNS (RFC 6762) traffic — later
also SSDP — between interfaces — what
avahi-daemon's reflector does — but under rules: which hosts and services may be seen from which interface,
whitelist or blacklist, source- and target-based, a little like iptables for `.local`. A daemon plus a CLI that
is good for scripts. Rust (D-1). The concept and the research are in
[mdns-router-research.md](mdns-router-research.md) (2026-10-10, its section 9 holds the Q&A outcome); the decisions
D-1…D-9 are in `roadmap-items/discowall/decisions.md`. Daemon `discowalld`, CLI `discoctl`. Code on
[github.com/hobbyquaker/discowall](https://github.com/hobbyquaker/discowall); the cold-start section of
`roadmap-items/discowall/HANDOFF.md` says where everything is.

**This repository holds both the code (later) and the agent working documents**, in the same shape as the
maintainer's agents repositories (`~/repos/openccu-lite-agents`, `~/repos/she-agent`): the index in
`roadmap-readmes/discowall.md`, everything else in `roadmap-items/discowall/`. The working documents stay in the
repository when it goes public (D-9), so **nothing lab-specific goes in**.

**Read [roadmap-readmes/discowall.md](roadmap-readmes/discowall.md) before making changes**, and work on
the task you were given, not next to it. The readme lists every task and bug by its header line and state;
each item's text is its own file (see *Roadmap items* below). What the repository is, milestones and open
questions **OQ-n** are in [roadmap-items/discowall/overview.md](roadmap-items/discowall/overview.md).
Decisions are **D-n** in [roadmap-items/discowall/decisions.md](roadmap-items/discowall/decisions.md); a
decision is only changed by the maintainer, and the change is recorded there with its date. Current state for
a cold start is in [roadmap-items/discowall/HANDOFF.md](roadmap-items/discowall/HANDOFF.md) (the newest
five dated entries; older ones in `roadmap-items/discowall/handoff-archive/<date>.md`); refresh it when
something changes, and move entries older than the newest five to the archive.

**Never `git add -A` or `git add .`** — name the paths. Nothing that is not a document or code of this
repository belongs in its working directory. Nothing is pushed, tagged or released without the maintainer's
word.

## Roadmap items: tasks and bugs

Tasks and bugs are **one file per item**: open ones in `roadmap-items/discowall/roadmap/`, done ones in
`roadmap-items/discowall/roadmap-archive/`, and nothing else in those two folders.
`roadmap-readmes/discowall.md` is **generated** from them by [scripts/roadmap.py](scripts/roadmap.py): the
items' header lines, newest first, tasks and bugs together, each with a link to its file — nothing more.

- **Tasks** are `task-<N>.md` with the first line `## <N>. <Title> <state>`; **bugs** are `bug-<N>.md` with
  `## B-<N>. <Title> <state>`. Numbers are per type, stable, never reused.
- **The state is the emoji at the very end of the first line**, exactly one:

  | State (for the script) | Emoji | Meaning |
  | --- | --- | --- |
  | `postponed` | 💤 | not now: waits for hardware, the maintainer, or simply later |
  | `blocked` | 🚧 | blocked by another task or another project's release |
  | `questionable` | 🚫 | questionable, do not implement |
  | `refine` | 🤷 | to be refined: open questions, research |
  | `ready` | ☑️ | implementation ready: decided, the shape clear |
  | `enqueued` | 📥 | ready and placed in a lane's queue by the coordinator |
  | `progress` | 🏗️ | in progress |
  | `testing` | 🕵️ | built; waiting for or undergoing tests |
  | `done` | ✅ | done — the only state that lives in `roadmap-archive/` |

  **The flag 🏆** may follow the state: the item needs Fable. The mapping lives in one place, `STATES` in the script.
- **The title is short.** The ask, "(maintainer, date)", the status, commits and test results go into the item's body.
- `roadmap-items/discowall/notes/` holds what is neither an item nor the overview: studies, appendices, backlogs.
- **The README's Roadmap section** (`README.md` English, `README.de.md` German, the same list in both) is the public
  summary of the items, one line per item with its state emoji, hand-written as in hm-radio-simulator. Whoever
  changes an item's state with `roadmap.py state` updates that line in both READMEs in the same commit.

**The rules:**
- **Create items only with** `python3 scripts/roadmap.py new discowall task|bug "<Title>" [<state>] [--fable]`,
  then write the body.
- **Change a state only with** `python3 scripts/roadmap.py state discowall <N|B-N> <state>` — it moves the file
  with `git mv` when the state becomes or stops being done, keeps the 🏆 flag, and regenerates the readme — and the
  flag only with `python3 scripts/roadmap.py fable discowall <N|B-N> on|off`.
- **Never hand-edit `roadmap-readmes/`.** `python3 scripts/roadmap.py generate discowall` writes it.
- **Run `python3 scripts/roadmap.py check discowall` before committing roadmap changes.**
- **An agent working on a roadmap item keeps its state live** (🏗️ when it starts, 🕵️ when built, ✅ when done, 💤/🚧/🚫
  with the reason in the item file when it stops), and commits each change at once — never batched at the end.
  Details — commits, test results, pending checks — go into the item file, never into the readme.
- `python3 scripts/test_roadmap.py` tests the script; change the script only together with its tests. The script
  is a copy of `openccu-lite-agents/scripts/roadmap.py` (2026-10-10); a fix made here is made there as well.

## Code rules (once there is code)

- Rust, stable toolchain, `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` green before every
  commit; CI runs the same. Integration tests use Linux network namespaces (`ip netns`, veth pairs) so that real
  multicast crosses real interfaces; they are skipped, not failed, without the capability.
- **Nothing lab-specific goes into the repository:** no addresses, host names, credentials, box names.
- Behaviour that imitates or interoperates with another implementation (avahi, Apple's mDNSResponder, umdns) is
  documented as *measured* (what, where, when) or as read from the RFC — never guessed.
