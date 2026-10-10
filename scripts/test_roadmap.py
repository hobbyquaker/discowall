#!/usr/bin/env python3
"""Tests for scripts/roadmap.py: python3 scripts/test_roadmap.py"""

import contextlib
import datetime
import importlib.util
import io
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("roadmap", HERE / "roadmap.py")
roadmap = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(roadmap)

S = roadmap.STATES
FLAG = roadmap.FLAG
VS16 = "\N{VARIATION SELECTOR-16}"
ZWJ = "\N{ZERO WIDTH JOINER}"
SHRUG = "\N{SHRUG}"


class Fixture(unittest.TestCase):
    project = "p"

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="roadmap-test-")
        self.root = Path(self.tmp)
        self.pdir = self.root / "roadmap-items" / self.project
        (self.pdir / "roadmap").mkdir(parents=True)
        (self.pdir / "roadmap-archive").mkdir()
        (self.root / "roadmap-readmes").mkdir()

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def item(self, folder, name, first, body="\nThe body stays.\n", mtime=None):
        path = self.pdir / folder / name
        path.write_text(first + "\n" + body, encoding="utf-8")
        if mtime is not None:
            os.utime(path, (mtime, mtime))
        return path

    def first(self, path):
        return path.read_text(encoding="utf-8").split("\n", 1)[0]

    def run_main(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = roadmap.main(["--root", str(self.root), *argv])
        return code, out.getvalue(), err.getvalue()

    def readme(self):
        return (self.root / "roadmap-readmes" / f"{self.project}.md").read_text(encoding="utf-8")

    def problems(self):
        return roadmap.check_project(self.root, self.project)


class TestMarkers(unittest.TestCase):
    def test_a_marker_with_its_selector_counts_once(self):
        self.assertEqual(len(roadmap.markers("## 1. A " + S["ready"])), 1)
        self.assertEqual(len(roadmap.markers("## 1. A \N{BALLOT BOX WITH CHECK}")), 1)
        self.assertEqual(len(roadmap.markers("## 1. A " + S["progress"])), 1)
        self.assertEqual(roadmap.state_of("## 1. A \N{BUILDING CONSTRUCTION}"), "progress")

    def test_shrug_variants_are_refine(self):
        for variant in (
            SHRUG,
            SHRUG + VS16,
            SHRUG + "\N{EMOJI MODIFIER FITZPATRICK TYPE-4}",
            SHRUG + ZWJ + "\N{FEMALE SIGN}" + VS16,
            SHRUG + "\N{EMOJI MODIFIER FITZPATRICK TYPE-6}" + ZWJ + "\N{MALE SIGN}" + VS16,
            SHRUG + ZWJ + "\N{MALE SIGN}",
        ):
            line = "## B-3. Something " + variant
            self.assertEqual(len(roadmap.markers(line)), 1, repr(variant))
            self.assertEqual(roadmap.state_of(line), "refine", repr(variant))
            self.assertEqual(roadmap.with_state(line, "refine"), "## B-3. Something " + SHRUG)

    def test_with_state_keeps_the_flag(self):
        self.assertEqual(roadmap.with_state("## 1. A " + S["refine"] + " " + FLAG, "ready"), "## 1. A " + S["ready"] + " " + FLAG)
        self.assertEqual(roadmap.with_state("## 1. A", "done"), "## 1. A " + S["done"])
        self.assertEqual(roadmap.state_of("## 1. A " + S["ready"] + " " + FLAG), "ready")
        self.assertEqual(roadmap.state_of("## 1. A " + S["enqueued"] + " " + FLAG), "enqueued")
        self.assertEqual(roadmap.with_state("## 1. A " + S["ready"] + " " + FLAG, "enqueued"), "## 1. A " + S["enqueued"] + " " + FLAG)

    def test_testing_variants(self):
        spy = "\N{SLEUTH OR SPY}"
        for variant in (
            spy,
            spy + VS16,
            spy + "\N{EMOJI MODIFIER FITZPATRICK TYPE-3}",
            spy + VS16 + ZWJ + "\N{FEMALE SIGN}" + VS16,
            spy + "\N{EMOJI MODIFIER FITZPATRICK TYPE-5}" + ZWJ + "\N{MALE SIGN}" + VS16,
        ):
            line = "## 98. Panels " + variant + " " + FLAG
            self.assertEqual(len(roadmap.markers(line)), 1, repr(variant))
            self.assertEqual(roadmap.state_of(line), "testing", repr(variant))
            self.assertEqual(roadmap.with_state(line, "testing"), "## 98. Panels " + spy + VS16 + " " + FLAG)

    def test_retired_markers_are_replaced(self):
        for old in ("\N{MEMO}", "\N{LARGE GREEN CIRCLE}", "\N{GEAR}" + VS16, "\N{ROBOT FACE}"):
            self.assertEqual(roadmap.with_state("## 2. B " + old, "blocked"), "## 2. B " + S["blocked"])

    def test_several_markers_become_one(self):
        line = "## 3. C " + S["done"] + " " + S["refine"] + " " + S["ready"]
        self.assertEqual(roadmap.with_state(line, "postponed"), "## 3. C " + S["postponed"])


class TestGenerate(Fixture):
    def test_lists_header_lines_newest_first_and_nothing_else(self):
        # No git repository here, so the mtime is the creation time.
        self.item("roadmap", "task-10.md", "## 10. Ten " + S["ready"], mtime=1000)
        self.item("roadmap", "task-2.md", "## 2. Two " + S["refine"] + " " + FLAG, mtime=3000)
        self.item("roadmap", "task-4a.md", "## 4a. Four a " + S["blocked"], mtime=2000)
        self.item("roadmap", "bug-3.md", "## B-3. Bug three " + S["testing"], mtime=3000)
        self.item("roadmap", "bug-1.md", "## B-1. Bug one " + S["postponed"], mtime=500)
        self.item("roadmap-archive", "task-1.md", "## 1. One " + S["done"], mtime=100)
        self.item("roadmap-archive", "task-5.md", "## 5. Five " + S["done"], mtime=100)
        self.item("roadmap-archive", "bug-5.md", "## B-5. Bug five " + S["done"], mtime=100)
        self.item("roadmap-archive", "bug-2.md", "## B-2. Bug two " + S["done"], mtime=100)
        self.item("roadmap-archive", "bug-12.md", "## B-12. Bug twelve " + S["done"], mtime=50)
        self.assertEqual(roadmap.cmd_generate(self.root, [self.project]), [self.root / "roadmap-readmes" / "p.md"])
        o = "../roadmap-items/p/roadmap/"
        a = "../roadmap-items/p/roadmap-archive/"
        expected = (
            "# p roadmap\n\n## Roadmap\n\n"
            f"- B-3. Bug three {S['testing']} — [bug-3.md]({o}bug-3.md)\n"  # a tie: the higher number first
            f"- 2. Two {S['refine']} {FLAG} — [task-2.md]({o}task-2.md)\n"
            f"- 4a. Four a {S['blocked']} — [task-4a.md]({o}task-4a.md)\n"
            f"- 10. Ten {S['ready']} — [task-10.md]({o}task-10.md)\n"
            f"- B-1. Bug one {S['postponed']} — [bug-1.md]({o}bug-1.md)\n"
            "\n## Archive\n\n"
            f"- 5. Five {S['done']} — [task-5.md]({a}task-5.md)\n"  # a tie on the number too: tasks first
            f"- B-5. Bug five {S['done']} — [bug-5.md]({a}bug-5.md)\n"
            f"- B-2. Bug two {S['done']} — [bug-2.md]({a}bug-2.md)\n"
            f"- 1. One {S['done']} — [task-1.md]({a}task-1.md)\n"
            f"- B-12. Bug twelve {S['done']} — [bug-12.md]({a}bug-12.md)\n"
        )
        self.assertEqual(self.readme(), expected)
        self.assertEqual(roadmap.cmd_generate(self.root, [self.project]), [], "a second run changes nothing")
        self.assertEqual(self.problems(), [])

    def test_an_empty_project(self):
        code, out, _ = self.run_main("generate", "--all")
        self.assertEqual(code, 0)
        self.assertEqual(self.readme(), "# p roadmap\n\n## Roadmap\n\n## Archive\n")

    def test_generate_needs_projects_or_all(self):
        code, _, err = self.run_main("generate")
        self.assertEqual(code, 2)
        self.assertIn("--all", err)


@unittest.skipUnless(shutil.which("git"), "git is not installed")
class TestOrderFromGit(Fixture):
    def git(self, *args, when=None):
        env = dict(os.environ)
        if when is not None:
            env["GIT_AUTHOR_DATE"] = env["GIT_COMMITTER_DATE"] = f"@{when} +0000"
        subprocess.run(
            ["git", "-C", str(self.root), "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
            check=True,
            env=env,
            stdout=subprocess.DEVNULL,
        )

    def order(self, section):
        text = self.readme().split("## Archive")[0 if section == "Roadmap" else 1]
        return [line.rsplit("](", 1)[0].rsplit("[", 1)[1] for line in text.splitlines() if line.startswith("- ")]

    def test_commit_time_renames_staged_moves_and_untracked_files(self):
        self.git("init", "-q")
        self.item("roadmap", "task-1.md", "## 1. One " + S["ready"])
        self.git("add", "roadmap-items")
        self.git("commit", "-q", "-m", "task 1", when=1_000_000_000)
        self.item("roadmap", "bug-1.md", "## B-1. One " + S["refine"])
        self.git("add", "roadmap-items")
        self.git("commit", "-q", "-m", "bug 1", when=1_000_001_000)
        # A committed rename (to the archive) keeps the time task 1 was first added.
        roadmap.cmd_state(self.root, "p", "1", "done")
        self.git("add", "-A", "roadmap-items")
        self.git("commit", "-q", "-m", "task 1 done", when=1_000_002_000)
        self.item("roadmap-archive", "task-7.md", "## 7. Seven " + S["done"])
        self.git("add", "roadmap-items")
        self.git("commit", "-q", "-m", "task 7", when=1_000_000_500)  # created between task 1 and bug 1
        # An untracked file: its mtime, here older than every commit.
        self.item("roadmap", "task-2.md", "## 2. Two " + S["refine"], mtime=900_000_000)
        self.item("roadmap", "task-3.md", "## 3. Three " + S["refine"], mtime=1_000_000_700)
        # A staged, uncommitted git mv keeps bug 1's commit time.
        roadmap.cmd_state(self.root, "p", "B-1", "done")
        self.assertEqual(self.order("Roadmap"), ["task-3.md", "task-2.md"])
        self.assertEqual(self.order("Archive"), ["bug-1.md", "task-7.md", "task-1.md"])
        self.assertEqual(self.problems(), [])
        self.git("add", "-A", "roadmap-items", "roadmap-readmes")
        self.git("commit", "-q", "-m", "bug 1 done", when=1_000_003_000)
        self.assertEqual(self.order("Archive"), ["bug-1.md", "task-7.md", "task-1.md"])
        self.assertEqual(self.problems(), [], "the order holds after the commit")

    def test_state_keeps_the_mtime_of_an_untracked_item(self):
        path = self.item("roadmap", "task-4.md", "## 4. Four " + S["ready"], mtime=1234)
        _, path = roadmap.cmd_state(self.root, "p", "4", "done")
        self.assertEqual(int(path.stat().st_mtime), 1234)
        roadmap.cmd_fable(self.root, "p", "4", "on")
        self.assertEqual(int(path.stat().st_mtime), 1234)


class TestState(Fixture):
    def test_replaces_the_marker_and_keeps_the_body(self):
        path = self.item("roadmap", "task-7.md", "## 7. Seven " + S["refine"], "\nLine one.\n\nLine two.\n")
        code, out, _ = self.run_main("state", "p", "7", "ready")
        self.assertEqual(code, 0)
        self.assertEqual(out, f"## 7. Seven {S['ready']}\nroadmap-items/p/roadmap/task-7.md\n")
        self.assertEqual(path.read_text(encoding="utf-8"), f"## 7. Seven {S['ready']}\n\nLine one.\n\nLine two.\n")
        self.assertIn(f"- 7. Seven {S['ready']} — ", self.readme())
        self.assertEqual(self.problems(), [])

    def test_sets_a_state_where_there_was_none_and_accepts_the_emoji(self):
        path = self.item("roadmap", "bug-4.md", "## B-4. Four")
        roadmap.cmd_state(self.root, "p", "B-4", "\N{CONSTRUCTION SIGN}")
        self.assertEqual(self.first(path), "## B-4. Four " + S["blocked"])

    def test_writes_the_selector_where_it_was_missing(self):
        path = self.item("roadmap", "task-1.md", "## 1. One \N{BALLOT BOX WITH CHECK}")
        self.assertTrue(any("not written as" in p for p in self.problems()))
        roadmap.cmd_state(self.root, "p", "1", "ready")
        self.assertEqual(self.first(path), "## 1. One \N{BALLOT BOX WITH CHECK}" + VS16)
        self.assertEqual(self.problems(), [])

    def test_done_moves_to_the_archive_and_back_with_the_flag(self):
        self.item("roadmap", "bug-9.md", "## B-9. Nine " + S["progress"] + " " + FLAG)
        header, path = roadmap.cmd_state(self.root, "p", "b-9", "done")
        self.assertEqual(path, self.pdir / "roadmap-archive" / "bug-9.md")
        self.assertFalse((self.pdir / "roadmap" / "bug-9.md").exists())
        self.assertEqual(header, f"## B-9. Nine {S['done']} {FLAG}")
        self.assertEqual(self.first(path), header)
        self.assertIn("roadmap-archive/bug-9.md", self.readme())
        self.assertEqual(self.problems(), [])
        header, path = roadmap.cmd_state(self.root, "p", "B-9", "refine")
        self.assertEqual(path, self.pdir / "roadmap" / "bug-9.md")
        self.assertEqual(header, f"## B-9. Nine {S['refine']} {FLAG}")
        self.assertFalse((self.pdir / "roadmap-archive" / "bug-9.md").exists())
        self.assertEqual(self.problems(), [])

    def test_testing_stays_open(self):
        self.item("roadmap-archive", "task-98.md", "## 98. Panels " + S["done"])
        _, path = roadmap.cmd_state(self.root, "p", "98", "testing")
        self.assertEqual(path, self.pdir / "roadmap" / "task-98.md")
        self.assertEqual(self.first(path), "## 98. Panels \N{SLEUTH OR SPY}" + VS16)
        self.assertEqual(self.problems(), [])

    def test_by_path(self):
        self.item("roadmap", "task-3.md", "## 3. Three " + S["ready"])
        _, path = roadmap.cmd_state(self.root, "p", str(self.pdir / "roadmap" / "task-3.md"), "done")
        self.assertEqual(path, self.pdir / "roadmap-archive" / "task-3.md")

    def test_unknown_items_and_states(self):
        self.item("roadmap", "task-3.md", "## 3. Three " + S["ready"])
        self.assertEqual(self.run_main("state", "p", "4", "ready")[0], 2)
        self.assertEqual(self.run_main("state", "p", "3", "someday")[0], 2)
        self.assertEqual(self.run_main("state", "nope", "3", "ready")[0], 2)

    @unittest.skipUnless(shutil.which("git"), "git is not installed")
    def test_uses_git_mv_for_a_tracked_file(self):
        self.item("roadmap", "task-6.md", "## 6. Six " + S["progress"])
        git = ["git", "-C", str(self.root), "-c", "user.name=t", "-c", "user.email=t@example.invalid"]
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run([*git, "add", "roadmap-items"], check=True)
        subprocess.run([*git, "commit", "-q", "-m", "task 6"], check=True)
        roadmap.cmd_state(self.root, "p", "6", "done")
        status = subprocess.run(
            ["git", "-C", str(self.root), "status", "--porcelain", "--", "roadmap-items"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        self.assertIn("roadmap-items/p/roadmap/task-6.md -> roadmap-items/p/roadmap-archive/task-6.md", status)


class TestFable(Fixture):
    def test_on_and_off(self):
        path = self.item("roadmap", "task-57.md", "## 57. Menu " + S["ready"])
        code, out, _ = self.run_main("fable", "p", "57", "on")
        self.assertEqual(code, 0)
        self.assertEqual(out.splitlines()[0], f"## 57. Menu {S['ready']} {FLAG}")
        roadmap.cmd_fable(self.root, "p", "57", "on")
        self.assertEqual(self.first(path), f"## 57. Menu {S['ready']} {FLAG}", "on twice is one flag")
        self.assertIn(f"- 57. Menu {S['ready']} {FLAG} — ", self.readme())
        self.assertEqual(self.problems(), [])
        roadmap.cmd_fable(self.root, "p", "57", "off")
        self.assertEqual(self.first(path), f"## 57. Menu {S['ready']}")
        self.assertIn(f"- 57. Menu {S['ready']} — ", self.readme())
        self.assertEqual(self.problems(), [])

    def test_a_flag_without_its_selector_is_recognised(self):
        path = self.item("roadmap", "bug-1.md", "## B-1. One " + S["refine"] + " \N{TROPHY}" + VS16)
        roadmap.cmd_state(self.root, "p", "B-1", "ready")
        self.assertEqual(self.first(path), f"## B-1. One {S['ready']} {FLAG}")
        roadmap.cmd_fable(self.root, "p", "B-1", "off")
        self.assertEqual(self.first(path), f"## B-1. One {S['ready']}")

    def test_stays_in_the_archive(self):
        path = self.item("roadmap-archive", "task-2.md", "## 2. Two " + S["done"])
        roadmap.cmd_fable(self.root, "p", "2", "on")
        self.assertEqual(self.first(path), f"## 2. Two {S['done']} {FLAG}")
        self.assertEqual(self.problems(), [])


class TestNew(Fixture):
    def test_numbers_continue_across_both_folders_and_types(self):
        self.item("roadmap", "task-3.md", "## 3. Three " + S["ready"])
        self.item("roadmap-archive", "task-7.md", "## 7. Seven " + S["done"])
        self.item("roadmap-archive", "bug-12.md", "## B-12. Twelve " + S["done"])
        self.item("roadmap", "bug-5.md", "## B-5. Five " + S["refine"])
        code, out, _ = self.run_main("new", "p", "task", "A   new task")
        self.assertEqual(code, 0)
        self.assertEqual(out, f"## 8. A new task {S['refine']}\nroadmap-items/p/roadmap/task-8.md\n")
        today = datetime.date.today().isoformat()
        self.assertEqual(
            (self.pdir / "roadmap" / "task-8.md").read_text(encoding="utf-8"),
            f"## 8. A new task {S['refine']}\n\n**Created** {today}.\n",
        )
        header, path = roadmap.cmd_new(self.root, "p", "bug", "It breaks", "blocked", fable=True)
        self.assertEqual(header, f"## B-13. It breaks {S['blocked']} {FLAG}")
        self.assertEqual(path, self.pdir / "roadmap" / "bug-13.md")
        header, path = roadmap.cmd_new(self.root, "p", "task", "Already finished " + S["ready"], "done")
        self.assertEqual(header, f"## 9. Already finished {S['done']}")
        self.assertEqual(path, self.pdir / "roadmap-archive" / "task-9.md")
        readme = self.readme()
        self.assertIn("task-8.md", readme)
        self.assertIn("bug-13.md", readme)
        self.assertIn("task-9.md", readme)
        self.assertEqual(self.problems(), [])

    def test_first_items_start_at_one(self):
        _, path = roadmap.cmd_new(self.root, "p", "bug", "First")
        self.assertEqual(path.name, "bug-1.md")
        _, path = roadmap.cmd_new(self.root, "p", "task", "First")
        self.assertEqual(path.name, "task-1.md")

    def test_letter_ids_count_by_their_number(self):
        self.item("roadmap-archive", "task-4a.md", "## 4a. Four a " + S["done"])
        _, path = roadmap.cmd_new(self.root, "p", "task", "Next")
        self.assertEqual(path.name, "task-5.md")

    def test_refuses_an_empty_title(self):
        self.assertEqual(self.run_main("new", "p", "task", "  ")[0], 2)


class TestCheck(Fixture):
    def assertProblem(self, fragment):
        found = self.problems()
        self.assertTrue(any(fragment in p for p in found), f"{fragment!r} not in {found}")
        code, _, err = self.run_main("check", "p")
        self.assertEqual(code, 1)
        self.assertIn(fragment, err)

    def test_a_clean_project_passes(self):
        self.item("roadmap", "task-1.md", "## 1. One " + S["ready"] + " " + FLAG)
        self.item("roadmap-archive", "bug-1.md", "## B-1. One " + S["done"])
        roadmap.cmd_generate(self.root, ["p"])
        code, out, _ = self.run_main("check", "--all")
        self.assertEqual((code, out), (0, "ok\n"))

    def generated(self):
        roadmap.cmd_generate(self.root, ["p"])

    def test_not_an_item_file(self):
        self.item("roadmap", "notes.md", "# Notes")
        self.generated()
        self.assertProblem("not an item file")

    def test_hidden_files_are_ignored(self):
        (self.pdir / "roadmap" / ".gitkeep").write_text("", encoding="utf-8")
        (self.pdir / "roadmap-archive" / ".gitkeep").write_text("", encoding="utf-8")
        self.generated()
        self.assertEqual(self.problems(), [])

    def test_header_form(self):
        self.item("roadmap", "task-1.md", "# Task 1 — One " + S["ready"])
        self.generated()
        self.assertProblem("is not `## <N>. <Title>")

    def test_bug_header_form(self):
        self.item("roadmap", "bug-1.md", "## 1. One " + S["ready"])
        self.generated()
        self.assertProblem("is not `## B-<N>. <Title>")

    def test_number_mismatch(self):
        self.item("roadmap", "task-2.md", "## 3. Two " + S["ready"])
        self.generated()
        self.assertProblem("number 3 is not the file's 2")

    def test_no_state(self):
        self.item("roadmap", "task-1.md", "## 1. One")
        self.generated()
        self.assertProblem("no state emoji")

    def test_several_states(self):
        self.item("roadmap", "task-1.md", "## 1. One " + S["ready"] + " " + S["refine"])
        self.generated()
        self.assertProblem("2 state emojis")

    def test_state_not_last(self):
        self.item("roadmap", "task-1.md", "## 1. One " + S["ready"] + " more")
        self.generated()
        self.assertProblem("not the last character")

    def test_retired_state(self):
        self.item("roadmap", "task-1.md", "## 1. One \N{MEMO}")
        self.generated()
        self.assertProblem("retired state emoji")

    def test_done_but_open(self):
        self.item("roadmap", "task-1.md", "## 1. One " + S["done"])
        self.generated()
        self.assertProblem("done but not in roadmap-archive/")

    def test_archived_but_not_done(self):
        self.item("roadmap-archive", "bug-1.md", "## B-1. One " + S["progress"])
        self.generated()
        self.assertProblem("in roadmap-archive/ but progress, not done")

    def test_duplicate_number(self):
        self.item("roadmap", "task-1.md", "## 1. One " + S["ready"])
        self.item("roadmap-archive", "task-1.md", "## 1. One " + S["done"])
        self.generated()
        self.assertProblem("task 1 again")

    def test_same_number_for_a_task_and_a_bug_is_fine(self):
        self.item("roadmap", "task-1.md", "## 1. One " + S["ready"])
        self.item("roadmap", "bug-1.md", "## B-1. One " + S["ready"])
        self.generated()
        self.assertEqual(self.problems(), [])

    def test_two_flags(self):
        self.item("roadmap", "task-1.md", "## 1. One " + S["ready"] + " " + FLAG + " " + FLAG)
        self.generated()
        self.assertProblem("2 " + FLAG + " flags")

    def test_flag_before_the_state(self):
        self.item("roadmap", "task-1.md", "## 1. One " + FLAG + " " + S["ready"])
        self.generated()
        self.assertProblem("flag is not at the end")

    def test_flag_without_a_state(self):
        self.item("roadmap", "task-1.md", "## 1. One " + FLAG)
        self.generated()
        self.assertProblem("no state emoji on the first line before the")

    def test_a_stale_readme_and_generate_fixes_it(self):
        self.item("roadmap", "task-1.md", "## 1. One " + S["ready"])
        self.assertProblem("differs from the items")
        self.assertEqual(self.run_main("generate", "p")[0], 0)
        self.assertEqual(self.run_main("check", "p")[0], 0)
        (self.pdir / "roadmap" / "task-1.md").write_text("## 1. One renamed " + S["ready"] + "\n", encoding="utf-8")
        self.assertProblem("differs from the items")


if __name__ == "__main__":
    unittest.main(verbosity=1)
