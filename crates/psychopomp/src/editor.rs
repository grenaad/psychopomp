use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::code::{
    CodeDocument, CodeLine, CodeSnapshot, InlinePart, LineId, LogicalRange, RangeId, SemanticRange,
    StyledSpan,
};

mod compiled;
pub mod diff;
mod stability;
pub use compiled::{CompiledEditor, CompiledInlineReveal};
pub use stability::inspect_steps;

pub const EDITOR_RECIPE: &str = "editor";
pub const POINTER_RECIPE: &str = "pointer";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorRecipePlan {
    pub file_name: String,
    pub lines: Vec<EditorLinePlan>,
    pub initial_line_ids: Vec<String>,
    pub final_line_ids: Vec<String>,
    /// Optional multi-step line order. Empty preserves the legacy two-snapshot
    /// layout/content channels. Otherwise the last order must equal final_line_ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<EditorSnapshotPlan>,
    pub line_height: f32,
    pub entering_offset_x: f32,
    pub focus_line_id: String,
    pub focus_height: f32,
    /// A width-revealing edit inside one stable line. Optional: plain code
    /// transitions need no reveal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_reveal: Option<EditorInlineRevealPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_inline_reveals: Vec<EditorInlineRevealPlan>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSnapshotPlan {
    pub at_nanos: u64,
    pub line_ids: Vec<String>,
}

impl EditorRecipePlan {
    pub fn compile(&self) -> Result<CompiledEditor> {
        if !self.line_height.is_finite()
            || self.line_height <= 0.0
            || !self.entering_offset_x.is_finite()
            || !self.focus_height.is_finite()
            || self.focus_height <= 0.0
        {
            bail!("editor line height must be positive and placement values finite");
        }
        let document = CodeDocument::new(
            self.lines
                .iter()
                .map(EditorLinePlan::code_line)
                .collect::<Result<Vec<_>>>()?,
        )?;
        let mut ranges: std::collections::HashMap<&str, Vec<std::ops::Range<usize>>> =
            std::collections::HashMap::new();
        let mut compiled_reveals = Vec::new();
        for reveal in self
            .inline_reveal
            .iter()
            .chain(&self.additional_inline_reveals)
        {
            let line = document
                .line(&LineId::new(&reveal.line_id))
                .with_context(|| {
                    format!("inline reveal references unknown line '{}'", reveal.line_id)
                })?;
            let range_id = RangeId::new(&reveal.range_id);
            let range = line.semantic_span_range(&range_id)?;
            let previous = ranges.entry(&reveal.line_id).or_default();
            if previous
                .iter()
                .any(|old| old.start < range.end && range.start < old.end)
            {
                bail!("inline reveal ranges overlap on line '{}'", reveal.line_id);
            }
            previous.push(range.clone());
            let resolved = line
                .resolve_logical_range(line.semantic_range(&range_id).expect("validated range"))?;
            compiled_reveals.push(CompiledInlineReveal {
                plan: reveal.clone(),
                spans: range,
                parts: resolved.start_part..resolved.end_part + 1,
            });
        }
        for line in &self.lines {
            let reveals = compiled_reveals
                .iter()
                .filter(|reveal| reveal.plan.line_id == line.id)
                .collect::<Vec<_>>();
            for after in [false, true] {
                let columns: usize = line
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|&(part_index, _)| {
                        !reveals.iter().any(|reveal| {
                            reveal.parts.contains(&part_index) && reveal.plan.reversed == after
                        })
                    })
                    .flat_map(|(_, part)| &part.spans)
                    .map(|span| span.text.chars().count())
                    .sum();
                if columns > diff::MAX_COLUMNS + 2 {
                    bail!(
                        "editor '{}' line '{}' is {columns} columns (max {})",
                        self.file_name,
                        line.id,
                        diff::MAX_COLUMNS
                    );
                }
            }
        }
        if !self.snapshots.is_empty() {
            document
                .validate_snapshot(&CodeSnapshot::new(self.initial_line_ids.iter().cloned()))?;
        }
        for snapshot in &self.snapshots {
            document.validate_snapshot(&CodeSnapshot::new(snapshot.line_ids.iter().cloned()))?;
        }
        if self
            .snapshots
            .last()
            .is_some_and(|snapshot| snapshot.line_ids != self.final_line_ids)
        {
            bail!("last editor snapshot must match finalLineIds");
        }
        let final_ids = if self.snapshots.is_empty() {
            self.final_line_ids.clone()
        } else {
            self.lines.iter().map(|line| line.id.clone()).collect()
        };
        if !self.initial_line_ids.contains(&self.focus_line_id)
            && !final_ids.contains(&self.focus_line_id)
        {
            bail!("editor focus references a line outside its snapshots");
        }
        CompiledEditor::new(self, document, compiled_reveals)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorLinePlan {
    pub id: String,
    pub parts: Vec<EditorPartPlan>,
    #[serde(default)]
    pub semantic_ranges: Vec<EditorSemanticRangePlan>,
    /// Diff decoration: a tinted row and a gutter sign whose presence is the
    /// `mark.<line-id>` channel (default 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<LineMarkPlan>,
}

/// The authored channel that dims one editor line's code ink: 1 (the rest
/// value, when the channel is absent) is lit, lower is dimmed. The renderer
/// multiplies it into the line's text and Line Mark, never into its blur or the
/// Diagnostics, Cursors, and Hover Cards on it.
pub fn lit_channel(line_id: &str) -> String {
    format!("lit.{line_id}")
}

/// How a line relates to the change being explained.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineMarkPlan {
    Added,
    Removed,
}

