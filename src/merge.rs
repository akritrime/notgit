use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::errors::NGitError;

pub enum BlobMerge {
    Clean(Vec<u8>),
    Conflict(Vec<u8>),
}

impl BlobMerge {
    pub fn has_conflict(&self) -> bool {
        matches!(self, Self::Conflict(_))
    }

    pub fn into_content(self) -> Vec<u8> {
        match self {
            Self::Clean(content) | Self::Conflict(content) => content,
        }
    }
}

pub struct BlobMerger;

impl BlobMerger {
    pub fn merge(
        base: impl AsRef<[u8]>,
        head: impl AsRef<[u8]>,
        other: impl AsRef<[u8]>,
    ) -> Result<BlobMerge, NGitError> {
        let workspace = MergeWorkspace::create()?;
        let result = merge_in_workspace(&workspace, base.as_ref(), head.as_ref(), other.as_ref());
        let cleanup = workspace.remove();

        match (result, cleanup) {
            (Ok(merge), Ok(())) => Ok(merge),
            (Ok(_), Err(err)) => Err(err),
            (Err(err), _) => Err(err),
        }
    }
}

fn merge_in_workspace(
    workspace: &MergeWorkspace,
    base: &[u8],
    head: &[u8],
    other: &[u8],
) -> Result<BlobMerge, NGitError> {
    let base_file = workspace.path("BASE");
    let head_file = workspace.path("HEAD");
    let other_file = workspace.path("MERGE_HEAD");

    std::fs::write(&base_file, base)?;
    std::fs::write(&head_file, head)?;
    std::fs::write(&other_file, other)?;

    let output = Command::new("diff3")
        .arg("-m")
        .arg("-L")
        .arg("HEAD")
        .arg("-L")
        .arg("BASE")
        .arg("-L")
        .arg("MERGE_HEAD")
        .arg(&head_file)
        .arg(&base_file)
        .arg(&other_file)
        .output()
        .map_err(|err| NGitError::SystemError("diff3".into(), err.to_string()))?;

    match output.status.code() {
        Some(0) => Ok(BlobMerge::Clean(output.stdout)),
        Some(1) => Ok(BlobMerge::Conflict(output.stdout)),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(NGitError::SystemError("diff3".into(), stderr.to_string()))
        }
    }
}

struct MergeWorkspace {
    root: PathBuf,
}

impl MergeWorkspace {
    fn create() -> Result<Self, NGitError> {
        let root = unique_temp_path()?;
        std::fs::create_dir(&root)?;
        Ok(Self { root })
    }

    fn path(&self, name: impl AsRef<Path>) -> PathBuf {
        self.root.join(name)
    }

    fn remove(self) -> Result<(), NGitError> {
        std::fs::remove_dir_all(self.root)?;
        Ok(())
    }
}

fn unique_temp_path() -> Result<PathBuf, NGitError> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(std::env::temp_dir().join(format!("notgit.merge.{}.{}", std::process::id(), nanos)))
}
