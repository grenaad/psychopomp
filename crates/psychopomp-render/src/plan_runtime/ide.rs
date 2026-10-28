//! IDE annotations attached to the editor root: Diagnostics, Hover Cards, and
//! Cursors are decoded once with strict channel names, checked against the
//! editor's Semantic Targets, and placed at every sample from the same
//! measured target geometry callouts and pointers use. They are drawn inside
//! the editor surface, so the panel's projection carries them too.
use anyhow::{Result, bail};
use psychopomp::{
    ide::{
        CURSOR_RECIPE, CursorPlan, DIAGNOSTIC_RECIPE, DiagnosticPlan, HOVER_RECIPE, HoverPlan,
        caret_blink,
    },
    plan::{ActorPlan, ContinuousChannelPlan, SemanticTargetPlan},
};

use super::preflight::{decode, strict_channels};
use crate::render::{CaretFrame, DiagnosticFrame, HoverFrame, SelectionFrame};

pub(super) struct PreparedAnnotation {
    id: String,
    kind: Kind,
}

enum Kind {
    Diagnostic(DiagnosticPlan),
    Hover(HoverPlan),
    Cursor(CursorPlan),
}

/// A measured range at one sample, in editor code coordinates, with the
/// sampled opacity of its line and the line's `lit` level.
#[derive(Clone, Copy)]
pub(super) struct RangeSample {
    pub x: f32,
    pub width: f32,
    pub line_y: f32,
    pub line_opacity: f32,
    /// The row's dimming (1 = lit). Diagnostics fade with it; Cursors and
    /// Hover Cards mark the current focus and keep their strength.
    pub line_lit: f32,
}

/// Everything the editor's annotations draw at one sample.
#[derive(Default)]
pub(super) struct Sampled<'a> {
    pub diagnostics: Vec<DiagnosticFrame>,
    pub selections: Vec<SelectionFrame>,
    pub carets: Vec<CaretFrame>,
    pub hovers: Vec<HoverFrame<'a>>,
}

impl PreparedAnnotation {
    /// Decode an annotation actor, or `None` for another recipe.
    pub(super) fn parse(
        actor: &ActorPlan,
        channels: &[ContinuousChannelPlan],
    ) -> Result<Option<Self>> {
        let kind = match actor.recipe.as_str() {
            DIAGNOSTIC_RECIPE => {
                let plan = decode(actor, "diagnostic", DiagnosticPlan::validate)?;
                strict_channels(&actor.id, channels, "diagnostic", |p| plan.accepts(p))?;
                Kind::Diagnostic(plan)
            }
            HOVER_RECIPE => {
                let plan = decode(actor, "hover-card", HoverPlan::validate)?;
                strict_channels(&actor.id, channels, "hover-card", |p| plan.accepts(p))?;
                Kind::Hover(plan)
            }
            CURSOR_RECIPE => {
                let plan = decode(actor, "cursor", CursorPlan::validate)?;
                strict_channels(&actor.id, channels, "cursor", |p| plan.accepts(p))?;
                Kind::Cursor(plan)
            }
            _ => return Ok(None),
        };
        Ok(Some(Self {
            id: actor.id.clone(),
            kind,
        }))
    }

    pub(super) fn id(&self) -> &str {
        &self.id
    }

    pub(super) fn is_cursor(&self) -> bool {
        matches!(self.kind, Kind::Cursor(_))
    }

    fn targets(&self) -> Vec<&str> {
        match &self.kind {
            Kind::Diagnostic(plan) => vec![&plan.target],
            Kind::Hover(plan) => vec![&plan.target],
            Kind::Cursor(plan) => plan.anchors.iter().map(|a| a.target.as_str()).collect(),
        }
    }

    /// Every target must be a Semantic Target of `editor`.
    pub(super) fn validate_targets(
        &self,
        editor: &str,
        targets: &[SemanticTargetPlan],
    ) -> Result<()> {
        for target in self.targets() {
            if !targets
                .iter()
                .any(|candidate| candidate.id == target && candidate.actor_id == editor)
            {
                bail!(
                    "annotation '{}' references unknown editor semantic target '{target}'",
                    self.id
                );
            }
        }
        Ok(())
    }

