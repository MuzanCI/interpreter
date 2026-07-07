use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;
use muzanci_interpreter::git::{self, GitClient};
use muzanci_interpreter::{EvalContext, Interpreter};

/// MuzanCI pipeline interpreter.
///
/// Parses and evaluates a muzan.star configuration file, injecting the
/// provided CI context globals, and prints the resulting pipelines as JSON.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Path to read the root pipeline configuration file.
    #[arg(default_value = "./external/customer-repo/muzan.py")]
    root_file: PathBuf,

    /// Git clone URL
    #[arg(long, default_value = "https://github.com/MuzanCI/customer-repo.git")]
    clone_url: String,

    /// Branch name
    #[arg(long, default_value = "main")]
    git_branch: String,

    /// Commit SHA
    #[arg(long, default_value = "62e23f12581dcd21d2fe57254aeed62b9afe1f54")]
    git_commit: String,

    #[arg(long, default_value = "./muzanci.eval_result.json")]
    /// Path to write the output JSON.
    output_file: PathBuf,
}

fn main() {
    let args = Args::parse();

    let target_dir = Path::new("./external/customer-repo");

    let git_client = GitClient::try_default().unwrap();
    git_client
        .checkout_commit(
            &args.clone_url,
            &args.git_branch,
            &args.git_commit,
            target_dir,
        )
        .unwrap();

    let interpreter = {
        let ctx = EvalContext {
            git_branch: args.git_branch,
            git_commit: args.git_commit,
            git_clone_url: args.clone_url,
        };
        Interpreter::new(ctx)
    };

    eprintln!("Evaluating root file [{}]...", args.root_file.display());
    let eval_result = match interpreter.evaluate(&args.root_file) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("error: {e:#}");
            process::exit(1);
        }
    };

    eprintln!("Evaluation successful!");

    eprintln!(
        "Writing evaluation result to [{}]...",
        args.output_file.display()
    );
    std::fs::write(
        &args.output_file,
        serde_json::to_string_pretty(&eval_result).unwrap(),
    )
    .unwrap();
    eprintln!("Done!");
}
