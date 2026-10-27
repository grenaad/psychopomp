//! Syntax-colored code labels showroom: the same code in the IDE editor and
//! drawn as Stage labels, in C# and in Python. Each language plays twice, as
//! an editor segment and then as a Stage segment whose rows
//! (`StageElement::code`) sit where the editor's rows did, so a dip between
//! the two shows the colors matching. Under the drawn code, one row is drawn
//! once as a single plain run and once as its syntax runs on the column grid,
//! with a tick every ten columns: the glyphs line up.
use anyhow::Result;
use psychopomp::{
    author::{PlanBuilder, seconds},
    caption::CaptionAlign,
    editor::diff::{Diff, keep},
    highlight::Syntax,
    plan::{ReelPlan, ScenePlan},
    stage::{MONO_ADVANCE, StageActor, StageElement, StagePlan, StagePost},
    tone::Tone,
};

/// The editor's code type size and row pitch, and where its first row sits
/// on screen (the left edge of column 0, the center of row 0).
const FONT: f32 = 28.0;
const PITCH: f32 = 44.0;
const X: f32 = 278.0;
const Y: f32 = 309.0;
/// Each segment's length.
const HOLD: f64 = 3.0;

pub const CSHARP: &[&str] = &[
    "// Pay one order, in cents.",
    "public sealed record Receipt(string Id, decimal Total);",
    "",
    "public class Checkout(IPayment payment)",
    "{",
    "    public Receipt Pay(Order order) =>",
    "        payment.Charge((long)(order.Total * 100m), $\"order {order.Id}\");",
    "}",
];

pub const PYTHON: &[&str] = &[
    "# Pay one order, in cents.",
    "@dataclass(frozen=True)",
    "class Receipt:",
    "    id: str",
    "    total: Decimal",
    "",
    "def pay(self, order: Order) -> Receipt:",
    "    return self.payment.charge(int(order.total * 100), f\"order {order.id}\")",
];

pub fn build_reel() -> Result<ReelPlan> {
    ReelPlan::dipped(
        "code-labels",
        vec![
            editor("cs-editor", "Checkout.cs", CSHARP)?,
            labels("cs-labels", Syntax::CSharp, CSHARP, 6)?,
            editor("py-editor", "checkout.py", PYTHON)?,
            labels("py-labels", Syntax::Python, PYTHON, 7)?,
        ],
        seconds(0.4),
    )
}

/// The code in the IDE editor; the file name picks the highlighter.
fn editor(id: &str, file_name: &'static str, lines: &'static [&'static str]) -> Result<ScenePlan> {
    let mut scene = PlanBuilder::new(id, seconds(HOLD));
    Diff {
        file_name,
        lines: lines.iter().map(|line| keep(line)).collect(),
    }
    .declare(&mut scene, &[], 0, false)?;
    Ok(scene.finish()?)
}

/// The same code drawn as Stage labels at the editor's rows, plus row `proof`
/// drawn again below, as one plain run and as its syntax runs.
fn labels(id: &str, syntax: Syntax, lines: &[&str], proof: usize) -> Result<ScenePlan> {
    let mut scene = PlanBuilder::new(id, seconds(HOLD));
    let mut elements = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(|(row, line)| {
            StageElement::code(
                &format!("row-{row}"),
                [X, Y + row as f32 * PITCH, 0.0],
                FONT,
                syntax,
                line,
            )
        })
        .collect::<Vec<_>>();
    let proof = lines[proof];
    let below = Y + (lines.len() as f32 + 1.0) * PITCH;
    elements.push(
        StageElement::label("plain", [X, below, 0.0], FONT, &[(proof, Tone::Plain)])
            .align(CaptionAlign::Left)
            .columns(),
    );
    elements.push(StageElement::code(
        "runs",
        [X, below + PITCH, 0.0],
        FONT,
        syntax,
        proof,
    ));
    let cell = MONO_ADVANCE * FONT;
    for column in (0..=proof.chars().count()).step_by(10) {
        elements.push(StageElement::label(
            &format!("tick-{column}"),
            [X + column as f32 * cell, below + PITCH * 1.75, 0.0],
            16.0,
            &[("|", Tone::Muted)],
        ));
    }
    let stage = StagePlan {
        post: StagePost::FLAT,
        elements,
    };
    StageActor::declare(&mut scene, "stage", &stage)?;
    Ok(scene.finish()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reel_builds_and_the_code_fits_the_editor() {
        let reel = build_reel().unwrap();
        assert_eq!(reel.segments.len(), 4);
        for line in CSHARP.iter().chain(PYTHON) {
            assert!(line.chars().count() <= psychopomp::editor::diff::MAX_COLUMNS);
        }
    }
}
