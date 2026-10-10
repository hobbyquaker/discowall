#!/usr/bin/env python3
"""Roadmap bookkeeping for the agents repository.

Every project under roadmap-items/<project>/ keeps its tasks and bugs as one file each:

    roadmap/task-<N>.md, roadmap/bug-<N>.md                  open items (every state except done)
    roadmap-archive/task-<N>.md, roadmap-archive/bug-<N>.md  done items

The first line of an item is its header. It ends with the state emoji, optionally followed by
the Fable flag (the item needs Fable):

    ## 107. Addon file ownership is fixed in the unit before every start ✅
    ## B-119. a confined addon's rc.d script still runs as root 🤷 🏆

roadmap-readmes/<project>.md is generated from those header lines and holds nothing else. Both of
its lists are newest first, tasks and bugs interleaved: newest is the time the item's path was first
added in git, following renames (a staged `git mv` included), or the file's mtime while it has no
commit; ties go to the higher number, then tasks before bugs.

Commands:
    generate [<project>...|--all]                        write the readmes
    state <project> <id> <state>                         set the state (moves the file on done / not done)
    fable <project> <id> on|off                          set or remove the Fable flag
    new <project> task|bug "<Title>" [<state>] [--fable] create an item with the next free number
    check [<project>...|--all]                           verify items and readmes (exit 1 on a problem)

<id> is 107, 4a, B-119, or a path. Standard library only. It never commits; agents commit.
"""

import argparse
import datetime
import os
import re
import subprocess
import sys
from pathlib import Path

VS16 = "\N{VARIATION SELECTOR-16}"

# The one place that maps state names to their emojis. Change a state's emoji here only.
# An emoji is written exactly as given here (with its VS16 where it has one) and recognised
# with or without the VS16.
STATES = {
    "postponed": "\N{SLEEPING SYMBOL}",  # 💤 not now: hardware, the maintainer, later
    "blocked": "\N{CONSTRUCTION SIGN}",  # 🚧 blocked by another task or another project's release
    "questionable": "\N{NO ENTRY SIGN}",  # 🚫 questionable, do not implement
    "refine": "\N{SHRUG}",  # 🤷 to be refined: open questions, research
    "ready": "\N{BALLOT BOX WITH CHECK}" + VS16,  # ☑️ implementation ready: decided, shape clear
    "enqueued": "\N{INBOX TRAY}",  # 📥 ready and placed in a lane's queue (the coordinator's plan)
    "progress": "\N{BUILDING CONSTRUCTION}" + VS16,  # 🏗️ in progress
    "testing": "\N{SLEUTH OR SPY}" + VS16,  # 🕵️ built; waiting for or undergoing tests and lab checks
    "done": "\N{WHITE HEAVY CHECK MARK}",  # ✅ done: the only state that lives in roadmap-archive/
}
DONE = "done"
DEFAULT_NEW_STATE = "refine"

# The Fable flag, independent of the state: the item needs Fable (waits for the Fable limit).
FLAG = "\N{TROPHY}"  # 🏆

# Emojis that were state markers once. Recognised, so `state` replaces them and `check`
# names them; never written.
RETIRED = [
    "\N{MEMO}",  # 📝
    "\N{LARGE GREEN CIRCLE}",  # 🟢
    "\N{GEAR}",  # ⚙
    "\N{ROBOT FACE}",  # 🤖
]

TYPES = ("task", "bug")
OPEN_DIR = "roadmap"
ARCHIVE_DIR = "roadmap-archive"
ITEMS_DIR = "roadmap-items"
READMES_DIR = "roadmap-readmes"

FILE_RE = re.compile(r"^(task|bug)-(\d+)([a-z]?)\.md$")
HEADER_RE = {
    "task": re.compile(r"^## (\d+[a-z]?)\. (\S.*)$"),
    "bug": re.compile(r"^## B-(\d+[a-z]?)\. (\S.*)$"),
}


def _base(emoji):
    return emoji.replace(VS16, "")


_BASE_TO_STATE = {_base(e): name for name, e in STATES.items()}
_RETIRED_BASES = [_base(e) for e in RETIRED]
assert len(_BASE_TO_STATE) == len(STATES), "two states share an emoji"
assert all(len(b) == 1 for b in list(_BASE_TO_STATE) + _RETIRED_BASES), "a state emoji must be one code point"
assert _base(FLAG) not in _BASE_TO_STATE and _base(FLAG) not in _RETIRED_BASES