    /// Add this annotation's frames at one sample. `value` reads its
    /// channels; `range` measures a target, `None` when it cannot be placed.
    pub(super) fn sample<'a>(
        &'a self,
        sampled: &mut Sampled<'a>,
        value: impl Fn(&str, f32) -> f32,
        range: impl Fn(&str) -> Option<RangeSample>,
    ) {
        match &self.kind {
            Kind::Diagnostic(plan) => {
                let Some(range) = range(&plan.target) else {
                    return;
                };
                let frame = DiagnosticFrame {
                    x: range.x,
                    width: range.width,
                    line_y: range.line_y,
                    severity: plan.severity,
                    gutter: plan.gutter,
                    draw: value("draw", 1.0).clamp(0.0, 1.0),
                    wave: value("wave", 1.0),
                    opacity: (value("opacity", 1.0) * range.line_opacity * range.line_lit)
                        .clamp(0.0, 1.0),
                };
                if frame.opacity > 0.001 && frame.draw > 0.001 {
                    sampled.diagnostics.push(frame);
                }
            }
            Kind::Hover(plan) => {
                let Some(range) = range(&plan.target) else {
                    return;
                };
                let presence = value("presence", 1.0);
                if presence * range.line_opacity > 0.001 {
                    sampled.hovers.push(HoverFrame {
                        plan,
                        x: range.x,
                        width: range.width,
                        line_y: range.line_y,
                        presence,
                        opacity: range.line_opacity,
                    });
                }
            }
            Kind::Cursor(plan) => {
                let opacity = value("opacity", 1.0).clamp(0.0, 1.0);
                if opacity <= 0.001 {
                    return;
                }
                let head = value("head", 1.0);
                let tail = value("tail", head);
                let mut caret = [0.0; 3];
                let mut total = 0.0;
                for (index, anchor) in plan.anchors.iter().enumerate() {
                    let weight = value(
                        &CursorPlan::weight_property(&anchor.id),
                        if index == 0 { 1.0 } else { 0.0 },
                    );
                    if weight.abs() < 1e-6 {
                        continue;
                    }
                    let Some(range) = range(&anchor.target) else {
                        continue;
                    };
                    caret[0] += (range.x + range.width * head) * weight;
                    caret[1] += range.line_y * weight;
                    caret[2] += range.line_opacity * weight;
                    total += weight;
                    let [from, to] = if head < tail {
                        [head, tail]
                    } else {
                        [tail, head]
                    };
                    let selected = (to - from) * range.width;
                    let alpha = opacity * weight.clamp(0.0, 1.0) * range.line_opacity;
                    if selected > 0.25 && alpha > 0.001 {
                        sampled.selections.push(SelectionFrame {
                            x: range.x + range.width * from,
                            width: selected,
                            line_y: range.line_y,
                            opacity: alpha,
                        });
                    }
                }
                if total.abs() < 1e-6 {
                    return;
                }
                let blink = value("blink", -1.0);
                sampled.carets.push(CaretFrame {
                    x: caret[0] / total,
                    line_y: caret[1] / total,
                    opacity: opacity * (caret[2] / total).clamp(0.0, 1.0) * caret_blink(blink),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use psychopomp::{
        author::PlanBuilder,
        callout::{CalloutActor, CalloutAnchorPlan, CalloutPlan, CalloutSide},
        caption::CaptionSpanPlan,
        editor::diff::{Diff, keep},
        ide::{CursorActor, CursorPlan, DiagnosticActor, DiagnosticPlan, HoverActor, HoverPlan},
        tone::Tone,
    };

    use super::super::validate_renderer_plan;

    fn plan(extra: impl FnOnce(&mut PlanBuilder)) -> psychopomp::plan::ScenePlan {
        let mut scene = PlanBuilder::new("ide-preflight", 3_000_000_000);
        let diff = Diff {
            file_name: "program.ts",
            lines: vec![
                keep("const program = Effect.succeed(1)")
                    .range("name", "program")
                    .inlay("type", "name", ": Effect<number>"),
                keep("Effect.runPromise(program)").range("arg", "program"),
            ],
        };
        let editor = diff.declare(&mut scene, &[], 0, false).unwrap();
        editor.target(&mut scene, "arg", 1, "arg").unwrap();
        editor.target(&mut scene, "name", 0, "name").unwrap();
        editor
            .inlay(&mut scene, "type")
            .show(&mut scene, 100_000_000);
        extra(&mut scene);
        scene.finish().unwrap()
    }

    #[test]
    fn annotations_attach_to_editor_targets_with_strict_channels() {
        validate_renderer_plan(&plan(|scene| {
            DiagnosticActor::declare(scene, "error", &DiagnosticPlan::error("arg"))
                .unwrap()
                .show(scene, 0);
            HoverActor::declare(
                scene,
                "why",
                &HoverPlan::new("arg").text(vec![CaptionSpanPlan::new("why", Tone::Plain)]),
            )
            .unwrap()
            .show(scene, 0);
            let caret = CursorActor::declare(
                scene,
                "caret",
                &CursorPlan::new("arg", "arg").anchor("name", "name"),
            )
            .unwrap();
            caret.show(scene, 0);
            caret.select(scene, "name", 500_000_000, 0.3).unwrap();
            // A callout pins to the same diff target.
            CalloutActor::declare(
                scene,
                "note",
                &CalloutPlan::new(
                    CalloutAnchorPlan::Editor {
                        id: "arg".into(),
                        target: "arg".into(),
                        edge: CalloutSide::Bottom,
                        side: None,
                    },
                    vec![CaptionSpanPlan::new("here", Tone::Plain)],
                ),
            )
            .unwrap();
        }))
        .unwrap();
        let unknown = plan(|scene| {
            DiagnosticActor::declare(scene, "error", &DiagnosticPlan::error("missing")).unwrap();
        });
        let error = validate_renderer_plan(&unknown).unwrap_err();
        assert!(format!("{error:#}").contains("unknown editor semantic target 'missing'"));
        let typo = plan(|scene| {
            let diagnostic =
                DiagnosticActor::declare(scene, "error", &DiagnosticPlan::error("arg")).unwrap();
            diagnostic.channel(scene, "drew", 0.0);
        });
        let error = validate_renderer_plan(&typo).unwrap_err();
        assert!(
            format!("{error:#}").contains("unknown property 'drew'"),
            "{error:#}"
        );
    }

    #[test]
    #[ignore = "requires a headless GPU; a steady caret's shutter samples merge though its clock runs"]
    fn steady_carets_share_visual_keys_and_fading_ones_do_not() {
        let plan = plan(|scene| {
            CursorActor::declare(scene, "caret", &CursorPlan::new("arg", "arg"))
                .unwrap()
                .show(scene, 0);
        });
        let mut renderer = pollster::block_on(super::super::new_renderer(&plan.id)).unwrap();
        let prepared =
            super::super::PreparedPlan::prepare(plan, std::path::Path::new("."), &mut renderer)
                .unwrap();
        let key = |time| prepared.visual_sample_key(time).unwrap();
        // After the inlay settles: on from 1.5 s, fading out from 1.91 s, off
        // from 2.0 s.
        assert_eq!(key(1.55), key(1.65), "steady on");
        assert_eq!(key(2.1), key(2.2), "steady off");
        assert_ne!(key(1.65), key(2.1));
        assert_ne!(key(1.95), key(1.96), "fading");
    }

    #[test]
    fn annotations_need_an_editor_root() {
        let mut scene = PlanBuilder::new("no-editor", 1_000_000_000);
        DiagnosticActor::declare(&mut scene, "error", &DiagnosticPlan::error("arg")).unwrap();
        let error = validate_renderer_plan(&scene.finish().unwrap()).unwrap_err();
        assert!(
            format!("{error:#}").contains("needs an editor root"),
            "{error:#}"
        );
    }
}

#[cfg(test)]
mod sampling {
    use psychopomp::ide::{CursorPlan, DiagnosticPlan, HoverPlan, caret_blink};

    use super::{Kind, PreparedAnnotation, RangeSample, Sampled};

    fn annotation(kind: Kind) -> PreparedAnnotation {
        PreparedAnnotation {
            id: "a".into(),
            kind,
        }
    }

    fn range(x: f32, line_y: f32) -> RangeSample {
        RangeSample {
            x,
            width: 120.0,
            line_y,
            line_opacity: 1.0,
            line_lit: 1.0,
        }
    }

    #[test]
    fn a_wave_dims_with_its_row_but_a_cursor_and_hover_do_not() {
        // Every opacity one annotation draws, on a row at `line_lit`.
        let opacities = |kind: Kind, line_lit: f32| {
            let annotation = annotation(kind);
            let mut sampled = Sampled::default();
            annotation.sample(
                &mut sampled,
                |property, default| if property == "opacity" { 0.8 } else { default },
                |_| {
                    Some(RangeSample {
                        line_lit,
                        ..range(10.0, 20.0)
                    })
                },
            );
            let diagnostics = sampled.diagnostics.iter().map(|d| d.opacity);
            let carets = sampled.carets.iter().map(|c| c.opacity);
            let hovers = sampled.hovers.iter().map(|h| h.opacity);
            diagnostics.chain(carets).chain(hovers).collect::<Vec<_>>()
        };
        let wave = || Kind::Diagnostic(DiagnosticPlan::info("run"));
        // A lit row leaves the wave exactly as before.
        assert_eq!(opacities(wave(), 1.0)[0].to_bits(), 0.8f32.to_bits());
        // A dimmed row dims it by the same level as the row's ink.
        assert_eq!(opacities(wave(), 0.35), [0.8 * 0.35]);
        assert!(opacities(wave(), 0.0).is_empty());
        let cursor = || Kind::Cursor(CursorPlan::new("a", "run"));
        assert_eq!(opacities(cursor(), 0.35), [0.8 * caret_blink(-1.0)]);
        let hover = || Kind::Hover(HoverPlan::new("run"));
        assert_eq!(opacities(hover(), 0.35), [1.0]);
    }

    #[test]
    fn diagnostics_take_their_range_and_fade_with_its_line() {
        let diagnostic = annotation(Kind::Diagnostic(DiagnosticPlan::error("run")));
        for (x, line_y, line_opacity) in
            [(40.0, 88.0, 1.0), (40.0, 113.25, 1.0), (61.5, 132.0, 0.5)]
        {
            let mut sampled = Sampled::default();
            diagnostic.sample(
                &mut sampled,
                |property, default| if property == "draw" { 0.5 } else { default },
                |target| {
                    assert_eq!(target, "run");
                    Some(RangeSample {
                        line_opacity,
                        ..range(x, line_y)
                    })
                },
            );
            let frame = sampled.diagnostics[0];
            assert_eq!((frame.x, frame.line_y, frame.width), (x, line_y, 120.0));
            assert_eq!((frame.draw, frame.opacity), (0.5, line_opacity));
        }
        let mut hidden = Sampled::default();
        diagnostic.sample(
            &mut hidden,
            |property, default| if property == "draw" { 0.0 } else { default },
            |_| Some(range(0.0, 0.0)),
        );
        assert!(
            hidden.diagnostics.is_empty(),
            "an undrawn wave inks nothing"
        );
    }

    #[test]
    fn a_caret_blends_between_anchors_and_selects_between_tail_and_head() {
        let cursor = annotation(Kind::Cursor(
            CursorPlan::new("a", "first").anchor("b", "second"),
        ));
        let sample = |values: &[(&str, f32)]| {
            let mut sampled = Sampled::default();
            cursor.sample(
                &mut sampled,
                |property, default| {
                    values
                        .iter()
                        .find(|(name, _)| *name == property)
                        .map_or(default, |(_, value)| *value)
                },
                |target| {
                    Some(if target == "first" {
                        range(0.0, 0.0)
                    } else {
                        range(200.0, 44.0)
                    })
                },
            );
            sampled
        };
        let resting = sample(&[]);
        assert_eq!(
            (resting.carets[0].x, resting.carets[0].line_y),
            (120.0, 0.0)
        );
        assert!(resting.selections.is_empty(), "tail defaults to head");
        let halfway = sample(&[("anchor.a", 0.5), ("anchor.b", 0.5), ("head", 0.0)]);
        assert_eq!(
            (halfway.carets[0].x, halfway.carets[0].line_y),
            (100.0, 22.0)
        );
        let selecting = sample(&[
            ("anchor.a", 0.0),
            ("anchor.b", 1.0),
            ("tail", 0.0),
            ("head", 0.75),
        ]);
        assert_eq!(selecting.carets[0].x, 200.0 + 90.0);
        assert_eq!(selecting.selections.len(), 1);
        assert_eq!(selecting.selections[0].x, 200.0);
        assert_eq!(selecting.selections[0].width, 90.0);
    }
}
