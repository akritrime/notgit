use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::{
    changes::TreeMerger,
    errors::NGitError,
    index::Index,
    objects::ObjectType,
    repository::Repository,
    types::{BlobOid, Oid, TreeOid},
    worktree::WalkDir,
};

#[derive(Clone, Debug, Default)]
pub struct TreeSnapshot {
    entries: HashMap<PathBuf, BlobSnapshot>,
}

#[derive(Clone, Debug)]
pub struct BlobSnapshot {
    oid: BlobOid,
    content: BlobContent,
}

#[derive(Clone, Debug)]
enum BlobContent {
    ObjectStore,
    Inline(Vec<u8>),
}

impl BlobSnapshot {
    pub fn object_store(oid: BlobOid) -> Self {
        Self {
            oid,
            content: BlobContent::ObjectStore,
        }
    }

    pub fn inline(oid: BlobOid, content: Vec<u8>) -> Self {
        Self {
            oid,
            content: BlobContent::Inline(content),
        }
    }

    pub fn oid(&self) -> &BlobOid {
        &self.oid
    }

    pub fn content(&self, repo: &Repository) -> Result<Vec<u8>, NGitError> {
        match &self.content {
            BlobContent::ObjectStore => repo.get_blob(&self.oid),
            BlobContent::Inline(content) => Ok(content.clone()),
        }
    }
}

impl TreeSnapshot {
    pub fn new(entries: HashMap<PathBuf, BlobOid>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(path, oid)| (path, BlobSnapshot::object_store(oid)))
                .collect(),
        }
    }

    pub fn insert(&mut self, path: PathBuf, oid: BlobOid) {
        self.entries.insert(path, BlobSnapshot::object_store(oid));
    }

    pub fn insert_inline(&mut self, path: PathBuf, oid: BlobOid, content: Vec<u8>) {
        self.entries
            .insert(path, BlobSnapshot::inline(oid, content));
    }

    pub fn extend(&mut self, other: TreeSnapshot) {
        self.entries.extend(other.entries);
    }

    pub fn into_object_ids(self) -> HashMap<PathBuf, BlobOid> {
        self.entries
            .into_iter()
            .map(|(path, blob)| (path, blob.oid))
            .collect()
    }

    pub fn compare(trees: Vec<TreeSnapshot>) -> HashMap<PathBuf, Vec<Option<BlobSnapshot>>> {
        let mut compared = HashMap::new();
        let count = trees.len();
        for (i, tree) in trees.into_iter().enumerate() {
            for (path, blob) in tree.entries {
                let blobs = compared.entry(path).or_insert(vec![None; count]);
                blobs[i] = Some(blob);
            }
        }

        compared
    }
}

#[derive(Debug)]
pub struct StoredTreeEntry {
    path: PathBuf,
    object: StoredTreeObject,
}

#[derive(Debug)]
enum StoredTreeObject {
    Blob(BlobOid),
    Tree(TreeOid),
}

pub struct TreeStore<'a> {
    repo: &'a Repository,
}

impl<'a> TreeStore<'a> {
    pub fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    pub fn write_index(&self) -> Result<TreeOid, NGitError> {
        let index = Index::read(self.repo)?;
        let wd = WalkDir::try_from(&index)?;
        wd.write(self.repo.objects())
    }

    pub fn read_snapshot(&self, oid: &TreeOid) -> Result<TreeSnapshot, NGitError> {
        self.read_snapshot_at(oid, self.repo.worktree())
    }

    pub fn checkout(&self, oid: &TreeOid) -> Result<Vec<PathBuf>, NGitError> {
        let tree = self.read_snapshot(oid)?;
        let mut index = Index::read(self.repo)?;
        index.update_raw(tree, false);
        index.write()?;

        self.checkout_from_index(&index)
    }

