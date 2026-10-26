//! Stepped Diffs: a unified-diff view of one change, told in steps. Each line
//! says in which step it appears and, optionally, in which step it is removed.
//! Lines keep identity across steps, so unchanged code only moves; removed lines
//! turn red before they go, and added lines arrive green.
use anyhow::{Context, Result};

use super::{
    EDITOR_RECIPE, EditorInlineRevealPlan, EditorLinePlan, EditorPartPlan, EditorRecipePlan,
    EditorSnapshotPlan, EditorTargetSelector, LineMarkPlan,
};
use crate::{
    author::{ActorHandle, PlanBuilder, SemanticTargetHandle},
    code::{StyledSpan, SyntaxStyle},
    highlight::Syntax,
    ide::{InlayHint, ghost},
};

/// The widest line that fits the editor card at 28 px CommitMono.
pub const MAX_COLUMNS: usize = 76;
/// Rows visible in the editor card.
pub const MAX_ROWS: usize = 14;
/// How long a pure insertion or removal holds blank rows, so moving code never
/// crosses entering or leaving code.
const ROOM: u64 = 300_000_000;

pub struct Line {
    pub text: &'static str,
    pub from: usize,
    pub until: Option<usize>,
    /// Semantic ranges: an ID and the text it selects (its first occurrence).
    ranges: Vec<(&'static str, &'static str)>,
    /// Inlay Hints: an ID, the range it follows, and its ghost text.
    inlays: Vec<(&'static str, &'static str, &'static str)>,
    mark: Option<LineMarkPlan>,
}

/// Present from the start and never removed.
pub const fn keep(text: &'static str) -> Line {
    Line {
        text,
        from: 0,
        until: None,
        ranges: Vec::new(),
        inlays: Vec::new(),
        mark: None,
    }
}

/// Added in `step`.
pub const fn add(step: usize, text: &'static str) -> Line {
    Line {
        text,
        from: step,
        until: None,
        ranges: Vec::new(),
        inlays: Vec::new(),
        mark: None,
    }
}

/// Present from the start and removed in `step`.
pub const fn remove(step: usize, text: &'static str) -> Line {
    Line {
        text,
        from: 0,
        until: Some(step),
        ranges: Vec::new(),
        inlays: Vec::new(),
        mark: None,
    }
}

impl Line {
    /// Name the first occurrence of `text` in this line as semantic range
    /// `id`, so callouts, diagnostics, hovers, and cursors can pin to it
    /// through [`DiffEditor::target`]. The line's highlighting is unchanged.
    pub fn range(mut self, id: &'static str, text: &'static str) -> Self {
        self.ranges.push((id, text));
        self
    }

    /// An Inlay Hint of ghost `text` after range `after`, revealed by
    /// [`DiffEditor::inlay`].
    pub fn inlay(mut self, id: &'static str, after: &'static str, text: &'static str) -> Self {
        self.inlays.push((id, after, text));
        self
    }

    /// Show `mark` on this line, whatever its steps imply (for example, a
    /// kept line that the change makes reachable).
    pub fn marked(mut self, mark: LineMarkPlan) -> Self {
        self.mark = Some(mark);
        self
    }

