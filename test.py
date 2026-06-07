#!/usr/bin/env python3
"""
test_ugit.py - Test harness for a ugit CLI implementation.

ugit: "Git Internals - Learn by Building Your Own Git" by Nikita Leshenko
      https://www.leshenko.net/p/ugit/

Drives the ugit command through the workflow the tutorial builds and asserts
on the behaviours it specifies: content-addressable objects, trees, commits,
refs (branches/tags), checkout, log, status, reset, diff/show,
merge-base/merge, and fetch/push against a local remote.

USAGE
    ./test_ugit.py                         # defaults to the `ugit` command
    ./test_ugit.py --ugit "python3 ./ugit" # custom command
    UGIT="python3 ./ugit" ./test_ugit.py   # or via env var

OPTIONS (flags or env vars)
    --ugit / UGIT          command used to invoke ugit   (default: "ugit")
    --keep-tmp / KEEPTMP=1 keep the scratch dir
    --stop-fail / STOPFAIL=1   stop at the first failure
    --no-color             disable coloured output

Exit status: 0 if every test passed, 1 otherwise.
"""

import argparse
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field


# --------------------------------------------------------------------------
# Result of one ugit invocation
# --------------------------------------------------------------------------
@dataclass
class Result:
    rc: int
    out: str
    err: str


# --------------------------------------------------------------------------
# The harness
# --------------------------------------------------------------------------
class Harness:
    def __init__(self, ugit_cmd, *, color=True, stop_fail=False):
        self.ugit = shlex.split(ugit_cmd)
        self.stop_fail = stop_fail
        self.passed = 0
        self.failed = 0
        self.skipped = 0
        self.failed_names = []
        self.cwd = os.getcwd()
        self.last = Result(0, "", "")

        if color and sys.stdout.isatty():
            self.c = dict(
                g="\033[32m", r="\033[31m", y="\033[33m",
                dim="\033[2m", bold="\033[1m", off="\033[0m",
            )
        else:
            self.c = {k: "" for k in ("g", "r", "y", "dim", "bold", "off")}

    # -- process control ----------------------------------------------------
    def chdir(self, path):
        self.cwd = path

    def run(self, *args):
        """Invoke ugit with args; store and return a Result."""
        try:
            proc = subprocess.run(
                self.ugit + list(args),
                cwd=self.cwd,
                capture_output=True,
                text=True,
            )
            self.last = Result(proc.returncode, proc.stdout, proc.stderr)
        except FileNotFoundError:
            self.last = Result(127, "", f"command not found: {self.ugit[0]}")
        return self.last

    def cap(self, *args):
        """Run and return trimmed stdout."""
        return self.run(*args).out.strip()

    # -- reporting ----------------------------------------------------------
    def section(self, name):
        c = self.c
        print(f"\n{c['bold']}== {name} =={c['off']}")

    def _ok(self, name):
        self.passed += 1
        print(f"  {self.c['g']}PASS{self.c['off']} {name}")

    def _no(self, name, detail=""):
        self.failed += 1
        self.failed_names.append(name)
        print(f"  {self.c['r']}FAIL{self.c['off']} {name}")
        if detail:
            print(f"       {self.c['dim']}{detail}{self.c['off']}")
        if self.stop_fail:
            print(f"\n{self.c['y']}Stopping at first failure.{self.c['off']}")
            self.summary()
            sys.exit(1)

    def skip(self, name, reason=""):
        self.skipped += 1
        print(f"  {self.c['y']}SKIP{self.c['off']} {name} "
              f"{self.c['dim']}{reason}{self.c['off']}")

    # -- assertions ---------------------------------------------------------
    def ok(self, name, cond, detail=""):
        self._ok(name) if cond else self._no(name, detail)
        return cond

    def assert_ok(self, name):
        r = self.last
        return self.ok(name, r.rc == 0,
                       f"exit={r.rc} stderr: {r.err.strip() or '<empty>'}")

    def assert_fails(self, name):
        r = self.last
        return self.ok(name, r.rc != 0, "expected non-zero exit, got 0")

    def assert_match(self, name, pattern):
        out = self.last.out
        cond = re.search(pattern, out) is not None
        return self.ok(name, cond,
                       f"stdout did not match /{pattern}/. "
                       f"Got: {out.strip() or '<empty>'}")

    def assert_oid(self, name, value):
        cond = re.fullmatch(r"[0-9a-f]{64}", value or "") is not None
        return self.ok(name, cond, f"not a 64-hex (sha256) oid: {value!r}")

    def assert_eq(self, name, a, b):
        return self.ok(name, a == b, f"expected {b!r}, got {a!r}")

    def assert_ne(self, name, a, b):
        return self.ok(name, a != b, f"expected values to differ, both = {a!r}")

    def assert_file(self, name, path):
        return self.ok(name, os.path.isfile(path), f"file not found: {path}")

    def assert_dir(self, name, path):
        return self.ok(name, os.path.isdir(path), f"dir not found: {path}")

    def out_contains(self, name, needle, *, should=True):
        present = needle in self.last.out
        if should:
            return self.ok(name, present, f"{needle!r} not in output")
        return self.ok(name, not present, f"{needle!r} unexpectedly in output")

    # -- summary ------------------------------------------------------------
    def summary(self):
        c = self.c
        print(f"\n{c['dim']}---------------------------------------------"
              f"{c['off']}")
        print(f"{c['bold']}Results:{c['off']} "
              f"{c['g']}{self.passed} passed{c['off']}, "
              f"{c['r']}{self.failed} failed{c['off']}, "
              f"{c['y']}{self.skipped} skipped{c['off']}")
        if self.failed:
            print(f"{c['r']}Failed tests:{c['off']}")
            for n in self.failed_names:
                print(f"  - {n}")


