use clap::Args;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use url::Url;

use crate::collector::Env;
use crate::config::Config;
use crate::git::GitClient;

/// CLI tool to parse, check, and visualize pipeline configurations.
#[derive(Parser, Debug)]
#[command(
    name = "interpreter",
    about = "A tool for managing and validating pipeline configurations"
)]
pub struct CliCommand {
    #[command(subcommand)]
    subcommand: CliSubcommand,
}

impl CliCommand {
    pub fn run(self) -> anyhow::Result<()> {
        match self.subcommand {
            CliSubcommand::Show(args) => run_show(args),
            CliSubcommand::Check(args) => run_check(args),
            CliSubcommand::GitCloneShow(args) => run_git_clone_show(args),
        }
    }
}

#[derive(Subcommand, Debug)]
enum CliSubcommand {
    #[command(
        name = "show",
        about = "Prints the dependency graph in the specified format"
    )]
    Show(ShowArgs),

    #[command(
        name = "check",
        about = "Checks for syntax errors and cyclical dependencies"
    )]
    Check(CheckArgs),

    #[command(
        name = "git-clone-show",
        about = "Clones a Git repository and prints the dependency graph. Intended to be used in CI."
    )]
    GitCloneShow(GitCloneShowArgs),
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum ShowFormat {
    ASCII,
    JSON,
    DOTGRAPH,
}

// We implement Display so we can easily print or use the format if needed
impl std::fmt::Display for ShowFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            ShowFormat::ASCII => "ascii",
            ShowFormat::JSON => "json",
            ShowFormat::DOTGRAPH => "dotgraph",
        };
        write!(f, "{}", val)
    }
}

#[derive(Args, Debug)]
struct ShowArgs {
    /// Path to the pipeline config file
    #[arg(long, value_name = "FILE", default_value = "muzan.py")]
    input: PathBuf,

    /// Format to print the dependency graph in
    #[arg(long, value_enum, value_name = "FORMAT", default_value_t = ShowFormat::ASCII)]
    format: ShowFormat,

    #[arg(long, value_parser = parse_key_val, action = clap::ArgAction::Append)]
    env: Vec<(String, String)>,
}

#[derive(Args, Debug)]
struct CheckArgs {
    /// Path to the pipeline config file
    #[arg(long, value_name = "FILE", default_value = "muzan.py")]
    input: PathBuf,

    #[arg(long, value_parser = parse_key_val, action = clap::ArgAction::Append)]
    env: Vec<(String, String)>,
}

#[derive(Args, Debug, Clone, Serialize, Deserialize)]
pub struct GitCloneShowArgs {
    /// Git repository URL to clone
    #[arg(long, value_name = "URL")]
    url: Url,

    /// Branch to checkout
    #[arg(long, value_name = "BRANCH")]
    branch: String,

    /// Specific commit SHA to checkout
    #[arg(long, value_name = "COMMIT_SHA")]
    commit: String,

    /// Directory where the repository should be cloned
    #[arg(long, value_name = "PATH")]
    target_dir: PathBuf,

    /// Path to the pipeline config file inside the cloned repo
    #[arg(long, value_name = "FILE")]
    input: PathBuf,

    /// Format to print the dependency graph in
    #[arg(long, value_enum, value_name = "FORMAT")]
    format: ShowFormat,

    #[arg(long, value_parser = parse_key_val, action = clap::ArgAction::Append)]
    env: Vec<(String, String)>,
}

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=VALUE: no `=` found in [{s}]"))?;
    let key = s[..pos].trim();
    let value = s[pos + 1..].trim();
    if key.is_empty() {
        return Err(format!("invalid KEY=VALUE: key is empty in [{s}]"));
    }
    Ok((key.to_string(), value.to_string()))
}

fn run_show(args: ShowArgs) -> anyhow::Result<()> {
    let ShowArgs { input, format, env } = args;

    let env = env
        .into_iter()
        .try_fold(Env::new(), |mut acc, (key, value)| {
            if acc.contains_key(&key) {
                anyhow::bail!("Duplicate env key [{key}]");
            }
            acc.insert(key, value);
            Ok(acc)
        })?;

    let config = Config::from_file(&input, &env)?;

    let output = match format {
        ShowFormat::ASCII => config.to_ascii_graph(),
        ShowFormat::JSON => config.to_json()?,
        ShowFormat::DOTGRAPH => config.to_dot_graph(),
    };

    println!("{}", output);

    Ok(())
}

fn run_check(args: CheckArgs) -> anyhow::Result<()> {
    let CheckArgs { input, env } = args;

    let env = env
        .into_iter()
        .try_fold(Env::new(), |mut acc, (key, value)| {
            if acc.contains_key(&key) {
                anyhow::bail!("Duplicate env key [{key}]");
            }
            acc.insert(key, value);
            Ok(acc)
        })?;

    let config = Config::from_file(&input, &env)?;

    println!("No syntax errors or dependency cycles detected.");
    println!("Found {} pipelines total.", config.pipelines.len());
    println!("Found {} jobs total.", config.jobs.len());

    Ok(())
}

fn run_git_clone_show(args: GitCloneShowArgs) -> anyhow::Result<()> {
    let GitCloneShowArgs {
        url,
        branch,
        commit,
        target_dir,
        input,
        format,
        env,
    } = args;

    let env = env
        .into_iter()
        .try_fold(Env::new(), |mut acc, (key, value)| {
            if acc.contains_key(&key) {
                anyhow::bail!("Duplicate env key [{key}]");
            }
            acc.insert(key, value);
            Ok(acc)
        })?;

    let git_client = GitClient::try_default()?;
    git_client.checkout_commit(&url, &branch, &target_dir, &commit)?;

    let input = target_dir.join(input);
    let config = Config::from_file(&input, &env)?;

    let output = match format {
        ShowFormat::ASCII => config.to_ascii_graph(),
        ShowFormat::JSON => config.to_json()?,
        ShowFormat::DOTGRAPH => config.to_dot_graph(),
    };

    println!("{}", output);

    Ok(())
}