    fn plan(
        &self,
        id: String,
        syntax: Syntax,
    ) -> Result<(EditorLinePlan, Vec<EditorInlineRevealPlan>)> {
        let mark = self.mark.or(if self.until.is_some() {
            Some(LineMarkPlan::Removed)
        } else if self.from > 0 {
            Some(LineMarkPlan::Added)
        } else {
            None
        });
        let mut line = editor_line(id, self.text, mark, syntax);
        if self.ranges.is_empty() && self.inlays.is_empty() {
            return Ok((line, Vec::new()));
        }
        let mut bounds = Vec::new();
        for &(range, needle) in &self.ranges {
            let start = self
                .text
                .find(needle)
                .filter(|_| !needle.is_empty())
                .with_context(|| format!("line '{}' does not contain '{needle}'", self.text))?;
            bounds.push((start, start + needle.len(), range));
        }
        bounds.sort_by_key(|&(start, ..)| start);
        if let Some(pair) = bounds.windows(2).find(|pair| pair[1].0 < pair[0].1) {
            anyhow::bail!("ranges '{}' and '{}' overlap", pair[0].2, pair[1].2);
        }
        let spans = line.parts.remove(0).spans;
        let mut cuts = bounds
            .iter()
            .flat_map(|&(start, end, _)| [start, end])
            .collect::<Vec<_>>();
        cuts.dedup();
        let mut gap = 0;
        let mut cursor = 0;
        for (piece, (start, end)) in split_spans(&spans, &cuts).into_iter().zip(
            std::iter::once(0)
                .chain(cuts.iter().copied())
                .zip(cuts.iter().copied().chain(std::iter::once(self.text.len()))),
        ) {
            debug_assert_eq!(cursor, start);
            cursor = end;
            if piece.is_empty() {
                continue;
            }
            let id = match bounds
                .iter()
                .find(|&&(from, to, _)| (from, to) == (start, end))
            {
                Some(&(.., range)) => range.to_owned(),
                None => {
                    gap += 1;
                    if gap == 1 {
                        "code".to_owned()
                    } else {
                        format!("code-{gap}")
                    }
                }
            };
            if bounds.iter().any(|&(.., range)| range == id) {
                line.semantic_ranges.push(super::EditorSemanticRangePlan {
                    id: id.clone(),
                    first_part_id: id.clone(),
                    last_part_id: id.clone(),
                });
            }
            line.parts.push(EditorPartPlan { id, spans: piece });
        }
        let reveals = self
            .inlays
            .iter()
            .map(|&(inlay, after, text)| {
                crate::ide::insert_inlay(&mut line, inlay, after, ghost(text))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((line, reveals))
    }
}

/// Split highlighted `spans` at the byte offsets `cuts` (ascending), keeping
/// each span's style, into `cuts.len() + 1` pieces.
fn split_spans(spans: &[StyledSpan], cuts: &[usize]) -> Vec<Vec<StyledSpan>> {
    let mut pieces = vec![Vec::new()];
    let mut cuts = cuts.iter().copied().peekable();
    let mut offset = 0;
    for span in spans {
        let mut text = span.text.as_str();
        while let Some(&cut) = cuts.peek() {
            if cut > offset + text.len() {
                break;
            }
            let (head, tail) = text.split_at(cut - offset);
            if !head.is_empty() {
                pieces
                    .last_mut()
                    .expect("one piece")
                    .push(StyledSpan::new(head, span.style));
            }
            pieces.push(Vec::new());
            offset = cut;
            text = tail;
            cuts.next();
        }
        if !text.is_empty() {
            pieces
                .last_mut()
                .expect("one piece")
                .push(StyledSpan::new(text, span.style));
        }
        offset += text.len();
    }
    pieces
}

/// The declared editor of a Stepped Diff: pin targets to its lines' ranges and
/// reveal its Inlay Hints.
pub struct DiffEditor {
    actor: ActorHandle,
}

impl DiffEditor {
    pub fn actor(&self) -> &ActorHandle {
        &self.actor
    }

    /// The stable ID of `lines[index]`.
    pub fn line_id(&self, index: usize) -> String {
        id(index)
    }

    /// Declare Semantic Target `id` on range `range` of `lines[index]`.
    pub fn target(
        &self,
        scene: &mut PlanBuilder,
        id: impl Into<String>,
        index: usize,
        range: &str,
    ) -> Result<SemanticTargetHandle> {
        Ok(scene.semantic_target(
            id,
            &self.actor,
            EditorTargetSelector {
                line_id: self.line_id(index),
                range_id: range.to_owned(),
            },
        )?)
    }

    /// The reveal handle of Inlay Hint `id`.
    pub fn inlay(&self, scene: &mut PlanBuilder, id: &str) -> InlayHint {
        InlayHint::on(scene, &self.actor, id)
    }
}

pub struct Diff {
    pub file_name: &'static str,
    pub lines: Vec<Line>,
}

impl Diff {
    fn steps(&self) -> usize {
        self.lines
            .iter()
            .flat_map(|line| [line.from, line.until.unwrap_or(0)])
            .max()
            .unwrap_or(0)
    }

    fn rows(&self, step: usize) -> Vec<String> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.from <= step && line.until.is_none_or(|until| step < until))
            .map(|(index, _)| id(index))
            .collect()
    }

    /// Declare the `editor` actor. `step_times[k]` is when step `k + 1` happens; removed
    /// lines turn red `warning` nanoseconds before their step. The returned
    /// handle pins Semantic Targets to the lines' ranges. The file name picks
    /// the highlighter ([`Syntax::for_file`]: `.py` is Python, anything else
    /// TypeScript).
    pub fn declare(
        &self,
        scene: &mut PlanBuilder,
        step_times: &[u64],
        warning: u64,
        entrance: bool,
    ) -> Result<DiffEditor> {
        let syntax = Syntax::for_file(self.file_name);
        self.declare_as(syntax, scene, step_times, warning, entrance)
    }