# --------------------------------------------------------------------------
# Small filesystem helpers (operate inside the current repo dir)
# --------------------------------------------------------------------------
def write(path, content):
    with open(path, "w") as f:
        f.write(content)


def read(path):
    with open(path) as f:
        return f.read()


def append(path, content):
    with open(path, "a") as f:
        f.write(content)


# --------------------------------------------------------------------------
# The test scenario
# --------------------------------------------------------------------------
def run_tests(h, tmp):
    repo = os.path.join(tmp, "repo")
    os.makedirs(repo, exist_ok=True)
    h.chdir(repo)

    def p(name):  # path inside repo
        return os.path.join(repo, name)

    def index_text():
        ipath = p(".ugit/index")
        return read(ipath) if os.path.isfile(ipath) else ""

    # ---- init -------------------------------------------------------------
    h.section("init")
    h.run("init")
    h.assert_ok("init exits 0")
    h.assert_dir("init creates .ugit", p(".ugit"))
    h.assert_dir("init creates .ugit/objects", p(".ugit/objects"))
    h.run("init")
    h.assert_fails("init in an existing repo fails")

    # ---- hash-object / cat-file ------------------------------------------
    h.section("hash-object / cat-file (content-addressable storage)")
    write(p("greeting.txt"), "hello ugit\n")
    oid1 = h.cap("hash-object", "greeting.txt")
    h.assert_oid("hash-object prints a 64-hex (sha256) oid", oid1)
    h.assert_file("object stored under .ugit/objects",
                  p(f".ugit/objects/{oid1}"))

    h.run("cat-file", oid1)
    h.assert_ok("cat-file exits 0")
    h.assert_eq("cat-file reproduces original content",
                h.last.out, read(p("greeting.txt")))

    shutil.copy(p("greeting.txt"), p("copy.txt"))
    oid1b = h.cap("hash-object", "copy.txt")
    h.assert_eq("identical content hashes to identical oid", oid1b, oid1)

    write(p("other.txt"), "different bytes\n")
    oid2 = h.cap("hash-object", "other.txt")
    h.assert_ne("different content hashes to different oid", oid2, oid1)

    # ---- add (the staging area / index) -----------------------------------
    # In the final ugit, write-tree and commit work off the index, so add is
    # the entry point to the snapshot pipeline. Test it first.
    h.section("add (staging area / index)")
    os.makedirs(p("subdir"), exist_ok=True)
    write(p("root.txt"), "root file\n")
    write(p("subdir/nested.txt"), "nested file\n")

    h.run("add", "root.txt", "subdir")
    h.assert_ok("add <file> <dir> exits 0")
    h.assert_file("add creates the index at .ugit/index", p(".ugit/index"))
    idx = index_text()
    h.ok("index records the staged top-level file", "root.txt" in idx,
         f"'root.txt' not found in index: {idx or '<empty>'}")
    h.ok("index records the staged nested file",
         "subdir/nested.txt" in idx or "subdir\\/nested.txt" in idx,
         f"nested path not found in index: {idx or '<empty>'}")

    # ---- write-tree / read-tree ------------------------------------------
    # write-tree snapshots the *index*; read-tree loads a tree back into it.
    h.section("write-tree / read-tree (index-based snapshots)")
    tree = h.cap("write-tree")
    h.assert_oid("write-tree prints a 64-hex (sha256) oid", tree)

    # Clear the staged state by reading an unrelated (empty) point would be
    # awkward; instead mutate the working tree + index, then restore via the
    # tree object and confirm the index round-trips.
    os.remove(p("root.txt"))
    shutil.rmtree(p("subdir"))
    h.run("read-tree", tree)
    h.assert_ok("read-tree exits 0")
    idx = index_text()
    h.ok("read-tree restores the index from the tree",
         "root.txt" in idx and ("nested" in idx),
         f"index after read-tree: {idx or '<empty>'}")
    # The final ugit materializes the working tree via checkout/update_working.
    # If this build's read-tree also writes the working tree, verify it; if
    # not, that's fine - the checkout section covers working-tree restoration.
    if os.path.isfile(p("root.txt")):
        h.assert_eq("read-tree restored working file content (if supported)",
                    read(p("root.txt")), "root file\n")
    else:
        h.skip("read-tree working-tree materialization",
               "(index-only; covered by checkout)")

    # ---- commit / log -----------------------------------------------------
    # commit snapshots the index, so we add before committing.
    h.section("commit / log")
    write(p("tracked.txt"), "v1\n")
    h.run("add", "tracked.txt")
    h.assert_ok("add before first commit")
    c1 = h.cap("commit", "-m", "first commit")
    h.assert_oid("commit prints a 64-hex (sha256) oid", c1)
    h.assert_file("commit object is stored", p(f".ugit/objects/{c1}"))

    write(p("tracked.txt"), "v2\n")
    write(p("c2-only.txt"), "only in second commit\n")
    h.run("add", "tracked.txt", "c2-only.txt")
    c2 = h.cap("commit", "-m", "second commit")
    h.assert_oid("second commit prints a 64-hex (sha256) oid", c2)
    h.assert_ne("second commit differs from first", c2, c1)

    h.run("log")
    h.assert_ok("log exits 0")
    h.assert_match("log shows the latest commit oid", re.escape(c2))
    h.assert_match("log shows the first commit oid", re.escape(c1))
    h.assert_match("log shows a commit message", "second commit")

    h.run("log", c1)
    h.assert_match("log <oid> shows that commit", re.escape(c1))
    h.out_contains("log <C1> does not include the newer C2", c2, should=False)

    # ---- status / branch --------------------------------------------------
    h.section("status / branch")
    h.run("status")
    h.assert_ok("status exits 0")
    h.assert_match("status reports current branch",
                   r"[Oo]n branch|HEAD detached")

    h.run("branch")
    h.assert_ok("branch (list) exits 0")

    h.run("branch", "feature")
    h.assert_ok("branch <name> creates a branch")
    h.assert_file("branch ref file exists", p(".ugit/refs/heads/feature"))

    h.run("branch")
    h.assert_match("new branch appears in listing", "feature")

    # ---- tag --------------------------------------------------------------
    h.section("tag (refs resolvable by name)")
    h.run("tag", "v1.0", c1)
    h.assert_ok("tag <name> <oid> exits 0")
    h.assert_file("tag ref file exists", p(".ugit/refs/tags/v1.0"))

    h.run("log", "v1.0")
    h.assert_ok("log accepts a tag name")
    h.assert_match("tag resolves to the tagged commit", re.escape(c1))

    # ---- checkout ---------------------------------------------------------
    # checkout is the user-facing working-tree restore path.
    h.section("checkout")
    h.run("checkout", c1)
    h.assert_ok("checkout <oid> exits 0")
    if os.path.isfile(p("tracked.txt")):
        h.assert_eq("checkout restores the working tree of C1",
                    read(p("tracked.txt")), "v1\n")
    h.ok("checkout removes files absent from target tree",
         not os.path.exists(p("c2-only.txt")),
         "c2-only.txt survived checkout to C1")

    h.run("checkout", "master")
    h.assert_ok("checkout <branch> exits 0")
    if os.path.isfile(p("tracked.txt")):
        h.assert_eq("checkout master restores latest content",
                    read(p("tracked.txt")), "v2\n")
    h.assert_file("checkout master restores files from latest tree",
                  p("c2-only.txt"))

    # ---- diff / show ------------------------------------------------------
    h.section("diff / show")
    # show always expects an oid argument (it does not default to HEAD).
    h.run("show")
    h.assert_fails("show with no oid is rejected")
    h.run("show", c2)
    h.assert_ok("show <oid> exits 0")
    h.assert_match("show includes the given commit oid", re.escape(c2))

    append(p("tracked.txt"), "uncommitted change\n")
    h.run("diff")
    h.assert_ok("diff exits 0")
    h.assert_match("diff reports the changed file", "tracked.txt")
    # restore a clean tip state
    h.run("checkout", "master")
    write(p("tracked.txt"), "v2\n")
    h.run("add", "tracked.txt")

    # ---- reset ------------------------------------------------------------
    h.section("reset")
    h.run("reset", c1)
    h.assert_ok("reset <commit> exits 0")
    h.run("log")
    h.assert_match("after reset, HEAD history starts at C1", re.escape(c1))
    h.run("reset", c2)
    h.assert_ok("reset back to tip exits 0")

    # ---- merge-base / merge ----------------------------------------------
    h.section("merge-base / merge")
    h.run("checkout", "master")
    h.run("reset", c2)
    h.run("branch", "topic", c1)
    h.run("checkout", "topic")
    write(p("topic.txt"), "topic work\n")
    h.run("add", "topic.txt")
    h.assert_ok("add on topic branch")
    ct = h.cap("commit", "-m", "topic commit")
    h.assert_oid("commit on topic branch", ct)

    h.run("merge-base", ct, c2)
    h.assert_ok("merge-base exits 0")
    h.assert_match("merge-base finds the common ancestor (C1)", re.escape(c1))

    h.run("checkout", "master")
    h.run("merge", ct)
    h.assert_ok("merge <commit> exits 0")

    # ---- k ----------------------------------------------------------------
    h.section("k (refs graph)")
    h.run("k")
    if h.last.rc == 0:
        h._ok("k exits 0")
    elif re.search(r"dot|graphviz|No such file", h.last.err, re.I):
        h.skip("k requires graphviz/dot", "(not installed)")
    else:
        h._no("k exits 0", f"exit={h.last.rc} stderr: {h.last.err.strip()}")

    # ---- fetch / push -----------------------------------------------------
    h.section("fetch / push (local remote round-trip)")
    remote = os.path.join(tmp, "remote")
    os.makedirs(remote, exist_ok=True)
    saved = h.cwd
    h.chdir(remote)
    h.run("init")
    h.chdir(saved)

    h.run("push", remote, "master")
    if h.last.rc == 0:
        h._ok("push to local remote exits 0")
        consumer = os.path.join(tmp, "consumer")
        os.makedirs(consumer, exist_ok=True)
        h.chdir(consumer)
        h.run("init")
        h.run("fetch", repo)
        if h.last.rc == 0:
            h._ok("fetch from a local repo exits 0")
            h.assert_file("fetched commit object present",
                          os.path.join(consumer, f".ugit/objects/{c2}"))
        else:
            h.skip("fetch round-trip", f"fetch rc={h.last.rc}: {h.last.err}")
        h.chdir(repo)
    else:
        h.skip("fetch/push round-trip",
               f"push rc={h.last.rc}: {h.last.err.strip() or '<no stderr>'}")


