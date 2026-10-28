//! Syntax-colored code labels showroom: the same code in the IDE editor and
//! drawn as Stage labels, in C# and in Python. Each language plays twice, as
//! an editor segment and then as a Stage segment whose rows
//! (`StageElement::code`) sit where the editor's rows did, so a dip between
//! the two shows the colors matching. Under the drawn code, one row is drawn
//! once as a single plain run and once as its syntax runs on the column grid,
//! with a tick every ten columns: the glyphs line up. A wavy underline drawn
//! under that row in the code is a Row Mark (`stage::marks`): when the row
//! dims, the wave dims with it, and it comes back when the row is lit.
use anyhow::Result;
use psychopomp::{
    author::{PlanBuilder, seconds},
    caption::CaptionAlign,
    editor::diff::{Diff, keep},
    highlight::Syntax,
    plan::{ReelPlan, ScenePlan},
    stage::{
        Curve, MONO_ADVANCE, StageActor, StageElement, StagePlan, StagePost, Waypoint,
        marks::RowMarks,
    },
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
/// In a drawn segment: the wave under row `proof` arrives, the row dims to
/// `DIM`, and it is lit again (seconds).
const WAVE_IN: f64 = 0.3;
const ROW_DIM: f64 = 1.0;
const ROW_LIT: f64 = 2.0;
const DIM: f32 = 0.2;

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
/// drawn again below, as one plain run and as its syntax runs. A wave under
/// row `proof` dims and comes back with it.
fn labels(id: &str, syntax: Syntax, lines: &[&str], proof: usize) -> Result<ScenePlan> {
    let mut scene = PlanBuilder::new(id, seconds(HOLD));
    let mut marks = RowMarks::new();
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
    let row = format!("row-{proof}");
    elements.push(wave("wave", lines[proof], Y + proof as f32 * PITCH + 17.0));
    marks.attach("wave", &[&row]);
    let proof_row = proof;
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
    let marks = marks.among(&stage.elements);
    let mut s = StageActor::declare(&mut scene, "stage", &stage)?;
    s.fade_in(&mut scene, "wave", seconds(WAVE_IN), 1.0, 0.3);
    let opacity = format!("row-{proof_row}.opacity");
    s.to(&mut scene, &opacity, seconds(ROW_DIM), DIM, 0.4);
    s.to(&mut scene, &opacity, seconds(ROW_LIT), 1.0, 0.4);
    let mut plan = scene.finish()?;
    marks.follow(&mut plan, s.id())?;
    Ok(plan)
}

/// A wavy underline under `line`'s text (leading spaces skipped) at `y`.
fn wave(id: &str, line: &str, y: f32) -> StageElement {
    let cell = MONO_ADVANCE * FONT;
    let x0 = X + (line.len() - line.trim_start().len()) as f32 * cell;
    let x1 = X + line.trim_end().len() as f32 * cell;
    let n = ((x1 - x0) / 9.0).round().clamp(4.0, 30.0) as usize;
    StageElement::Path {
        id: id.into(),
        through: (0..=n)
            .map(|i| {
                let dy = if i % 2 == 0 { -2.6 } else { 2.6 };
                Waypoint::Point([x0 + (x1 - x0) * i as f32 / n as f32, y + dy, 0.0])
            })
            .collect(),
        curve: Curve::Smooth,
        corner: 0.0,
        bend: 0.0,
        tone: Tone::Request,
        width: 2.2,
        dash: None,
        arrow: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reel_builds_and_the_code_fits_the_editor() {
        let reel = build_reel().unwrap();
        assert_eq!(reel.segments.len(), 4);
        // The wave fades in on a lit row, dims with it, and comes back.
        let plan = &reel.segments[1].plan;
        let wave = plan
            .continuous_channels
            .iter()
            .find(|c| c.id == "stage.wave.opacity")
            .expect("the wave's opacity");
        let targets: Vec<f32> = wave
            .events
            .iter()
            .map(|e| match e.scalar() {
                psychopomp::plan::ScalarPlan::Literal(v) => *v,
                psychopomp::plan::ScalarPlan::Target(_) => f32::NAN,
            })
            .collect();
        assert_eq!(targets, [1.0, DIM, 1.0]);
        for line in CSHARP.iter().chain(PYTHON) {
            assert!(line.chars().count() <= psychopomp::editor::diff::MAX_COLUMNS);
        }
    }
}
