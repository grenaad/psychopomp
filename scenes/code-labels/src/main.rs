use std::{env, fs, path::PathBuf};

/// Writes the reel to `target/code-labels/reel.json` (or the given path) and
/// each segment beside it.
fn main() -> anyhow::Result<()> {
    let output = env::args().nth(1).map_or_else(
        || PathBuf::from("target/code-labels/reel.json"),
        PathBuf::from,
    );
    let directory = output.parent().map(PathBuf::from).unwrap_or_default();
    fs::create_dir_all(&directory)?;
    let reel = psychopomp_code_labels::build_reel()?;
    for segment in &reel.segments {
        segment
            .plan
            .write_or_print(Some(directory.join(format!("{}.json", segment.plan.id))))?;
    }
    fs::write(&output, serde_json::to_string_pretty(&reel)? + "\n")?;
    eprintln!("Wrote {}", output.display());
    Ok(())
}
