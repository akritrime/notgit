use std::{collections::HashMap, path::PathBuf};

use crate::{data::Data, errors::NGitError};

pub type ContentTree = HashMap<PathBuf, String>;
pub fn compare_trees(trees: Vec<ContentTree>) -> HashMap<PathBuf, Vec<Option<String>>> {
	let mut h = HashMap::new();
	let l = trees.len();
	for (i, tree) in trees.into_iter().enumerate() {
		for (path, oid) in tree {
			let oids = h.entry(path).or_insert(vec![None; l]);
			oids[i] = Some(oid);
		}
	}
	
	h

}

#[derive(Default)]
pub struct DiffTree {
	pub changed: HashMap<PathBuf, String>,
	pub added: Vec<PathBuf>,
	pub deleted: Vec<PathBuf>
}

impl DiffTree {
	pub fn between(data: &Data, from: ContentTree, to: ContentTree) -> Result<DiffTree, NGitError> {
		let mut d = DiffTree::default();
		for (path, oids) in compare_trees(vec![from, to]) {
			assert!(oids.len() == 2 && oids.iter().any(|o| o.is_some()));
			let [from, to] = oids.as_slice() else {
				unreachable!()
			};
			match (from, to) {
				(Some(from), Some(to)) => {
					if from == to {
						continue
					}
					let p = path.to_str().unwrap_or("blob");
					
					let a = data.get_object(from, crate::data::DataType::Blob)?;
					let b = data.get_object(to, crate::data::DataType::Blob)?;
					let diff = diff_blobs(a, b, p)?;
					d.changed.insert(path, diff);
				},
				(Some(_), None) => {
					d.deleted.push(path);
				},
				(None, Some(_)) => {
					d.added.push(path)
				},
				(None, None) => unreachable!()
			}
			
		}
		Ok(d)
	}

	pub fn to_list(self) -> Vec<(&'static str, PathBuf)> {
		let mut files = vec![];
		for a in self.added {
			files.push(("new file", a));
		}

		for (a, _) in self.changed {
			files.push(("modified", a))
		}

		for a in self.deleted {
			files.push(("deleted", a))
		}

		files
	}

	// pub fn print(self) {
	// 	let diff = self;
	// 	if diff.added.len() > 0 {
	// 		println!("ADDED:");
	// 		for a in diff.added {
	// 			println!("    - {}", a.to_str().unwrap())
	// 		}
	// 	}

	// 	if diff.deleted.len() > 0 {
	// 		println!("DELETED:");
	// 		for d in diff.deleted {
	// 			println!("    - {}", d.to_str().unwrap())
	// 		}
	// 	}

	// 	if diff.changed.len() > 0 {
	// 		println!("CHANGED:");
	// 		for (c, d) in diff.changed {
	// 			println!("    - {}", c.to_str().unwrap());
	// 			for l in d.lines() {
	// 				println!("        {}", l);
	// 			}
	// 		}
	// 	}
	// } 
}


fn diff_blobs(a: impl AsRef<str>, b: impl AsRef<str>, path: impl AsRef<str>) -> Result<String, NGitError> {
	let path = path.as_ref();
	let output = std::process::Command::new("bash")
		.arg("-c")
		.arg(format!("diff --unified --show-c-function --label a/{} <(echo \"$1\") --label b/{} <(echo \"$2\")", path, path))
		.arg("--") // End of bash options
		.arg(a.as_ref()) // Passed to bash as $1
		.arg(b.as_ref()) // Passed to bash as $2
		.output()?;

	// Note: diff returns exit code 1 if differences are found!
	if !output.status.success() && output.status.code() != Some(1) {
		let stderr = String::from_utf8_lossy(&output.stderr);
		return Err(NGitError::SystemError("diff".into(), stderr.to_string()));
	}

	Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}


pub fn merge_tree(d: &Data, base_t: ContentTree, head_t: ContentTree, other_t: ContentTree) -> Result<HashMap<PathBuf, String>, NGitError> {
	let mut tree = HashMap::new();
	for (path, oids) in compare_trees(vec![base_t, head_t, other_t]) {
		assert!(oids.len() == 3 && oids.iter().any(|o| o.is_some()));
		let contents: Result<Vec<String>, NGitError> = oids
			.into_iter()
			.map(|o| {
				match o {
					Some(o) => d.get_object(&o, crate::data::DataType::Blob),
					None => Ok(String::new())
				}
			})
			.collect();
		let contents = contents?;
		let [base, head, other] = contents.as_slice() else {
			unreachable!()
		};
		tree.insert(path, d.hash_object(merge_blobs(base, head, other)?, crate::data::DataType::Blob)?);
		
	}
	Ok(tree)
}


fn merge_blobs(base: impl AsRef<str>, head: impl AsRef<str>, other: impl AsRef<str>) -> Result<String, NGitError> {
	let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
	let tmp_dir = format!("/tmp/notgit.merge.{}", ts);
	std::fs::create_dir_all(&tmp_dir)?;

	let base_f = format!("{}/BASE", &tmp_dir);
	let head_f = format!("{}/HEAD", &tmp_dir);
	let other_f = format!("{}/MERGE_HEAD", &tmp_dir);

	std::fs::write(&base_f, base.as_ref())?;
	std::fs::write(&head_f, head.as_ref())?;
	std::fs::write(&other_f, other.as_ref())?;

	let output = std::process::Command::new("bash")
		.arg("-c")
		.arg("diff3 -m -L HEAD \"$1\" -L BASE \"$2\" -L MERGE_HEAD  \"$3\"")
		.arg("--") // End of bash options
		.arg(head_f) // Passed to bash as $1
		.arg(base_f) // Passed to bash as $2
		.arg(other_f) // Passed to bash as $3
		.output()?;

	// Note: diff returns exit code 1 if differences are found!
	if !output.status.success() && output.status.code().map_or(false,|c| c == 1 || c==0) {
		let stderr = String::from_utf8_lossy(&output.stderr);
		return Err(NGitError::SystemError("diff3".into(), stderr.to_string()));
	}

	std::fs::remove_dir_all(tmp_dir)?;

	Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}