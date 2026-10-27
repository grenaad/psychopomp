use std::{collections::HashMap, ops::Range};

use anyhow::{Context, Result, bail};
use psychopomp::{
    callout::CalloutSide,
    code::RangeId,
    editor::{CompiledEditor, EditorRecipePlan, EditorTargetSelector},
    math::{
        Vec2,
        shapes::{Box2, Shape},
        vec2,
    },
    motion::MotionState,
    plan::{ActorPlan, ScalarPlan, ScenePlan, TrackEventPlan},
};

use super::{TargetGeometry, generated, ide};
use crate::render::{
    EditorAnnotations, EditorFrame, EditorPanel, HeadlessRenderer, InlayFrame, InlineRangeMetrics,
    InlineRevealFrame, LineMarkFrame, PointerFrame, TokenHighlight,
};

pub(super) struct PreparedEditor {
    actor_id: String,
    editor: CompiledEditor,
    targets: HashMap<String, MeasuredTarget>,
    annotations: Vec<ide::PreparedAnnotation>,
}

pub(super) struct EditorSelection {
    line_id: String,
    spans: Range<usize>,
    endpoints: [[f32; 2]; 2],
}

struct MeasuredTarget {
    line_id: String,
    keyed_lines: bool,
    segments: Vec<(InlineRangeMetrics, Option<(String, bool)>)>,
    before: [f32; 2],
    after: [f32; 2],
}

pub(super) struct TargetMotion {
    pub x: MotionState,
    pub width: MotionState,
    pub line_y: MotionState,
}

impl PreparedEditor {
    pub(super) fn new(actor: &ActorPlan) -> Result<Self> {
        let recipe = serde_json::from_value::<EditorRecipePlan>(actor.data.clone())
            .with_context(|| format!("parse editor recipe for actor '{}'", actor.id))?;
        let editor = recipe.compile()?;
        for reveal in editor.inline_reveals() {
            editor
                .line_endpoints(&reveal.plan.line_id)
                .with_context(|| {
                    format!(
                        "editor actor '{}' inline reveal references unknown line '{}'",
                        actor.id, reveal.plan.line_id
                    )
                })?;
        }
        Ok(Self {
            actor_id: actor.id.clone(),
            editor,
            targets: HashMap::new(),
            annotations: Vec::new(),
        })
    }

    /// The actor IDs of attached Cursors.
    pub(super) fn cursor_ids(&self) -> impl Iterator<Item = &str> {
        self.annotations
            .iter()
            .filter(|annotation| annotation.is_cursor())
            .map(ide::PreparedAnnotation::id)
    }

    /// Attach a Diagnostic, Hover Card, or Cursor drawn on this editor.
    pub(super) fn attach(
        &mut self,
        annotation: ide::PreparedAnnotation,
        targets: &[psychopomp::plan::SemanticTargetPlan],
    ) -> Result<()> {
        annotation.validate_targets(&self.actor_id, targets)?;
        self.annotations.push(annotation);
        Ok(())
    }

    pub(super) fn actor_id(&self) -> &str {
        &self.actor_id
    }

    pub(super) fn file_name(&self) -> &str {
        self.editor.file_name()
    }

    pub(super) fn select(
        &self,
        id: &str,
        selector: &EditorTargetSelector,
    ) -> Result<EditorSelection> {
        let endpoints = self
            .editor
            .line_endpoints(&selector.line_id)
            .with_context(|| {
                format!(
                    "editor actor '{}' target '{}' references unknown line '{}'",
                    self.actor_id, id, selector.line_id
                )
            })?;
        let spans = self
            .editor
            .line(&selector.line_id)
            .expect("participating catalog line")
            .semantic_span_range(&RangeId::new(&selector.range_id))?;
        Ok(EditorSelection {
            line_id: selector.line_id.clone(),
            spans,
            endpoints,
        })
    }

