use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::{
    errors::NGitError,
    types::{BlobOid, CommitOid, Oid, TreeOid},
};

const DIVIDER: u8 = b'\0';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectType {
    Blob,
    Tree,
    Commit,
    Any,
}

impl std::fmt::Display for ObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Blob => "blob",
                Self::Tree => "tree",
                Self::Commit => "commit",
                Self::Any => "_",
            }
        )
    }
}

impl std::str::FromStr for ObjectType {
    type Err = NGitError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ty = match s {
            "blob" => ObjectType::Blob,
            "tree" => ObjectType::Tree,
            "commit" => ObjectType::Commit,
            "_" => ObjectType::Any,
            a => return Err(NGitError::InvalidDataType(a.to_owned())),
        };

        Ok(ty)
    }
}

#[derive(Clone, Debug)]
pub struct ObjectStore {
    dir: PathBuf,
}

impl ObjectStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> PathBuf {
        self.dir.clone()
    }

    pub fn path(&self, oid: &Oid) -> PathBuf {
        self.dir.join(oid.as_str())
    }

    pub fn blob_path(&self, oid: &BlobOid) -> PathBuf {
        self.path(oid.as_oid())
    }

    pub fn tree_path(&self, oid: &TreeOid) -> PathBuf {
        self.path(oid.as_oid())
    }

    pub fn commit_path(&self, oid: &CommitOid) -> PathBuf {
        self.path(oid.as_oid())
    }

    pub fn hash(&self, content: impl AsRef<[u8]>, ty: ObjectType) -> Result<Oid, NGitError> {
        let object = encode_object(content.as_ref(), ty)?;
        Oid::new(base16ct::lower::encode_string(&Sha256::digest(&object)))
    }

    pub fn hash_blob(&self, content: impl AsRef<[u8]>) -> Result<BlobOid, NGitError> {
        Ok(BlobOid::from_oid(self.hash(content, ObjectType::Blob)?))
    }

    pub fn write(&self, content: impl AsRef<[u8]>, ty: ObjectType) -> Result<Oid, NGitError> {
        let obj = encode_object(content.as_ref(), ty)?;
        let digest = Oid::new(base16ct::lower::encode_string(&Sha256::digest(&obj)))?;
        let path = self.path(&digest);
        std::fs::write(path, obj)?;

        Ok(digest)
    }

    pub fn write_blob(&self, content: impl AsRef<[u8]>) -> Result<BlobOid, NGitError> {
        Ok(BlobOid::from_oid(self.write(content, ObjectType::Blob)?))
    }

    pub fn write_tree(&self, content: impl AsRef<[u8]>) -> Result<TreeOid, NGitError> {
        Ok(TreeOid::from_oid(self.write(content, ObjectType::Tree)?))
    }

    pub fn write_commit(&self, content: impl AsRef<[u8]>) -> Result<CommitOid, NGitError> {
        Ok(CommitOid::from_oid(
            self.write(content, ObjectType::Commit)?,
        ))
    }

    pub fn read(&self, digest: &Oid, ty: ObjectType) -> Result<Vec<u8>, NGitError> {
        let content = std::fs::read(self.path(digest))?;
        let Some(divider) = content.iter().position(|byte| *byte == DIVIDER) else {
            return Err(NGitError::InvalidObject(digest.to_string()));
        };

        let header = std::str::from_utf8(&content[..divider])
            .map_err(|_| NGitError::InvalidObject(digest.to_string()))?;
        let ty2: ObjectType = header.parse()?;
        if !matches!(ty, ObjectType::Any) && ty != ty2 {
            return Err(NGitError::UnexpectedDataType(
                ty.to_string(),
                ty2.to_string(),
            ));
        }

        Ok(content[divider + 1..].to_vec())
    }

    pub fn read_blob(&self, oid: &BlobOid) -> Result<Vec<u8>, NGitError> {
        self.read(oid.as_oid(), ObjectType::Blob)
    }

    pub fn read_tree(&self, oid: &TreeOid) -> Result<Vec<u8>, NGitError> {
        self.read(oid.as_oid(), ObjectType::Tree)
    }

    pub fn read_commit(&self, oid: &CommitOid) -> Result<Vec<u8>, NGitError> {
        self.read(oid.as_oid(), ObjectType::Commit)
    }

    pub fn copy_from(&self, remote: &ObjectStore, oid: &Oid, force: bool) -> Result<(), NGitError> {
        let local_path = self.path(oid);
        if !force && local_path.is_file() {
            return Ok(());
        }

        std::fs::copy(remote.path(oid), local_path)?;
        Ok(())
    }

    pub fn copy_to(&self, remote: &ObjectStore, oid: &Oid) -> Result<(), NGitError> {
        std::fs::copy(self.path(oid), remote.path(oid))?;
        Ok(())
    }
}

fn encode_object(content: &[u8], ty: ObjectType) -> Result<Vec<u8>, NGitError> {
    if matches!(ty, ObjectType::Any) {
        return Err(NGitError::MissingDataType);
    }
    let mut obj = Vec::new();
    obj.extend_from_slice(ty.to_string().as_bytes());
    obj.push(DIVIDER);
    obj.extend_from_slice(content);
    Ok(obj)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn stores_blob_bytes_verbatim() -> Result<(), NGitError> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "notgit.object-test.{}.{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir(&dir)?;

        let store = ObjectStore::new(dir.clone());
        let content = vec![0, 0xff, b'a', b'\n', 0, b'z'];
        let oid = store.write_blob(&content)?;

        assert_eq!(store.hash_blob(&content)?, oid);
        assert_eq!(store.read_blob(&oid)?, content);

        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
