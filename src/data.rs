use std::{collections::HashMap, path::PathBuf, time::SystemTime};
use serde_json::{Map, Value};
use sha2::{Sha256, Digest};

use crate::{diff::ContentTree, errors::NGitError};

const DIVIDER: char = '\x00';
fn is_ignored(p: &PathBuf) -> bool {
	vec![".git", ".ugit", "target", "dir/mdir/ignored.txt"].iter().any(|a| p.ends_with(*a))
}
#[derive(Debug)]
pub struct WalkDir {
	files: Vec<PathBuf>,
	dirs: Vec<WalkDir>,
	root: PathBuf
}

impl WalkDir {
	pub fn empty(root: PathBuf) -> Self {
		Self {
			files: vec![],
			dirs: vec![],
			root
		}
	}
	pub fn new(p: PathBuf) -> Result<Self, NGitError> {
		let mut wd = Self::empty(p);
		for entry in std::fs::read_dir(&wd.root)? {
			let entry = entry?;
			let full = entry.path();
			if is_ignored(&full) {
				continue
			}
			if full.is_dir() {
				wd.dirs.push(WalkDir::new(full)?);
			} else {
				assert!(full.is_file());
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
					if ![std::io::ErrorKind::DirectoryNotEmpty, std::io::ErrorKind::NotFound].contains(&e.kind()) {

						return Err(e.into())
					}
				},
				_ =>()
			}
		}
		for f in self.files {
			match std::fs::remove_file(f) {
				Err(e) => {
					if e.kind() != std::io::ErrorKind::NotFound {
						return Err(e.into())
					}
				},
				_ => ()
			}
		}
		Ok(())
	}

	pub fn write(self, d: &Data) -> Result<String, NGitError> {
		let mut entries = vec![];
		
		let files = self.files
			.into_iter()
			.map(|f| {
				let content = std::fs::read_to_string(&f)?;
				let oid = d.hash_object(content, DataType::Blob)?;
				Ok((f, oid, DataType::Blob))
			})
			.collect::<Result<Vec<_>, NGitError>>()?;
		let dirs = self.dirs.into_iter()
			.map(|f| {
				let root = f.root.clone();
				let oid = f.write(d)?;
				Ok((root, oid, DataType::Tree))
			})
			.collect::<Result<Vec<_>,NGitError>>()?;

		entries.extend(files);
		entries.extend(dirs);
		
		entries.sort_by(|a, b| a.1.cmp(&b.1));
		let content = entries.iter()
			.map(|e| format!("{} {} {}\n", e.2, e.1, e.0.file_name().unwrap().to_string_lossy()))
			.collect::<Vec<_>>()
			.join("");

		d.hash_object(content, DataType::Tree)
	}
}

#[derive(Debug)]
pub struct Data {
	dir: PathBuf,
	pub home: PathBuf,
	pub created_at: Option<SystemTime>
	
}

#[derive(Debug, PartialEq, Eq)]
pub enum DataType {
	Blob,
	Tree,
	Commit,
	Any,
}

impl std::fmt::Display for DataType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", match self {
			Self::Blob => "blob",
			Self::Tree => "tree",
			Self::Commit => "commit",
			Self::Any => "_"
		})
	}
}

impl std::str::FromStr for DataType {
	type Err = NGitError;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let ty = match s {
			"blob" => DataType::Blob,
			"tree" => DataType::Tree,
			"commit" => DataType::Commit,
			"_" => DataType::Any,
			a => return Err(NGitError::InvalidDataType(a.to_owned()))
		};

		Ok(ty)
	}
}

#[derive(Debug)]
pub struct RefValue {
	pub value: String,
	pub symbolic: bool
}

impl Data {
	fn new(home: PathBuf) -> Result<Self, NGitError> {
		let dir = home.join(".ugit");
		
		let created_at = if dir.exists() {
			Some(std::fs::metadata(&dir)?.created()?)
		} else {
			None
		};

		Ok(Data {
			dir,
			home,
			created_at
		})
	}
	

	fn get_objects_path(&self, h: &String) -> PathBuf {
		let mut dir = self.dir.clone();
		dir.push("objects");
		dir.push(h);
		dir
	}
	

