//! Row Marks: marks on a drawn code panel's rows follow their row's dim.
//!
//! A code panel drawn on the Stage is one Code Label per row
//! ([`StageElement::code`](super::StageElement::code)), and a walk through it
//! dims rows by their `<row>.opacity` channel. A **Row Mark** is anything drawn
//! *on* such a row: a wavy underline, a bracket down several rows, a diff
//! tint behind a row and its `+`, a bracket's caption. It should take its
//! row's dim, so it fades when the row dims and comes back when the row is
//! lit, the way an editor's Diagnostics take `lit.<line-id>`. Connectors and
//! arrows are not Row Marks; they stay as the choreography sets them.
//!
//! [`RowMarks`] is the record of which marks sit on which rows. Authoring
//! code records each mark as it draws it ([`RowMarks::attach`]), keeps the
//! records whose mark and rows are on the stage ([`RowMarks::among`]), and,
//! once the plan is finished, [`RowMarks::follow`] rewrites each mark's own
//! `opacity` channel:
//!
//! - every event the mark has keeps its time and curve, with its target
//!   scaled by the row's level then;
//! - every change of the row's level while the mark is up adds the same
//!   change to the mark (the row's event, curve and all, retargeted to the
//!   mark's level times the row's);
//! - a mark on several rows takes the brightest;
//! - a row lit again less than [`MarkTuning::short_nanos`] before the mark
//!   clears does not bring it back for a moment: the mark stays dim and
//!   leaves.
//!
//! With every row lit the scale is exactly 1, so a plan without a walk is
//! unchanged byte for byte. A mark that appears on a row below
//! [`MarkTuning::lit`] is refused: the narration would point at something it
//! cannot show.
//!
//! The record is a plain value the author owns and passes; nothing here keeps
//! hidden state.
//!
//! ```
//! use psychopomp::{plan::ScenePlan, stage::{StageElement, marks::RowMarks}};
//!
//! # fn main() -> anyhow::Result<()> {
//! let elements = vec![
//!     StageElement::label("row-0", [0.0, 0.0, 0.0], 28.0, &[]),
//!     StageElement::label("wave", [0.0, 20.0, 0.0], 28.0, &[]),
//! ];
//! let mut marks = RowMarks::new();
//! marks.attach("wave", &["row-0"]);
//! let marks = marks.among(&elements);
//! // … declare the stage, dim `row-0`, show and hide `wave` …
//! let mut plan = ScenePlan::new("example", 1_000_000_000);
//! marks.follow(&mut plan, "stage")?;
//! # Ok(())
//! # }
//! ```
use anyhow::{Result, bail};

use super::StageElement;
use crate::plan::{ContinuousChannelPlan, ScalarPlan, ScenePlan, TrackEventPlan};

/// A row below this level when its mark appears means the mark was drawn on
/// a dimmed row.
pub const LIT: f32 = 0.99;

/// A row lit again less than this (nanoseconds) before its mark clears does
/// not bring the mark back.
pub const SHORT: u64 = 800_000_000;

/// How [`RowMarks::follow`] judges a row's level. The default is [`LIT`] and
/// [`SHORT`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkTuning {
    /// The lowest row level a mark may appear on.
    pub lit: f32,
    /// A row relit within this many nanoseconds of its mark clearing leaves
    /// the mark dim.
    pub short_nanos: u64,
}

impl Default for MarkTuning {
    fn default() -> Self {
        Self {
            lit: LIT,
            short_nanos: SHORT,
        }
    }
}

/// Which marks sit on which code rows, by stage element id. See the
/// [module docs](self).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowMarks {
    /// Each mark and its rows, in recorded order; a later record of the same
    /// mark replaces an earlier one.
    marks: Vec<(String, Vec<String>)>,
}