    /// [`Diff::declare`], highlighted as `syntax` whatever the file name
    /// (for a tab label without an extension, such as `used everywhere`).
    pub fn declare_as(
        &self,
        syntax: Syntax,
        scene: &mut PlanBuilder,
        step_times: &[u64],
        warning: u64,
        entrance: bool,
    ) -> Result<DiffEditor> {
        let steps = self.steps();
        anyhow::ensure!(
            step_times.len() == steps,
            "{} has {steps} steps but {} step times",
            self.file_name,
            step_times.len()
        );
        for (index, line) in self.lines.iter().enumerate() {
            let columns = line.text.chars().count()
                + line
                    .inlays
                    .iter()
                    .map(|&(.., text)| text.chars().count())
                    .sum::<usize>();
            anyhow::ensure!(
                columns <= MAX_COLUMNS,
                "{} line {index} is {columns} columns (max {MAX_COLUMNS}): {}",
                self.file_name,
                line.text
            );
        }
        for step in 0..=steps {
            let rows = self.rows(step).len();
            anyhow::ensure!(
                rows <= MAX_ROWS,
                "{} step {step} has {rows} rows",
                self.file_name
            );
        }
        let snapshots = step_times
            .iter()
            .enumerate()
            .flat_map(|(step, &at)| step_snapshots(&self.rows(step), &self.rows(step + 1), at))
            .collect::<Vec<_>>();
        let mut reveals = Vec::new();
        let mut lines = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            let (line, inlays) = line
                .plan(id(index), syntax)
                .with_context(|| format!("{} line {index}", self.file_name))?;
            lines.push(line);
            reveals.extend(inlays);
        }
        lines.extend(gap_lines(&snapshots));
        let initial = self.rows(0);
        let final_ids = self.rows(steps);
        let recipe = EditorRecipePlan {
            file_name: self.file_name.to_owned(),
            focus_line_id: initial.first().cloned().unwrap_or_else(|| id(0)),
            lines,
            initial_line_ids: initial,
            final_line_ids: final_ids,
            snapshots,
            line_height: 44.0,
            entering_offset_x: 0.0,
            focus_height: 44.0,
            inline_reveal: None,
            additional_inline_reveals: reveals,
        };
        let editor = scene.actor("editor", EDITOR_RECIPE, &recipe)?;
        // Removed lines start unmarked and turn red just before they leave.
        for (index, line) in self.lines.iter().enumerate() {
            if let Some(until) = line.until {
                let mark = scene.continuous(&editor, format!("mark.{}", id(index)), 0.0);
                let at = step_times[until - 1];
                scene.spring(&mark, at.saturating_sub(warning), 1.0, 0.35, 0.0);
            }
        }
        if entrance {
            // The card rises into place.
            let y = scene.continuous(&editor, "panel-y", 70.0);
            let opacity = scene.continuous(&editor, "panel-opacity", 0.0);
            scene.spring(&y, 0, 0.0, 0.7, 0.0);
            scene.spring(&opacity, 0, 1.0, 0.5, 0.0);
        }
        Ok(DiffEditor { actor: editor })
    }
}

/// The snapshots that change an editor's rows from `previous` to `next` at
/// `at`. A pure insertion or removal that moves retained lines first holds
/// blank `gap-N` rows for a moment, so moving code never crosses entering or
/// leaving code; declare [`gap_lines`] alongside the recipe's own lines.
pub fn step_snapshots(previous: &[String], next: &[String], at: u64) -> Vec<EditorSnapshotPlan> {
    let settled = |at_nanos| EditorSnapshotPlan {
        at_nanos,
        line_ids: next.to_vec(),
    };
    match make_room(previous, next) {
        Some(room) => vec![
            EditorSnapshotPlan {
                at_nanos: at,
                line_ids: room,
            },
            settled(at + ROOM),
        ],
        None => vec![settled(at)],
    }
}

/// Blank lines for every `gap-N` row that `snapshots` hold.
pub fn gap_lines(snapshots: &[EditorSnapshotPlan]) -> impl Iterator<Item = EditorLinePlan> {
    let gaps = snapshots
        .iter()
        .map(|snapshot| {
            snapshot
                .line_ids
                .iter()
                .filter(|id| id.starts_with("gap-"))
                .count()
        })
        .max()
        .unwrap_or(0);
    (0..gaps).map(|gap| editor_line(format!("gap-{gap}"), "", None, Syntax::TypeScript))
}