	pub fn init() -> Result<Self, NGitError> {
		let home = std::env::current_dir()?;
		let d = Self::new(home)?;
		if d.created_at.is_none() {
			let objects_path = d.dir.join("objects");
			std::fs::create_dir_all(objects_path)?;

			let refs_path = d.dir.join("refs");
			std::fs::create_dir_all(refs_path)?;
		}
		
		Ok(d)

	}
	
	pub fn read() -> Result<Self, NGitError> {
		let dir = std::env::current_dir()?;
		let d = Self::new(dir)?;
		if d.created_at.is_none() {
			return Err(NGitError::Uninitialized(d.dir))
		}
		Ok(d)
	}

	pub fn fetch(dir: PathBuf) -> Result<Self, NGitError> {
		let d = Self::new(dir)?;
		if d.created_at.is_none() {
			return Err(NGitError::RemoteUninitialized(d.home.to_string_lossy().into()))
		}
		Ok(d)

	}

	pub fn hash_object(&self, content: String, ty: DataType) -> Result<String, NGitError> {
		if matches!(ty, DataType::Any) {
			return Err(NGitError::MissingDataType)
		}
		let mut obj = String::new();
		obj.push_str(&ty.to_string());
		obj.push(DIVIDER);
		obj.push_str(&content);
		let h = Sha256::digest(&obj);
		let digest = base16ct::lower::encode_string(&h).to_owned();
		let path = self.get_objects_path(&digest);
		std::fs::write(path, obj)?;

		Ok(digest)
	}

	pub fn get_object(&self, digest: &String, ty: DataType) -> Result<String, NGitError> {
		let dir = self.get_objects_path(digest);
		let content = std::fs::read_to_string(dir)?;

		let c: Vec<&str> = content.split(DIVIDER).collect();
		if c.len() != 2 {
			return Err(NGitError::InvalidObject(digest.to_owned()))
		}

		let ty2: DataType = c[0].parse()?;
		if !matches!(ty, DataType::Any) && ty != ty2 {
			return Err(NGitError::UnexpectedDataType(ty.to_string(), ty2.to_string()))
		}


		Ok(c[1].to_owned())
	}

	fn resolve_ref(&self, r: impl AsRef<str>, deref: bool) -> Result<(PathBuf, Option<RefValue>), NGitError> {
		let p = self.dir.join(r.as_ref());
		if !p.is_file() {
			return Ok((p, None))
		}
		let r = std::fs::read_to_string(&p)?;
		let symbolic = r.starts_with("ref:");
		if symbolic && deref {
			self.resolve_ref(&r[4..].trim(), true)
		} else {
			Ok((p, Some(RefValue {value: r, symbolic })))
		}
	}

	pub fn update_ref(&self, r: impl AsRef<str>, value: &RefValue, deref: bool) -> Result<(), NGitError> {
		let p = self.resolve_ref(r, deref)?.0;
		std::fs::create_dir_all(p.parent().unwrap())?;

		let prefix = if value.symbolic { "ref: "} else {""};
		let content = format!("{}{}", prefix, value.value);
		std::fs::write(p, content)?;
		Ok(())
	}

	pub fn get_ref(&self, r: impl AsRef<str>, deref: bool) -> Result<Option<RefValue>, NGitError> {
		let (_, r) = self.resolve_ref(r, deref)?;
		Ok(r)
	}

	pub fn del_ref(&self, r: impl AsRef<str>, deref: bool) -> Result<(), NGitError> {
		let r = self.resolve_ref(r, deref)?;
		std::fs::remove_file(r.0)?;
		Ok(())
	}

	pub fn resolve(&self, r: impl AsRef<str>) -> Result<String, NGitError> {
		let mut r = r.as_ref().to_owned();
		if r == "@" {
			r = "HEAD".into();
		}
		let targets = [
			"",
			"refs/",
			"refs/tags/",
			"refs/heads/"
		];

		for t in targets {
			if let Some(o) = self.get_ref(format!("{}{}", t, r), true)? {
				return Ok(o.value)
			}
		}
		if r.chars().all(|c| c.is_ascii_hexdigit()) {
			Ok(r)
		} else {
			Err(NGitError::Unresolvable(r))
		}
	}

