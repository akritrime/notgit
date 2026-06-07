use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use crate::{
    changes::TreeDiff,
    commit::{Commit, CommitGraph},
    errors::NGitError,
    index::Index,
    objects::ObjectType,
    refs::RefValue,
    repository::Repository,
    tree::TreeStore,
    types::{BranchName, CommitOid, RefName, Revision, TreeOid},
};

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Init,
    HashObject {
        file: PathBuf,
    },
    CatFile {
        revision: Revision,
    },
    WriteTree,
    ReadTree {
        revision: Revision,
    },
    Commit {
        message: String,
    },
    Log {
        revision: Revision,
    },
    Checkout {
        revision: Revision,
    },
    Tag {
        name: String,
        target: Revision,
    },
    K,
    ListBranch,
    Branch {
        name: BranchName,
        start_point: Revision,
    },
    Status,
    Reset {
        revision: Revision,
    },
    Show {
        revision: Revision,
    },
    Diff {
        commit: Option<Revision>,
        cached: bool,
    },
    Merge {
        revision: Revision,
    },
    MergeBase {
        commit1: CommitOid,
        commit2: CommitOid,
    },
    Fetch {
        remote: PathBuf,
    },
    Push {
        remote: PathBuf,
        branch: BranchName,
    },
    Add {
        files: Vec<PathBuf>,
    },

    Unknown(String),
    Empty,
}

impl Command {
    fn print_commit(refs: Option<&Vec<String>>, commit: Commit) {
        let refs = refs.map_or("".into(), |v| v.join(", "));
        println!("commit {}{}\n", commit.oid, refs);
        // let commit = Commit::load(&d, &c)?;
        for m in commit.message.lines() {
            println!("    {}", m);
        }
        println!("---");
    }

