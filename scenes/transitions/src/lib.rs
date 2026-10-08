//! Transitions showroom: one reel that changes scenes every way a reel can,
//! between Stage diagrams, code, and title cards. Each segment names the
//! transition that brought it in, in a chip at the bottom of the frame.
use anyhow::Result;
use psychopomp::{
    author::{PlanBuilder, seconds},
    caption::{CaptionActor, CaptionAlign, CaptionPlan, CaptionSpanPlan},
    editor::{EDITOR_RECIPE, EditorLinePlan, EditorPartPlan, EditorRecipePlan},
    highlight,
    plan::{
        ReelPlan, ReelSegmentPlan, ReelWipePlan, ScenePlan,
        WipeDirection::{Left, Right, Up},
    },
    stage::{StageActor, StageElement, StagePlan, StagePost, StatusText},
    tone::Tone,
};
use serde_json::json;

/// How long every frame lasts, including the transitions at either end.
const FRAME: f64 = 3.2;

/// The overview's API card, which a match carries onto the detail's.
const API: [f32; 3] = [960.0, 720.0, 0.0];
const API_SIZE: [f32; 2] = [300.0, 120.0];
const DETAIL_API: [f32; 3] = [960.0, 440.0, 0.0];
const DETAIL_API_SIZE: [f32; 2] = [760.0, 250.0];
const DB: [f32; 3] = [1440.0, 720.0, 0.0];
const CARD: [f32; 2] = [300.0, 120.0];
const GATEWAY: [f32; 3] = [960.0, 330.0, 0.0];
const GATEWAY_RADIUS: f32 = 74.0;

pub fn build_reel() -> Result<ReelPlan> {
    let s = seconds;
    ReelPlan::new(
        "transitions",
        vec![
            ReelSegmentPlan::cut(title("opening", "transitions", None)?),
            ReelSegmentPlan::crossfaded(system("overview", "crossfade", "")?, s(0.8)),
            ReelSegmentPlan::matched(
                detail("detail", "match", "")?,
                s(1.3),
                rect(API, API_SIZE),
                rect(DETAIL_API, DETAIL_API_SIZE),
            ),
            ReelSegmentPlan::pushed(code("handler", "push", "left", HANDLER)?, s(0.75), Left),
            ReelSegmentPlan::flipped(
                code("handler-fixed", "flip", "left", HANDLER_FIXED)?,
                s(1.1),
                Left,
            ),
            ReelSegmentPlan::dipped(system("gateway", "dip", "")?, s(0.8)),
            ReelSegmentPlan::irised(
                title("shipping", "ship it", Some(("iris", "ring")))?,
                s(1.1),
                true,
            )
            .focused(circle(GATEWAY, GATEWAY_RADIUS)),
            ReelSegmentPlan::slid(incident("incident", true, "slide", "up")?, s(0.8), Up),
            ReelSegmentPlan::glitched(incident("recovered", false, "glitch", "")?, s(0.42)),
            ReelSegmentPlan::whipped(system("regions", "whip", "right")?, s(0.5), Right),
            ReelSegmentPlan::zoomed(code("query", "zoom", "", QUERY)?, s(1.15), rect(DB, CARD)),
            ReelSegmentPlan::zoomed_out(
                system("pulled-back", "zoom out", "")?,
                s(1.15),
                rect(DB, CARD),
            ),
            ReelSegmentPlan::cubed(code("client", "cube", "left", CLIENT)?, s(1.1), Left),
            ReelSegmentPlan::inked(detail("detail-ink", "ink", "")?, s(1.4)),
            ReelSegmentPlan::wiped(
                incident("compare", false, "wipe", "")?,
                s(0.9),
                ReelWipePlan::new(Left).labeled("BEFORE", "AFTER"),
            ),
            ReelSegmentPlan::flashed(title("deployed", "deployed", Some(("flash", "")))?, s(0.7)),
            ReelSegmentPlan::cubed(system("closing-system", "cube", "up")?, s(1.0), Up),
            ReelSegmentPlan::leaked(
                title("closing", "psychopomp", Some(("light leak", "")))?,
                s(1.6),
            ),
        ],
    )
}

fn rect(at: [f32; 3], size: [f32; 2]) -> [f32; 4] {
    [
        at[0] - size[0] * 0.5,
        at[1] - size[1] * 0.5,
        size[0],
        size[1],
    ]
}

fn circle(at: [f32; 3], radius: f32) -> [f32; 4] {
    rect(at, [radius * 2.0, radius * 2.0])
}

