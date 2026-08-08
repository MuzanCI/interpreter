use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let input = PathBuf::from(manifest_dir)
        .join("tests")
        .join("pipeline_integration_test.muzan");
    let cmd_str = format!("interpreter check --input {}", input.display());
    let output = Command::new("sh").arg("-c").arg(&cmd_str).output().unwrap();
    if !output.status.success() {
        eprintln!("Non-zero exit status for command [{}]", &cmd_str);
        eprintln!("\nStderr:");
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        std::process::exit(1);
    }
    eprintln!("Successfully ran [{}]", &cmd_str);
}