    pub fn checkout_merged(
        &self,
        base_tree: &Option<TreeOid>,
        head_tree: &TreeOid,
        other_tree: &TreeOid,
    ) -> Result<MergedCheckout, NGitError> {
        let base = if let Some(base_tree) = base_tree {
            self.read_snapshot(base_tree)?
        } else {
            TreeSnapshot::default()
        };
        let head = self.read_snapshot(head_tree)?;
        let other = self.read_snapshot(other_tree)?;

        let merged = TreeMerger::new(self.repo).merge(base, head, other)?;
        let mut index = Index::read(self.repo)?;
        index.update_raw(merged.snapshot, false);
        index.write()?;
        let files = self.checkout_from_index(&index)?;
        Ok(MergedCheckout {
            files,
            conflicts: merged.conflicts,
        })
    }

    pub fn read_snapshot_at(&self, oid: &TreeOid, base: &Path) -> Result<TreeSnapshot, NGitError> {
        let object = self.repo.get_tree_text(oid)?;
        let mut snapshot = TreeSnapshot::default();

        for entry in parse_tree(object, base)? {
            match entry.object {
                StoredTreeObject::Blob(oid) => {
                    snapshot.insert(entry.path, oid);
                }
                StoredTreeObject::Tree(oid) => {
                    snapshot.extend(self.read_snapshot_at(&oid, &entry.path)?);
                }
            }
        }

        Ok(snapshot)
    }

    pub fn object_ids_in_tree(
        &self,
        oid: &TreeOid,
        visited: &mut HashSet<Oid>,
    ) -> Result<Vec<Oid>, NGitError> {
        visited.insert(oid.as_oid().clone());
        let mut oids = vec![oid.as_oid().clone()];
        let tree = self.repo.get_tree_text(oid)?;

        for entry in parse_tree(tree, self.repo.worktree())? {
            match entry.object {
                StoredTreeObject::Tree(tree_oid) => {
                    if visited.contains(tree_oid.as_oid()) {
                        continue;
                    }
                    oids.extend(self.object_ids_in_tree(&tree_oid, visited)?);
                }
                StoredTreeObject::Blob(blob_oid) => {
                    if visited.insert(blob_oid.as_oid().clone()) {
                        oids.push(blob_oid.into_oid());
                    }
                }
            }
        }

        Ok(oids)
    }

    fn checkout_from_index(&self, index: &Index<'_>) -> Result<Vec<PathBuf>, NGitError> {
        let mut files = vec![];

        self.clean_worktree()?;

        for (fp, oid) in index.items() {
            if let Some(dir) = fp.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(fp, self.repo.get_blob(oid)?)?;
            files.push(fp.clone());
        }
        Ok(files)
    }

    fn clean_worktree(&self) -> Result<(), NGitError> {
        WalkDir::new(self.repo.worktree().to_path_buf())?.clean()
    }
}

pub struct MergedCheckout {
    pub files: Vec<PathBuf>,
    pub conflicts: Vec<PathBuf>,
}

fn parse_tree(content: String, base: &Path) -> Result<Vec<StoredTreeEntry>, NGitError> {
    content
        .split('\n')
        .map(|r| r.trim().split(' ').collect::<Vec<_>>())
        .filter(|r| r.len() == 3)
        .map(|r| {
            let oid = Oid::new(r[1])?;
            let object_type = r[0].parse()?;
            let object = match object_type {
                ObjectType::Blob => StoredTreeObject::Blob(BlobOid::from_oid(oid)),
                ObjectType::Tree => StoredTreeObject::Tree(TreeOid::from_oid(oid)),
                ObjectType::Commit => {
                    return Err(NGitError::UnexpectedDataType(
                        "tree or blob".into(),
                        "commit".into(),
                    ));
                }
                ObjectType::Any => {
                    return Err(NGitError::UnexpectedDataType(
                        "tree or blob".into(),
                        "_".into(),
                    ));
                }
            };
            Ok(StoredTreeEntry {
                path: base.join(r[2]),
                object,
            })
        })
        .collect()
}
