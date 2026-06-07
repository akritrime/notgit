//! Black-box integration tests for the `notgit` CLI.
//!
//! These drive the compiled binary (the `cli` bin target) against real
//! temporary worktrees and assert on stdout/exit codes and on-disk state.
//!
//! The suite has two kinds of tests:
//!
//!   * `sanity_*` / `regression_*` — exercise the documented happy paths.
//!     These should pass on a correct implementation.
//!
//!   * `bug_*` — each targets one defect found in review. They assert the
//!     *correct* behaviour, so they are expected to FAIL on the code as it
//!     currently stands and to PASS once the corresponding bug is fixed.
//!     Every one carries a `// BUG:` note describing the defect.
//!
//! Notes for running:
//!   * `cargo test` builds the bin and sets `CARGO_BIN_EXE_cli`, which we use
//!     to locate it. You can override with `NOTGIT_BIN=/path/to/binary`.
//!   * The merge tests shell out via the program, which itself requires the
//!     system `diff` and `diff3` binaries to be on PATH.
//!   * A couple of tests deliberately corrupt `.ugit/HEAD` to reach error
//!     paths; they run the binary as a child process, so a panic or stack
//!     overflow in the child cannot take down the test runner.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Locate the binary under test.
fn bin() -> String {
    if let Ok(p) = std::env::var("NOTGIT_BIN") {
        return p;
    }
    option_env!("CARGO_BIN_EXE_cli")
        .expect("binary path unknown: run via `cargo test`, or set NOTGIT_BIN")
        .to_string()
}

/// Outcome of one CLI invocation.
struct Run {
    code: Option<i32>,
    success: bool,
    /// True when the child was killed by a signal (e.g. a stack-overflow
    /// abort) rather than exiting with a status code.
    killed_by_signal: bool,
    stdout: String,
    stderr: String,
}

impl Run {
    fn from_output(out: Output) -> Self {
        Run {
            code: out.status.code(),
            success: out.status.success(),
            killed_by_signal: out.status.code().is_none(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    fn panicked(&self) -> bool {
        self.code == Some(101) || self.killed_by_signal
    }

    fn diag(&self) -> String {
        format!(
            "exit={:?} signal_killed={} \n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.code, self.killed_by_signal, self.stdout, self.stderr
        )
    }
}

/// A throwaway worktree that cleans itself up on drop.
struct Sandbox {
    root: PathBuf,
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

impl Sandbox {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "notgit-it.{}.{}.{}",
            std::process::id(),
            nanos,
            n
        ));
        std::fs::create_dir_all(&root).unwrap();
        Sandbox { root }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    fn write(&self, rel: &str, contents: &str) {
        let p = self.path(rel);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(p, contents).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel))
            .unwrap_or_else(|e| panic!("reading {rel}: {e}"))
    }

    /// Run the CLI with the given args, with cwd set to this worktree.
    fn ngit(&self, args: &[&str]) -> Run {
        let out = std::process::Command::new(bin())
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("failed to spawn notgit binary");
        Run::from_output(out)
    }

    /// Run and assert success; returns the Run for further inspection.
    fn ngit_ok(&self, args: &[&str]) -> Run {
        let r = self.ngit(args);
        assert!(r.success, "expected `{args:?}` to succeed:\n{}", r.diag());
        r
    }

