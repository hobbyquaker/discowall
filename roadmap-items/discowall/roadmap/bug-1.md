## B-1. Log output carries ANSI colour codes when stdout is not a terminal (journal under systemd) 🏗️

**Created** 2026-10-13.

**Found** 2026-10-13 in the Proxmox lab (task 11, case 1): `journalctl -u discowalld -o cat` shows `[32m INFO[0m …`
— tracing-subscriber's fmt layer colours by default and does not check whether stdout is a terminal. systemd's
journal keeps the bytes; `journalctl` shows them as garbage unless the pager strips them.

**Fix:** `.with_ansi(std::io::stdout().is_terminal())` in `main.rs`; colours stay on when a person runs the
daemon in a terminal and go off under systemd, in a pipe or a file.