impl RowMarks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `mark` sits on `rows` (stage element ids). Recording a
    /// mark again replaces its rows.
    pub fn attach<S: AsRef<str>>(&mut self, mark: &str, rows: &[S]) {
        let rows = rows.iter().map(|r| r.as_ref().to_owned()).collect();
        self.marks.push((mark.to_owned(), rows));
    }

    /// Record that `mark` (a bracket's caption, say) sits on the rows of the
    /// already recorded `like`.
    pub fn attach_like(&mut self, mark: &str, like: &str) -> Result<()> {
        let Some(rows) = self.rows(like).map(<[String]>::to_vec) else {
            bail!("row mark '{mark}' follows '{like}', which has no rows recorded");
        };
        self.attach(mark, &rows);
        Ok(())
    }

    /// The rows `mark` was last recorded on.
    pub fn rows(&self, mark: &str) -> Option<&[String]> {
        self.marks
            .iter()
            .rev()
            .find(|(m, _)| m == mark)
            .map(|(_, rows)| rows.as_slice())
    }

    /// Each recorded mark and its rows, one entry per mark (its latest
    /// record), in the order of those records.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.marks
            .iter()
            .enumerate()
            .filter(|(n, (mark, _))| !self.marks[n + 1..].iter().any(|(m, _)| m == mark))
            .map(|(_, (mark, rows))| (mark.as_str(), rows.as_slice()))
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The records whose mark is among `elements`, each keeping its latest
    /// record and only the rows among `elements`; a mark left with no rows
    /// is dropped. Helpers that draw marks for several stages can record
    /// freely and let each stage keep its own.
    pub fn among(self, elements: &[StageElement]) -> Self {
        let has = |id: &str| elements.iter().any(|e| e.id() == id);
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for (mark, rows) in self.marks.into_iter().rev() {
            if out.iter().any(|(m, _)| *m == mark) || !has(&mark) {
                continue;
            }
            let rows: Vec<String> = rows.into_iter().filter(|r| has(r)).collect();
            if !rows.is_empty() {
                out.push((mark, rows));
            }
        }
        out.reverse();
        Self { marks: out }
    }

    /// Scale every mark's `opacity` on stage actor `actor` by its rows' level
    /// (see the [module docs](self)), with the default [`MarkTuning`].
    pub fn follow(&self, plan: &mut ScenePlan, actor: &str) -> Result<()> {
        self.follow_tuned(plan, actor, MarkTuning::default())
    }

    /// [`Self::follow`] with explicit `tuning`. Fails when a mark appears on
    /// a row below `tuning.lit`. A mark or row without an opacity channel,
    /// or a mark whose opacity is not literal values, is left alone.
    pub fn follow_tuned(
        &self,
        plan: &mut ScenePlan,
        actor: &str,
        tuning: MarkTuning,
    ) -> Result<()> {
        for (mark, rows) in self.iter() {
            follow_one(plan, actor, mark, rows, tuning)?;
        }
        Ok(())
    }
}

fn follow_one(
    plan: &mut ScenePlan,
    actor: &str,
    mark: &str,
    rows: &[String],
    tuning: MarkTuning,
) -> Result<()> {
    let rows: Vec<ContinuousChannelPlan> = rows
        .iter()
        .filter_map(|r| {
            plan.continuous_channels
                .iter()
                .find(|c| c.id == opacity(actor, r))
                .cloned()
        })
        .collect();
    if rows.is_empty() {
        return Ok(());
    }
    let Some(channel) = plan
        .continuous_channels
        .iter_mut()
        .find(|c| c.id == opacity(actor, mark))
    else {
        return Ok(());
    };
    let Some(initial) = literal(&channel.initial) else {
        return Ok(());
    };
    if channel.events.iter().any(|e| literal(e.scalar()).is_none()) {
        return Ok(());
    }
    let level = |at: Option<u64>| {
        rows.iter()
            .map(|r| destination(r, at))
            .fold(0.0_f32, f32::max)
    };
    let own = channel.clone();
    let mut events = Vec::new();
    for event in &own.events {
        let at = event.at_nanos();
        let base = literal(event.scalar()).unwrap_or(0.0);
        let row = level(Some(at));
        let before = destination(&own, at.checked_sub(1));
        if base > 0.0 && before <= 0.0 && row < tuning.lit {
            bail!(
                "row mark '{mark}' appears at {:.2} s on a row at {row:.2}: light its row first",
                at as f64 / 1e9
            );
        }
        events.push(with_target(event, base * row));
    }
    let mut times: Vec<(u64, &TrackEventPlan)> = Vec::new();
    for r in &rows {
        for event in &r.events {
            let at = event.at_nanos();
            if own.events.iter().any(|e| e.at_nanos() == at) || times.iter().any(|(t, _)| *t == at)
            {
                continue;
            }
            times.push((at, event));
        }
    }
    for (at, event) in times {
        let base = destination(&own, Some(at));
        if base <= 0.0 {
            continue;
        }
        // A row lit again just before its mark clears: the mark stays dim
        // and leaves, rather than flashing back for a moment.
        let raised = level(Some(at)) > level(at.checked_sub(1));
        let leaves = own
            .events
            .iter()
            .find(|e| e.at_nanos() > at)
            .is_some_and(|e| {
                e.at_nanos() - at <= tuning.short_nanos && literal(e.scalar()).unwrap_or(1.0) <= 0.0
            });
        if level(Some(at)) == level(at.checked_sub(1)) || (raised && leaves) {
            continue;
        }
        events.push(with_target(event, base * level(Some(at))));
    }
    events.sort_by_key(TrackEventPlan::at_nanos);
    channel.initial = ScalarPlan::Literal(initial * level(None));
    channel.events = events;
    Ok(())
}