    pub fn execute(self) -> Result<(), NGitError> {
        match self {
            Command::Init => {
                let d = Repository::current()?;
                if !d.exists() {
                    d.create_storage_dirs()?;
                    let value = RefValue::symbolic(RefName::branch("master")?);
                    d.update_ref(&RefName::head(), &value, true)?;
                    println!("created ugit repository at {:?}", d.worktree());
                } else {
                    println!("ugit repositry already exists");
                    return Err(NGitError::OperationFailed(
                        "can't init an existing repo".into(),
                    ));
                }
            }
            Command::HashObject { file } => {
                let d = Repository::read()?;
                if !file.try_exists()? {
                    return Err(NGitError::OperationFailed(format!(
                        "can't hash non-existent file at {:?}",
                        file
                    )));
                } else {
                    let content = std::fs::read(file)?;
                    let d = d.write_blob(content)?;
                    println!("{}", d);
                }
            }
            Command::CatFile { revision } => {
                use std::io::Write;

                let d = Repository::read()?;
                let digest = d.resolve(revision)?;
                let content = d.get_object(&digest, ObjectType::Any)?;
                // println!("File at {}:", digest);
                std::io::stdout().write_all(&content)?;
            }
            Command::WriteTree => {
                let d = Repository::read()?;

                let oid = TreeStore::new(&d).write_index()?;
                println!("{}", oid);
            }

            Command::ReadTree { revision } => {
                let d = Repository::read()?;
                let digest = resolve_treeish(&d, revision)?;
                let fs = TreeStore::new(&d).checkout(&digest)?;
                println!("restored {} files", fs.len());
                for f in fs {
                    println!("- {}", f.to_str().unwrap())
                }
            }

            Command::Commit { message } => {
                let d = Repository::read()?;
                let oid = TreeStore::new(&d).write_index()?;
                let mut commit = String::new();
                commit += &format!("tree {}\n", oid);
                if let Some(head) = d.get_ref(&RefName::head(), true)? {
                    if let Some(head) = head.oid() {
                        commit += &format!("parent {}\n", head);
                    }
                }
                if let Some(merge_head) = d.get_ref(&RefName::merge_head(), true)? {
                    if let Some(merge_head) = merge_head.oid() {
                        commit += &format!("parent {}\n", merge_head);
                    }
                    d.del_ref(&RefName::merge_head(), false)?;
                }
                commit += &format!("\n{}\n", message);
                let oid = d.write_commit(commit)?;
                let r = RefValue::direct(oid);
                d.update_ref(&RefName::head(), &r, true)?;
                println!("{}", r.value_text());
            }

            Command::Log { revision } => {
                let d = Repository::read()?;
                let oid = d.resolve_commit(revision)?;

                let mut h: HashMap<CommitOid, Vec<String>> = HashMap::new();
                for (r, v) in d.iter_refs(true, None::<String>)? {
                    if let Some(oid) = v.oid() {
                        h.entry(oid.clone()).or_default().push(r.to_string());
                    }
                }

                for c in CommitGraph::new(&d).commits_and_parents(vec![oid])? {
                    Self::print_commit(h.get(&c), Commit::load(&d, c)?);
                }
                // while let Some(ref commit) = oid {
                //     println!("commit {}\n", commit);
                //     let commit = Commit::load(&d, commit)?;
                //     for m in commit.message.lines() {
                //         println!("    {}", m);
                //     }
                //     println!("---");
                //     oid = commit.parent;
                // }
            }
            Command::Checkout { revision } => {
                let d = Repository::read()?;
                let oid = d.resolve_commit(&revision)?;
                let commit = Commit::load(&d, oid)?;
                TreeStore::new(&d).checkout(&commit.tree)?;
                let ref_val = if d.get_branch(revision.as_str())?.is_some() {
                    RefValue::symbolic(RefName::branch(revision.as_str())?)
                } else {
                    RefValue::direct(commit.oid)
                };
                d.update_ref(&RefName::head(), &ref_val, false)?;
                println!("checked out {}", ref_val.value_text());
            }
            Command::Tag { name, target } => {
                let d = Repository::read()?;
                let oid = d.resolve_commit(target)?;
                let ref_val = RefValue::direct(oid);
                d.update_ref(&RefName::tag(&name)?, &ref_val, true)?;
                println!("tagged '{}' with '{}'", ref_val.value_text(), name);
            }
            Command::K => {
                let d = Repository::read()?;

                let mut dot = String::from("digraph commits {\n");
                let refs = d.iter_refs(false, None::<String>)?;
                let mut oids = HashSet::new();
                for (name, oid) in refs {
                    dot += &format!("\"{}\" [shape=note]\n", &name);
                    dot += &format!("\"{}\" -> \"{}\"\n", &name, oid.value_text());
                    // println!("{} {}", name, &oid);
                    if let Some(oid) = oid.into_oid() {
                        oids.insert(oid);
                    }
                }
                println!("\n\n");

                let tree = CommitGraph::new(&d).history_for(oids)?;
                for (oid, parent) in tree {
                    dot += &format!(
                        "\"{}\" [shape=box style=filled label=\"{}\"]\n",
                        &oid,
                        oid.short(10)
                    );
                    for p in parent {
                        // println!("Parent {}", oid);
                        dot += &format!("\"{}\" -> \"{}\"\n", &oid, &p);
                    }
                }

                dot += "}";
                println!("{}", dot);

                use std::io::Write;

                use std::process::{Command as TerminalCommand, Stdio};

                let mut child = TerminalCommand::new("dot")
                    .args(["-Tgtk", "/dev/stdin"])
                    .stdin(Stdio::piped())
                    .spawn()?;

                let mut stdin = child.stdin.take().unwrap();
                stdin.write_all(&dot.into_bytes())?;
                drop(stdin);

                let status = child.wait()?;
                println!("Exited with {}", status)
            }
            Command::Branch { name, start_point } => {
                let d = Repository::read()?;
                let start_point = RefValue::direct(d.resolve_commit(start_point)?);
                d.create_branch(name.as_str(), &start_point)?;
                println!(
                    "created branch '{}' at {}",
                    name,
                    start_point.oid().unwrap().short(10)
                )
            }
            Command::ListBranch => {
                let d = Repository::read()?;
                let current_branch = d.get_current_branch()?;
                for b in d.iter_branch_names()? {
                    let prefix = if let Some(ref cb) = current_branch
                        && b == *cb
                    {
                        "*"
                    } else {
                        " "
                    };
                    println!("{} {}", prefix, b)
                }
            }
            Command::Status => {
                let d = Repository::read()?;
                let head = d.resolve_commit("@")?;
                match d.get_current_branch()? {
                    Some(branch) => println!("on branch {}", branch),
                    None => {
                        println!("HEAD detached at {}", head.short(10))
                    }
                }
                if let Some(merge_head) = d.get_ref(&RefName::merge_head(), true)? {
                    if let Some(merge_head) = merge_head.oid() {
                        println!("merging with {}", merge_head.short(10))
                    }
                }
                let head = Commit::load(&d, head)?;

                let index = Index::read(&d)?;
                let head_tree = TreeStore::new(&d).read_snapshot(&head.tree)?;
                let index_tree = index.to_index_tree();
                let working_tree = d.get_working_tree()?;

                let diff = TreeDiff::between(&d, head_tree, index_tree.clone())?;
                println!("changes to be committed:");
                for (action, file) in diff.to_list() {
                    let fp = file.to_string_lossy().to_string();
                    println!("    {action} {fp}")
                }
                println!();
                let diff = TreeDiff::between(&d, index_tree, working_tree)?;
                println!("changes not staged for commit:");
                for (action, file) in diff.to_list() {
                    let fp = file.to_string_lossy().to_string();
                    println!("    {action} {fp}")
                }
            }

            Command::Reset { revision } => {
                let d = Repository::read()?;
                let oid = d.resolve_commit(revision)?;
                let val = RefValue::direct(oid.clone());
                d.update_ref(&RefName::head(), &val, true)?;
                let h = d.get_ref(&RefName::head(), true)?;
                match h {
                    Some(rf) if rf.oid() == Some(&oid) => println!("HEAD reset to {}", oid),
                    _ => return Err(NGitError::OperationFailed("reset".into())),
                }
            }
            Command::Show { revision } => {
                let d = Repository::read()?;
                let oid = d.resolve_commit(revision)?;
                let commit = Commit::load(&d, oid)?;
                let from = if let Some(p) = commit.parents.first() {
                    let commit = Commit::load(&d, p.clone())?;
                    TreeStore::new(&d).read_snapshot(&commit.tree)?
                } else {
                    Default::default()
                };
                let to = TreeStore::new(&d).read_snapshot(&commit.tree)?;

                Self::print_commit(None, commit);
                let diff = TreeDiff::between(&d, from, to)?;
                for (action, file) in diff.to_list() {
                    let fp = file.to_string_lossy().to_string();
                    println!("    {action} {fp}")
                }
            }
            Command::Diff { cached, commit } => {
                let d = Repository::read()?;
                let index = Index::read(&d)?;
                let (from, to) = match (cached, commit) {
                    (false, None) => (index.to_index_tree(), d.get_working_tree()?),
                    (f, a) => {
                        let commit = Commit::load(
                            &d,
                            d.resolve_commit(a.unwrap_or_else(Revision::at_head))?,
                        )?;
                        let from = TreeStore::new(&d).read_snapshot(&commit.tree)?;
                        let to = if f {
                            index.to_index_tree()
                        } else {
                            d.get_working_tree()?
                        };
                        (from, to)
                    }
                };
                let diff = TreeDiff::between(&d, from, to)?;
                for (action, file) in diff.to_list() {
                    let fp = file.to_string_lossy().to_string();
                    println!("    {action} {fp}")
                }
            }

            Command::Merge { revision } => {
                let d = Repository::read()?;
                let commit = d.resolve_commit(revision)?;
                let head = d.resolve_commit("HEAD")?;

                let o_commit = Commit::load(&d, commit)?;
                let merge_base =
                    CommitGraph::new(&d).merge_base(o_commit.oid.clone(), head.clone())?;

                if merge_base.as_ref().map_or(false, |b| *b == head) {
                    TreeStore::new(&d).checkout(&o_commit.tree)?;
                    d.update_ref(&RefName::head(), &RefValue::direct(o_commit.oid), true)?;
                    println!("fast forward merge, no need to commit")
                } else {
                    let h_commit = Commit::load(&d, head)?;
                    d.update_ref(
                        &RefName::merge_head(),
                        &RefValue::direct(o_commit.oid.clone()),
                        true,
                    )?;
                    let base_tree = if let Some(b) = merge_base {
                        Some(Commit::load(&d, b)?.tree)
                    } else {
                        None
                    };
                    let checkout = TreeStore::new(&d).checkout_merged(
                        &base_tree,
                        &h_commit.tree,
                        &o_commit.tree,
                    )?;
                    if checkout.conflicts.is_empty() {
                        println!("merged in working tree");
                    } else {
                        println!("merged in working tree with conflicts");
                        println!("conflicts:");
                        for f in &checkout.conflicts {
                            println!("    {:?}", f)
                        }
                    }
                    println!("files touched:");
                    for f in checkout.files {
                        println!("    {:?}", f)
                    }
                    println!("please commit")
                }
            }
            Command::MergeBase { commit1, commit2 } => {
                let d = Repository::read()?;
                println!(
                    "common ancestor found: {:?}",
                    CommitGraph::new(&d).merge_base(commit1, commit2)?
                )
            }
            Command::Fetch { remote } => {
                let rd = Repository::fetch(remote)?;
                let ld = Repository::read()?;
                let remote_prefix = "refs/heads";
                let local_prefix = "refs/remote";
                println!("Will fetch the following refs:");
                let refs = rd.iter_refs(true, Some(remote_prefix))?;
                let commits = refs
                    .iter()
                    .filter_map(|(_, rv)| rv.oid().cloned())
                    .collect();
                for oid in CommitGraph::new(&rd).objects_in_commits(commits)? {
                    ld.fetch_from_remote(&rd, &oid, false)?;
                }

                for (rname, rval) in refs {
                    ld.update_ref(
                        &RefName::new(format!(
                            "{}/{}",
                            local_prefix,
                            &rname.as_str()[remote_prefix.len() + 1..]
                        ))?,
                        &rval,
                        true,
                    )?;
                    // println!("- {}", refs.0)
                }
            }
            Command::Push { remote, branch } => {
                let rd = Repository::fetch(remote)?;
                let ld = Repository::read()?;

                let ref_addr = RefName::branch(branch.as_str())?;
                let ref_val = match ld.get_ref(&ref_addr, true)? {
                    Some(rv) => rv,
                    None => {
                        return Err(NGitError::OperationFailed(
                            "push branch doesn't exist".into(),
                        ));
                    }
                };
                let ref_oid = ref_val.oid().cloned().ok_or_else(|| {
                    NGitError::OperationFailed("push branch is not a direct ref".into())
                })?;

                let remote_ref = rd.get_ref(&ref_addr, true)?;
                if let Some(rv) = remote_ref
                    && let Some(remote_oid) = rv.into_oid()
                    && CommitGraph::new(&rd)
                        .objects_in_commits(vec![remote_oid])?
                        .contains(ref_oid.as_oid())
                {
                    return Err(NGitError::NoForcePush(format!(
                        "branch '{ref_addr}' is an ancestor of current branch"
                    )));
                }

                let remote_refs = rd.iter_refs(true, None::<&str>)?;
                let known_remote_refs = remote_refs
                    .iter()
                    .filter_map(|(_, rv)| {
                        rv.oid()
                            .filter(|oid| ld.commit_path(oid).is_file())
                            .cloned()
                    })
                    .collect();
                let remote_objs = CommitGraph::new(&ld)
                    .objects_in_commits(known_remote_refs)?
                    .into_iter()
                    .collect::<HashSet<_>>();
                let local_objs = CommitGraph::new(&ld)
                    .objects_in_commits(vec![ref_oid.clone()])?
                    .into_iter()
                    .collect::<HashSet<_>>();

                let obj_to_push = local_objs.difference(&remote_objs);

                for obj in obj_to_push {
                    ld.push_to_remote(&rd, &obj)?;
                }

                rd.update_ref(&ref_addr, &RefValue::direct(ref_oid), true)?;
            }
            Command::Add { files } => {
                if files.len() == 0 {
                    return Err(NGitError::MissingArgument("add".into(), "files".into()));
                }
                let d = Repository::read()?;
                let mut i = Index::read(&d)?;
                for f in files {
                    i.stage_path(f)?;
                }
                i.write()?;
            }
            Command::Unknown(s) => println!("{} is not a valid command", s),
            Command::Empty => println!("no command provided"),
        }
        Ok(())
    }
}

fn resolve_treeish(repo: &Repository, revision: Revision) -> Result<TreeOid, NGitError> {
    let oid = repo.resolve(revision)?;
    let tree_oid = TreeOid::from_oid(oid.clone());
    match repo.get_tree_text(&tree_oid) {
        Ok(_) => Ok(tree_oid),
        Err(NGitError::UnexpectedDataType(_, actual)) if actual == "commit" => {
            Ok(Commit::load(repo, CommitOid::from_oid(oid))?.tree)
        }
        Err(err) => Err(err),
    }
}
