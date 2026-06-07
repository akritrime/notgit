use std::path::PathBuf;

use crate::{
    cli::commands::Command,
    errors::NGitError,
    types::{BranchName, CommitOid, Revision},
};

#[derive(Clone, Copy, Debug)]
enum Token {
    CommitOid(&'static str),
    Path(&'static str),
    Ref(&'static str),
    Branch(&'static str),
    Text(&'static str),
    Flag {
        name: &'static str,
        choices: &'static [&'static str],
    },
    OptionalFlag(&'static str),
    OptionalRef,
    Rest(&'static str),
    Paths(&'static str),
}

#[derive(Debug)]
enum ParsedToken {
    CommitOid(CommitOid),
    Path(PathBuf),
    Ref(Revision),
    Branch(BranchName),
    Text(String),
    Flag(bool),
    Rest(String),
    Paths(Vec<PathBuf>),
}

impl ParsedToken {
    fn into_commit_oid(self) -> CommitOid {
        match self {
            Self::CommitOid(value) => value,
            _ => unreachable!("parser produced unexpected token type"),
        }
    }

    fn into_path(self) -> PathBuf {
        match self {
            Self::Path(value) => value,
            _ => unreachable!("parser produced unexpected token type"),
        }
    }

    fn into_ref(self) -> Revision {
        match self {
            Self::Ref(value) => value,
            _ => unreachable!("parser produced unexpected token type"),
        }
    }

    fn into_branch(self) -> BranchName {
        match self {
            Self::Branch(value) => value,
            _ => unreachable!("parser produced unexpected token type"),
        }
    }

    fn into_text(self) -> String {
        match self {
            Self::Text(value) => value,
            _ => unreachable!("parser produced unexpected token type"),
        }
    }

    fn into_flag(self) -> bool {
        match self {
            Self::Flag(value) => value,
            _ => unreachable!("parser produced unexpected token type"),
        }
    }

    fn into_rest(self) -> String {
        match self {
            Self::Rest(value) => value,
            _ => unreachable!("parser produced unexpected token type"),
        }
    }

    fn into_paths(self) -> Vec<PathBuf> {
        match self {
            Self::Paths(value) => value,
            _ => unreachable!("parser produced unexpected token type"),
        }
    }
}

struct ParsedTokens {
    values: std::vec::IntoIter<ParsedToken>,
}

impl ParsedTokens {
    fn next(&mut self) -> ParsedToken {
        self.values
            .next()
            .expect("parser token shape and consumer are out of sync")
    }
}

pub fn parse_env() -> Result<Command, NGitError> {
    parse_args(std::env::args().skip(1))
}

pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, NGitError> {
    let args = args.into_iter().collect::<Vec<_>>();
    let Some((command, raw_args)) = args.split_first() else {
        return Ok(Command::Empty);
    };

    match command.as_str() {
        "init" => {
            parse_tokens("init", raw_args, &[])?;
            Ok(Command::Init)
        }
        "hash-object" => {
            let mut args = parse_tokens("hash-object", raw_args, &[Token::Path("file")])?;
            Ok(Command::HashObject {
                file: args.next().into_path(),
            })
        }
        "cat-file" => {
            let mut args = parse_tokens("cat-file", raw_args, &[Token::Ref("hash digest")])?;
            Ok(Command::CatFile {
                revision: args.next().into_ref(),
            })
        }
        "write-tree" => {
            parse_tokens("write-tree", raw_args, &[])?;
            Ok(Command::WriteTree)
        }
        "commit" => {
            let mut args = parse_tokens(
                "commit",
                raw_args,
                &[
                    Token::Flag {
                        name: "message",
                        choices: &["-m", "--message"],
                    },
                    Token::Rest("message"),
                ],
            )?;
            let _message_flag = args.next().into_flag();
            Ok(Command::Commit {
                message: args.next().into_rest(),
            })
        }
        "read-tree" => {
            let mut args = parse_tokens("read-tree", raw_args, &[Token::Ref("hash digest")])?;
            Ok(Command::ReadTree {
                revision: args.next().into_ref(),
            })
        }
        "log" => {
            let mut args = parse_tokens("log", raw_args, &[Token::OptionalRef])?;
            Ok(Command::Log {
                revision: args.next().into_ref(),
            })
        }
        "checkout" => {
            let mut args = parse_tokens("checkout", raw_args, &[Token::Ref("commit")])?;
            Ok(Command::Checkout {
                revision: args.next().into_ref(),
            })
        }
        "tag" => {
            let mut args =
                parse_tokens("tag", raw_args, &[Token::Text("name"), Token::OptionalRef])?;
            Ok(Command::Tag {
                name: args.next().into_text(),
                target: args.next().into_ref(),
            })
        }
        "branch" => {
            if raw_args.is_empty() {
                Ok(Command::ListBranch)
            } else {
                let mut args = parse_tokens(
                    "branch",
                    raw_args,
                    &[Token::Branch("name"), Token::OptionalRef],
                )?;
                Ok(Command::Branch {
                    name: args.next().into_branch(),
                    start_point: args.next().into_ref(),
                })
            }
        }
        "K" | "k" => {
            parse_tokens(command, raw_args, &[])?;
            Ok(Command::K)
        }
        "status" => {
            parse_tokens("status", raw_args, &[])?;
            Ok(Command::Status)
        }
        "reset" => {
            let mut args = parse_tokens("reset", raw_args, &[Token::Ref("revision")])?;
            Ok(Command::Reset {
                revision: args.next().into_ref(),
            })
        }
        "show" => {
            let mut args = parse_tokens("show", raw_args, &[Token::Ref("revision")])?;
            Ok(Command::Show {
                revision: args.next().into_ref(),
            })
        }
        "diff" => {
            let mut args = parse_tokens(
                "diff",
                raw_args,
                &[Token::OptionalFlag("--cached"), Token::OptionalRef],
            )?;
            let cached = args.next().into_flag();
            let commit = if raw_args.len() > usize::from(cached) {
                Some(args.next().into_ref())
            } else {
                // let _default = args.next().into_ref();
                None
            };
            Ok(Command::Diff { commit, cached })
        }
        "merge" => {
            let mut args = parse_tokens("merge", raw_args, &[Token::Ref("commit")])?;
            Ok(Command::Merge {
                revision: args.next().into_ref(),
            })
        }
        "merge-base" => {
            let mut args = parse_tokens(
                "merge-base",
                raw_args,
                &[Token::CommitOid("commit1"), Token::CommitOid("commit2")],
            )?;
            Ok(Command::MergeBase {
                commit1: args.next().into_commit_oid(),
                commit2: args.next().into_commit_oid(),
            })
        }
        "fetch" => {
            let mut args = parse_tokens("fetch", raw_args, &[Token::Path("remote")])?;
            Ok(Command::Fetch {
                remote: args.next().into_path(),
            })
        }
        "push" => {
            let mut args = parse_tokens(
                "push",
                raw_args,
                &[Token::Path("remote"), Token::Branch("branch")],
            )?;
            Ok(Command::Push {
                remote: args.next().into_path(),
                branch: args.next().into_branch(),
            })
        }
        "add" => {
            let mut args = parse_tokens("add", raw_args, &[Token::Paths("files")])?;
            Ok(Command::Add {
                files: args.next().into_paths(),
            })
        }
        other => Ok(Command::Unknown(other.to_owned())),
    }
}

fn parse_tokens(
    command: &str,
    raw_args: &[String],
    tokens: &[Token],
) -> Result<ParsedTokens, NGitError> {
    let mut arg_index = 0;
    let mut values = vec![];

    for token in tokens {
        match *token {
            Token::CommitOid(name) => {
                let raw = required_arg(command, raw_args, arg_index, name)?;
                values.push(ParsedToken::CommitOid(CommitOid::new(raw)?));
                arg_index += 1;
            }
            Token::Path(name) => {
                let raw = required_arg(command, raw_args, arg_index, name)?;
                values.push(ParsedToken::Path(PathBuf::from(raw)));
                arg_index += 1;
            }
            Token::Ref(name) => {
                let raw = required_arg(command, raw_args, arg_index, name)?;
                values.push(ParsedToken::Ref(Revision::new(raw)?));
                arg_index += 1;
            }
            Token::Branch(name) => {
                let raw = required_arg(command, raw_args, arg_index, name)?;
                values.push(ParsedToken::Branch(BranchName::new(raw)?));
                arg_index += 1;
            }
            Token::Text(name) => {
                let raw = required_arg(command, raw_args, arg_index, name)?;
                values.push(ParsedToken::Text(raw.to_owned()));
                arg_index += 1;
            }
            Token::Flag { name, choices } => {
                let raw = required_arg(command, raw_args, arg_index, name)?;
                if !choices.contains(&raw) {
                    return Err(NGitError::MissingArgument(command.into(), name.into()));
                }
                values.push(ParsedToken::Flag(true));
                arg_index += 1;
            }
            Token::OptionalFlag(flag) => {
                let present = raw_args.get(arg_index).map_or(false, |raw| raw == flag);
                if present {
                    arg_index += 1;
                }
                values.push(ParsedToken::Flag(present));
            }
            Token::OptionalRef => {
                let value = if let Some(raw) = raw_args.get(arg_index) {
                    arg_index += 1;
                    Revision::new(raw.clone())?
                } else {
                    Revision::at_head()
                };
                values.push(ParsedToken::Ref(value));
            }
            Token::Rest(name) => {
                if arg_index >= raw_args.len() {
                    return Err(NGitError::MissingArgument(command.into(), name.into()));
                }
                values.push(ParsedToken::Rest(raw_args[arg_index..].join(" ")));
                arg_index = raw_args.len();
            }
            Token::Paths(name) => {
                if arg_index >= raw_args.len() {
                    return Err(NGitError::MissingArgument(command.into(), name.into()));
                }
                values.push(ParsedToken::Paths(
                    raw_args[arg_index..].iter().map(PathBuf::from).collect(),
                ));
                arg_index = raw_args.len();
            }
        }
    }

    if arg_index < raw_args.len() {
        return Err(NGitError::NoArgumentExpected);
    }

    Ok(ParsedTokens {
        values: values.into_iter(),
    })
}

fn required_arg<'a>(
    command: &str,
    raw_args: &'a [String],
    index: usize,
    name: &str,
) -> Result<&'a str, NGitError> {
    raw_args
        .get(index)
        .map(|value| value.as_str())
        .ok_or_else(|| NGitError::MissingArgument(command.into(), name.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn parse(raw: &[&str]) -> Result<Command, NGitError> {
        parse_args(raw.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn parses_empty_input() {
        assert_eq!(parse(&[]).unwrap(), Command::Empty);
    }

    #[test]
    fn parses_commands_without_args_and_rejects_extra_args() {
        assert_eq!(parse(&["status"]).unwrap(), Command::Status);
        assert!(matches!(
            parse(&["status", "extra"]),
            Err(NGitError::NoArgumentExpected)
        ));
    }

    #[test]
    fn parses_commit_message_rest() {
        assert_eq!(
            parse(&["commit", "-m", "subject", "body"]).unwrap(),
            Command::Commit {
                message: "subject body".into()
            }
        );
        assert_eq!(
            parse(&["commit", "--message", "subject"]).unwrap(),
            Command::Commit {
                message: "subject".into()
            }
        );
    }

    #[test]
    fn rejects_commit_without_message_flag_or_message() {
        assert!(matches!(
            parse(&["commit", "subject"]),
            Err(NGitError::MissingArgument(command, name))
                if command == "commit" && name == "message"
        ));
        assert!(matches!(
            parse(&["commit", "-m"]),
            Err(NGitError::MissingArgument(command, name))
                if command == "commit" && name == "message"
        ));
    }

    #[test]
    fn parses_optional_refs() {
        assert_eq!(
            parse(&["log"]).unwrap(),
            Command::Log {
                revision: Revision::at_head()
            }
        );
        assert_eq!(
            parse(&["log", "topic"]).unwrap(),
            Command::Log {
                revision: Revision::new("topic").unwrap()
            }
        );
        assert_eq!(
            parse(&["tag", "v1"]).unwrap(),
            Command::Tag {
                name: "v1".into(),
                target: Revision::at_head()
            }
        );
    }

    #[test]
    fn parses_branch_list_and_create() {
        assert_eq!(parse(&["branch"]).unwrap(), Command::ListBranch);
        assert_eq!(
            parse(&["branch", "topic"]).unwrap(),
            Command::Branch {
                name: BranchName::new("topic").unwrap(),
                start_point: Revision::at_head()
            }
        );
        assert_eq!(
            parse(&["branch", "topic", "main"]).unwrap(),
            Command::Branch {
                name: BranchName::new("topic").unwrap(),
                start_point: Revision::new("main").unwrap()
            }
        );
    }

    #[test]
    fn parses_diff_forms() {
        assert_eq!(
            parse(&["diff"]).unwrap(),
            Command::Diff {
                commit: None,
                cached: false
            }
        );
        assert_eq!(
            parse(&["diff", "--cached"]).unwrap(),
            Command::Diff {
                commit: None,
                cached: true
            }
        );
        assert_eq!(
            parse(&["diff", "HEAD"]).unwrap(),
            Command::Diff {
                commit: Some(Revision::new("HEAD").unwrap()),
                cached: false
            }
        );
        assert_eq!(
            parse(&["diff", "--cached", "HEAD"]).unwrap(),
            Command::Diff {
                commit: Some(Revision::new("HEAD").unwrap()),
                cached: true
            }
        );
    }

    #[test]
    fn parses_oid_commands_and_rejects_invalid_oids() {
        assert_eq!(
            parse(&["reset", OID_A]).unwrap(),
            Command::Reset {
                revision: Revision::new(OID_A).unwrap()
            }
        );
        assert_eq!(
            parse(&["show", "@"]).unwrap(),
            Command::Show {
                revision: Revision::new("@").unwrap()
            }
        );
        assert_eq!(
            parse(&["merge-base", OID_A, OID_B]).unwrap(),
            Command::MergeBase {
                commit1: CommitOid::new(OID_A).unwrap(),
                commit2: CommitOid::new(OID_B).unwrap()
            }
        );
        assert!(matches!(
            parse(&["merge-base", OID_A, "not-an-oid"]),
            Err(NGitError::InvalidOid(_))
        ));
    }

    #[test]
    fn parses_path_commands() {
        assert_eq!(
            parse(&["hash-object", "file.txt"]).unwrap(),
            Command::HashObject {
                file: PathBuf::from("file.txt")
            }
        );
        assert_eq!(
            parse(&["add", "file.txt", "src/lib.rs"]).unwrap(),
            Command::Add {
                files: vec![PathBuf::from("file.txt"), PathBuf::from("src/lib.rs")]
            }
        );
        assert!(matches!(
            parse(&["add"]),
            Err(NGitError::MissingArgument(command, name))
                if command == "add" && name == "files"
        ));
    }

    #[test]
    fn parses_unknown_commands_without_validating_args() {
        assert_eq!(
            parse(&["wat", "anything"]).unwrap(),
            Command::Unknown("wat".into())
        );
    }
}