fn literal(scalar: &ScalarPlan) -> Option<f32> {
    match scalar {
        ScalarPlan::Literal(v) => Some(*v),
        ScalarPlan::Target(_) => None,
    }
}

/// Where a channel is headed at `at` (the latest event at or before it,
/// equal times in written order), or its initial value; the initial value
/// alone for `None`.
fn destination(channel: &ContinuousChannelPlan, at: Option<u64>) -> f32 {
    let latest = at.and_then(|at| {
        channel
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.at_nanos() <= at)
            .max_by_key(|(n, e)| (e.at_nanos(), *n))
            .map(|(_, e)| e.scalar())
    });
    literal(latest.unwrap_or(&channel.initial)).unwrap_or(1.0)
}

fn with_target(event: &TrackEventPlan, to: f32) -> TrackEventPlan {
    let mut event = event.clone();
    match &mut event {
        TrackEventPlan::Set { value, .. } => *value = ScalarPlan::Literal(to),
        TrackEventPlan::Spring { target, .. } | TrackEventPlan::Ease { target, .. } => {
            *target = ScalarPlan::Literal(to);
        }
    }
    event
}

fn opacity(actor: &str, id: &str) -> String {
    format!("{actor}.{id}.opacity")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::easing::Ease;

    const S: u64 = 1_000_000_000;

    fn ease(at: u64, to: f32) -> TrackEventPlan {
        TrackEventPlan::Ease {
            at_nanos: at,
            target: ScalarPlan::Literal(to),
            duration_nanos: 250_000_000,
            curve: Ease::GLIDE,
        }
    }

    fn channel(id: &str, events: Vec<TrackEventPlan>) -> ContinuousChannelPlan {
        ContinuousChannelPlan {
            id: format!("stage.{id}.opacity"),
            actor_id: "stage".into(),
            property: format!("{id}.opacity"),
            initial: ScalarPlan::Literal(0.0),
            events,
        }
    }

    fn targets(plan: &ScenePlan, id: &str) -> Vec<(u64, f32)> {
        let channel = plan
            .continuous_channels
            .iter()
            .find(|c| c.id == format!("stage.{id}.opacity"))
            .expect("a channel");
        channel
            .events
            .iter()
            .map(|e| (e.at_nanos() / S, literal(e.scalar()).expect("literal")))
            .collect()
    }

    fn plan(row: Vec<TrackEventPlan>, mark: Vec<TrackEventPlan>) -> ScenePlan {
        let mut plan = ScenePlan::new("t", 20 * S);
        plan.continuous_channels = vec![channel("row", row), channel("w", mark)];
        plan
    }

    fn marks() -> RowMarks {
        let mut marks = RowMarks::new();
        marks.attach("w", &["row"]);
        marks
    }

    fn element(id: &str) -> StageElement {
        StageElement::label(id, [0.0; 3], 28.0, &[])
    }

    #[test]
    fn a_mark_dims_with_its_row_and_comes_back_with_it() {
        let mut p = plan(
            vec![ease(0, 1.0), ease(4 * S, 0.13), ease(8 * S, 1.0)],
            vec![ease(2 * S, 1.0), ease(12 * S, 0.0)],
        );
        marks().follow(&mut p, "stage").unwrap();
        assert_eq!(targets(&p, "w"), [(2, 1.0), (4, 0.13), (8, 1.0), (12, 0.0)]);
    }

    #[test]
    fn a_mark_on_a_lit_row_is_unchanged() {
        let mut p = plan(
            vec![ease(0, 1.0), ease(4 * S, 1.0)],
            vec![ease(2 * S, 1.0), ease(3 * S, 0.0)],
        );
        let before = targets(&p, "w");
        marks().follow(&mut p, "stage").unwrap();
        assert_eq!(targets(&p, "w"), before);
    }

    #[test]
    fn a_plan_with_every_row_lit_is_unchanged_byte_for_byte() {
        let mut p = plan(
            vec![ease(0, 1.0), ease(3 * S, 1.0), ease(6 * S, 1.0)],
            vec![ease(S, 0.8), ease(5 * S, 1.0), ease(9 * S, 0.0)],
        );
        p.continuous_channels
            .push(channel("row2", vec![ease(0, 1.0)]));
        let mut marks = marks();
        marks.attach("w", &["row", "row2"]);
        let before = serde_json::to_string(&p).unwrap();
        marks.follow(&mut p, "stage").unwrap();
        assert_eq!(serde_json::to_string(&p).unwrap(), before);
    }

    #[test]
    fn a_row_lit_just_before_its_mark_clears_does_not_bring_it_back() {
        let mut p = plan(
            vec![ease(0, 1.0), ease(4 * S, 0.13), ease(10 * S, 1.0)],
            vec![ease(2 * S, 1.0), ease(10 * S + S / 2, 0.0)],
        );
        marks().follow(&mut p, "stage").unwrap();
        assert_eq!(targets(&p, "w"), [(2, 1.0), (4, 0.13), (10, 0.0)]);
    }

    #[test]
    fn a_shorter_window_lets_the_relit_row_bring_its_mark_back() {
        let mut p = plan(
            vec![ease(0, 1.0), ease(4 * S, 0.13), ease(10 * S, 1.0)],
            vec![ease(2 * S, 1.0), ease(10 * S + S / 2, 0.0)],
        );
        let tuning = MarkTuning {
            short_nanos: S / 4,
            ..MarkTuning::default()
        };
        marks().follow_tuned(&mut p, "stage", tuning).unwrap();
        assert_eq!(
            targets(&p, "w"),
            [(2, 1.0), (4, 0.13), (10, 1.0), (10, 0.0)]
        );
    }

    #[test]
    fn a_mark_drawn_on_a_dimmed_row_is_refused() {
        let mut p = plan(
            vec![ease(0, 1.0), ease(4 * S, 0.13)],
            vec![ease(6 * S, 1.0)],
        );
        let error = marks().follow(&mut p, "stage").unwrap_err().to_string();
        assert!(error.contains("'w' appears at 6.00 s"), "{error}");
        assert!(error.contains("light its row first"), "{error}");
    }

    #[test]
    fn a_bracket_takes_its_brightest_row() {
        let mut p = plan(vec![ease(0, 1.0), ease(4 * S, 0.13)], vec![ease(S, 1.0)]);
        p.continuous_channels
            .push(channel("row2", vec![ease(0, 1.0), ease(5 * S, 0.13)]));
        let mut marks = RowMarks::new();
        marks.attach("w", &["row", "row2"]);
        marks.follow(&mut p, "stage").unwrap();
        assert_eq!(targets(&p, "w"), [(1, 1.0), (5, 0.13)]);
    }

    #[test]
    fn among_keeps_the_latest_record_of_marks_on_the_stage() {
        let mut marks = RowMarks::new();
        marks.attach("w", &["row-0"]);
        marks.attach("gone", &["row-0"]);
        marks.attach("b", &["row-0", "row-9"]);
        marks.attach("w", &["row-1"]);
        marks.attach("off", &["row-9"]);
        let elements = ["row-0", "row-1", "w", "b", "off"].map(element);
        let kept = marks.among(&elements);
        let kept: Vec<(&str, Vec<&str>)> = kept
            .iter()
            .map(|(m, rows)| (m, rows.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(kept, [("b", vec!["row-0"]), ("w", vec!["row-1"])]);
    }

    #[test]
    fn attach_like_copies_rows_and_names_a_missing_mark() {
        let mut marks = RowMarks::new();
        marks.attach("b", &["row-0", "row-1"]);
        marks.attach_like("b-caption", "b").unwrap();
        assert_eq!(
            marks.rows("b-caption"),
            Some(&["row-0".to_owned(), "row-1".to_owned()][..])
        );
        let error = marks.attach_like("x", "nope").unwrap_err().to_string();
        assert!(error.contains("'nope'"), "{error}");
    }
}
