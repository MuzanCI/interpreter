use std::process;

use clap::Parser;
use muzanci_interpreter::Args;
use muzanci_interpreter::EvalContext;
use muzanci_interpreter::Interpreter;
use muzanci_interpreter::git::GitClient;

fn main() {
    let args = Args::parse();

    let git_client = GitClient::try_default().unwrap();
    git_client
        .checkout_commit(
            &args.clone_url,
            &args.clone_target_dir,
            &args.git_branch,
            &args.git_commit,
        )
        .unwrap();

    let interpreter = {
        let ctx = EvalContext {
            git_branch: args.git_branch,
            git_commit: args.git_commit,
            git_clone_url: args.clone_url.to_string(),
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
