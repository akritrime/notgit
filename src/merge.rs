use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::errors::NGitError;

pub struct BlobChange;

impl BlobChange {
    pub fn diff(a: &[u8], b: &[u8], path: impl AsRef<str>) -> Result<String, NGitError> {
        let path = path.as_ref();
        ProcessWorkspace::run(|workspace| {
            let a_file = workspace.write("a", a)?;
            let b_file = workspace.write("b", b)?;

            let output = Command::new("diff")
                .arg("--unified")
                .arg("--show-c-function")
                .arg("--label")
                .arg(format!("a/{path}"))
                .arg("--label")
                .arg(format!("b/{path}"))
                .arg(&a_file)
                .arg(&b_file)
                .output()?;

            match output.status.code() {
                Some(0) | Some(1) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
                _ => Err(system_error("diff", output)),
            }
        })
    }

    pub fn merge(
        base: impl AsRef<[u8]>,
        head: impl AsRef<[u8]>,
        other: impl AsRef<[u8]>,
    ) -> Result<BlobMerge, NGitError> {
        ProcessWorkspace::run(|workspace| {
            let base_file = workspace.write("BASE", base.as_ref())?;
            let head_file = workspace.write("HEAD", head.as_ref())?;
            let other_file = workspace.write("MERGE_HEAD", other.as_ref())?;

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
                _ => Err(system_error("diff3", output)),
            }
        })
    }
}

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

struct ProcessWorkspace {
    root: PathBuf,
}

impl ProcessWorkspace {
    fn run<T>(f: impl FnOnce(&Self) -> Result<T, NGitError>) -> Result<T, NGitError> {
        let workspace = Self::create()?;
        let result = f(&workspace);
        let cleanup = workspace.remove();

        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(err)) => Err(err),
            (Err(err), _) => Err(err),
        }
    }

    fn create() -> Result<Self, NGitError> {
        let root = unique_temp_path()?;
        std::fs::create_dir(&root)?;
        Ok(Self { root })
    }

    fn write(&self, name: impl AsRef<Path>, content: &[u8]) -> Result<PathBuf, NGitError> {
        let path = self.root.join(name);
        std::fs::write(&path, content)?;
        Ok(path)
    }

    fn remove(self) -> Result<(), NGitError> {
        std::fs::remove_dir_all(self.root)?;
        Ok(())
    }
}

fn unique_temp_path() -> Result<PathBuf, NGitError> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(std::env::temp_dir().join(format!("notgit.change.{}.{}", std::process::id(), nanos)))
}

fn system_error(command: &str, output: Output) -> NGitError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    NGitError::SystemError(command.into(), stderr.to_string())
}