/// A pure insertion first opens blank rows where the new lines go; a pure
/// removal first leaves blank rows where the old lines were. Mixed steps and
/// steps where no retained line moves need no room.
fn make_room(previous: &[String], next: &[String]) -> Option<Vec<String>> {
    let entering = next.iter().filter(|id| !previous.contains(id)).count();
    let leaving = previous.iter().filter(|id| !next.contains(id)).count();
    let (base, other) = match (entering, leaving) {
        (0, 0) => return None,
        (_, 0) => (next, previous),
        (0, _) => (previous, next),
        _ => return None,
    };
    let row = |ids: &[String], id: &String| ids.iter().position(|candidate| candidate == id);
    if !next
        .iter()
        .any(|id| row(previous, id).is_some_and(|from| Some(from) != row(next, id)))
    {
        return None;
    }
    let mut gap = 0;
    Some(
        base.iter()
            .map(|id| {
                if other.contains(id) {
                    id.clone()
                } else {
                    gap += 1;
                    format!("gap-{}", gap - 1)
                }
            })
            .collect(),
    )
}

fn editor_line(
    id: String,
    text: &str,
    mark: Option<LineMarkPlan>,
    syntax: Syntax,
) -> EditorLinePlan {
    let spans = if text.trim().is_empty() {
        vec![StyledSpan::new(" ", SyntaxStyle::Plain)]
    } else {
        syntax.spans(text)
    };
    EditorLinePlan {
        id,
        parts: vec![EditorPartPlan {
            id: "code".to_owned(),
            spans,
        }],
        semantic_ranges: Vec::new(),
        mark,
    }
}

