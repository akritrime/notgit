use std::path::PathBuf;

use crate::{
    errors::NGitError,
    objects::{ObjectStore, ObjectType},
    types::TreeOid,
};

pub(crate) fn is_ignored(p: &PathBuf) -> bool {
    vec![".git", ".ugit", "target"]
        .iter()
        .any(|a| p.ends_with(*a))
}

#[derive(Debug)]
pub struct WalkDir {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) dirs: Vec<WalkDir>,
    pub(crate) root: PathBuf,
}

impl WalkDir {
    pub fn empty(root: PathBuf) -> Self {
        Self {
            files: vec![],
            dirs: vec![],
            root,
        }
    }

    pub fn new(p: PathBuf) -> Result<Self, NGitError> {
        let mut wd = Self::empty(p);
        for entry in std::fs::read_dir(&wd.root)? {
            let entry = entry?;
            let full = entry.path();
            if is_ignored(&full) {
                continue;
            }
            if full.is_dir() {
                wd.dirs.push(WalkDir::new(full)?);
            } else if full.is_file() {
                wd.files.push(full);
            }
        }

        Ok(wd)
    }

    pub fn clean(self) -> Result<(), NGitError> {
        for d in self.dirs {
            let root = d.root.clone();
            d.clean()?;
            match std::fs::remove_dir(root) {
                Err(e) => {
                    if ![
                        std::io::ErrorKind::DirectoryNotEmpty,
                        std::io::ErrorKind::NotFound,
                    ]
                    .contains(&e.kind())
                    {
                        return Err(e.into());
                    }
                }
                _ => (),
            }
        }
        for f in self.files {
            match std::fs::remove_file(f) {
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        return Err(e.into());
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }

    pub fn write(self, objects: &ObjectStore) -> Result<TreeOid, NGitError> {
        let mut entries = vec![];

        let files = self
            .files
            .into_iter()
            .map(|f| {
                let content = std::fs::read(&f)?;
                let oid = objects.write_blob(content)?;
                Ok((f, oid.into_oid(), ObjectType::Blob))
            })
            .collect::<Result<Vec<_>, NGitError>>()?;
        let dirs = self
            .dirs
            .into_iter()
            .map(|f| {
                let root = f.root.clone();
                let oid = f.write(objects)?;
                Ok((root, oid.into_oid(), ObjectType::Tree))
            })
            .collect::<Result<Vec<_>, NGitError>>()?;

        entries.extend(files);
        entries.extend(dirs);

        entries.sort_by(|a, b| a.1.cmp(&b.1));
        let content = entries
            .iter()
            .map(|e| {
                format!(
                    "{} {} {}\n",
                    e.2,
                    e.1,
                    e.0.file_name().unwrap().to_string_lossy()
                )
            })
            .collect::<Vec<_>>()
            .join("");

        objects.write_tree(content)
    }
}
