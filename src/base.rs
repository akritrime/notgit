use std::{collections::{HashMap, HashSet, VecDeque}, path::PathBuf, vec};
use crate::{data::{Data, DataType, Index, RefValue, WalkDir}, diff::{ContentTree, merge_tree}, errors::NGitError};


#[derive(Debug)]
pub struct  Node (pub PathBuf, pub DataType, pub String);


#[derive(Debug)]
pub struct Commit {
	pub message: String,
	pub tree: String,
	pub parents: Vec<String>,
	pub oid: String
}

impl Commit {
	pub fn get(d: &Data, oid: String) -> Result<Self, NGitError> {
		let o = d.get_object(&oid, DataType::Commit)?;
		let mut commit = o.trim()
			.lines()
			.peekable();

		
		let err = || NGitError::InvalidCommit(oid.to_owned());
		let parse = |l: Option<&str>, term| -> Result<String, NGitError> {
			let val = l.filter(|s| s.starts_with(term))
				.ok_or_else(err)?
				.split(" ")
				.nth(1)
				.ok_or_else(err)?;
			Ok(val.to_owned())
		};

		let tree = parse(commit.next(),"tree")?;
		let mut parents = vec![];
		while let Some(l) = commit.peek() {
			if l.starts_with("parent") {
				parents.push(parse(commit.next(), "parent")?);
			} else {
				break
			}
		}
		let message = commit.collect::<Vec<_>>().join("\n").trim().to_owned();

		Ok(Self {
			message,
			tree,
			parents,
			oid
		})

	}
}

pub fn write_tree(d: &Data) -> Result<String, NGitError> {
	let i = Index::read(d)?;
	let wd = WalkDir::try_from(&i)?;
	wd.write(&d)
}


fn parse_tree(content: String, base: &PathBuf) -> Result<Vec<Node>, NGitError> {
	content
		.split('\n')
		.map(|r| r.trim().split(' ').collect::<Vec<_>>())
		.filter(|r| r.len() == 3)
		.map(|r| Ok(Node(base.join(r[2]), r[0].parse()?, r[1].to_owned())))
		.collect()
	
}

pub fn get_tree(oid: &String, d: &Data, base: Option<&PathBuf>) -> Result<ContentTree, NGitError> {
	let o = d.get_object(oid, DataType::Tree)?;
	let mut res = HashMap::new();
	let base = match base {
		Some(base) => base,
		None => &d.home
	};
	
	for node in parse_tree(o, base)? {
		match node.1 {
			DataType::Blob => {
				res.insert(node.0, node.2);
			},
			DataType::Tree => {
				res.extend(get_tree(&node.2, d,  Some(&node.0))?);
			},
			DataType::Commit => return Err(NGitError::UnexpectedDataType("tree or blob".into(), "commit".into())),
			DataType::Any => return Err(NGitError::UnexpectedDataType("tree or blob".into(), "_".into()))
		}

	}
	Ok(res)
}

pub fn read_tree(oid: &String, d: &Data) -> Result<Vec<PathBuf>, NGitError> {
	let tree = get_tree(&oid, d, None)?;
	let mut index = Index::read(d)?;
	index.update_raw(tree, false);

	checkout_from_index(index)
}


fn checkout_from_index(index: Index<'_>) -> Result<Vec<PathBuf>, NGitError> {
	let mut files = vec![];

	let wd = WalkDir::try_from(&index)?;
	wd.clean()?;

	for (fp, oid) in index.items() {

		if let Some(dir) = fp.parent() {
			std::fs::create_dir_all(dir)?;
		}
		std::fs::write(&fp, index.d.get_object(&oid.to_string(), DataType::Blob)?)?;
		files.push(fp.clone());
	}
	Ok(files)
}

pub type CommitTree = HashMap<String, Vec<String>>;


pub fn get_history_for_commits(d: &Data, oids: HashSet<String>) -> Result<CommitTree, NGitError> {
	let mut tree = CommitTree::new();
	
	fn walk(d: &Data, oid: String, tree: &mut CommitTree) -> Result<(), NGitError> {
		if tree.contains_key(&oid) {
			return Ok(())
		}
		let commit = Commit::get(d, oid)?;
		tree.entry(commit.oid).or_default().extend(commit.parents.clone());
		 
		for p in commit.parents {
			walk(d, p, tree)?;
		}


		return Ok(())
		
	}
	for oid in oids {
		walk(d, oid, &mut tree)?;
	}

	Ok(tree)	
}

pub fn create_branch(d: &Data, name: &String, start_point: &RefValue) -> Result<(), NGitError> {
	d.update_ref(format!("refs/heads/{}", name), start_point, true)
}

pub fn read_tree_merged(d: &Data, t_base: &Option<String>, t_head: &String, t_other: &String) -> Result<Vec<PathBuf>, NGitError> {
	let base_tree = if let Some(t_base) = t_base {
		get_tree(t_base, d, None)? 
	} else { 
		Default::default() 
	};
	let head_tree = get_tree(t_head, d, None)?;
	let other_tree = get_tree(t_other, d, None)?;

	let merged_tree = merge_tree(d, base_tree, head_tree, other_tree)?;
	let mut index = Index::read(d)?;
	index.update_raw(merged_tree, false);
	checkout_from_index(index)
}

pub fn iter_commits_and_parents(d: &Data, commits: Vec<String>) -> Result<Vec<String>, NGitError> {
	let mut history = get_history_for_commits(d, commits.iter().cloned().collect())?;
	let mut retval = vec![];
	let mut commits = VecDeque::from(commits);
	
	while let Some(c) = commits.pop_back() {
		let ps = history.remove(&c).unwrap_or_default();
		if ps.len() > 0 {
			commits.push_back(ps[0].clone());
			for p in &ps[1..] {
				commits.push_front(p.clone());
			}
		}
		retval.push(c)
	}

	Ok(retval)
}

pub fn get_merge_base(d: &Data, commit1: String, commit2: String) -> Result<Option<String>, NGitError> {
		let parents1 = iter_commits_and_parents(d, vec![commit1])?;
		let parents2 = iter_commits_and_parents(d, vec![commit2])?;
		for p in parents2 {
			if parents1.contains(&p) {
				return Ok(Some(p))
			}
		}

		Ok(None)
}

pub fn iter_objects_in_commits(d: &Data, commits: Vec<String>) -> Result<Vec<String>, NGitError> {
	let mut oids = vec![];

	let mut visited = HashSet::new();
	fn iter_objects_in_trees(d: &Data, oid: &String, visited:&mut HashSet<String>) -> Result<Vec<String>, NGitError> {
		visited.insert(oid.clone());
		let mut rval = vec![oid.clone()];
		let tree = d.get_object(oid, DataType::Tree)?;
		for node in parse_tree(tree, &d.home)? {
			if visited.contains(&node.2) {
				continue
			}
			match node.1 {
				DataType::Tree  => rval.extend(iter_objects_in_trees(d, &node.2, visited)?),
				_ => {
					rval.push(node.2.clone());
					visited.insert(node.2);
				}
			}
		}
		
		Ok(rval)
	}

	for oid in iter_commits_and_parents(d, commits)? {
		oids.push(oid.clone());
		let commit = Commit::get(d, oid)?;
		if !visited.contains(&commit.tree) {
			oids.extend(iter_objects_in_trees(d, &commit.tree, &mut visited)?)
		}
	}

	Ok(oids)
}