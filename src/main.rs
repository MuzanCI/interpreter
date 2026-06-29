use std::path::PathBuf;
use std::process;

use clap::Parser;
use muzanci_interpreter::{EvalContext, Interpreter};

/// MuzanCI pipeline interpreter.
///
/// Parses and evaluates a muzan.star configuration file, injecting the
/// provided CI context globals, and prints the resulting pipelines as JSON.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Path to the root pipeline configuration file.
    #[arg(default_value = "muzan.py")]
    file: PathBuf,

    /// Repository URL (injected as GIT_REPO).
    #[arg(long, default_value = "GIT_REPO")]
    git_repo: String,

    /// Branch name (injected as GIT_BRANCH).
    #[arg(long, default_value = "GIT_BRANCH")]
    git_branch: String,

    /// Commit SHA (injected as GIT_COMMIT).
    #[arg(long, default_value = "GIT_COMMIT")]
    git_commit: String,
}

fn main() {
    let args = Args::parse();

    let ctx = EvalContext {
        git_repo: args.git_repo,
        git_branch: args.git_branch,
        git_commit: args.git_commit,
    };

    let interpreter = Interpreter::new(ctx);
    match interpreter.evaluate(&args.file) {
        Ok(result) => {
            let output = serde_json::json!({
                "jobs": result.jobs,
                "pipelines": result.pipelines,
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            process::exit(1);
        }
    }
}
