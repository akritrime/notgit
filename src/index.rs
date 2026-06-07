use std::{collections::HashMap, path::PathBuf};

use serde_json::{Map, Value};

use crate::{
    errors::NGitError,
    repository::Repository,
    tree::TreeSnapshot,
    types::BlobOid,
    worktree::{WalkDir, is_ignored},
};

#[derive(Debug)]
pub struct Index<'a> {
    i: HashMap<PathBuf, BlobOid>,
    pub repo: &'a Repository,
}

impl<'a> Index<'a> {
    pub fn read(repo: &'a Repository) -> Result<Self, NGitError> {
        let mut i = Index {
            i: HashMap::default(),
            repo,
        };

        if i.exists() {
            i.read_from_path()?;
        }

        Ok(i)
    }

    fn get_path(&self) -> PathBuf {
        self.repo.index_path()
    }

    fn exists(&self) -> bool {
        let p = self.get_path();
        p.exists() && p.is_file()
    }

    fn read_from_path(&mut self) -> Result<(), NGitError> {
        let index: Map<String, Value> = std::fs::read_to_string(self.get_path())?
            .parse()
            .map_err(|e| NGitError::OperationFailed(format!("parsing index: {e}")))?;

        for (p, oid) in index {
            self.i.insert(
                self.repo.worktree().join(p),
                BlobOid::new(
                    oid.as_str()
                        .ok_or_else(|| NGitError::OperationFailed("invalid index oid".into()))?,
                )?,
            );
        }

        Ok(())
    }

    pub fn write(&self) -> Result<(), NGitError> {
        let mut index: Map<String, Value> = Map::new();
        for (p, oid) in &self.i {
            let oid = oid.to_string().into();
            let fp = p.strip_prefix(self.repo.worktree()).map_err(|_| {
                NGitError::OperationFailed(format!("operation failed while normalizing {:?}", p))
            })?;
            index.insert(fp.to_string_lossy().to_string(), oid);
        }
        std::fs::write(self.get_path(), Value::from(index).to_string())?;
        Ok(())
    }

    fn stage_file(&mut self, fp: PathBuf) -> Result<(), NGitError> {
        if is_ignored(&fp) {
            return Ok(());
        }
        let content = std::fs::read(&fp)?;
        let oid = self.repo.write_blob(content)?;
        self.i.insert(fp, oid);
        Ok(())
    }

    fn stage_dir(&mut self, root: PathBuf) -> Result<(), NGitError> {
        let wd = WalkDir::new(root)?;
        fn inner<'a>(i: &mut Index<'a>, wd: WalkDir) -> Result<(), NGitError> {
            for f in wd.files {
                i.stage_file(f)?;
            }
            for d in wd.dirs {
                inner(i, d)?;
            }

            Ok(())
        }
        inner(self, wd)
    }

    pub fn stage_path(&mut self, p: PathBuf) -> Result<(), NGitError> {
        let original = p.to_string_lossy().to_string();
        let fp = p;
        let fp = if fp.is_absolute() {
            if !fp.starts_with(self.repo.worktree()) {
                return Err(NGitError::OperationFailed(format!(
                    "{original} is not part of repo"
                )));
            } else {
                fp
            }
        } else {
            std::path::absolute(self.repo.worktree().join(fp).as_path())?
        };

        if fp.is_file() {
            self.stage_file(fp)?
        } else if fp.is_dir() {
            self.stage_dir(fp)?
        } else {
            return Err(NGitError::OperationFailed(format!(
                "no such file exists: {original}"
            )));
        }
        Ok(())
    }

    pub fn update_raw(&mut self, raw: TreeSnapshot, merge: bool) {
        if !merge {
            self.i = raw.into_object_ids()
        } else {
            for (k, v) in raw.into_object_ids() {
                self.i.insert(k, v);
            }
        }
    }

    pub fn items(&self) -> impl Iterator<Item = (&PathBuf, &BlobOid)> {
        self.i.iter()
    }

    pub fn to_index_tree(&self) -> TreeSnapshot {
        TreeSnapshot::new(self.i.clone())
    }
}

impl<'a> TryFrom<&'_ Index<'a>> for WalkDir {
    type Error = NGitError;

    fn try_from(value: &Index<'a>) -> Result<Self, Self::Error> {
        let mut wd = Self::empty(value.repo.worktree().to_path_buf());

        for (fp, _) in &value.i {
            let fps = fp
                .strip_prefix(value.repo.worktree())
                .unwrap()
                .to_string_lossy();
            let mut parts = fps.split("/").peekable();
            let mut cur_dir = &mut wd;
            while let Some(p) = parts.next() {
                let cur_p = cur_dir.root.join(p);
                if parts.peek().is_none() {
                    cur_dir.files.push(cur_p);
                    break;
                }

                cur_dir = match cur_dir
                    .dirs
                    .iter()
                    .enumerate()
                    .find_map(|(i, d)| if d.root == cur_p { Some(i) } else { None })
                {
                    Some(i) => &mut cur_dir.dirs[i],
                    None => {
                        let i = cur_dir.dirs.len();
                        cur_dir.dirs.push(WalkDir::empty(cur_p));
                        &mut cur_dir.dirs[i]
                    }
                }
            }
        }

        Ok(wd)
    }
}
