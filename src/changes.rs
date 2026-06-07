use std::{collections::HashMap, path::PathBuf};

use crate::{errors::NGitError, merge::BlobChange, repository::Repository, tree::TreeSnapshot};

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
                    let diff_text = BlobChange::diff(&a, &b, p)?;
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
            let merged = BlobChange::merge(base, head, other)?;
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