impl EditorLinePlan {
    fn code_line(&self) -> Result<CodeLine> {
        CodeLine::with_parts(
            self.id.clone(),
            self.parts.iter().map(EditorPartPlan::inline_part).collect(),
            self.semantic_ranges
                .iter()
                .map(EditorSemanticRangePlan::semantic_range),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorPartPlan {
    pub id: String,
    pub spans: Vec<StyledSpan>,
}

impl EditorPartPlan {
    fn inline_part(&self) -> InlinePart {
        InlinePart::new(self.id.clone(), self.spans.clone())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSemanticRangePlan {
    pub id: String,
    pub first_part_id: String,
    pub last_part_id: String,
}

impl EditorSemanticRangePlan {
    fn semantic_range(&self) -> SemanticRange {
        SemanticRange::new(
            self.id.clone(),
            LogicalRange::spanning(self.first_part_id.clone(), self.last_part_id.clone()),
        )
    }
}

/// A line with one part per span, named `span-0`, `span-1`, …, for code
/// whose inline parts need no identity of their own.
pub fn line(id: &str, spans: Vec<StyledSpan>) -> EditorLinePlan {
    let parts = spans
        .into_iter()
        .enumerate()
        .map(|(index, span)| part(&format!("span-{index}"), vec![span]))
        .collect();
    semantic_line(id, parts, Vec::new())
}

/// A line of named parts with Semantic Targets ranging over them.
pub fn semantic_line(
    id: &str,
    parts: Vec<EditorPartPlan>,
    semantic_ranges: Vec<EditorSemanticRangePlan>,
) -> EditorLinePlan {
    EditorLinePlan {
        id: id.to_owned(),
        parts,
        semantic_ranges,
        mark: None,
    }
}

/// A named inline part: its identity survives between snapshots.
pub fn part(id: &str, spans: Vec<StyledSpan>) -> EditorPartPlan {
    EditorPartPlan {
        id: id.to_owned(),
        spans,
    }
}

/// A logical range from part `first` through part `last`.
pub fn semantic_range(id: &str, first: &str, last: &str) -> EditorSemanticRangePlan {
    EditorSemanticRangePlan {
        id: id.to_owned(),
        first_part_id: first.to_owned(),
        last_part_id: last.to_owned(),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorInlineRevealPlan {
    pub line_id: String,
    pub range_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
}

impl EditorInlineRevealPlan {
    pub fn channel(&self) -> &str {
        self.channel.as_deref().unwrap_or("inline-reveal")
    }

    pub fn progress(&self, value: f32) -> f32 {
        if self.reversed { 1.0 - value } else { value }
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTargetSelector {
    pub line_id: String,
    pub range_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerRecipePlan {
    pub editor_id: String,
}

#[cfg(test)]
mod tests {
    use crate::code::{StyledSpan, SyntaxStyle};

    use super::{
        EditorInlineRevealPlan, EditorLinePlan, EditorPartPlan, EditorRecipePlan,
        EditorSemanticRangePlan, LineMarkPlan,
    };

    #[test]
    fn editor_recipe_compiles_stable_code_transition() {
        let recipe = EditorRecipePlan {
            file_name: "demo.ts".to_owned(),
            lines: vec![
                EditorLinePlan {
                    id: "stable".to_owned(),
                    parts: vec![EditorPartPlan {
                        id: "content".to_owned(),
                        spans: vec![StyledSpan::new("const stable = true", SyntaxStyle::Plain)],
                    }],
                    semantic_ranges: vec![EditorSemanticRangePlan {
                        id: "content".to_owned(),
                        first_part_id: "content".to_owned(),
                        last_part_id: "content".to_owned(),
                    }],
                    mark: None,
                },
                EditorLinePlan {
                    id: "entering".to_owned(),
                    parts: vec![EditorPartPlan {
                        id: "content".to_owned(),
                        spans: vec![StyledSpan::new("const entering = true", SyntaxStyle::Plain)],
                    }],
                    semantic_ranges: vec![],
                    mark: None,
                },
            ],
            initial_line_ids: vec!["stable".to_owned()],
            final_line_ids: vec!["stable".to_owned(), "entering".to_owned()],
            snapshots: Vec::new(),
            line_height: 44.0,
            entering_offset_x: 96.0,
            focus_line_id: "stable".to_owned(),
            focus_height: 44.0,
            inline_reveal: Some(EditorInlineRevealPlan {
                line_id: "stable".to_owned(),
                range_id: "content".to_owned(),
                channel: None,
                reversed: false,
            }),
            additional_inline_reveals: Vec::new(),
        };

        let editor = recipe.compile().unwrap();
        assert_eq!(editor.sample_lines(|_, _| 1.0).len(), 2);

        let mut marked = recipe.clone();
        marked.lines[1].mark = Some(LineMarkPlan::Added);
        let compiled = marked.compile().unwrap();
        let marks = compiled.marks().collect::<Vec<_>>();
        assert_eq!(
            marks,
            vec![("entering", LineMarkPlan::Added, "mark.entering".to_owned())]
        );
        let json = serde_json::to_value(&marked.lines[1]).unwrap();
        assert_eq!(json["mark"], "added");
        assert!(
            serde_json::to_value(&marked.lines[0])
                .unwrap()
                .get("mark")
                .is_none()
        );
    }

    #[test]
    fn inline_reveals_can_share_one_channel_in_opposing_directions() {
        let entering = EditorInlineRevealPlan {
            line_id: "line".to_owned(),
            range_id: "new".to_owned(),
            channel: Some("api-change".to_owned()),
            reversed: false,
        };
        let exiting = EditorInlineRevealPlan {
            line_id: "line".to_owned(),
            range_id: "old".to_owned(),
            channel: Some("api-change".to_owned()),
            reversed: true,
        };

        assert_eq!(entering.channel(), "api-change");
        assert_eq!(entering.progress(0.25), 0.25);
        assert_eq!(exiting.progress(0.25), 0.75);
    }

    #[test]
    fn keyed_snapshots_retain_intermediate_lines_and_unchanged_targets() {
        let line = |id: &str| EditorLinePlan {
            id: id.into(),
            parts: vec![EditorPartPlan {
                id: "body".into(),
                spans: vec![StyledSpan::new(id, SyntaxStyle::Plain)],
            }],
            semantic_ranges: vec![EditorSemanticRangePlan {
                id: "body".into(),
                first_part_id: "body".into(),
                last_part_id: "body".into(),
            }],
            mark: None,
        };
        let mut recipe = EditorRecipePlan {
            file_name: "test.ts".into(),
            lines: vec![line("a"), line("insert"), line("b")],
            initial_line_ids: vec!["a".into(), "b".into()],
            final_line_ids: vec!["a".into(), "b".into()],
            snapshots: vec![
                super::EditorSnapshotPlan {
                    at_nanos: 1_000_000_000,
                    line_ids: vec!["a".into(), "insert".into(), "b".into()],
                },
                super::EditorSnapshotPlan {
                    at_nanos: 2_000_000_000,
                    line_ids: vec!["b".into(), "insert".into()],
                },
                super::EditorSnapshotPlan {
                    at_nanos: 3_000_000_000,
                    line_ids: vec!["a".into(), "b".into()],
                },
            ],
            line_height: 44.,
            entering_offset_x: 99.,
            focus_line_id: "a".into(),
            focus_height: 44.,
            inline_reveal: Some(EditorInlineRevealPlan {
                line_id: "a".into(),
                range_id: "body".into(),
                channel: None,
                reversed: false,
            }),
            additional_inline_reveals: Vec::new(),
        };
        let channels = |recipe: &EditorRecipePlan| {
            recipe
                .compile()
                .and_then(|editor| editor.snapshot_channels("editor", 4_000_000_000))
        };
        assert_eq!(recipe.compile().unwrap().sample_lines(|_, d| d).len(), 3);
        let canonical = channels(&recipe).unwrap();
        assert_eq!(canonical.len(), 6);
        let insert_y = canonical
            .iter()
            .find(|channel| channel.property == "line.insert.y")
            .unwrap();
        assert!(insert_y.events.is_empty()); // entering/exiting must not restart an unchanged row
        assert!(matches!(
            insert_y.initial,
            crate::plan::ScalarPlan::Literal(44.)
        ));
        let b = canonical
            .iter()
            .find(|channel| channel.property == "line.b.y")
            .unwrap();
        assert_eq!(b.events.len(), 3);
        recipe.snapshots.insert(
            1,
            super::EditorSnapshotPlan {
                at_nanos: 2_000_000_000,
                line_ids: vec!["insert".into(), "a".into(), "b".into()],
            },
        );
        assert_eq!(
            serde_json::to_value(channels(&recipe).unwrap()).unwrap(),
            serde_json::to_value(canonical).unwrap()
        );
        recipe.snapshots.remove(1);
        recipe.snapshots[1].at_nanos = 5_000_000_000;
        assert!(channels(&recipe).is_err());
        recipe.snapshots[1].at_nanos = 2_000_000_000;
        recipe.snapshots[1].line_ids.push("missing".into());
        assert!(channels(&recipe).is_err());
    }

    #[test]
    fn line_builders_name_parts_and_ranges() {
        use super::{line, part, semantic_line, semantic_range};
        use crate::code::SyntaxStyle::{Keyword, Plain};
        let spans = line(
            "plain",
            vec![
                StyledSpan::new("let", Keyword),
                StyledSpan::new(" x", Plain),
            ],
        );
        let ids = spans
            .parts
            .iter()
            .map(|part| part.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["span-0", "span-1"]);
        let named = semantic_line(
            "named",
            vec![
                part("keyword", vec![StyledSpan::new("let", Keyword)]),
                part("name", vec![StyledSpan::new(" x", Plain)]),
            ],
            vec![semantic_range("binding", "keyword", "name")],
        );
        assert_eq!(named.semantic_ranges[0].last_part_id, "name");
        assert!(named.code_line().is_ok());
        assert!(named.mark.is_none());
    }
}