    /// Convenience: commit current index and return the printed oid.
    fn commit(&self, msg: &str) -> String {
        self.ngit_ok(&["commit", "-m", msg]).stdout.trim().to_string()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// An initialized repo with one committed top-level file. Returns (sandbox, oid).
fn init_with_one_commit() -> (Sandbox, String) {
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("file.txt", "v1\n");
    s.ngit_ok(&["add", "file.txt"]);
    let oid = s.commit("first");
    (s, oid)
}

// ===========================================================================
// SANITY / HAPPY-PATH (should pass on a correct implementation)
// ===========================================================================

#[test]
fn sanity_init_creates_repo() {
    let s = Sandbox::new();
    let r = s.ngit_ok(&["init"]);
    assert!(s.exists(".ugit"), "init should create the repo dir:\n{}", r.diag());
    assert!(s.exists(".ugit/objects"), "objects dir should exist");
    assert!(s.exists(".ugit/refs"), "refs dir should exist");
    // Re-init must fail.
    let again = s.ngit(&["init"]);
    assert!(!again.success, "re-init in an existing repo should fail:\n{}", again.diag());
}

#[test]
fn sanity_hash_object_and_cat_file_round_trip() {
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("greeting.txt", "hello notgit\n");
    let oid = s.ngit_ok(&["hash-object", "greeting.txt"]).stdout.trim().to_string();
    assert!(is_hex64(&oid), "hash-object should print a 64-hex sha256 oid, got {oid:?}");
    assert!(s.exists(&format!(".ugit/objects/{oid}")), "object should be stored on disk");

    let cat = s.ngit_ok(&["cat-file", &oid]);
    assert_eq!(cat.stdout, "hello notgit\n", "cat-file should reproduce bytes verbatim");
}

#[test]
fn sanity_content_addressing_is_deterministic() {
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("a.txt", "same\n");
    s.write("b.txt", "same\n");
    s.write("c.txt", "different\n");
    let a = s.ngit_ok(&["hash-object", "a.txt"]).stdout.trim().to_string();
    let b = s.ngit_ok(&["hash-object", "b.txt"]).stdout.trim().to_string();
    let c = s.ngit_ok(&["hash-object", "c.txt"]).stdout.trim().to_string();
    assert_eq!(a, b, "identical content must hash identically");
    assert_ne!(a, c, "different content must hash differently");
}

#[test]
fn sanity_add_populates_the_index() {
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("tracked.txt", "x\n");
    s.ngit_ok(&["add", "tracked.txt"]);
    assert!(s.exists(".ugit/index"), "add should create the index");
    let idx = s.read(".ugit/index");
    assert!(idx.contains("tracked.txt"), "index should record the staged path, got: {idx}");
}

#[test]
fn sanity_write_tree_and_commit_print_oids() {
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("f.txt", "1\n");
    s.ngit_ok(&["add", "f.txt"]);
    let tree = s.ngit_ok(&["write-tree"]).stdout.trim().to_string();
    assert!(is_hex64(&tree), "write-tree should print a 64-hex oid, got {tree:?}");
    let c1 = s.commit("one");
    assert!(is_hex64(&c1), "commit should print a 64-hex oid, got {c1:?}");
    s.write("f.txt", "2\n");
    s.ngit_ok(&["add", "f.txt"]);
    let c2 = s.commit("two");
    assert_ne!(c1, c2, "a new commit must have a new oid");
}

#[test]
fn sanity_log_lists_all_commits_in_history() {
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("f.txt", "1\n");
    s.ngit_ok(&["add", "f.txt"]);
    let c1 = s.commit("first");
    s.write("f.txt", "2\n");
    s.ngit_ok(&["add", "f.txt"]);
    let c2 = s.commit("second");
    let log = s.ngit_ok(&["log"]).stdout;
    assert!(log.contains(&c1), "log should mention first commit");
    assert!(log.contains(&c2), "log should mention second commit");
    assert!(log.contains("second"), "log should show commit messages");
}

#[test]
fn sanity_branch_create_and_list() {
    let (s, _oid) = init_with_one_commit();
    s.ngit_ok(&["branch", "feature"]);
    assert!(s.exists(".ugit/refs/heads/feature"), "branch ref file should exist");
    let list = s.ngit_ok(&["branch"]).stdout;
    assert!(list.contains("feature"), "branch listing should include the new branch");
    assert!(list.contains("master"), "branch listing should include master");
    assert!(list.contains('*'), "branch listing should mark the current branch with '*'");
}

#[test]
fn sanity_tag_is_resolvable() {
    let (s, oid) = init_with_one_commit();
    s.ngit_ok(&["tag", "v1", &oid]);
    assert!(s.exists(".ugit/refs/tags/v1"), "tag ref file should exist");
    // A tag should be usable wherever a revision is accepted.
    let log = s.ngit_ok(&["log", "v1"]).stdout;
    assert!(log.contains(&oid), "log should accept and resolve a tag name");
}

#[test]
fn sanity_checkout_restores_top_level_content() {
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("f.txt", "v1\n");
    s.ngit_ok(&["add", "f.txt"]);
    let c1 = s.commit("v1");
    s.write("f.txt", "v2\n");
    s.ngit_ok(&["add", "f.txt"]);
    s.commit("v2");

    s.ngit_ok(&["checkout", &c1]); // detached checkout of the first commit
    assert_eq!(s.read("f.txt"), "v1\n", "checkout should restore the committed content");

    s.ngit_ok(&["checkout", "master"]);
    assert_eq!(s.read("f.txt"), "v2\n", "checkout master should restore the tip");
}

#[test]
fn sanity_status_reports_branch() {
    let (s, _oid) = init_with_one_commit();
    let st = s.ngit_ok(&["status"]).stdout;
    assert!(
        st.to_lowercase().contains("branch") || st.contains("detached"),
        "status should report branch/HEAD state, got: {st}"
    );
}

#[test]
fn sanity_reset_moves_head() {
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("f.txt", "1\n");
    s.ngit_ok(&["add", "f.txt"]);
    let c1 = s.commit("one");
    s.write("f.txt", "2\n");
    s.ngit_ok(&["add", "f.txt"]);
    let _c2 = s.commit("two");
    // reset takes a literal oid in this CLI; use one.
    let r = s.ngit_ok(&["reset", &c1]);
    assert!(r.stdout.contains(&c1), "reset should report moving HEAD to {c1}");
    let log = s.ngit_ok(&["log"]).stdout;
    assert!(log.contains(&c1), "after reset, history should start at c1");
}

#[test]
fn regression_merge_base_is_the_common_ancestor() {
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("f.txt", "a\n");
    s.ngit_ok(&["add", "f.txt"]);
    let a = s.commit("a");
    s.write("f.txt", "b\n");
    s.ngit_ok(&["add", "f.txt"]);
    let b = s.commit("b"); // master: a <- b

    s.ngit_ok(&["branch", "topic", &a]);
    s.ngit_ok(&["checkout", "topic"]);
    s.write("g.txt", "c\n");
    s.ngit_ok(&["add", "g.txt"]);
    let c = s.commit("c"); // topic: a <- c

    let mb = s.ngit_ok(&["merge-base", &c, &b]).stdout;
    assert!(mb.contains(&a), "merge-base of b and c should be a; got: {mb}");
}

#[test]
fn regression_clean_merge_then_commit() {
    // Requires system `diff`/`diff3`.
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("shared.txt", "base\n");
    s.ngit_ok(&["add", "shared.txt"]);
    let a = s.commit("base");

    s.write("master_only.txt", "m\n");
    s.ngit_ok(&["add", "master_only.txt"]);
    s.commit("master work"); // master: a <- b

    s.ngit_ok(&["branch", "topic", &a]);
    s.ngit_ok(&["checkout", "topic"]);
    s.write("topic_only.txt", "t\n");
    s.ngit_ok(&["add", "topic_only.txt"]);
    let c = s.commit("topic work"); // topic: a <- c

    s.ngit_ok(&["checkout", "master"]);
    let m = s.ngit(&["merge", &c]);
    assert!(m.success, "non-conflicting merge should succeed:\n{}", m.diag());
    // Disjoint files: both should be present in the working tree after merge.
    assert!(s.exists("topic_only.txt"), "merge should bring in topic's file");
    assert!(s.exists("master_only.txt"), "merge should keep master's file");
}

#[test]
fn regression_fetch_push_local_round_trip() {
    // local repo with one commit on master
    let local = Sandbox::new();
    local.ngit_ok(&["init"]);
    local.write("f.txt", "data\n");
    local.ngit_ok(&["add", "f.txt"]);
    let oid = local.commit("c");

    // a bare-ish remote (just an initialized repo in another dir)
    let remote = Sandbox::new();
    remote.ngit_ok(&["init"]);

    let remote_path = remote.root.to_string_lossy().to_string();
    let push = local.ngit(&["push", &remote_path, "master"]);
    assert!(push.success, "push to a local remote should succeed:\n{}", push.diag());
    assert!(
        remote.exists(&format!(".ugit/objects/{oid}")),
        "pushed commit object should exist in the remote"
    );

    // a third repo fetches from local
    let consumer = Sandbox::new();
    consumer.ngit_ok(&["init"]);
    let local_path = local.root.to_string_lossy().to_string();
    let fetch = consumer.ngit(&["fetch", &local_path]);
    assert!(fetch.success, "fetch from a local repo should succeed:\n{}", fetch.diag());
    assert!(
        consumer.exists(&format!(".ugit/objects/{oid}")),
        "fetched commit object should exist locally"
    );
}

// ===========================================================================
// BUG-SURFACING TESTS  (assert correct behaviour; expected to FAIL today)
// ===========================================================================

#[test]
fn bug_nested_directories_are_flattened_on_commit() {
    // BUG: `WalkDir::try_from(&Index)` pushes/indexes the root `wd.dirs` in both
    // match arms instead of `cur_dir.dirs`, so any path nested two or more
    // levels deep is hoisted to the top level when the tree is written. A
    // commit+checkout round trip therefore loses the directory structure.
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("a/b/c.txt", "deep\n");
    s.ngit_ok(&["add", "a"]);
    s.commit("nested");

    // Round-trip through the stored tree.
    s.ngit_ok(&["checkout", "master"]);

    assert!(
        s.exists("a/b/c.txt"),
        "the nested path a/b/c.txt must survive a commit+checkout round trip"
    );
    assert_eq!(s.read("a/b/c.txt"), "deep\n", "nested file content must be intact");
    assert!(
        !s.exists("b/c.txt"),
        "the file must NOT be hoisted to b/c.txt at the repo root"
    );
}

#[test]
fn bug_filenames_with_spaces_are_dropped() {
    // BUG: the tree line format is `type oid name` split on ' ', and
    // `parse_tree` keeps only rows that split into exactly 3 fields. Any
    // filename containing a space splits into >3 fields and is silently
    // discarded on read.
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("my file.txt", "spaced\n");
    s.ngit_ok(&["add", "my file.txt"]);
    s.commit("spaced");

    s.ngit_ok(&["checkout", "master"]);
    assert!(
        s.exists("my file.txt"),
        "a filename containing a space must survive a commit+checkout round trip"
    );
    assert_eq!(s.read("my file.txt"), "spaced\n");
}

#[test]
fn bug_show_rejects_a_revision() {
    // BUG: `show` takes a `CommitOid` token (a literal 64-hex string), unlike
    // `log`/`checkout`/`merge` which take a `Revision`. So `show @` (or a
    // branch/tag) fails at parse time instead of resolving HEAD.
    let (s, _oid) = init_with_one_commit();
    let r = s.ngit(&["show", "@"]);
    assert!(
        r.success,
        "`show @` should resolve HEAD like the other revision-taking commands:\n{}",
        r.diag()
    );
}

#[test]
fn bug_reset_rejects_a_revision() {
    // BUG: same inconsistency as `show` — `reset` requires a literal oid, so
    // `reset @` / `reset <branch>` cannot be expressed.
    let (s, _oid) = init_with_one_commit();
    let r = s.ngit(&["reset", "@"]);
    assert!(
        r.success,
        "`reset @` should resolve HEAD rather than reject it:\n{}",
        r.diag()
    );
}

#[test]
fn bug_read_tree_cannot_take_a_commitish() {
    // BUG: `read-tree` runs its argument through `resolve`, which returns a
    // *commit* oid for any ref, then casts it to a TreeOid and reads it as a
    // tree -> type mismatch. Only a literal tree oid works, but nothing except
    // `write-tree` ever surfaces one. `read-tree @` should resolve the commit
    // and read its tree.
    let (s, _oid) = init_with_one_commit();
    let r = s.ngit(&["read-tree", "@"]);
    assert!(
        r.success,
        "`read-tree @` should read the tree of the HEAD commit:\n{}",
        r.diag()
    );
}

#[test]
fn bug_checkout_destroys_untracked_files() {
    // BUG: `checkout`/`read-tree` call `clean_worktree`, which recursively
    // deletes everything not in the hardcoded ignore list before writing the
    // tracked files. Untracked work is silently destroyed (real git refuses).
    let (s, _oid) = init_with_one_commit();
    s.write("scratch_notes.txt", "important untracked work\n");
    s.ngit_ok(&["checkout", "master"]);
    assert!(
        s.exists("scratch_notes.txt"),
        "checkout must not delete untracked files"
    );
    assert_eq!(s.read("scratch_notes.txt"), "important untracked work\n");
}

#[test]
fn bug_hardcoded_ignore_drops_a_real_file() {
    // BUG: `is_ignored` hardcodes a leftover debug path
    // ("dir/mdir/ignored.txt") alongside .git/.ugit/target, so a genuine file
    // at that path is silently skipped by add/walk. (Asserted at the index
    // level to isolate from the nested-tree bug above.)
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    s.write("dir/mdir/ignored.txt", "should be tracked\n");
    s.write("dir/mdir/kept.txt", "tracked\n");
    s.ngit_ok(&["add", "dir"]);
    let idx = s.read(".ugit/index");
    assert!(idx.contains("kept.txt"), "sanity: a normal nested file should stage");
    assert!(
        idx.contains("ignored.txt"),
        "a file named ignored.txt must not be silently dropped by a hardcoded rule; index: {idx}"
    );
}

#[test]
fn bug_hash_object_on_missing_file_should_error() {
    // BUG: `hash-object <missing>` prints a message but returns Ok(()), so the
    // process exits 0. A missing input should be a non-zero exit.
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    let r = s.ngit(&["hash-object", "does_not_exist.txt"]);
    assert!(
        !r.success,
        "hash-object on a missing file should exit non-zero:\n{}",
        r.diag()
    );
}

#[test]
fn bug_log_is_not_topologically_ordered() {
    // BUG: `log` iterates the `history_for` HashMap directly, so commits print
    // in randomized hash order rather than reverse-topological order. With a
    // long linear chain the odds of the hash order coinciding with the correct
    // order are negligible, so this reliably fails today.
    let s = Sandbox::new();
    s.ngit_ok(&["init"]);
    let mut oids = Vec::new();
    for i in 0..6 {
        s.write("f.txt", &format!("rev {i}\n"));
        s.ngit_ok(&["add", "f.txt"]);
        oids.push(s.commit(&format!("commit {i}")));
    }
    // Expected order for a linear history is newest -> oldest.
    let log = s.ngit_ok(&["log"]).stdout;
    let positions: Vec<usize> = oids
        .iter()
        .map(|oid| log.find(oid).unwrap_or_else(|| panic!("commit {oid} missing from log")))
        .collect();
    // positions indexed oldest..newest; newest should appear first (smallest idx).
    let mut topo = positions.clone();
    topo.sort_unstable();
    let expected_newest_first: Vec<usize> = positions.iter().rev().cloned().collect();
    assert_eq!(
        topo, expected_newest_first,
        "log should print commits newest-first (reverse-topological); got byte positions {positions:?}"
    );
}

#[test]
fn bug_head_pointing_outside_refs_heads_should_not_panic() {
    // BUG: `get_current_branch` does `assert!(name.starts_with("refs/heads"))`,
    // so a HEAD that symbolically points elsewhere panics the process instead
    // of returning an error / "detached"-style result.
    let (s, _oid) = init_with_one_commit();
    // Point HEAD at a non-branch symbolic ref.
    std::fs::write(s.path(".ugit/HEAD"), "ref: refs/tags/foo").unwrap();
    let r = s.ngit(&["branch"]); // ListBranch -> get_current_branch
    assert!(
        !r.panicked(),
        "an unusual HEAD must be handled gracefully, not panic:\n{}",
        r.diag()
    );
}

#[test]
fn bug_symbolic_ref_cycle_should_error_not_overflow() {
    // BUG: `resolve_ref` has no cycle detection, so a self-referential HEAD
    // ("ref: HEAD") recurses until the stack overflows (the child aborts via a
    // signal) instead of returning a clean error.
    let (s, _oid) = init_with_one_commit();
    std::fs::write(s.path(".ugit/HEAD"), "ref: HEAD").unwrap();
    let r = s.ngit(&["status"]); // resolve("@") -> follows HEAD -> cycle
    assert!(
        !r.killed_by_signal,
        "a ref cycle should produce a graceful error, not a crash/stack overflow:\n{}",
        r.diag()
    );
    assert!(
        !r.success,
        "a ref cycle should still be reported as a failure (non-zero exit):\n{}",
        r.diag()
    );
}

#[cfg(unix)]
#[test]
fn bug_special_files_panic_the_worktree_walk() {
    // BUG: `WalkDir::new` does `assert!(full.is_file())` for any non-dir entry.
    // A broken symlink is neither a dir nor a file, so the walk (reached via
    // `status`/`add`/`checkout`) panics instead of skipping or erroring.
    use std::os::unix::fs::symlink;
    let (s, _oid) = init_with_one_commit();
    symlink(Path::new("nonexistent-target"), s.path("dangling")).unwrap();
    let r = s.ngit(&["status"]); // get_working_tree -> WalkDir::new
    assert!(
        !r.panicked(),
        "a special/symlink entry must not panic the worktree walk:\n{}",
        r.diag()
    );
}