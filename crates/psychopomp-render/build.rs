//! Stamps the binary with the engine commit it was built from, for
//! `psychopomp --version`. A project that depends on the `psychopomp` library
//! by git `rev` compares this commit with its `Cargo.lock` (the plan JSON is the
//! contract between the two). `cargo install --git` builds from a git checkout,
//! so the commit is known there too; a source tree without git reports
//! `unknown` unless `PSYCHOPOMP_COMMIT` is set at build time.
use std::{path::Path, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn main() {
    println!("cargo:rerun-if-env-changed=PSYCHOPOMP_COMMIT");
    let commit = std::env::var("PSYCHOPOMP_COMMIT")
        .ok()
        .filter(|commit| !commit.is_empty())
        .or_else(|| git(&["rev-parse", "HEAD"]));
    // Rebuild the stamp when HEAD moves: HEAD itself, the branch it names, and
    // packed refs (`--git-path` resolves worktrees and the common directory).
    let mut watched = vec!["HEAD".to_owned(), "packed-refs".to_owned()];
    watched.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    println!(
        "cargo:rustc-env=PSYCHOPOMP_COMMIT={}",
        commit.as_deref().unwrap_or("unknown")
    );
}
