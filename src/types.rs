use std::{
    fmt::{Display, Formatter},
    path::{Component, Path, PathBuf},
};

use crate::errors::NGitError;

pub const OID_HEX_LEN: usize = 64;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Oid(String);

impl Oid {
    pub fn new(value: impl Into<String>) -> Result<Self, NGitError> {
        let value = value.into();
        if value.len() != OID_HEX_LEN || !value.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(NGitError::InvalidOid(value));
        }
        Ok(Self(value))
    }

    pub fn unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn short(&self, len: usize) -> &str {
        &self.0[..len.min(self.0.len())]
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<Oid> for Oid {
    fn as_ref(&self) -> &Oid {
        self
    }
}

impl AsRef<str> for Oid {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for Oid {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

macro_rules! typed_oid {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(Oid);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, NGitError> {
                Ok(Self(Oid::new(value)?))
            }

            pub fn from_oid(oid: Oid) -> Self {
                Self(oid)
            }

            pub fn as_oid(&self) -> &Oid {
                &self.0
            }

            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }

            pub fn short(&self, len: usize) -> &str {
                self.0.short(len)
            }

            pub fn into_oid(self) -> Oid {
                self.0
            }
        }

        impl AsRef<Oid> for $name {
            fn as_ref(&self) -> &Oid {
                self.as_oid()
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<$name> for Oid {
            fn from(value: $name) -> Self {
                value.into_oid()
            }
        }
    };
}

typed_oid!(BlobOid);
typed_oid!(TreeOid);
typed_oid!(CommitOid);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RefName(String);

impl RefName {
    pub fn new(value: impl Into<String>) -> Result<Self, NGitError> {
        let value = value.into();
        if value.is_empty()
            || value.starts_with('/')
            || value.ends_with('/')
            || value.contains("..")
            || value.contains("//")
            || value.chars().any(|c| c.is_whitespace())
        {
            return Err(NGitError::InvalidRefName(value));
        }
        Ok(Self(value))
    }

    pub fn head() -> Self {
        Self("HEAD".into())
    }

    pub fn merge_head() -> Self {
        Self("MERGE_HEAD".into())
    }

    pub fn branch(name: impl AsRef<str>) -> Result<Self, NGitError> {
        Self::new(format!("refs/heads/{}", name.as_ref()))
    }

    pub fn tag(name: impl AsRef<str>) -> Result<Self, NGitError> {
        Self::new(format!("refs/tags/{}", name.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for RefName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for RefName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Revision(String);

impl Revision {
    pub fn new(value: impl Into<String>) -> Result<Self, NGitError> {
        let value = value.into();
        if value.is_empty() {
            return Err(NGitError::Unresolvable(value));
        }
        Ok(Self(value))
    }

    pub fn at_head() -> Self {
        Self("@".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Revision {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for Revision {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BranchName(String);

impl BranchName {
    pub fn new(value: impl Into<String>) -> Result<Self, NGitError> {
        let value = value.into();
        RefName::branch(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for BranchName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for BranchName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepoPath(PathBuf);

impl RepoPath {
    pub fn new(value: impl Into<PathBuf>) -> Result<Self, NGitError> {
        let value = value.into();
        if value.is_absolute() || !is_normal_relative_path(&value) {
            return Err(NGitError::InvalidRepoPath(value));
        }
        Ok(Self(value))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn join_to(&self, root: impl AsRef<Path>) -> PathBuf {
        root.as_ref().join(&self.0)
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for RepoPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl Display for RepoPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string_lossy())
    }
}

fn is_normal_relative_path(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
}