# One marker: the base emoji, then skin-tone modifiers or VS16, then an optional ZWJ with a
# gender sign and its VS16 (a gendered 🤷). The whole run counts as one marker.
MARKER_RE = re.compile(
    "["
    + "".join(re.escape(b) for b in list(_BASE_TO_STATE) + _RETIRED_BASES)
    + "](?:[\U0001F3FB-\U0001F3FF]|"
    + VS16
    + ")*(?:\N{ZERO WIDTH JOINER}[\N{FEMALE SIGN}\N{MALE SIGN}]"
    + VS16
    + "?)?"
)
FLAG_RE = re.compile(re.escape(_base(FLAG)) + VS16 + "?")


class RoadmapError(Exception):
    pass


# --- header lines ------------------------------------------------------------------------


def markers(line):
    """The state markers in a line: a list of (match, state name, or None for a retired emoji)."""
    return [(m, _BASE_TO_STATE.get(m.group(0)[0])) for m in MARKER_RE.finditer(line)]


def has_flag(line):
    return FLAG_RE.search(line) is not None


def _strip(regex, line):
    return re.sub(r"[ \t]*(?:" + regex.pattern + ")", "", line).rstrip()


def strip_markers(line):
    """The line without state markers and without the Fable flag."""
    return _strip(FLAG_RE, _strip(MARKER_RE, line))


def with_state(line, state):
    """The header line with exactly one marker for `state`; the Fable flag stays as it was."""
    return strip_markers(line) + " " + STATES[state] + (" " + FLAG if has_flag(line) else "")


def with_flag(line, on):
    return _strip(FLAG_RE, line) + (" " + FLAG if on else "")


def header_problems(first, kind, ident):
    """What is wrong with an item's first line; an empty list when it is right."""
    problems = []
    m = HEADER_RE[kind].match(first)
    form = ("## <N>. " if kind == "task" else "## B-<N>. ") + "<Title> <state emoji> [" + FLAG + "]"
    if not m:
        return [f"the first line is not `{form}`: {first!r}"]
    if m.group(1) != ident:
        problems.append(f"the header's number {m.group(1)} is not the file's {ident}")
    rest = first
    flags = list(FLAG_RE.finditer(first))
    if len(flags) > 1:
        problems.append(f"{len(flags)} {FLAG} flags on the first line")
    elif flags:
        f = flags[0]
        if f.end() != len(first):
            problems.append(f"the {FLAG} flag is not at the end of the first line")
        elif f.group(0) != FLAG:
            problems.append(f"the {FLAG} flag is not written as {FLAG!r}; set it again")
        elif not first[: f.start()].endswith(" ") or first[: f.start()].endswith("  "):
            problems.append(f"the {FLAG} flag needs exactly one space before it")
        rest = first[: f.start()].rstrip(" ")
    found = markers(rest)
    if not found:
        problems.append("no state emoji on the first line" + (f" before the {FLAG} flag" if flags else ""))
        return problems
    if len(found) > 1:
        problems.append(f"{len(found)} state emojis on the first line")
        return problems
    mk, name = found[0]
    if name is None:
        problems.append(f"a retired state emoji {mk.group(0)!r}; set the state again")
    elif mk.end() != len(rest):
        problems.append(f"the state emoji is not the last character of the first line (only {FLAG} may follow it)")
    elif mk.group(0) != STATES[name]:
        problems.append(f"the {name} emoji is not written as {STATES[name]!r}; set the state again")
    elif not rest[: mk.start()].endswith(" ") or rest[: mk.start()].endswith("  "):
        problems.append("the state emoji needs exactly one space before it")
    elif strip_markers(m.group(2)) == "":
        problems.append("the title is empty")
    return problems


def state_of(line):
    """The state name of a well-formed header line (the flag allowed after it), else None."""
    rest = _strip(FLAG_RE, line)
    found = markers(rest)
    if len(found) != 1:
        return None
    mk, name = found[0]
    return name if name and mk.end() == len(rest) else None


# --- files -------------------------------------------------------------------------------


class Item:
    def __init__(self, project, folder, path):
        self.project = project
        self.folder = folder  # OPEN_DIR or ARCHIVE_DIR
        self.path = path
        m = FILE_RE.match(path.name)
        self.kind = m.group(1) if m else None
        self.number = int(m.group(2)) if m else None
        self.suffix = m.group(3) if m else ""
        self.id = (m.group(2) + m.group(3)) if m else None
        self._first = None

    @property
    def first_line(self):
        if self._first is None:
            with open(self.path, encoding="utf-8") as f:
                self._first = f.readline().rstrip("\r\n")
        return self._first

    def label(self):
        return f"{ITEMS_DIR}/{self.project}/{self.folder}/{self.path.name}"


