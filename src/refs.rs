use std::path::{Path, PathBuf};

use crate::{
    errors::NGitError,
    types::{CommitOid, Oid, RefName},
    worktree::WalkDir,
};

#[derive(Clone, Debug)]
pub enum RefValue {
    Direct(CommitOid),
    Symbolic(RefName),
}

impl RefValue {
    pub fn direct(oid: CommitOid) -> Self {
        Self::Direct(oid)
    }

    pub fn symbolic(name: RefName) -> Self {
        Self::Symbolic(name)
    }

    pub fn is_symbolic(&self) -> bool {
        matches!(self, Self::Symbolic(_))
    }

    pub fn oid(&self) -> Option<&CommitOid> {
        match self {
            Self::Direct(oid) => Some(oid),
            Self::Symbolic(_) => None,
        }
    }

    pub fn into_oid(self) -> Option<CommitOid> {
        match self {
            Self::Direct(oid) => Some(oid),
            Self::Symbolic(_) => None,
        }
    }

    pub fn value_text(&self) -> String {
        match self {
            Self::Direct(oid) => oid.to_string(),
            Self::Symbolic(name) => name.to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RefStore {
    git_dir: PathBuf,
    refs_dir: PathBuf,
}

impl RefStore {
    pub fn new(git_dir: PathBuf) -> Self {
        let refs_dir = git_dir.join("refs");
        Self { git_dir, refs_dir }
    }

    pub fn refs_dir(&self) -> PathBuf {
        self.refs_dir.clone()
    }

    pub fn ref_path(&self, name: &RefName) -> PathBuf {
        self.git_dir.join(name.as_ref())
    }

    fn resolve_ref(
        &self,
        r: &RefName,
        deref: bool,
    ) -> Result<(PathBuf, Option<RefValue>), NGitError> {
        let p = self.ref_path(r);
        if !p.is_file() {
            return Ok((p, None));
        }
        let r = std::fs::read_to_string(&p)?;
        let symbolic = r.starts_with("ref:");
        if symbolic && deref {
            self.resolve_ref(&RefName::new(r[4..].trim())?, true)
        } else {
            let value = if symbolic {
                RefValue::symbolic(RefName::new(r[4..].trim())?)
            } else {
                RefValue::direct(CommitOid::new(r.trim())?)
            };
            Ok((p, Some(value)))
        }
    }

    pub fn update_ref(&self, r: &RefName, value: &RefValue, deref: bool) -> Result<(), NGitError> {
        let p = self.resolve_ref(r, deref)?.0;
        std::fs::create_dir_all(p.parent().unwrap())?;

        let content = match value {
            RefValue::Direct(oid) => oid.to_string(),
            RefValue::Symbolic(name) => format!("ref: {}", name),
        };
        std::fs::write(p, content)?;
        Ok(())
    }

    pub fn get_ref(&self, r: &RefName, deref: bool) -> Result<Option<RefValue>, NGitError> {
        let (_, r) = self.resolve_ref(r, deref)?;
        Ok(r)
    }

    pub fn del_ref(&self, r: &RefName, deref: bool) -> Result<(), NGitError> {
        let r = self.resolve_ref(r, deref)?;
        std::fs::remove_file(r.0)?;
        Ok(())
    }

    pub fn resolve(&self, r: impl AsRef<str>) -> Result<Oid, NGitError> {
        let mut r = r.as_ref().to_owned();
        if r == "@" {
            r = "HEAD".into();
        }
        let targets = ["", "refs/", "refs/tags/", "refs/heads/"];

        for t in targets {
            if let Some(o) = self.get_ref(&RefName::new(format!("{}{}", t, r))?, true)? {
                if let RefValue::Direct(oid) = o {
                    return Ok(oid.into_oid());
                }
            }
        }
        if r.chars().all(|c| c.is_ascii_hexdigit()) {
            Oid::new(r)
        } else {
            Err(NGitError::Unresolvable(r))
        }
    }

    pub fn iter_refs(
        &self,
        deref: bool,
        prefix: Option<impl AsRef<str>>,
    ) -> Result<Vec<(RefName, RefValue)>, NGitError> {
        let mut refs = vec![RefName::head(), RefName::merge_head()];
        let wd = WalkDir::new(self.refs_dir())?;
        fn walk(wd: WalkDir, root: &Path, refs: &mut Vec<RefName>) -> Result<(), NGitError> {
            let files = wd.files;
            for f in files {
                let r = f.strip_prefix(root).unwrap();
                refs.push(RefName::new(r.to_string_lossy().to_string())?)
            }
            for d in wd.dirs {
                walk(d, root, refs)?
            }
            Ok(())
        }
        walk(wd, &self.git_dir, &mut refs)?;
        let mut res = vec![];
        for r in refs {
            if let Some(ref p) = prefix
                && !r.as_str().starts_with(p.as_ref())
            {
                continue;
            }
            if let Some(oid) = self.get_ref(&r, deref)? {
                res.push((r, oid))
            }
        }
        Ok(res)
    }

    pub fn get_branch(&self, branch: &str) -> Result<Option<RefValue>, NGitError> {
        self.get_ref(&RefName::branch(branch)?, true)
    }

    pub fn get_current_branch(&self) -> Result<Option<String>, NGitError> {
        let head = match self.get_ref(&RefName::head(), false)? {
            None => return Ok(None),
            Some(RefValue::Direct(_)) => return Ok(None),
            Some(a @ RefValue::Symbolic(_)) => a,
        };
        let RefValue::Symbolic(name) = head else {
            return Ok(None);
        };
        let name = name.as_str();
        assert!(name.starts_with("refs/heads"));
        Ok(Some(name.replace("refs/heads/", "")))
    }

    pub fn iter_branch_names(&self) -> Result<Vec<String>, NGitError> {
        let prefix = "refs/heads/";
        let branches = self
            .iter_refs(true, Some(&prefix))?
            .into_iter()
            .map(|(v, _)| v.as_str().replace(prefix, ""))
            .collect();
        Ok(branches)
    }
}