	pub fn iter_refs(&self, deref: bool, prefix: Option<impl AsRef<str>>) -> Result<Vec<(String, RefValue)>, NGitError> {
		let mut refs = vec!["HEAD".into(), "MERGE_HEAD".into()];
		let p = self.dir.join("refs");

		dbg!(&p);
		let wd = WalkDir::new(p)?;
		dbg!(&wd);
		fn walk(wd: WalkDir, root: &PathBuf, refs: &mut Vec<String>) {
			// let root = wd.root;
			let files = wd.files;
			for f in files {
				let r = f.strip_prefix(root).unwrap();
				refs.push(r.to_string_lossy().to_string())
			}
			for d in wd.dirs {
				walk(d, root, refs)
			}
		} 
		walk(wd, &self.dir, &mut refs);
		let mut res = vec![];
		for r in refs {
			if let Some(ref p) = prefix && !r.starts_with(p.as_ref()){
				continue
			}
			if let Some(oid) = self.get_ref(&r, deref)? {
				res.push((r, oid))
			}
		}
		Ok(res)
	}
	

	pub fn get_branch(&self, branch: &String) -> Result<Option<RefValue>, NGitError> {
		self.get_ref(format!("refs/heads/{}", branch), true)
	}

	pub fn get_current_branch(&self) -> Result<Option<String>, NGitError> {
		let head = match self.get_ref("HEAD", false)? {
			None => return Ok(None),
			Some(a) if !a.symbolic => return Ok(None),
			Some(a) => a 
		};
		assert!(head.value.starts_with("ref: refs/heads"));
		Ok(Some(head.value.replace("ref: refs/heads/", "")))
		
	}

	pub fn iter_branch_names(&self) -> Result<Vec<String>, NGitError> {
		let prefix = "refs/heads/";
		let branches = self.iter_refs(true, Some(&prefix))?
			.into_iter()
			.map(|(v, _)| v.replace(prefix, ""))
			.collect();
		Ok(branches)
	}

	pub fn get_working_tree(&self) -> Result<ContentTree, NGitError> {

		fn walk(d: &Data, wd: WalkDir) -> Result<ContentTree, NGitError> {
			let mut tree = ContentTree::default();
			// let root = wd.root;
			let files = wd.files;
			for f in files {
				let h = d.hash_object(std::fs::read_to_string(&f)?, DataType::Blob)?;
				tree.insert(f, h);
			}
			for dir in wd.dirs {
				tree.extend(walk(d, dir)?)
			}
			Ok(tree)
		}

		let wd = WalkDir::new(self.home.clone())?;
		walk(self, wd)
	}

	pub fn get_path(&self, p: &[&str]) -> PathBuf {
		p.iter()
			.fold(self.dir.clone(), |acc, a| acc.join(a))
			
	}

	pub fn fetch_from_remote(&self, remote: &Data, oid: &String, force: bool) -> Result<(), NGitError> {
		let obj_path = ["objects", oid.as_ref()];
		let local_path = self.get_path(&obj_path);
		if !force && local_path.is_file() {
			return Ok(())
		}

		let remote_path = remote.get_path(&obj_path);
		std::fs::copy(remote_path, local_path)?;

		Ok(())
	} 

	pub fn push_to_remote(&self, remote: &Self, oid: &String) -> Result<(), NGitError> {

		let obj_path = ["objects", oid.as_ref()];
		let local_path = self.get_path(&obj_path);
		let remote_path = remote.get_path(&obj_path);
		std::fs::copy(local_path, remote_path)?;
		Ok(())

	}
}

#[derive(Debug)]
pub struct Index<'a> {
	i: HashMap<PathBuf, String>,
	pub d: &'a Data
}

impl<'a> Index<'a> {
	pub fn read(d: &'a Data) -> Result<Self, NGitError> {
		let mut i = Index{ i: HashMap::default(), d};
		
		if i.exists() {
			i.read_from_path()?;
		}

		Ok(i)
	}

