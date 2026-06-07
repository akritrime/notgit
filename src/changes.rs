use std::{collections::HashMap, path::PathBuf};

use crate::{errors::NGitError, merge::BlobMerger, repository::Repository, tree::TreeSnapshot};

#[derive(Default)]
pub struct TreeDiff {
    pub changed: HashMap<PathBuf, String>,
    pub added: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
}

impl TreeDiff {
    pub fn between(
        repo: &Repository,
        from: TreeSnapshot,
        to: TreeSnapshot,
    ) -> Result<Self, NGitError> {
        let mut diff = TreeDiff::default();
        for (path, oids) in TreeSnapshot::compare(vec![from, to]) {
            assert!(oids.len() == 2 && oids.iter().any(|o| o.is_some()));
            let [from, to] = oids.as_slice() else {
                unreachable!()
            };
            match (from, to) {
                (Some(from), Some(to)) => {
                    if from.oid() == to.oid() {
                        continue;
                    }
                    let p = path.to_str().unwrap_or("blob");

                    let a = from.content(repo)?;
                    let b = to.content(repo)?;
                    let diff_text = diff_blobs(&a, &b, p)?;
                    diff.changed.insert(path, diff_text);
                }
                (Some(_), None) => {
                    diff.deleted.push(path);
                }
                (None, Some(_)) => diff.added.push(path),
                (None, None) => unreachable!(),
            }
        }
        Ok(diff)
    }

    pub fn to_list(self) -> Vec<(&'static str, PathBuf)> {
        let mut files = vec![];
        for path in self.added {
            files.push(("new file", path));
        }

        for (path, _) in self.changed {
            files.push(("modified", path))
        }

        for path in self.deleted {
            files.push(("deleted", path))
        }

        files
    }
}

pub struct TreeMerger<'a> {
    repo: &'a Repository,
}

pub struct TreeMerge {
    pub snapshot: TreeSnapshot,
    pub conflicts: Vec<PathBuf>,
}

impl<'a> TreeMerger<'a> {
    pub fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    pub fn merge(
        &self,
        base: TreeSnapshot,
        head: TreeSnapshot,
        other: TreeSnapshot,
    ) -> Result<TreeMerge, NGitError> {
        let mut tree = HashMap::new();
        let mut conflicts = vec![];
        for (path, oids) in TreeSnapshot::compare(vec![base, head, other]) {
            assert!(oids.len() == 3 && oids.iter().any(|o| o.is_some()));
            let contents: Result<Vec<Vec<u8>>, NGitError> = oids
                .into_iter()
                .map(|o| match o {
                    Some(o) => o.content(self.repo),
                    None => Ok(Vec::new()),
                })
                .collect();
            let contents = contents?;
            let [base, head, other] = contents.as_slice() else {
                unreachable!()
            };
            let merged = BlobMerger::merge(base, head, other)?;
            if merged.has_conflict() {
                conflicts.push(path.clone());
            }
            tree.insert(path, self.repo.write_blob(merged.into_content())?);
        }
        Ok(TreeMerge {
            snapshot: TreeSnapshot::new(tree),
            conflicts,
        })
    }
}

fn diff_blobs(a: &[u8], b: &[u8], path: impl AsRef<str>) -> Result<String, NGitError> {
    let path = path.as_ref();
    let workspace = DiffWorkspace::create()?;
    let result = diff_in_workspace(&workspace, a, b, path);
    let cleanup = workspace.remove();

    match (result, cleanup) {
        (Ok(diff), Ok(())) => Ok(diff),
        (Ok(_), Err(err)) => Err(err),
        (Err(err), _) => Err(err),
    }
}

fn diff_in_workspace(
    workspace: &DiffWorkspace,
    a: &[u8],
    b: &[u8],
    path: &str,
) -> Result<String, NGitError> {
    let a_file = workspace.path("a");
    let b_file = workspace.path("b");
    std::fs::write(&a_file, a)?;
    std::fs::write(&b_file, b)?;

    let output = std::process::Command::new("diff")
        .arg("--unified")
        .arg("--show-c-function")
        .arg("--label")
        .arg(format!("a/{path}"))
        .arg("--label")
        .arg(format!("b/{path}"))
        .arg(&a_file)
        .arg(&b_file)
        .output()?;

    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(NGitError::SystemError("diff".into(), stderr.to_string()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

struct DiffWorkspace {
    root: PathBuf,
}

impl DiffWorkspace {
    fn create() -> Result<Self, NGitError> {
        let root = unique_temp_path()?;
        std::fs::create_dir(&root)?;
        Ok(Self { root })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn remove(self) -> Result<(), NGitError> {
        std::fs::remove_dir_all(self.root)?;
        Ok(())
    }
}

fn unique_temp_path() -> Result<PathBuf, NGitError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    Ok(std::env::temp_dir().join(format!("notgit.diff.{}.{}", std::process::id(), nanos)))
}