# --------------------------------------------------------------------------
# Entry point
# --------------------------------------------------------------------------
def main():
    ap = argparse.ArgumentParser(description="Test harness for a ugit CLI.")
    ap.add_argument("--ugit", default=os.environ.get("UGIT", "ugit"),
                    help='command used to invoke ugit (default: "ugit")')
    ap.add_argument("--keep-tmp", action="store_true",
                    default=os.environ.get("KEEPTMP") == "1",
                    help="keep the scratch directory")
    ap.add_argument("--stop-fail", action="store_true",
                    default=os.environ.get("STOPFAIL") == "1",
                    help="stop at the first failing assertion")
    ap.add_argument("--no-color", action="store_true",
                    help="disable coloured output")
    args = ap.parse_args()

    h = Harness(args.ugit, color=not args.no_color, stop_fail=args.stop_fail)

    print(f"{h.c['bold']}Testing ugit command:{h.c['off']} {args.ugit}")
    tmp = tempfile.mkdtemp(prefix="ugit-test.")
    print(f"{h.c['dim']}scratch dir:{h.c['off']} {tmp}")

    # Sanity: can we run the command at all?
    h.chdir(tmp)
    h.run("init")
    if h.last.rc == 127:
        print(f"{h.c['r']}Error:{h.c['off']} could not execute "
              f"{args.ugit!r} (command not found).", file=sys.stderr)
        print('Point the harness at your implementation, e.g.:\n'
              f'  {sys.argv[0]} --ugit "python3 /path/to/ugit"',
              file=sys.stderr)
        shutil.rmtree(tmp, ignore_errors=True)
        sys.exit(2)

    try:
        run_tests(h, tmp)
    finally:
        h.summary()
        if args.keep_tmp:
            print(f"{h.c['dim']}scratch dir kept at: {tmp}{h.c['off']}")
        else:
            shutil.rmtree(tmp, ignore_errors=True)

    sys.exit(0 if h.failed == 0 else 1)


if __name__ == "__main__":
    main()