	fn get_path(&self) -> PathBuf {
		self.d.dir.join("index")
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
				self.d.home.join(p),
				oid.as_str().unwrap().to_owned()
			);
		}
		
		Ok(())
	}

	fn sync_to_path(&self) -> Result<(), NGitError> {
		let mut index: Map<String, Value> = Map::new();
		for (p, oid) in &self.i {
			let oid: &str = oid.as_ref();
			let oid = oid.into();
			let fp = p.strip_prefix(&self.d.home).map_err(|_| NGitError::OperationFailed(format!("operation failed while normalizing {:?}", p)))?;
			index.insert(fp.to_string_lossy().to_string(), oid);
		}
		std::fs::write(self.get_path(), Value::from(index).to_string())?;
		Ok(())
	}

	fn add_file(&mut self, fp: PathBuf) -> Result<(), NGitError> {
		if is_ignored(&fp) {
			return Ok(())
		}
		let content = std::fs::read_to_string(&fp)?;
		let oid = self.d.hash_object(content, DataType::Blob)?;
		self.i.insert(fp, oid);
		Ok(())
	}

	fn add_dir(&mut self, root: PathBuf) -> Result<(), NGitError> {
		let wd = WalkDir::new(root)?;
		fn inner<'a>(i: &mut Index<'a>, wd: WalkDir) -> Result<(), NGitError> {
			for f in wd.files {
				i.add_file(f)?;
			}
			for d in wd.dirs {
				inner(i, d)?;
			}

			Ok(())
		}
		inner(self, wd)
	}

	pub fn add(&mut self, p: String) -> Result<(), NGitError> {
		let fp = PathBuf::from(&p);
		let fp = if fp.is_absolute() {
			if !fp.starts_with(&self.d.home) {
				return Err(NGitError::OperationFailed(format!("{p} is not part of repo")))
			} else {
				fp
			}
		} else {
			std::path::absolute(self.d.home.join(fp).as_path())?
		};

		if fp.is_file() {
			self.add_file(fp)?
		} else if fp.is_dir() {
			self.add_dir(fp)?
		} else {
			return Err(NGitError::OperationFailed(format!("no such file exists: {p}")))
		}
		Ok(())
	}

	pub fn update_raw(&mut self, raw: HashMap<PathBuf, String>, merge: bool) {
		if !merge {
			self.i = raw
		} else {
			for (k, v) in raw {
				self.i.insert(k, v);
			}
		}
	}

	pub fn items(&self) -> impl Iterator<Item=(&PathBuf, &String)> {
		self.i.iter()
	}

	pub fn to_index_tree(mut self) -> HashMap<PathBuf, String> {
		let tree = std::mem::replace(&mut self.i, Default::default());
		tree
	}
}

impl<'a> Drop for Index<'a> {
	fn drop(&mut self) {
		self.sync_to_path().unwrap();
	}
}

impl<'a> TryFrom<&'_ Index<'a>> for WalkDir {
	type Error = NGitError;
	
	fn try_from(value: &Index<'a>) -> Result<Self, Self::Error> {
		// let mut nested: HashMap<String, Vec<String>> = HashMap::new(); 
		let mut wd = Self::empty(value.d.home.clone());
		
		for (fp, _) in &value.i {
			let fps = fp.strip_prefix(&value.d.home).unwrap().to_string_lossy();
			let mut parts = fps.split("/").peekable();
			let mut cur_dir = &mut wd;
			while let Some(p) = parts.next() {
				let cur_p = cur_dir.root.join(p);
				if parts.peek().is_none() {
					cur_dir.files.push(cur_p);
					break;
				}

				cur_dir = match cur_dir.dirs.iter().enumerate().find_map(|(i, d)| if d.root == cur_p {Some(i)} else {None}) {
					Some(i) => &mut wd.dirs[i],
					None => {
						let i = wd.dirs.len();
						wd.dirs.push(WalkDir::empty(cur_p));
						&mut wd.dirs[i]
					}
				}

			}
		} 
		
		Ok(wd)
	}
}