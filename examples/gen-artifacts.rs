//! Generate the man page and shell completions from the clap definition.
//!
//! Run `cargo run --example gen-artifacts` after changing the CLI, then commit
//! the refreshed `man/` and `completions/`. CI regenerates them and fails if
//! they drift.

use std::path::PathBuf;

use clap::CommandFactory;
use clap_complete::Shell;
use overflight::cli::Cli;

fn main() -> std::io::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let man_dir = root.join("man");
    let completions_dir = root.join("completions");
    std::fs::create_dir_all(&man_dir)?;
    std::fs::create_dir_all(&completions_dir)?;

    let mut command = Cli::command();

    let mut man_file = std::fs::File::create(man_dir.join("overflight.1"))?;
    clap_mangen::Man::new(command.clone()).render(&mut man_file)?;

    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        clap_complete::generate_to(shell, &mut command, "overflight", &completions_dir)?;
    }

    Ok(())
}