fn span(text: &str, tone: Tone) -> CaptionSpanPlan {
    CaptionSpanPlan::new(text, tone)
}

/// The chip naming the transition that brought this frame in.
fn name(scene: &mut PlanBuilder, transition: &str, detail: &str) -> Result<()> {
    let mut spans = vec![span(transition, Tone::Accent)];
    if !detail.is_empty() {
        spans.push(span(&format!("  {detail}"), Tone::Muted));
    }
    CaptionActor::declare(
        scene,
        "transition",
        &CaptionPlan::line([960.0, 1000.0], 22.0, spans)
            .aligned(CaptionAlign::Center)
            .chip(),
    )?;
    Ok(())
}

fn title(id: &str, text: &str, transition: Option<(&str, &str)>) -> Result<ScenePlan> {
    let mut scene = PlanBuilder::new(id, seconds(FRAME));
    scene.actor("title", "title-card", json!({ "title": text }))?;
    if let Some((transition, detail)) = transition {
        name(&mut scene, transition, detail)?;
    }
    Ok(scene.finish()?)
}

fn card(
    id: &str,
    at: [f32; 3],
    size: [f32; 2],
    title: &str,
    status: &[(&str, Tone)],
    tone: Tone,
) -> StageElement {
    StageElement::Card {
        id: id.into(),
        at,
        size,
        title: title.into(),
        status: status
            .iter()
            .map(|(text, tone)| StatusText {
                text: (*text).into(),
                tone: *tone,
            })
            .collect(),
        tone,
        mark: Default::default(),
    }
}

fn beam(id: &str, from: &str, to: &str, tone: Tone) -> StageElement {
    StageElement::Beam {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        bend: 0.0,
        tone,
    }
}

fn stage(
    id: &str,
    elements: Vec<StageElement>,
    transition: &str,
    detail: &str,
) -> Result<ScenePlan> {
    let mut scene = PlanBuilder::new(id, seconds(FRAME));
    StageActor::declare(
        &mut scene,
        "stage",
        &StagePlan {
            post: StagePost::default(),
            elements,
        },
    )?;
    name(&mut scene, transition, detail)?;
    Ok(scene.finish()?)
}

/// A gateway orb above three services.
fn system(id: &str, transition: &str, detail: &str) -> Result<ScenePlan> {
    stage(
        id,
        vec![
            StageElement::Orb {
                id: "gateway".into(),
                at: GATEWAY,
                radius: GATEWAY_RADIUS,
                points: 720,
                tone: Tone::Accent,
            },
            card(
                "client",
                [480.0, 720.0, 0.0],
                CARD,
                "client",
                &[("idle", Tone::Muted)],
                Tone::Request,
            ),
            card(
                "api",
                API,
                API_SIZE,
                "api",
                &[("p99 96 ms", Tone::Success)],
                Tone::Accent,
            ),
            card(
                "db",
                DB,
                CARD,
                "db",
                &[("12 conns", Tone::Muted)],
                Tone::Plain,
            ),
            beam("to-client", "gateway", "client", Tone::Request),
            beam("to-api", "gateway", "api", Tone::Accent),
            beam("to-db", "gateway", "db", Tone::Plain),
        ],
        transition,
        detail,
    )
}

/// The API card up close, with what it depends on.
fn detail(id: &str, transition: &str, detail: &str) -> Result<ScenePlan> {
    stage(
        id,
        vec![
            card(
                "api",
                DETAIL_API,
                DETAIL_API_SIZE,
                "api",
                &[("p99 96 ms · 2.4k rps", Tone::Success)],
                Tone::Accent,
            ),
            card(
                "auth",
                [600.0, 790.0, 0.0],
                CARD,
                "auth",
                &[("jwt", Tone::Muted)],
                Tone::Plain,
            ),
            card(
                "cache",
                [1320.0, 790.0, 0.0],
                CARD,
                "cache",
                &[("hit 94%", Tone::Success)],
                Tone::Plain,
            ),
            beam("to-auth", "api", "auth", Tone::Plain),
            beam("to-cache", "api", "cache", Tone::Plain),
        ],
        transition,
        detail,
    )
}