    pub(super) fn resolve_selection(
        &mut self,
        renderer: &mut HeadlessRenderer,
        id: &str,
        selection: &EditorSelection,
    ) -> Result<TargetGeometry> {
        let [before, after] = selection.endpoints;
        let line = self
            .editor
            .line(&selection.line_id)
            .expect("participating catalog line");
        let selected = selection.spans.clone();
        let reveals = self
            .editor
            .inline_reveals()
            .iter()
            .filter(|reveal| reveal.plan.line_id == selection.line_id)
            .collect::<Vec<_>>();
        let frames = reveals
            .iter()
            .map(|reveal| InlineRevealFrame {
                line_id: &reveal.plan.line_id,
                start_span: reveal.spans.start,
                end_span: reveal.spans.end,
                progress: 1.0,
            })
            .collect::<Vec<_>>();
        let segments = renderer
            .measure_inline_target(line, &frames, selected)?
            .into_iter()
            .map(|metrics| {
                let driver = reveals
                    .iter()
                    .find(|reveal| reveal.spans == metrics.spans)
                    .map(|reveal| (reveal.plan.channel().to_owned(), reveal.plan.reversed));
                (metrics, driver)
            })
            .collect();
        let measured = MeasuredTarget {
            line_id: selection.line_id.clone(),
            keyed_lines: self.editor.is_keyed(),
            segments,
            before,
            after,
        };
        let motion = measured.sample(|_, default| MotionState::at(default), true);
        self.targets.insert(id.to_owned(), measured);
        Ok(TargetGeometry {
            x: motion.x.position,
            width: motion.width.position,
            line_y: motion.line_y.position,
        })
    }

    pub(super) fn target_motion(
        &self,
        id: &str,
        sample: impl Fn(&str, &str) -> Option<MotionState>,
    ) -> Option<TargetMotion> {
        self.targets.get(id).map(|target| {
            target.sample(
                |property, default| {
                    sample(&self.actor_id, property).unwrap_or(MotionState::at(default))
                },
                false,
            )
        })
    }

    /// Where `edge` of a measured code range is on the canvas at this sample:
    /// the range's sampled glyph box, carried through the panel's projection.
    pub(super) fn anchor(
        &self,
        target: &str,
        edge: CalloutSide,
        canvas: [u32; 2],
        sample: impl Fn(&str, &str) -> Option<MotionState>,
    ) -> Option<Vec2> {
        let motion = self.target_motion(target, &sample)?;
        let value = |property: &str, default: f32| {
            sample(&self.actor_id, property).map_or(default, |state| state.position)
        };
        let center = motion.line_y.position + self.editor.line_height() * 0.5;
        let half = self.editor.line_height() * 0.5;
        let range = Box2 {
            min: vec2(motion.x.position, center - half),
            max: vec2(motion.x.position + motion.width.position, center + half),
        };
        let point = edge.on(Shape::Box(range));
        let panel = EditorPanel {
            offset: [value("panel-x", 0.0), value("panel-y", 0.0)],
            scale: value("panel-scale", 1.0),
            rotation: value("panel-rotation", 0.0),
            tilt: [value("panel-tilt-x", 0.0), value("panel-tilt-y", 0.0)],
        };
        Some(Vec2::from(crate::render::editor_canvas_point(
            canvas,
            panel,
            point.to_array(),
        )))
    }

    pub(super) fn target_scale(&self, id: &str) -> Option<f32> {
        self.targets.get(id).map(|target| {
            let width = target
                .segments
                .iter()
                .map(|(metrics, _)| metrics.advance)
                .sum::<f32>();
            // Conservative local text/line extent, in pixels. Companion weights
            // must not inherit pixel tolerances as dimensionless tolerances.
            (width
                + target.before[0].abs()
                + target.after[0].abs()
                + self.editor.lines().len() as f32 * self.editor.line_height())
            .max(1.0)
        })
    }

    /// Add the snapshot schedule's generated line channels to `plan`. Channels
    /// that drive code geometry must be literal: a semantic attachment would
    /// make the geometry depend on itself.
    pub(super) fn compile_channels(&self, plan: &mut ScenePlan) -> Result<()> {
        generated::extend(
            plan,
            self.editor
                .snapshot_channels(&self.actor_id, plan.duration_nanos)?,
            generated::Owner::Editor,
        )?;
        let drivers = self.geometry_channels();
        for channel in plan.continuous_channels.iter().filter(|channel| {
            channel.actor_id == self.actor_id && drivers.contains(&channel.property)
        }) {
            if std::iter::once(&channel.initial)
                .chain(channel.events.iter().map(TrackEventPlan::scalar))
                .any(|value| matches!(value, ScalarPlan::Target(_)))
            {
                bail!(
                    "editor geometry channel '{}' must use literal values, not a cyclic semantic attachment",
                    channel.id
                );
            }
        }
        Ok(())
    }

    fn geometry_channels(&self) -> Vec<String> {
        std::iter::once("layout".into())
            .chain(std::iter::once("content".into()))
            .chain(
                self.editor
                    .inline_reveals()
                    .iter()
                    .map(|reveal| reveal.plan.channel().to_owned()),
            )
            .chain(
                self.editor
                    .lines()
                    .map(|line| format!("line.{}.y", line.id.as_str())),
            )
            .collect()
    }

