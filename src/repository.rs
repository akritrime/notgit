use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::{
    errors::NGitError,
    objects::{ObjectStore, ObjectType},
    refs::{RefStore, RefValue},
    tree::TreeSnapshot,
    types::{BlobOid, CommitOid, Oid, RefName, TreeOid},
    worktree::WalkDir,
};

#[derive(Clone, Debug)]
pub struct Repository {
    worktree: PathBuf,
    git_dir: PathBuf,
    objects: ObjectStore,
    refs: RefStore,
}

impl Repository {
    pub const GIT_DIR_NAME: &'static str = ".ugit";

    pub fn at_worktree(worktree: impl Into<PathBuf>) -> Self {
        let worktree = worktree.into();
        let git_dir = worktree.join(Self::GIT_DIR_NAME);
        let objects = ObjectStore::new(git_dir.join("objects"));
        let refs = RefStore::new(git_dir.clone());
        Self {
            worktree,
            git_dir,
            objects,
            refs,
        }
    }

    pub fn current() -> Result<Self, NGitError> {
        Ok(Self::at_worktree(std::env::current_dir()?))
    }

    pub fn read() -> Result<Self, NGitError> {
        let repo = Self::current()?;
        if !repo.exists() {
            return Err(NGitError::Uninitialized(repo.git_dir().to_path_buf()));
        }
        Ok(repo)
    }

    pub fn fetch(dir: PathBuf) -> Result<Self, NGitError> {
        let repo = Self::at_worktree(dir);
        if !repo.exists() {
            return Err(NGitError::RemoteUninitialized(
                repo.worktree.to_string_lossy().into(),
            ));
        }
        Ok(repo)
    }

    pub fn worktree(&self) -> &Path {
        &self.worktree
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn exists(&self) -> bool {
        self.git_dir.is_dir()
    }

    pub fn created_at(&self) -> Result<Option<SystemTime>, NGitError> {
        if self.exists() {
            Ok(Some(std::fs::metadata(&self.git_dir)?.created()?))
        } else {
            Ok(None)
        }
    }

    pub fn create_storage_dirs(&self) -> Result<(), NGitError> {
        std::fs::create_dir_all(self.objects_dir())?;
        std::fs::create_dir_all(self.refs_dir())?;
        Ok(())
    }

    pub fn objects(&self) -> &ObjectStore {
        &self.objects
    }

    pub fn objects_dir(&self) -> PathBuf {
        self.objects.dir()
    }

    pub fn object_path(&self, oid: &Oid) -> PathBuf {
        self.objects.path(oid)
    }

    pub fn blob_path(&self, oid: &BlobOid) -> PathBuf {
        self.objects.blob_path(oid)
    }

    pub fn tree_path(&self, oid: &TreeOid) -> PathBuf {
        self.objects.tree_path(oid)
    }

    pub fn commit_path(&self, oid: &CommitOid) -> PathBuf {
        self.objects.commit_path(oid)
    }

    pub fn refs_dir(&self) -> PathBuf {
        self.refs.refs_dir()
    }

    pub fn ref_path(&self, name: &RefName) -> PathBuf {
        self.refs.ref_path(name)
    }

    pub fn index_path(&self) -> PathBuf {
        self.git_dir.join("index")
    }

    pub fn git_path(&self, parts: &[&str]) -> PathBuf {
        parts
            .iter()
            .fold(self.git_dir.clone(), |path, part| path.join(part))
    }

    pub fn blob_id(&self, content: impl AsRef<[u8]>) -> Result<BlobOid, NGitError> {
        self.objects.hash_blob(content)
    }

    pub fn write_blob(&self, content: impl AsRef<[u8]>) -> Result<BlobOid, NGitError> {
        self.objects.write_blob(content)
    }

    pub fn write_tree(&self, content: impl AsRef<[u8]>) -> Result<TreeOid, NGitError> {
        self.objects.write_tree(content)
    }

    pub fn write_commit(&self, content: impl AsRef<[u8]>) -> Result<CommitOid, NGitError> {
        self.objects.write_commit(content)
    }

    pub fn get_object(&self, digest: &Oid, ty: ObjectType) -> Result<Vec<u8>, NGitError> {
        self.objects.read(digest, ty)
    }

    pub fn get_blob(&self, oid: &BlobOid) -> Result<Vec<u8>, NGitError> {
        self.objects.read_blob(oid)
    }

    pub fn get_tree_text(&self, oid: &TreeOid) -> Result<String, NGitError> {
        String::from_utf8(self.objects.read_tree(oid)?)
            .map_err(|_| NGitError::InvalidObject(oid.to_string()))
    }

    pub fn get_commit_text(&self, oid: &CommitOid) -> Result<String, NGitError> {
        String::from_utf8(self.objects.read_commit(oid)?)
            .map_err(|_| NGitError::InvalidObject(oid.to_string()))
    }

    pub fn update_ref(&self, r: &RefName, value: &RefValue, deref: bool) -> Result<(), NGitError> {
        self.refs.update_ref(r, value, deref)
    }

    pub fn get_ref(&self, r: &RefName, deref: bool) -> Result<Option<RefValue>, NGitError> {
        self.refs.get_ref(r, deref)
    }

    pub fn del_ref(&self, r: &RefName, deref: bool) -> Result<(), NGitError> {
        self.refs.del_ref(r, deref)
    }

    pub fn resolve(&self, r: impl AsRef<str>) -> Result<Oid, NGitError> {
        self.refs.resolve(r)
    }

    pub fn resolve_commit(&self, r: impl AsRef<str>) -> Result<CommitOid, NGitError> {
        Ok(CommitOid::from_oid(self.resolve(r)?))
    }

    pub fn iter_refs(
        &self,
        deref: bool,
        prefix: Option<impl AsRef<str>>,
    ) -> Result<Vec<(RefName, RefValue)>, NGitError> {
        self.refs.iter_refs(deref, prefix)
    }

    pub fn get_branch(&self, branch: &str) -> Result<Option<RefValue>, NGitError> {
        self.refs.get_branch(branch)
    }

    pub fn get_current_branch(&self) -> Result<Option<String>, NGitError> {
        self.refs.get_current_branch()
    }

    pub fn iter_branch_names(&self) -> Result<Vec<String>, NGitError> {
        self.refs.iter_branch_names()
    }

    pub fn create_branch(&self, name: &str, start_point: &RefValue) -> Result<(), NGitError> {
        self.update_ref(&RefName::branch(name)?, start_point, true)
    }

    pub fn get_working_tree(&self) -> Result<TreeSnapshot, NGitError> {
        fn walk(repo: &Repository, wd: WalkDir) -> Result<TreeSnapshot, NGitError> {
            let mut tree = TreeSnapshot::default();
            let files = wd.files;
            for f in files {
                let content = std::fs::read(&f)?;
                let h = repo.blob_id(&content)?;
                tree.insert_inline(f, h, content);
            }
            for dir in wd.dirs {
                tree.extend(walk(repo, dir)?)
            }
            Ok(tree)
        }

        let wd = WalkDir::new(self.worktree.clone())?;
        walk(self, wd)
    }

    pub fn fetch_from_remote(
        &self,
        remote: &Repository,
        oid: &Oid,
        force: bool,
    ) -> Result<(), NGitError> {
        self.objects.copy_from(remote.objects(), oid, force)
    }

    pub fn push_to_remote(&self, remote: &Self, oid: &Oid) -> Result<(), NGitError> {
        self.objects.copy_to(remote.objects(), oid)
    }
}