/// Four regions, failing or recovered.
fn incident(id: &str, failing: bool, transition: &str, detail: &str) -> Result<ScenePlan> {
    let tone = if failing { Tone::Error } else { Tone::Success };
    let latency = if failing {
        ["timeout", "1.2 s", "timeout", "980 ms"]
    } else {
        ["120 ms", "96 ms", "140 ms", "110 ms"]
    };
    let mut elements = vec![StageElement::Orb {
        id: "gateway".into(),
        at: GATEWAY,
        radius: GATEWAY_RADIUS,
        points: 720,
        tone: if failing { Tone::Error } else { Tone::Accent },
    }];
    for (index, region) in ["eu-west", "us-east", "ap-south", "sa-east"]
        .into_iter()
        .enumerate()
    {
        let status = format!("p99 {}", latency[index]);
        elements.push(card(
            region,
            [360.0 + index as f32 * 400.0, 720.0, 0.0],
            CARD,
            region,
            &[(&status, tone)],
            tone,
        ));
        elements.push(beam(&format!("to-{region}"), "gateway", region, tone));
    }
    stage(id, elements, transition, detail)
}

const HANDLER: (&str, &[&str]) = (
    "handler.ts",
    &[
        "export const handler = Effect.gen(function* () {",
        "  const request = yield* HttpRequest",
        "  const user = yield* Users.find(request.id)",
        "  return Response.json(user)",
        "})",
    ],
);

const HANDLER_FIXED: (&str, &[&str]) = (
    "handler.ts",
    &[
        "export const handler = Effect.gen(function* () {",
        "  const request = yield* HttpRequest",
        "  const user = yield* Users.find(request.id).pipe(",
        "    Effect.timeout(\"2 seconds\"),",
        "    Effect.retry(Schedule.exponential(\"100 millis\"))",
        "  )",
        "  return Response.json(user)",
        "})",
    ],
);

const QUERY: (&str, &[&str]) = (
    "db.ts",
    &[
        "export const findUser = (id: UserId) =>",
        "  sql`select * from users where id = ${id}`.pipe(",
        "    Effect.map(rows => rows[0]),",
        "    Effect.withSpan(\"db.findUser\")",
        "  )",
    ],
);

const CLIENT: (&str, &[&str]) = (
    "client.ts",
    &[
        "const client = HttpClient.make({",
        "  baseUrl: \"https://api.example.com\",",
        "  retry: Schedule.recurs(3)",
        "})",
        "",
        "export const getUser = (id: UserId) =>",
        "  client.get(`/users/${id}`)",
    ],
);

/// A still editor showing `file`.
fn code(
    id: &str,
    transition: &str,
    detail: &str,
    (file, source): (&str, &[&str]),
) -> Result<ScenePlan> {
    let mut scene = PlanBuilder::new(id, seconds(FRAME));
    let lines = source
        .iter()
        .enumerate()
        .map(|(index, text)| EditorLinePlan {
            id: format!("line-{index}"),
            parts: vec![EditorPartPlan {
                id: "code".into(),
                spans: highlight::typescript(if text.is_empty() { " " } else { text }),
            }],
            semantic_ranges: Vec::new(),
            mark: None,
        })
        .collect::<Vec<_>>();
    let ids = lines.iter().map(|line| line.id.clone()).collect::<Vec<_>>();
    scene.actor(
        "editor",
        EDITOR_RECIPE,
        &EditorRecipePlan {
            file_name: file.into(),
            focus_line_id: ids[ids.len() / 2].clone(),
            lines,
            initial_line_ids: ids.clone(),
            final_line_ids: ids,
            snapshots: Vec::new(),
            line_height: 44.0,
            entering_offset_x: 0.0,
            focus_height: 44.0,
            inline_reveal: None,
            additional_inline_reveals: Vec::new(),
        },
    )?;
    name(&mut scene, transition, detail)?;
    Ok(scene.finish()?)
}

#[cfg(test)]
mod tests {
    use psychopomp::plan::ReelTransitionStyle;

    #[test]
    fn the_showroom_names_every_transition_once() {
        let reel = super::build_reel().unwrap();
        let styles = reel
            .segments
            .iter()
            .skip(1)
            .map(|segment| segment.transition_style)
            .collect::<Vec<_>>();
        assert!(styles.contains(&ReelTransitionStyle::Ink));
        assert!(styles.contains(&ReelTransitionStyle::Iris { ring: true }));
        // Never more than two segments at once, from start to end.
        let end = reel.duration_nanos() as f64 / 1e9;
        for step in 0..=(end * 20.0) as u32 {
            assert!(reel.layers_at(f64::from(step) / 20.0).len() <= 2);
        }
    }
}
