use notgit::{cli, errors::NGitError};

fn main() -> Result<(), NGitError> {
    let command = cli::parse_env()?;
    command.execute()
}
