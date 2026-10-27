use std::{env, path::PathBuf};

/// Writes the showroom plan to `target/diff-pacing.json` (or the given path).
/// `--plain` declares the same diff without paced marks, for comparison.
fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    let plain = args.iter().any(|arg| arg == "--plain");
    args.retain(|arg| arg != "--plain");
    let output = args
        .first()
        .map_or_else(|| PathBuf::from("target/diff-pacing.json"), PathBuf::from);
    psychopomp_diff_pacing::build(!plain)?.write_or_print(Some(output.clone()))?;
    eprintln!("Wrote {}", output.display());
    Ok(())
}