    pub(super) fn render(
        &self,
        renderer: &mut HeadlessRenderer,
        time: f64,
        pointer_actor_id: Option<&str>,
        sample: impl Fn(&str, &str, f64) -> Option<MotionState>,
    ) -> Result<Vec<u8>> {
        let value = |actor: &str, property: &str, default: f32| {
            sample(actor, property, time).map_or(default, |state| state.position)
        };
        let mut lines = self.editor.sample_lines(|p, d| value(&self.actor_id, p, d));
        let presence = dim_lines(&mut lines, |id| {
            value(&self.actor_id, &psychopomp::editor::lit_channel(id), 1.0)
        });
        let focus_line_y = lines
            .iter()
            .find(|line| line.line.id.as_str() == self.editor.focus_line_id())
            .with_context(|| {
                format!(
                    "editor actor '{}' focus references unknown line '{}'",
                    self.actor_id,
                    self.editor.focus_line_id()
                )
            })?
            .y;
        let token_highlight = TokenHighlight {
            x: value(&self.actor_id, "highlight-x", 0.0),
            y: value(&self.actor_id, "highlight-y", 0.0),
            width: value(&self.actor_id, "highlight-width", 0.0),
            opacity: value(&self.actor_id, "highlight-opacity", 0.0).clamp(0.0, 1.0),
        };
        let pointer = if let Some(actor) = pointer_actor_id {
            pointer_frame(actor, time, &sample)?
        } else {
            PointerFrame {
                x: 0.0,
                y: 0.0,
                opacity: 0.0,
                rotation: 0.0,
                scale: 1.0,
                blur: 0.0,
            }
        };
        let inline_reveals = self
            .editor
            .inline_reveals()
            .iter()
            .map(|reveal| {
                let progress = value(&self.actor_id, reveal.plan.channel(), 0.0);
                InlineRevealFrame {
                    line_id: &reveal.plan.line_id,
                    start_span: reveal.spans.start,
                    end_span: reveal.spans.end,
                    progress: reveal.plan.progress(progress),
                }
            })
            .collect::<Vec<_>>();
        let inlays = self
            .editor
            .inline_reveals()
            .iter()
            .filter(|reveal| {
                reveal
                    .plan
                    .range_id
                    .starts_with(psychopomp::ide::INLAY_PART_PREFIX)
            })
            .map(|reveal| InlayFrame {
                line_id: &reveal.plan.line_id,
                start_span: reveal.spans.start,
                end_span: reveal.spans.end,
            })
            .collect::<Vec<_>>();
        let mut sampled = ide::Sampled::default();
        for annotation in &self.annotations {
            annotation.sample(
                &mut sampled,
                |property, default| value(annotation.id(), property, default),
                |target| {
                    let motion = self
                        .target_motion(target, |actor, property| sample(actor, property, time))?;
                    let line = &self.targets.get(target)?.line_id;
                    // Annotations follow the line's presence, not its dimming.
                    let line_opacity = lines
                        .iter()
                        .position(|placed| placed.line.id.as_str() == line)
                        .map_or(0.0, |row| presence[row]);
                    Some(ide::RangeSample {
                        x: motion.x.position,
                        width: motion.width.position,
                        line_y: motion.line_y.position,
                        line_opacity,
                    })
                },
            );
        }
        let line_marks = self
            .editor
            .marks()
            .map(|(line_id, mark, channel)| LineMarkFrame {
                line_id,
                mark,
                presence: value(&self.actor_id, &channel, 1.0).clamp(0.0, 1.0),
                row_height: self.editor.line_height(),
            })
            .collect::<Vec<_>>();
        renderer.render_editor(&EditorFrame {
            panel_offset_x: value(&self.actor_id, "panel-x", 0.0),
            panel_offset_y: value(&self.actor_id, "panel-y", 0.0),
            panel_opacity: value(&self.actor_id, "panel-opacity", 1.0).clamp(0.0, 1.0),
            line_marks: &line_marks,
            panel_rotation: value(&self.actor_id, "panel-rotation", 0.0),
            panel_tilt_x: value(&self.actor_id, "panel-tilt-x", 0.0),
            panel_tilt_y: value(&self.actor_id, "panel-tilt-y", 0.0),
            panel_scale: value(&self.actor_id, "panel-scale", 1.0),
            panel_near_blur: value(&self.actor_id, "panel-near-blur", 0.0),
            focus_intensity: value(&self.actor_id, "focus", 0.0).clamp(0.0, 1.0),
            focus_line_y,
            focus_height: self.editor.focus_height(),
            token_highlight,
            pointer,
            inline_reveals: &inline_reveals,
            lines: &lines,
            annotations: EditorAnnotations {
                inlays: &inlays,
                diagnostics: &sampled.diagnostics,
                selections: &sampled.selections,
                carets: &sampled.carets,
                hovers: &sampled.hovers,
            },
        })
    }
}