def project_dir(root, project):
    return root / ITEMS_DIR / project


def projects(root):
    base = root / ITEMS_DIR
    if not base.is_dir():
        return []
    return sorted(p.name for p in base.iterdir() if p.is_dir() and not p.name.startswith("."))


def scan(root, project):
    """Every file in the project's two folders, as Items (files with other names included)."""
    items = []
    for folder in (OPEN_DIR, ARCHIVE_DIR):
        d = project_dir(root, project) / folder
        if not d.is_dir():
            continue
        for p in sorted(d.iterdir()):
            if p.is_file() and not p.name.startswith("."):  # .gitkeep and the like are not items
                items.append(Item(project, folder, p))
    return items


def _git(root, *args):
    try:
        r = subprocess.run(
            ["git", "-C", str(root), "-c", "core.quotePath=false", *args],
            capture_output=True,
            text=True,
        )
    except OSError:
        return None
    return r.stdout if r.returncode == 0 else None


_HISTORY = {}  # (root, HEAD) -> {path: first-added time}; the history does not change within a run


def _first_added(root):
    """Path -> the author time of the commit that first added it, following renames.

    One pass over the whole history (renames from outside roadmap-items/ included), then the
    renames staged in the index, so a `git mv` not yet committed keeps the item's time."""
    head = _git(root, "rev-parse", "--verify", "-q", "HEAD")
    key = (str(root), head)
    if key not in _HISTORY:
        first = {}
        out = _git(root, "log", "--reverse", "--format=@%at", "--name-status", "-M") if head else None
        ts = 0
        for line in (out or "").splitlines():
            if line.startswith("@"):
                ts = int(line[1:])
                continue
            parts = line.split("\t")
            if len(parts) == 3 and parts[0][:1] == "R":
                first[parts[2]] = first.get(parts[1], ts)
            elif len(parts) == 3 and parts[0][:1] == "C":
                first.setdefault(parts[2], ts)
            elif len(parts) == 2 and parts[0] == "A":
                first.setdefault(parts[1], ts)
        _HISTORY[key] = first
    first = dict(_HISTORY[key])
    staged = _git(root, "diff", "--cached", "--name-status", "-M", "HEAD") if head else None
    for line in (staged or "").splitlines():
        parts = line.split("\t")
        if len(parts) == 3 and parts[0][:1] == "R" and parts[1] in first:
            first[parts[2]] = first[parts[1]]
    return first


def readme_text(root, project):
    items = [i for i in scan(root, project) if i.kind]
    first = _first_added(root)

    def created(i):
        rel = i.path.relative_to(root).as_posix()
        return first[rel] if rel in first else int(i.path.stat().st_mtime)

    def ordered(folder):
        return sorted(
            (i for i in items if i.folder == folder),
            key=lambda i: (-created(i), -i.number, TYPES.index(i.kind), -ord(i.suffix) if i.suffix else 0),
        )

    def line(i):
        text = re.sub(r"^#+\s*", "", i.first_line)
        return f"- {text} — [{i.path.name}](../{ITEMS_DIR}/{project}/{i.folder}/{i.path.name})"

    def section(title, rows):
        body = f"## {title}\n"
        if rows:
            body += "\n" + "\n".join(line(i) for i in rows) + "\n"
        return body

    return (
        f"# {project} roadmap\n\n"
        + section("Roadmap", ordered(OPEN_DIR))
        + "\n"
        + section("Archive", ordered(ARCHIVE_DIR))
    )


def readme_path(root, project):
    return root / READMES_DIR / f"{project}.md"


def write_readme(root, project):
    path = readme_path(root, project)
    text = readme_text(root, project)
    old = path.read_text(encoding="utf-8") if path.exists() else None
    if old == text:
        return False
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return True


