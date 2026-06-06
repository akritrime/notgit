use std::{collections::{HashMap, HashSet}, path::PathBuf};

use crate::{base::{Commit, create_branch, get_history_for_commits, get_merge_base, get_tree, iter_objects_in_commits, read_tree, read_tree_merged, write_tree}, data::{Data, Index, RefValue}, diff::DiffTree, errors::NGitError};

mod data;
mod base;
mod errors;
mod diff;

#[derive(Debug)]
enum Command {
	Init,
	HashObject {
		file: PathBuf
	},
	CatFile {
		digest: String
	},
	WriteTree,
	ReadTree {
		digest: String
	},
	Commit {
		message: String
	},
	Log {
		oid: String
	},
	Checkout {
		commit: String
	},
	Tag {
		name: String,
		oid: String
	},
	K,
	ListBranch,
	Branch {
		name: String,
		start_point: String
	},
	Status,
	Reset {
		oid: String
	},
	Show {
		oid: String
	},
	Diff {
		commit: Option<String>,
		cached: bool
	},
	Merge {
		commit: String
	},
	MergeBase {
		commit1: String,
		commit2: String
	},
	Fetch {
		remote: String
	},
	Push {
		remote: String,
		branch: String
	},
	Add {
		files: Vec<String>
	},

	Unknown(String),
	Empty
}


impl Command {

	fn print_commit(refs: Option<&Vec<String>>, commit: Commit) {
		let refs = refs.map_or("".into(), |v| v.join(", "));
		println!("commit {}{}\n", commit.oid, refs);
		// let commit = Commit::get(&d, &c)?;
		for m in commit.message.lines() {
			println!("    {}", m);
		}
		println!("---");
	}

	fn run() -> Result<Self, NGitError> {
		let mut args = std::env::args().peekable();
		let _ = args.next(); // eat first arg
		let command_token = args.next();
		let cmd = match command_token.as_ref().map(|a| a.as_str()) {
			Some("init") => Command::Init,
			Some("hash-object") => {
				match args.next() {
					Some(f) => Command::HashObject {
						file: PathBuf::from(f)
					},
					None => return Err(NGitError::MissingArgument("hash-object".into(), "file".into()))
				}

			}
			Some("cat-file") => {
				match args.next() {
					Some(d) => Command::CatFile { digest: d },
					None => return Err(NGitError::MissingArgument("cat-file".into(), "hash digest".into()))
				}
			}
			
			Some("write-tree") => {
				if args.next().is_some() {
					return Err(NGitError::NoArgumentExpected)
				}
				Command::WriteTree
			}

			Some("commit") => {
				match (args.next(), args.next()) {
					(Some(ref c), Some(mut m)) if c == "-m" || c == "--message" => {
						for arg in args {
							m.push(' ');
							m.push_str(&arg);
						}
						Command::Commit { message: m }
					}
					
					_ => return Err(NGitError::MissingArgument("commit".into(), "message".into()))
				}
			}

			Some("read-tree") => {
				match args.next() {
					Some(d) => Command::ReadTree { digest: d },
					None => return Err(NGitError::MissingArgument("read-tree".into(), "hash digest".into()))
				}
			}
			Some("log") => Command::Log { oid: args.next().unwrap_or("@".into()) },
			Some("checkout") => {
				match args.next() {
					Some(d) => Command::Checkout { commit: d },
					None => return Err(NGitError::MissingArgument("checkout".into(), "commit".into()))
				}
 
			} 
			Some("tag") => {
				match args.next() {
					Some(n) => Command::Tag { 
						name: n, 
						oid: args.next().unwrap_or("@".into())
					},
					None => return Err(NGitError::MissingArgument("tag".into(), "name".into()))
				}
 
			}
			Some("branch") => {
				match args.next() {
					Some(n) => Command::Branch {
						name: n,
						start_point: args.next().unwrap_or("@".into())
					},
					None => Command::ListBranch
				}
			}
			Some("K" | "k") => Command::K,
			Some("status") => Command::Status,
			Some("reset") => Command::Reset { 
				oid: args.next()
					.ok_or(NGitError::MissingArgument(
						"reset".into(), 
						"oid".into()
					))? 
			},
			Some("show") => Command::Show {
				oid: args.next()
					// .unwrap_or("@".into())
					.ok_or(NGitError::MissingArgument(
						"show".into(),
						"oid".into()
					))?
				},

			Some("diff") => {
				if args.next().map_or(false, |term| term == "--cached") { 
					Command::Diff { cached: true, commit: args.next() } 
				} else {
					Command::Diff { cached: false, commit: args.next() }
				}

			}
			Some("merge") => Command::Merge {
				commit: args.next()
					.ok_or(NGitError::MissingArgument(
						"merge".into(), 
						"commit".into()
					))?
			},
			Some("merge-base") => {
				let err = |arg: &str| NGitError::MissingArgument("merge-base".into(), arg.into());
				Command::MergeBase { 
					commit1: args.next().ok_or(err("commit1"))?, 
					commit2: args.next().ok_or(err("commit2"))? 
				}
			},
			Some("fetch") => Command::Fetch {
				remote: args.next()
					.ok_or(NGitError::MissingArgument(
						"fetch".into(), 
						"remote".into()
					))?
			},
			Some("push") => Command::Push {
				remote: args.next()
					.ok_or(NGitError::MissingArgument(
						"push".into(), 
						"remote".into()
					))?,
				branch: args.next()
					.ok_or(NGitError::MissingArgument(
						"push".into(), 
						"branch".into()
					))?
			},
			Some("add") => Command::Add {
				files: args.into_iter().collect()
			},
			Some(a) => Command::Unknown(a.to_owned()),
			None => Command::Empty
		};
		
		Ok(cmd)
	}

