use std::{
    collections::{HashMap, HashSet, VecDeque},
    vec,
};

use crate::types::{CommitOid, Oid, TreeOid};
use crate::{errors::NGitError, repository::Repository, tree::TreeStore};

#[derive(Debug)]
pub struct Commit {
    pub message: String,
    pub tree: TreeOid,
    pub parents: Vec<CommitOid>,
    pub oid: CommitOid,
}

impl Commit {
    pub fn load(repo: &Repository, oid: CommitOid) -> Result<Self, NGitError> {
        let object = repo.get_commit_text(&oid)?;
        let mut commit = object.trim().lines().peekable();

        let err = || NGitError::InvalidCommit(oid.to_string());
        let parse = |l: Option<&str>, term| -> Result<Oid, NGitError> {
            let val = l
                .filter(|s| s.starts_with(term))
                .ok_or_else(err)?
                .split(" ")
                .nth(1)
                .ok_or_else(err)?;
            Oid::new(val)
        };

        let tree = TreeOid::from_oid(parse(commit.next(), "tree")?);
        let mut parents = vec![];
        while let Some(l) = commit.peek() {
            if l.starts_with("parent") {
                parents.push(CommitOid::from_oid(parse(commit.next(), "parent")?));
            } else {
                break;
            }
        }
        let message = commit.collect::<Vec<_>>().join("\n").trim().to_owned();

        Ok(Self {
            message,
            tree,
            parents,
            oid,
        })
    }
}

pub type CommitParents = HashMap<CommitOid, Vec<CommitOid>>;

pub struct CommitGraph<'a> {
    repo: &'a Repository,
}

impl<'a> CommitGraph<'a> {
    pub fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    pub fn history_for(&self, oids: HashSet<CommitOid>) -> Result<CommitParents, NGitError> {
        let mut graph = CommitParents::new();

        fn walk(
            repo: &Repository,
            oid: CommitOid,
            graph: &mut CommitParents,
        ) -> Result<(), NGitError> {
            if graph.contains_key(&oid) {
                return Ok(());
            }
            let commit = Commit::load(repo, oid)?;
            graph
                .entry(commit.oid.clone())
                .or_default()
                .extend(commit.parents.clone());

            for parent in commit.parents {
                walk(repo, parent, graph)?;
            }

            Ok(())
        }

        for oid in oids {
            walk(self.repo, oid, &mut graph)?;
        }

        Ok(graph)
    }

    pub fn commits_and_parents(
        &self,
        commits: Vec<CommitOid>,
    ) -> Result<Vec<CommitOid>, NGitError> {
        let mut history = self.history_for(commits.iter().cloned().collect())?;
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

    pub fn merge_base(
        &self,
        commit1: CommitOid,
        commit2: CommitOid,
    ) -> Result<Option<CommitOid>, NGitError> {
        let parents1 = self.commits_and_parents(vec![commit1])?;
        let parents2 = self.commits_and_parents(vec![commit2])?;
        for p in parents2 {
            if parents1.contains(&p) {
                return Ok(Some(p));
            }
        }

        Ok(None)
    }

    pub fn objects_in_commits(&self, commits: Vec<CommitOid>) -> Result<Vec<Oid>, NGitError> {
        let mut oids = vec![];
        let mut visited = HashSet::new();

        fn iter_objects_in_trees(
            repo: &Repository,
            oid: &TreeOid,
            visited: &mut HashSet<Oid>,
        ) -> Result<Vec<Oid>, NGitError> {
            TreeStore::new(repo).object_ids_in_tree(oid, visited)
        }

        for oid in self.commits_and_parents(commits)? {
            oids.push(oid.clone().into_oid());
            let commit = Commit::load(self.repo, oid)?;
            if !visited.contains(commit.tree.as_oid()) {
                oids.extend(iter_objects_in_trees(
                    self.repo,
                    &commit.tree,
                    &mut visited,
                )?)
            }
        }

        Ok(oids)
    }
}