/// Dim each placed line's ink by its sampled `lit` level (clamped to 0..1),
/// leaving its blur, and return each line's undimmed presence for the
/// annotations drawn on it. A line without a `lit` channel samples 1 and stays
/// bit-for-bit as placed.
fn dim_lines(
    lines: &mut [psychopomp::code::PlacedLine<'_>],
    lit: impl Fn(&str) -> f32,
) -> Vec<f32> {
    lines
        .iter_mut()
        .map(|placed| {
            let presence = placed.opacity;
            let level = lit(placed.line.id.as_str());
            if level != 1.0 {
                placed.opacity *= level.clamp(0.0, 1.0);
            }
            presence
        })
        .collect()
}

fn add(a: MotionState, b: MotionState) -> MotionState {
    MotionState {
        position: a.position + b.position,
        velocity: a.velocity + b.velocity,
    }
}
fn scale(a: MotionState, factor: f32) -> MotionState {
    MotionState {
        position: a.position * factor,
        velocity: a.velocity * factor,
    }
}

impl MeasuredTarget {
    fn sample(&self, sample: impl Fn(&str, f32) -> MotionState, expanded: bool) -> TargetMotion {
        let mut cursor = MotionState::at(0.0);
        let mut left = None;
        let mut right = MotionState::at(0.0);
        for (metrics, driver) in &self.segments {
            let progress = if let Some((channel, reversed)) = driver.as_ref().filter(|_| !expanded)
            {
                let state = sample(channel, 0.0);
                let state = if *reversed {
                    add(MotionState::at(1.0), scale(state, -1.0))
                } else {
                    state
                };
                MotionState {
                    position: state.position.clamp(0.0, 1.0),
                    velocity: if (0.0..=1.0).contains(&state.position) {
                        state.velocity
                    } else {
                        0.0
                    },
                }
            } else {
                MotionState::at(1.0)
            };
            let width = scale(progress, metrics.advance);
            if let Some([start, end]) = metrics.selection {
                let clipped = |edge| {
                    if width.position < edge {
                        width
                    } else {
                        MotionState::at(edge)
                    }
                };
                left.get_or_insert(add(cursor, clipped(start)));
                right = add(cursor, clipped(end));
            }
            cursor = add(cursor, width);
        }
        let left = left.expect("nonempty validated semantic selection");
        let layout = if expanded {
            MotionState::at(1.0)
        } else {
            sample("layout", 0.0)
        };
        let content = if expanded {
            MotionState::at(1.0)
        } else {
            sample("content", 0.0)
        };
        let x = add(
            MotionState::at(self.before[0]),
            scale(content, self.after[0] - self.before[0]),
        );
        let mut y = add(
            MotionState::at(self.before[1]),
            scale(layout, self.after[1] - self.before[1]),
        );
        if !expanded && self.keyed_lines {
            y = sample(&format!("line.{}.y", self.line_id), y.position);
        }
        TargetMotion {
            x: add(x, left),
            width: add(right, scale(left, -1.0)),
            line_y: y,
        }
    }
}

fn pointer_frame(
    actor_id: &str,
    time: f64,
    sample: impl Fn(&str, &str, f64) -> Option<MotionState>,
) -> Result<PointerFrame> {
    let state = |property: &str, at: f64| {
        sample(actor_id, property, at)
            .with_context(|| format!("pointer actor '{actor_id}' has no '{property}' channel"))
    };
    let x = state("x", time)?;
    let y = state("y", time)?;
    let derivative_step = 1.0 / 240.0;
    let previous_time = (time - derivative_step).max(0.0);
    let previous_x = state("x", previous_time)?;
    let previous_y = state("y", previous_time)?;
    let acceleration_x = (x.velocity - previous_x.velocity) / derivative_step as f32;
    let acceleration_y = (y.velocity - previous_y.velocity) / derivative_step as f32;
    Ok(PointerFrame {
        x: x.position,
        y: y.position,
        opacity: state("opacity", time)?.position.clamp(0.0, 1.0),
        rotation: (x.velocity * 0.00012 + y.velocity * 0.00004
            - acceleration_x * 0.000012
            - acceleration_y * 0.000004)
            .clamp(-0.30, 0.30),
        scale: state("scale", time)?.position,
        blur: state("blur", time)?.position.max(0.0),
    })
}

#[cfg(test)]
mod tests {
    use psychopomp::motion::MotionState;

    use super::MeasuredTarget;
    use crate::render::InlineRangeMetrics;

    #[test]
    fn lit_levels_dim_ink_but_not_blur_or_annotation_presence() {
        use psychopomp::code::{CodeLine, PlacedLine, StyledSpan, SyntaxStyle};
        let lines = ["a", "b", "c"]
            .map(|id| CodeLine::new(id, vec![StyledSpan::new("x", SyntaxStyle::Plain)]));
        let placed = |opacity| {
            lines
                .iter()
                .map(|line| PlacedLine {
                    line,
                    x: 0.,
                    y: 0.,
                    opacity,
                    blur: (1. - opacity) * 4.,
                })
                .collect::<Vec<_>>()
        };
        let mut dimmed = placed(0.8);
        let presence = super::dim_lines(&mut dimmed, |id| match id {
            "a" => 1.0,
            "b" => 0.35,
            _ => -2.0,
        });
        assert_eq!(presence, [0.8, 0.8, 0.8]);
        let opacity = dimmed.iter().map(|line| line.opacity).collect::<Vec<_>>();
        assert_eq!(opacity, [0.8, 0.8 * 0.35, 0.0]);
        let reference = placed(0.8);
        for (line, reference) in dimmed.iter().zip(&reference) {
            assert_eq!(line.blur.to_bits(), reference.blur.to_bits());
        }
        // An absent channel samples 1: every line stays bit-for-bit as placed.
        let mut lit = placed(0.3);
        super::dim_lines(&mut lit, |_| 1.0);
        assert!(
            lit.iter()
                .all(|line| line.opacity.to_bits() == 0.3f32.to_bits())
        );
    }

    /// `const program` | an Inlay Hint | `= run()`, targeting `run`.
    fn target() -> MeasuredTarget {
        let metrics = |spans: std::ops::Range<usize>, advance, selection| InlineRangeMetrics {
            spans,
            advance,
            selection,
        };
        MeasuredTarget {
            line_id: "program".into(),
            keyed_lines: true,
            segments: vec![
                (metrics(0..1, 218.0, None), None),
                (
                    metrics(1..2, 604.5, None),
                    Some(("inlay.type".into(), false)),
                ),
                (metrics(2..3, 134.0, Some([33.5, 117.5])), None),
            ],
            before: [0.0, 132.0],
            after: [0.0, 132.0],
        }
    }

    #[test]
    fn ranges_follow_line_motion_and_an_inlay_opening_before_them() {
        let target = target();
        let sample = |inlay: MotionState, y: MotionState| {
            target.sample(
                |property, default| match property {
                    "inlay.type" => inlay,
                    "line.program.y" => y,
                    "layout" | "content" => MotionState::at(1.0),
                    _ => MotionState::at(default),
                },
                false,
            )
        };
        let closed = sample(MotionState::at(0.0), MotionState::at(132.0));
        assert_eq!(closed.x.position, 218.0 + 33.5);
        assert_eq!(closed.width.position, 84.0);
        assert_eq!(closed.line_y.position, 132.0);
        // Mid-opening, the range sits after exactly the opened room, keeps its
        // width, and carries the room's velocity; the line's row carries its own.
        let opening = sample(
            MotionState {
                position: 0.4,
                velocity: 2.0,
            },
            MotionState {
                position: 150.0,
                velocity: 90.0,
            },
        );
        assert!((opening.x.position - (218.0 + 604.5 * 0.4 + 33.5)).abs() < 1e-3);
        assert!((opening.x.velocity - 604.5 * 2.0).abs() < 1e-3);
        assert_eq!(opening.width.position, 84.0);
        assert_eq!(opening.width.velocity, 0.0);
        assert_eq!(opening.line_y.position, 150.0);
        assert_eq!(opening.line_y.velocity, 90.0);
        let open = sample(MotionState::at(1.0), MotionState::at(176.0));
        assert_eq!(open.x.position, 218.0 + 604.5 + 33.5);
        assert_eq!(open.line_y.position, 176.0);
        // The expanded preparation baseline is the fully open layout.
        let expanded = target.sample(|_, default| MotionState::at(default), true);
        assert_eq!(expanded.x.position, open.x.position);
    }
}