	fn handle(self) -> Result<(), NGitError> {
		match self {
			Command::Init => {
				let d = Data::init()?;
				if d.created_at.is_none() {
					let value = RefValue{symbolic: true, value: "refs/heads/master".into()};
					d.update_ref("HEAD", &value, true)?;
					println!("created ugit repository at {:?}", d.home);
				} else {
					println!("ugit repositry already exists");
					return Err(NGitError::OperationFailed("can't init an existing repo".into()))
				}
			},
			Command::HashObject { file } => {
				let d = Data::read()?;
				if !file.try_exists()? {
					println!("can't hash non-existent file at {:?}", file)
				} else {
					let content = std::fs::read_to_string(file)?;
					let d = d.hash_object(content, data::DataType::Blob)?;
					println!("{}", d);
				}

			}
			Command::CatFile { digest } => {
				let d = Data::read()?;
				let digest = d.resolve(digest)?;
				let content = d.get_object(&digest, data::DataType::Any)?;
				// println!("File at {}:", digest);
				print!("{}", content);

			}
			Command::WriteTree => {
				let d = Data::read()?;
				
				let oid = write_tree(&d)?;
				println!("{}", oid);
				
			}

			Command::ReadTree { digest } => {
				let d = Data::read()?;
				let digest = d.resolve(digest)?;
				let fs = read_tree(&digest, &d)?;
				println!("restored {} files", fs.len());
				for f in fs {
					println!("- {}", f.to_str().unwrap())
				}
			}

			Command::Commit { message } => {
				let d = Data::read()?;
				let oid = write_tree(&d)?;
				let mut commit = String::new();
				commit += &format!("tree {}\n", oid);
				if let Some(head) = d.get_ref("HEAD", true)? {
					commit += &format!("parent {}\n", head.value);
				}
				if let Some(merge_head) = d.get_ref("MERGE_HEAD", true)? {
					commit += &format!("parent {}\n", merge_head.value);
					d.del_ref("MERGE_HEAD", false)?;
				}
				commit += &format!("\n{}\n", message);
				let oid = d.hash_object(commit, data::DataType::Commit)?;
				let r = RefValue { value: oid, symbolic: false};
				d.update_ref("HEAD", &r, true)?;
				println!("{}", r.value);
			}

			Command::Log { oid } => {
				let d = Data::read()?;
				let oid = d.resolve(oid)?;

				let mut h: HashMap<String, Vec<String>> = HashMap::new();
				for (r, v) in d.iter_refs(true, None::<String>)? {
					h.entry(r).or_default().push(v.value);

				}

				for (c, _) in get_history_for_commits(&d, HashSet::from([oid]))? {
					Self::print_commit(h.get(&c), Commit::get(&d, c)?);
				}
				// while let Some(ref commit) = oid {
				//     println!("commit {}\n", commit);
				//     let commit = Commit::get(&d, commit)?;
				//     for m in commit.message.lines() {
				//         println!("    {}", m);
				//     }
				//     println!("---");
				//     oid = commit.parent;
				// }
				
			}
			Command::Checkout { commit: name } => {
				let d = Data::read()?;
				let oid = d.resolve(&name)?;
				let commit = Commit::get(&d, oid)?;
				read_tree(&commit.tree, &d)?;
				let ref_val = if d.get_branch(&name)?.is_some() { 
					RefValue { symbolic: true, value: format!("refs/heads/{}", name)} 
				} else { 
					RefValue { value: commit.oid, symbolic: false }
				};
				d.update_ref("HEAD", &ref_val, false)?;
				println!("checked out {}", ref_val.value);
			}
			Command::Tag { name, oid } => {
				let d = Data::read()?;
				let oid = d.resolve(oid)?;
				let ref_val = RefValue { symbolic: false, value: oid };
				d.update_ref(format!("refs/tags/{}", &name), &ref_val, true)?;
				println!("tagged '{}' with '{}'", ref_val.value, name);
			}
			Command::K => {
				let d = Data::read()?;

				let mut dot = String::from("digraph commits {\n");
				let refs = d.iter_refs(false, None::<String>)?;
				let mut oids = HashSet::new();
				for (name, oid) in refs {
					dot += &format!("\"{}\" [shape=note]\n", &name);
					dot += &format!("\"{}\" -> \"{}\"\n", &name, &oid.value);
					// println!("{} {}", name, &oid);
					if !oid.symbolic {
						oids.insert(oid.value);
					}
				}
				println!("\n\n");

				let tree = get_history_for_commits(&d, oids)?;
				for (oid, parent) in tree {
					dot += &format!("\"{}\" [shape=box style=filled label=\"{}\"]\n", &oid, &oid[..10]);
					for p in parent {
						// println!("Parent {}", oid);
						dot += &format!("\"{}\" -> \"{}\"\n", &oid, &p);
					}
				}

				dot += "}";
				println!("{}", dot);
				
				use std::io::Write;

				use std::process::{Command as TerminalCommand, Stdio};

				let mut child = TerminalCommand::new("dot")
					.args(["-Tgtk", "/dev/stdin"])
					.stdin(Stdio::piped())
					.spawn()?;

				let mut stdin = child.stdin.take().unwrap();
				stdin.write_all(&dot.into_bytes())?;
				drop(stdin);

				let status = child.wait()?;
				println!("Exited with {}", status)

			}
			Command::Branch { name, start_point } => {
				let d = Data::read()?;
				let start_point = RefValue {
					symbolic: false, 
					value: d.resolve(start_point)?
				};
				create_branch(&d, &name, &start_point)?;
				println!("created branch '{}' at {}", name, &start_point.value[..10])
			}
			Command::ListBranch => {
				let d = Data::read()?;
				let current_branch = d.get_current_branch()?;
				for b in d.iter_branch_names()? {
					let prefix = if let Some(ref cb) = current_branch && b == *cb {
						"*"
					} else {
						" "
					};
					println!("{} {}", prefix, b)
				}
			}
			Command::Status => {
				let d = Data::read()?;
				let head = d.resolve("@")?;
				match d.get_current_branch()? {
					Some(branch) => println!("on branch {}", branch), 
					None => {
						println!("HEAD detached at {}", &head[..10])
					}
				}
				if let Some(merge_head) = d.get_ref("MERGE_HEAD", true)? {
					println!("merging with {}", &merge_head.value[..10])
				}
				let head = Commit::get(&d, head)?;

				let index = Index::read(&d)?;
				let head_tree = get_tree(&head.tree, &d, None)?;
				let index_tree = index.to_index_tree();
				let working_tree = d.get_working_tree()?;
				
				let diff = DiffTree::between(&d, head_tree, index_tree.clone())?;
				println!("changes to be committed:");
				for (action, file) in diff.to_list() {
					let fp = file.to_string_lossy().to_string();
					println!("    {action} {fp}")
				}
				println!();
				let diff = DiffTree::between(&d, index_tree, working_tree)?;
				println!("changes not staged for commit:");
				for (action, file) in diff.to_list() {
					let fp = file.to_string_lossy().to_string();
					println!("    {action} {fp}")
				}

			}

			Command::Reset { oid } => {
				let d = Data::read()?;
				let val = RefValue { value: oid, symbolic: false };
				d.update_ref("HEAD", &val, true)?;
				let h = d.get_ref("HEAD", true)?;
				match h {
					Some(rf) if rf.value == val.value => println!("HEAD reset to {}", rf.value),
					_ => return Err(NGitError::OperationFailed("reset".into()))
				}
			}
			Command::Show { oid } => {
				let d = Data::read()?;
				let commit = Commit::get(&d, oid)?;
				let from = if let Some(p) = commit.parents.first() {
					let commit = Commit::get(&d, p.clone())?;
					get_tree(&commit.tree, &d, None)?
				} else {
					Default::default()
				};
				let to = get_tree(&commit.tree ,&d, None)?;

				Self::print_commit(None, commit);
				let diff = DiffTree::between(&d, from, to)?;
				for (action, file) in diff.to_list() {
					let fp = file.to_string_lossy().to_string();
					println!("    {action} {fp}")
				}
			}
			Command::Diff { cached, commit } => {
				let d = Data::read()?;
				let index = Index::read(&d)?;
				let (from, to) = match (cached, commit) {
					(false, None) => (index.to_index_tree(), d.get_working_tree()?),
					(f, a) => {
						let commit = Commit::get(&d, d.resolve(a.unwrap_or("@".into()))?)?;
						let from = get_tree(&commit.tree, &d, None)?;
						let to = if f {index.to_index_tree()} else {d.get_working_tree()?};
						(from, to)
					} 
				};
				let diff = DiffTree::between(&d, from, to)?;
				for (action, file) in diff.to_list() {
					let fp = file.to_string_lossy().to_string();
					println!("    {action} {fp}")
				}
			}

			Command::Merge { commit } => {
				let d = Data::read()?;
				let commit = d.resolve(commit)?;
				let head = d.resolve("HEAD")?;

				let o_commit = Commit::get(&d, commit)?;
				let merge_base = get_merge_base(&d, o_commit.oid.clone(), head.clone())?;
				
				if merge_base.as_ref().map_or(false, |b| *b == head) {
					read_tree(&o_commit.tree, &d)?;
					d.update_ref("HEAD", &RefValue { value: o_commit.oid, symbolic: false }, true)?;
					println!("fast forward merge, no need to commit")
				} else {
					let h_commit = Commit::get(&d, head)?;
					d.update_ref("MERGE_HEAD", &RefValue { value: o_commit.oid, symbolic: false }, true)?;
					let base_tree = if let Some(b) = merge_base {
						Some(Commit::get(&d, b)?.tree)
					} else {
						None
					};
					let files = read_tree_merged(&d, &base_tree, &h_commit.tree, &o_commit.tree)?;
					println!("merged in working tree");
					println!("files touched:");
					for f in files {
						println!("    {:?}", f)
					}
					println!("please commit")
				}
			}
			Command::MergeBase { commit1, commit2 } => {
				let d = Data::read()?;
				println!("common ancestor found: {:?}", get_merge_base(&d, commit1, commit2)?)


			}
			Command::Fetch { remote } => {
				let rd = Data::fetch(PathBuf::from(remote))?;
				let ld = Data::read()?;
				let remote_prefix = "refs/heads";
				let local_prefix = "refs/remote";
				println!("Will fetch the following refs:");
				let refs = rd.iter_refs(true, Some(remote_prefix))?;
				let commits = refs.iter().map(|(_, rv)| &rv.value).cloned().collect();
				for oid in iter_objects_in_commits(&rd, commits)?{
					ld.fetch_from_remote(&rd, &oid, false)?;
				}

				for (rname, rval) in refs {
					ld.update_ref(format!("{}/{}", local_prefix, &rname[remote_prefix.len()+1..]), &rval, true)?;
					// println!("- {}", refs.0)
				}
			}
			Command::Push { remote, branch } => {
				let rd = Data::fetch(PathBuf::from(remote))?;
				let ld = Data::read()?;

				let ref_addr = format!("refs/heads/{branch}");
				let ref_val = match ld.get_ref(&ref_addr, true)? {
					Some(rv) => rv,
					None => return Err(NGitError::OperationFailed("push branch doesn't exist".into()))
				};

				let remote_ref = rd.get_ref(&ref_addr, true)?;
				if let Some(rv) = remote_ref && iter_objects_in_commits(&rd, vec![rv.value])?.contains(&ref_val.value) {
					return Err(NGitError::NoForcePush(format!("branch '{ref_addr}' is an ancestor of current branch")))
				}

				let remote_refs = rd.iter_refs(true, None::<&str>)?;
				let known_remote_refs = remote_refs.iter().filter_map(|(_, rv)| if ld.get_path(&["object", rv.value.as_ref()]).is_file() {Some(rv.value.clone())} else {None}).collect();
				let remote_objs = iter_objects_in_commits(&ld, known_remote_refs)?.into_iter().collect::<HashSet<_>>();
				let local_objs = iter_objects_in_commits(&ld, vec![ref_val.value.clone()])?.into_iter().collect::<HashSet<_>>();

				let obj_to_push = local_objs.difference(&remote_objs);

				for obj in obj_to_push {
					ld.push_to_remote(&rd, &obj)?;
				}
				
				rd.update_ref(ref_addr, &RefValue { value: ref_val.value, symbolic: false }, true)?;
			}
			Command::Add { files } => {
				if files.len() == 0 {
					return Err(NGitError::MissingArgument("add".into(), "files".into()))
				}
				let d = Data::read()?;
				let mut i = Index::read(&d)?;
				for f in files {
					i.add(f)?;
				}
			}
			Command::Unknown(s) => println!("{} is not a valid command", s),
			Command::Empty => println!("no command provided")
		}
		Ok(())
	}
}
fn main() -> Result<(), NGitError> {
	let cmd = Command::run()?;
	cmd.handle()
}