fn id(index: usize) -> String {
    format!("line-{index}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight;

    #[test]
    fn a_pure_insertion_opens_room_before_the_new_lines_arrive() {
        let diff = Diff {
            file_name: "a.ts",
            lines: vec![keep("a"), add(1, "b"), keep("c")],
        };
        let mut scene = PlanBuilder::new("diff", 2_000_000_000);
        diff.declare(&mut scene, &[1_000_000_000], 0, false)
            .unwrap();
        let plan = scene.finish().unwrap();
        let recipe: EditorRecipePlan = serde_json::from_value(plan.actors[0].data.clone()).unwrap();
        let snapshots = recipe
            .snapshots
            .iter()
            .map(|snapshot| (snapshot.at_nanos, snapshot.line_ids.join(" ")))
            .collect::<Vec<_>>();
        assert_eq!(
            snapshots,
            vec![
                (1_000_000_000, "line-0 gap-0 line-2".to_owned()),
                (1_000_000_000 + ROOM, "line-0 line-1 line-2".to_owned()),
            ]
        );
        assert_eq!(recipe.lines[1].mark, Some(LineMarkPlan::Added));
    }

    #[test]
    fn only_steps_that_move_retained_lines_hold_room() {
        let rows = |ids: &str| ids.split(' ').map(str::to_owned).collect::<Vec<_>>();
        let appended = step_snapshots(&rows("a b"), &rows("a b c"), 5);
        assert_eq!(appended.len(), 1);
        assert_eq!(gap_lines(&appended).count(), 0);
        let inserted = step_snapshots(&rows("a b"), &rows("x y a b"), 5);
        assert_eq!(inserted[0].line_ids, rows("gap-0 gap-1 a b"));
        assert_eq!(inserted[1].at_nanos, 5 + ROOM);
        assert_eq!(gap_lines(&inserted).count(), 2);
    }

    #[test]
    fn ranges_split_a_highlighted_line_without_changing_its_spans() {
        let text = "  yield* signal(info.pid, \"SIGKILL\")";
        let diff = Diff {
            file_name: "a.ts",
            lines: vec![
                keep("const a = 1"),
                keep(text)
                    .range("call", "signal(info.pid, \"SIGKILL\")")
                    .range("kw", "yield")
                    .marked(LineMarkPlan::Added),
                remove(1, "return").range("ret", "return"),
            ],
        };
        let mut scene = PlanBuilder::new("diff", 2_000_000_000);
        let editor = diff
            .declare(&mut scene, &[1_000_000_000], 0, false)
            .unwrap();
        editor.target(&mut scene, "sigkill", 1, "call").unwrap();
        let plan = scene.finish().unwrap();
        let recipe: EditorRecipePlan = serde_json::from_value(plan.actors[0].data.clone()).unwrap();
        recipe.compile().unwrap();
        let unchanged = serde_json::to_value(&recipe.lines[0]).unwrap();
        assert_eq!(
            unchanged["parts"][0]["id"], "code",
            "rangeless lines are untouched"
        );
        let line = &recipe.lines[1];
        assert_eq!(
            line.parts
                .iter()
                .map(|part| part.id.as_str())
                .collect::<Vec<_>>(),
            ["code", "kw", "code-2", "call"]
        );
        let flat = |spans: &[StyledSpan]| serde_json::to_value(spans).unwrap();
        let joined = line
            .parts
            .iter()
            .flat_map(|part| part.spans.iter().cloned())
            .collect::<Vec<_>>();
        assert_eq!(flat(&joined), flat(&highlight::typescript(text)));
        assert_eq!(line.mark, Some(LineMarkPlan::Added));
        assert_eq!(recipe.lines[2].mark, Some(LineMarkPlan::Removed));
        assert_eq!(plan.semantic_targets[0].selector["lineId"], "line-1");
        assert_eq!(plan.semantic_targets[0].selector["rangeId"], "call");

        let missing = Diff {
            file_name: "a.ts",
            lines: vec![keep("const a = 1").range("b", "b =")],
        };
        let mut scene = PlanBuilder::new("diff", 2_000_000_000);
        assert!(missing.declare(&mut scene, &[], 0, false).is_err());
    }

    #[test]
    fn diff_inlays_reveal_inside_their_line() {
        let diff = Diff {
            file_name: "a.ts",
            lines: vec![
                keep("const program = run()")
                    .range("name", "program")
                    .inlay("type", "name", ": Effect<number>"),
            ],
        };
        let mut scene = PlanBuilder::new("diff", 2_000_000_000);
        let editor = diff.declare(&mut scene, &[], 0, false).unwrap();
        editor
            .inlay(&mut scene, "type")
            .show(&mut scene, 500_000_000);
        let plan = scene.finish().unwrap();
        let recipe: EditorRecipePlan = serde_json::from_value(plan.actors[0].data.clone()).unwrap();
        assert_eq!(
            recipe.lines[0]
                .parts
                .iter()
                .map(|part| part.id.as_str())
                .collect::<Vec<_>>(),
            ["code", "name", "inlay:type", "code-2"]
        );
        assert_eq!(recipe.additional_inline_reveals[0].channel(), "inlay.type");
        assert_eq!(plan.continuous_channels[0].id, "editor.inlay.type");
        recipe.compile().unwrap();

        let overflowing = Diff {
            file_name: "a.ts",
            lines: vec![
                keep("static Live = Layer.effect(this, Effect.gen(function* () {")
                    .range("live", "Live")
                    .inlay("type", "live", ": Layer<Codebase, never, Scope>"),
            ],
        };
        let mut scene = PlanBuilder::new("diff", 2_000_000_000);
        assert!(overflowing.declare(&mut scene, &[], 0, false).is_err());
    }

    #[test]
    fn a_py_file_is_highlighted_as_python_and_others_as_before() {
        let text = "    return None  # nothing to pay";
        let first_line = |diff: &Diff, syntax: Option<Syntax>| {
            let mut scene = PlanBuilder::new("diff", 2_000_000_000);
            match syntax {
                Some(syntax) => diff.declare_as(syntax, &mut scene, &[], 0, false),
                None => diff.declare(&mut scene, &[], 0, false),
            }
            .unwrap();
            let plan = scene.finish().unwrap();
            let recipe: EditorRecipePlan =
                serde_json::from_value(plan.actors[0].data.clone()).unwrap();
            serde_json::to_value(&recipe.lines[0].parts[0].spans).unwrap()
        };
        let named = |file_name| Diff {
            file_name,
            lines: vec![keep(text)],
        };
        let json = |spans: Vec<StyledSpan>| serde_json::to_value(spans).unwrap();
        assert_eq!(
            first_line(&named("orders.py"), None),
            json(highlight::python(text))
        );
        assert_eq!(
            first_line(&named("Orders.cs"), None),
            json(highlight::typescript(text))
        );
        assert_eq!(
            first_line(&named("used everywhere"), None),
            json(highlight::typescript(text))
        );
        assert_eq!(
            first_line(&named("used everywhere"), Some(Syntax::Python)),
            json(highlight::python(text))
        );
    }

    #[test]
    fn a_step_count_mismatch_is_rejected() {
        let diff = Diff {
            file_name: "a.ts",
            lines: vec![keep("a"), remove(2, "b")],
        };
        let mut scene = PlanBuilder::new("diff", 2_000_000_000);
        assert!(diff.declare(&mut scene, &[1], 0, false).is_err());
    }
}