def _tracked(root, path):
    try:
        r = subprocess.run(
            ["git", "-C", str(root), "ls-files", "--error-unmatch", "--", str(path)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError:
        return False
    return r.returncode == 0


def _move(root, src, dst):
    if dst.exists():
        raise RoadmapError(f"{dst} already exists")
    dst.parent.mkdir(parents=True, exist_ok=True)
    if _tracked(root, src):
        r = subprocess.run(["git", "-C", str(root), "mv", "--", str(src), str(dst)])
        if r.returncode != 0:
            raise RoadmapError(f"git mv {src} {dst} failed ({r.returncode})")
    else:
        os.rename(src, dst)


def _require_project(root, project):
    if not project_dir(root, project).is_dir():
        raise RoadmapError(f"unknown project {project!r} (no {ITEMS_DIR}/{project}/)")


def resolve(root, project, ident):
    """The Item for `107`, `4a`, `B-119`, `task-107`, `bug-119` or a path."""
    _require_project(root, project)
    if "/" in ident or ident.endswith(".md"):
        p = Path(ident)
        if not p.is_absolute():
            p = (Path.cwd() / p) if (Path.cwd() / p).exists() else (root / p)
        p = p.resolve()
        for item in scan(root, project):
            if item.path.resolve() == p:
                if not item.kind:
                    raise RoadmapError(f"{ident} is not a task-<N>.md or bug-<N>.md file")
                return item
        raise RoadmapError(f"{ident} is not an item of {project}")
    m = re.fullmatch(r"(?:(b|bug)-?|task-)?(\d+[a-z]?)", ident.strip(), re.IGNORECASE)
    if not m:
        raise RoadmapError(f"cannot read the id {ident!r} (use 107, B-119 or a path)")
    kind = "bug" if m.group(1) else "task"
    number = m.group(2).lower()
    found = [i for i in scan(root, project) if i.kind == kind and i.id == number]
    if not found:
        raise RoadmapError(f"no {kind} {number} in {project}")
    if len(found) > 1:
        raise RoadmapError(f"{kind} {number} is in both folders of {project}")
    return found[0]


def _state_name(state):
    if state in STATES:
        return state
    for name, emoji in STATES.items():
        if state and _base(state) == _base(emoji):
            return name
    raise RoadmapError(f"unknown state {state!r}; one of: {', '.join(STATES)}")


def _write_keeping_mtime(path, text, stat):
    # The mtime orders an item that has no commit yet; a state change must not make it "newer".
    path.write_text(text, encoding="utf-8")
    os.utime(path, ns=(stat.st_atime_ns, stat.st_mtime_ns))


# --- commands ----------------------------------------------------------------------------


def cmd_generate(root, names):
    changed = []
    for project in names:
        _require_project(root, project)
        if write_readme(root, project):
            changed.append(readme_path(root, project))
    return changed


def cmd_state(root, project, ident, state):
    state = _state_name(state)
    item = resolve(root, project, ident)
    new_first = with_state(item.first_line, state)
    folder = ARCHIVE_DIR if state == DONE else OPEN_DIR
    target = project_dir(root, project) / folder / item.path.name
    text = item.path.read_text(encoding="utf-8")
    stat = item.path.stat()
    if target != item.path:
        _move(root, item.path, target)
    _, sep, rest = text.partition("\n")
    _write_keeping_mtime(target, new_first + (sep or "\n") + rest, stat)
    write_readme(root, project)
    return new_first, target


def cmd_fable(root, project, ident, on):
    if on not in ("on", "off", True, False):
        raise RoadmapError(f"the flag is on or off, not {on!r}")
    item = resolve(root, project, ident)
    new_first = with_flag(item.first_line, on in ("on", True))
    text = item.path.read_text(encoding="utf-8")
    _, sep, rest = text.partition("\n")
    _write_keeping_mtime(item.path, new_first + (sep or "\n") + rest, item.path.stat())
    write_readme(root, project)
    return new_first, item.path


def cmd_new(root, project, kind, title, state=None, fable=False):
    _require_project(root, project)
    if kind not in TYPES:
        raise RoadmapError(f"the type is task or bug, not {kind!r}")
    state = _state_name(state or DEFAULT_NEW_STATE)
    title = strip_markers(" ".join(title.split()))
    if not title:
        raise RoadmapError("the title is empty")
    folder = ARCHIVE_DIR if state == DONE else OPEN_DIR
    pdir = project_dir(root, project)
    (pdir / folder).mkdir(parents=True, exist_ok=True)
    n = max((i.number for i in scan(root, project) if i.kind == kind), default=0) + 1
    today = datetime.date.today().isoformat()
    while True:
        header = f"## {'B-' if kind == 'bug' else ''}{n}. {title} {STATES[state]}" + (f" {FLAG}" if fable else "")
        name = f"{kind}-{n}.md"
        path = pdir / folder / name
        if any((pdir / d / name).exists() for d in (OPEN_DIR, ARCHIVE_DIR)):
            n += 1
            continue
        try:
            with open(path, "x", encoding="utf-8") as f:
                f.write(f"{header}\n\n**Created** {today}.\n")
        except FileExistsError:  # another agent took the number a moment ago
            n += 1
            continue
        break
    write_readme(root, project)
    return header, path


def check_project(root, project):
    problems = []
    seen = {}
    for item in scan(root, project):
        where = item.label()
        if not item.kind:
            problems.append(f"{where}: not an item file (task-<N>.md or bug-<N>.md)")
            continue
        found = header_problems(item.first_line, item.kind, item.id)
        problems += [f"{where}: {p}" for p in found]
        name = state_of(item.first_line)
        if not found and name:
            if name == DONE and item.folder != ARCHIVE_DIR:
                problems.append(f"{where}: done but not in {ARCHIVE_DIR}/")
            if name != DONE and item.folder == ARCHIVE_DIR:
                problems.append(f"{where}: in {ARCHIVE_DIR}/ but {name}, not done")
        key = (item.kind, item.id)
        if key in seen:
            problems.append(f"{where}: {item.kind} {item.id} again (also {seen[key]})")
        else:
            seen[key] = where
    path = readme_path(root, project)
    current = path.read_text(encoding="utf-8") if path.exists() else None
    if current != readme_text(root, project):
        problems.append(f"{READMES_DIR}/{project}.md: differs from the items (run generate)")
    return problems


def cmd_check(root, names):
    problems = []
    for project in names:
        _require_project(root, project)
        problems += check_project(root, project)
    return problems


# --- CLI ---------------------------------------------------------------------------------


def _names(root, args):
    if args.all and args.projects:
        raise RoadmapError("give projects or --all, not both")
    if args.all:
        return projects(root)
    if not args.projects:
        raise RoadmapError("give one or more projects, or --all")
    return args.projects


def main(argv=None):
    default_root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(
        prog="roadmap.py",
        description="Roadmap items and the generated readmes. States: "
        + ", ".join(f"{n} {e}" for n, e in STATES.items())
        + f"; the flag {FLAG} means the item needs Fable.",
    )
    parser.add_argument("--root", type=Path, default=default_root, help="the agents repository (default: %(default)s)")
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("generate", help="write roadmap-readmes/<project>.md from the items")
    p.add_argument("projects", nargs="*")
    p.add_argument("--all", action="store_true")

    p = sub.add_parser("state", help="set an item's state; moves it between roadmap/ and roadmap-archive/")
    p.add_argument("project")
    p.add_argument("id", help="107, B-119, or a path")
    p.add_argument("state", help=", ".join(STATES))

    p = sub.add_parser("fable", help=f"set (on) or remove (off) the {FLAG} flag: the item needs Fable")
    p.add_argument("project")
    p.add_argument("id", help="107, B-119, or a path")
    p.add_argument("flag", choices=("on", "off"))

    p = sub.add_parser("new", help="create an item with the next free number")
    p.add_argument("project")
    p.add_argument("type", choices=TYPES)
    p.add_argument("title")
    p.add_argument("state", nargs="?", default=None, help=f"default {DEFAULT_NEW_STATE}")
    p.add_argument("--fable", action="store_true", help=f"set the {FLAG} flag")

    p = sub.add_parser("check", help="verify items and readmes; exit 1 on a problem")
    p.add_argument("projects", nargs="*")
    p.add_argument("--all", action="store_true")

    args = parser.parse_args(argv)
    root = args.root.resolve()
    try:
        if args.command == "generate":
            for path in cmd_generate(root, _names(root, args)):
                print(f"wrote {path.relative_to(root)}")
        elif args.command in ("state", "fable", "new"):
            if args.command == "state":
                header, path = cmd_state(root, args.project, args.id, args.state)
            elif args.command == "fable":
                header, path = cmd_fable(root, args.project, args.id, args.flag)
            else:
                header, path = cmd_new(root, args.project, args.type, args.title, args.state, args.fable)
            print(header)
            print(path.relative_to(root))
        elif args.command == "check":
            problems = cmd_check(root, _names(root, args))
            if problems:
                print(f"{len(problems)} problem(s):", file=sys.stderr)
                for line in problems:
                    print(f"  {line}", file=sys.stderr)
                return 1
            print("ok")
    except RoadmapError as e:
        print(f"roadmap.py: {e}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
