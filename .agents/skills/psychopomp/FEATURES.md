# Feature catalogue

Use this catalogue to pick the Psychopomp feature that fits what a film needs.
Each row gives the handle that authors it, a working Scene Program to copy,
the doc section with its payload and channels, and how well it is proven. Read the rows
you need, then open only their sections: [SCENE_PLANS.md][sp-recipes] and
[CONTEXT.md][cx] stay the source for payload fields, channel names, and terms.
Paths are relative to the [Psychopomp repository](https://github.com/kitlangton/psychopomp).

- **Handles.** *score*: a `psychopomp::score` value handle. Its `&self` methods
  return a `Beat` ([SP score][sp-score]). *imp*: the imperative `*Actor`. Its
  methods take `(&mut PlanBuilder, …, at, …)` and return a time, and most scenes
  use it. Both lower to the same plan events: `scenes/hello/src/bin/hello_dsl.rs`
  is a byte-identical score twin of `scenes/hello`. `actor.beats()` turns an
  imperative actor into its score handle. To call an imp-only method from score
  code, wrap it in `score::action(|scene, at| …)` (which returns the end time) or
  `score::impulse`. A recipe with no handle is declared with
  `scene.actor(id, RECIPE, payload)` and driven by `PlanBuilder` channels.
- **Status.** *supported*: a scene in [`verify.json`][verify] uses it, so
  `verify compare` re-runs that scene and compares its plan and key frames.
  *outside verify*: a working scene exists, but `verify` does not run it. *no demo*: the API exists, but no scene calls it.
  Test it before you rely on it. *provisional*: awaiting visual review, so it may
  change or disappear. *export-only*: it renders to video, but `plan present`
  rejects it.
- **Docs.** SP is SCENE_PLANS.md, CX is CONTEXT.md, AR is ARCHITECTURE.md, and FX
  is EFFECTS.md. Motion guidance for each beat family is in the
  `explainer-motion` skill's [TECHNIQUES.md](../explainer-motion/TECHNIQUES.md#beat-families).

## Roots

A plan has at most one root. `stage`, `editor`, `title-card`, and `keyed-grid`
are mutually exclusive, and preflight rejects a second one. A plan without a root
draws its overlays on the theme background. Editor attachments need an `editor`
root. `stage` anchors need a Stage root. Every other recipe is an overlay over any
root.

| Root | Use it for | Author with | Example | Docs | Status |
| --- | --- | --- | --- | --- | --- |
| `stage` | A 2.5D GPU diagram with cards, particle orbs and forms, wires, packets, effects, a film camera, and bloom | score `Stage::declare(scene, id, &StagePlan)`; imp `StageActor::declare` | `scenes/hello`; `scenes/pr-walkthrough/src/flagship.rs` | [SP recipes][sp-recipes] (`stage`), [CX Stage][cx-stage], [AR Stage][ar-stage] | supported |
| `editor` | Code whose lines and inline parts keep their identity | no score handle; `editor::diff::Diff::declare` (Stepped Diff) or `scene.actor(id, EDITOR_RECIPE, &EditorRecipePlan)` | `scenes/diagnostics`; `scenes/transitions/src/lib.rs` (`code`) | [SP keyed edits][sp-snapshots], [CX Stepped Diff][cx-stepped-diff] | supported |
| `title-card` | A plain title or section card | no handle; `scene.actor(id, "title-card", json!({ "title": … }))`, with an optional `subtitle` State Channel | `scenes/transitions/src/lib.rs` (`title`); State Channel: `scenes/agent-demo` | [SP typed handles][sp-handles] | supported (State Channel: outside verify) |
| `keyed-grid` | A finite product shown as a connected 3D grid that grows and regroups | `GridRecipePlan` through `scene.actor(id, GRID_RECIPE, …)` | `scenes/keyed-grid` | [SP 3D grid][sp-grid], [CX Keyed Grid][cx-keyed-grid], [README][grid-readme] | outside verify; native deck; at most 256 cells; never write `__grid.*` channels |
| none | Overlays on the theme background | declare no root | `scenes/charts`; `scenes/text-surfaces` | — | supported |

## Stage elements

A `StagePlan { post, elements }` holds 1 to 128 elements. An element ID has no
dots or whitespace, and `camera` and `post` are reserved. Six kinds have
constructors. Write the rest as JSON through `serde_json::from_value`, as
`scenes/stage-forms` does, or as `StageElement::Icon { … }`-style literals, as
`scenes/shape-of-openness` does. Every `<element>.<property>` channel rests at
its value in one defaults table (`StageElement::channel_defaults` in
`crates/psychopomp/src/stage.rs`).

| Element | Use it for | Build with | Example | Notes |
| --- | --- | --- | --- | --- |
| `card` | A service, client, or state, shown as a panel with a title and status lines | `StageElement::card(id, at, size, title)`, `.tone`, `.statuses` (up to six), `.mark(Mark::Cross)` | `flagship.rs` (`stage_plan`) | Also takes the deletion channels (`cool`, `damage`, `glitch`, `cut`, `ghost`), the spinner clocks, `charge`, `dissolve`, and `scan` |
| `orb` | The hero or a live service, shown as a particle sphere | `StageElement::orb(id, at, radius)`, `.points`, `.tone` | `scenes/hello` | `pulse` changes light, never radius; `burst`; `charge` |
| `form` | Particle shapes that morph and tumble: sphere, box, plane, lattice, cylinder, torus, helix, knot | JSON `"kind": "form"` with `shapes` (there is no constructor) | `scenes/stage-forms/src/main.rs` | `helix` and `knot`: no demo. A Hollow form (`solid` 0) appears only in the balls films |
| `shape` | A rect, circle, arc, or polygon without card chrome, whose stroke draws on | JSON `"kind": "shape"` | `scenes/stage-forms` | Arcs can carry arrowheads |
| `path` | An arrowed route through points and elements, where element waypoints act as relay stops | JSON `"kind": "path"`, `through` | `scenes/stage-forms` | `curve: "bezier"`: no demo |
| `icon` | A bundled Phosphor icon or SVG path data | JSON `"kind": "icon"`; `ink` for a fixed pigment | `scenes/stage-forms`; SVG path and `ink`: `scenes/shape-of-openness` | Names are in `stage::ICONS` |
| `beam` | A wire between two positioned elements | `StageElement::beam(id, from, to)`, `.bend`, `.tone` | `scenes/hello` | `flow`, `surge`, `twang`, and `break` move only when written |
| `packet` | A message riding a beam or path | `StageElement::packet(id, beam)`, `.reversed`, `.labeled`, `.tone` | `scenes/hello` | One `age` clock; see `send` |
| `label` | World-space text that is typed in or faded | `StageElement::label(id, at, size, spans)`, `.align`, `.face(Face::…)`, `.columns()` (Column Grid: one 0.6 em cell per character, mono only) | `scenes/effects-showroom` (captions) | Faces other than `mono` are installed by macOS |
| Code Label | One line of code drawn on the Stage in the editor's syntax colors | `StageElement::code(id, at, size, Syntax::CSharp, line)` (left edge of column 0 at `at`; `Lang::syntax()` picks the highlighter); custom runs: `syntax.label_spans(line)` → `CaptionSpanPlan`s in `Tone::{Keyword, Type, String, Call, Number, Muted}`, then `.columns()` | `scenes/code-labels` | Character `c` starts at `at.x + c × stage::MONO_ADVANCE × size`; leading spaces count. Opt-in: plain labels lay out as before |
| `ring` | A timer arc, a halo, or a ripple | `StageElement::ring(id, at, radius)`, `.thickness` | `flagship.rs` (halo) | `sweep`, `expand` |
| `bolt` | Lightning between elements, shields, or points | JSON `"kind": "bolt"` | `scenes/effects-showroom/src/lib.rs` (`strike`) | See `zap`, `hum`; [FX Lightning][fx-lightning] |
| `shield` | A forcefield bubble that ripples on contact | JSON `"kind": "shield"` | `scenes/effects-showroom/src/lib.rs` (`shield`) | See `raise`; [FX shield][fx-dissolve] |
| `footage` | A still, clip, or image sequence at Stage depth | JSON `"kind": "footage"`, plus `scene.media(footage::media(…))` | `scenes/footage/src/lib.rs` (`build_stage`) | export-only; see `footage_playhead` |
| `post` | The Stage's look | `StagePost::default()`, `StagePost::RESTRAINED` (narrated films), `StagePost::FLAT` (editorial) | `flagship.rs`; `scenes/shape-of-openness` | `post.*` channels: [SP recipes][sp-recipes] (`stage`) |

## Stage beats

An imperative `StageActor` beat takes `(scene, …, at, …)` and returns the time
after its arrow. The score `Stage` method of the same name drops `scene` and
`at`. It returns a Beat whose Span ends at that time, or a zero-length Span for
an impulse.

| Family | Beats | Example | Notes |
| --- | --- | --- | --- |
| Entrances | `settle_in(card)` → ready to wire (+500 ms); `orb_in(orb, OrbEntrance::HERO)`; `fade_in`, `fade_out`; `type_in(label, chars_per_second)` → last character | `scenes/hello`; `flagship.rs` | `orb_in` and `fade_*` are score impulses |
| Wires | `connect(beam, s)` → contact; `connect_contacting` (contact lands on the cue); `disconnect` → port gone; `twang` | `scenes/hello`; `scenes/pr-50231/src/permissions.rs`; `scenes/2password/src/main.rs` | A connection never implies `flow`, `surge`, or `land` |
| Packets | `send(packet, s)` → arrival; `send_arriving` (arrival lands on the cue); `relay` → each leg's arrival; `stage::reply_after(arrival)`, `CueTime::reply()` | `scenes/hello`; `scenes/pr-50231/src/tools.rs`; `scenes/stage-forms`; `flagship.rs` | Score `send` spans `[launch, arrival]`, and `relay` spans to the final arrival |
| Contact | `land(element)`; `hit(property, peak, rest)`; `kick`; `jolt(direction, strength)`; `shock_kick(source, at, card, push, falloff)` → front passes | `scenes/hello`; `flagship.rs`; `scenes/stage-forms`; `kick` alone: `scenes/balls-v3/src/theory.rs` | `land` flashes a card, shape, or icon and pulses an orb or form. Nothing scales |
| Status and attention | `swap_status(card, [from, to], s)`; `swap_labels([from, to], gap)`; `resolve_spinner(card, started, done)` → mark drawn, with `clock("<card>.spinner")` and `clock("<card>.release")`; `dim(cards, amount, s)`; `halo`, `halo_out`; `ring_timer(ring, s, sweep)`; `glitch(card, seeds)` | `flagship.rs` (`stage_film`); `scenes/opencode-jr-architecture/src/session.rs` (`ring_timer`); `scenes/psychopomp-intro/src/main.rs` (`dim`) | Shown only inside narrated films, not in a showroom |
| Destruction and replay | burst: `clock_for("<orb>.burst", combustion::DURATION)`; `unburst(orb, s)`; `rewind(chroma)`; `morph(form, index, s)` | `flagship.rs`; form burst and `morph`: `scenes/stage-forms` | A burst has no named beat |
| Effects | `zap(bolt)` → contact; `charge(element, intensity, s)`; `hum(bolt, intensity, s)`; `raise`, `lower(shield, s)`; `dissolve(card)` → gone; `materialize(card, s)` → whole; `scan(card, s)` → done | `scenes/effects-showroom/src/lib.rs` | `lower`: no demo; [FX][fx-dissolve] |
| Primitives | `channel`, `set`, `to`, `spring`, `bounce`, `ease`, `glide`, `clock`, `clock_for` | every Stage scene | Imp `to`, `spring`, `ease`, and `set` start an undeclared channel at its default. Score `Stage::spring` starts it at 0, so declare the channel first |
| Footage | `footage_playhead(element, &media)` → `Playhead` | `scenes/footage/src/lib.rs` (`build_stage`) | Imp only |

## Camera

Shots belong to the `CameraRig` (imp, `stage.camera()`) or to the score `Camera`
(`s.camera()`, whose `.rig()` reaches the imp-only methods). Each shot reads the
pose written so far, writes only the `camera.*` channels that change, and moves
by a `Move`: `Spring(s)`, `Glide(s)`, `Ease(s, curve)`, or `Cut`. Impacts add on
top of any shot: the `jolt` Stage beat, a sustained `camera.quake`, and the
`post.zoom` and `post.flash` channels. The hype register in
`scenes/psychopomp-intro/src/main.rs` uses all four
([TECHNIQUES.md](../explainer-motion/TECHNIQUES.md#hype-register)). Docs:
[SP recipes][sp-recipes] (the Stage camera bullet),
[CX Stage Camera][cx-stage-camera], and [AR camera][ar-stage-camera].

| Shot or helper | Score `Camera` | Example | Status |
| --- | --- | --- | --- |
| `establish`, `frame`, `drift`, `whip`, `orbit`, `dolly_zoom`, `roll`, `focus_on`, `aperture`, `follow`, `release`, `handheld`, `pose` | yes | `scenes/camera/src/main.rs` | supported |
| `push_in`, `pull_back` | yes | `scenes/footage/src/lib.rs` (`build_stage`) | supported |
| `framing` (returns a pose, writes nothing), `move_to(pose)` | no | `scenes/camera/src/main.rs` | supported |
| `screen_box(element, pose)` → `Box2` | no | `scenes/balls-v3/src/theory.rs` | supported |
| `crash(targets, padding, roll, s)` | no | none (the balls films' `crash` is a local helper in `kit.rs`) | no demo |
| `sized(size)`, for a frame other than 1920×1080 | no | none | no demo |
| `stage::Camera::at(position, size).project_rect(center, size)` | n/a | `flagship.rs` (`client_rect`) | supported |

## Overlays and anchors

An overlay pins to a target the renderer places with
`AnchorPlan::point`, `::stage`, `::editor`, `::participant`, or `::row`, plus
`.with_offset`. Callouts and lenses use `CalloutAnchorPlan`. The overlay's
`move_to(anchor)` springs its `anchor.<id>` weights ([SP anchors][sp-anchors],
[CX Anchor][cx-anchor], [AR Anchors][ar-anchors]).

| Recipe | Use it for | Author with | Example | Docs | Status |
| --- | --- | --- | --- | --- | --- |
| `caption` | Short terminal-voice lines, chips, and frosted glass labels | score `Caption` (`show`, `hide`, `move_to`, `type_in`; `Caption::header`, `::chip`, `::footer`); imp `CaptionActor`, `psychopomp::chrome`; `CaptionPlan::line(…).aligned().chip().anchor().glass()` | `scenes/anchors/src/lib.rs` (`build_stage`); glass: `scenes/loupe/src/lib.rs` (`build_stage`) | [SP recipes][sp-recipes], [CX Caption][cx-caption] | supported |
| `callout` | A leader and label pinned to a card, code range, or diagram row | score `Callout` (`show`, `hide`, `move_to`, `emphasize`); imp `CalloutActor`; `CalloutPlan::new(anchor, spans)` | `scenes/callouts` | [SP recipes][sp-recipes], [CX Callout][cx-callout] | supported |
| `rolling-number` | A version, count, or timer whose digits roll | score `RollingNumber` (`show`, `hide`, `roll`, `move_to`); imp `RollingNumberActor`; `RollingNumberPlan::new(…).roll(at, value)` | `scenes/rolling-number`; as a timer: `scenes/pr-walkthrough/src/stop_stage.rs` | [SP recipes][sp-recipes], [CX Rolling Number][cx-rolling-number] | supported; export-only; `move_to`: no demo |
| `text` | Plain text that is swapped in place or masked | score `Text` (`show`, `hide`, `swap`, `move_to`); imp `TextActor` (adds `show_during`); `TextPlan`; optional `content` State Channel | `scenes/anchors/src/lib.rs` (`build_stage`) | [SP recipes][sp-recipes], [SP mask][sp-mask] | supported; `show_during`, `move_to`: no demo |
| `lens` | A glass loupe that magnifies code or a card | score `Lens` (`show`, `hide`, `move_to`, `slide`, `magnify`, `resize`, `focus`); imp `LensActor`; `LensPlan::circle`, `LensPlan::capsule` | `scenes/loupe` | [SP recipes][sp-recipes], [CX Lens][cx-lens], [AR Lenses][ar-lenses] | supported; `LensPlan::frost`: no demo |

## Code

Everything here works on the `editor` root. `Diff::declare` creates that root
for you.

| Feature | Use it for | Author with | Example | Docs | Status |
| --- | --- | --- | --- | --- | --- |
| Syntax highlighting | Coloring a code line by language | `highlight::Syntax::for_file(name)` (`.py`/`.pyi` Python, `.cs`/`.csx` C#, else TypeScript), `Lang::syntax()`, `Syntax::spans(line)`; `highlight::{typescript, csharp, python}`; `Diff::declare_as(syntax, …)` overrides the file name | `scenes/code-labels`; `scenes/diff-pacing` (C#) | [AR highlighting][ar-highlight] | supported; line-local: strings, raw strings and block comments that span lines are not tracked |
| Stepped Diff | A change told in steps keyed to phrases | `editor::diff::{Diff, keep, add, remove}`; `Line::range`, `.inlay`, `.marked`; `Diff::declare(scene, step_times, warning, entrance)` → `DiffEditor` (`target`, `inlay`); `diff::step_snapshots`, `diff::gap_lines` | `scenes/diagnostics`; PR template: `scenes/pr-walkthrough/src/film.rs` (`code_with`) | [SP reel][sp-reel], [CX Stepped Diff][cx-stepped-diff] | supported; at most 76 columns and 14 rows |
| Paced Line Marks | `+` only on rows that changed, fading once read | `Diff::declare_paced(scene, step_times, warning, entrance, ChangeMarks::paced())`; `ChangeMarks { only_changed, fade_after }` (`FADE_AFTER` = 1.5 s); `Line::unmarked()` to say a re-added row did not change | `scenes/diff-pacing` | [CX Line Mark][cx-line-mark] | supported; a row re-added in a later step than it left still counts as changed (use `.unmarked()`) |
| Row dimming | Light some rows and dim the rest | `DiffEditor::focus(scene, rows, at)` (to `diff::DIM` = 0.35 over 0.25 s), `focus_to(…, level)`, `unfocus(scene, at)`; any editor: the `lit.<line-id>` channel (`editor::lit_channel`, rest 1) | `scenes/diff-pacing` | [CX Line Mark][cx-line-mark] | supported; dims code ink and Line Marks, not Diagnostics, Cursors, Hover Cards, or the token highlight |
| Keyed edits and Inline Reveals | Line-order snapshots; spans that open inside a stable line | `EditorRecipePlan.snapshots`, `EditorInlineRevealPlan`; `editor::{line, part, semantic_line, semantic_range}` | `flagship.rs` (`stable_code`); `scenes/effect-succeed-slides` | [SP keyed edits][sp-snapshots], [SP stability][sp-stability], [CX Inline Reveal][cx-inline-reveal] | supported |
| Semantic Targets | Code ranges that callouts, lenses, annotations, and highlights attach to | `scene.semantic_target(id, &editor, EditorTargetSelector)`, or `DiffEditor::target` | `scenes/loupe/src/lib.rs` (`build_editor`) | [CX Semantic Target][cx-semantic-target] | supported |
| Scalar Plans | Channel values measured from a target, such as a highlight or pointer that follows a range | `SemanticTargetHandle::x`, `width`, `center_x`, `line_y`, `offset`, written with `spring_to` | `scenes/hero/src/lib.rs` | [CX Scalar Plan][cx-scalar-plan] | outside verify |
| Panel motion | Moving, tilting, or zooming the code card | editor channels `panel-x`, `panel-y`, `panel-scale`, `panel-rotation`, `panel-tilt-x`, `panel-tilt-y`, `panel-opacity`, `panel-near-blur` (`plan_runtime/editor.rs`) | `scenes/callouts` (scale); tilt, rotation, near blur: `scenes/quark-before-after` | [SP recipes][sp-recipes] (Line Marks bullet) | supported; tilt, rotation, near blur: outside verify |
| `diagnostic` | A severity wave and gutter icon under a range | score `Diagnostic` (`show`, `clear`); imp `DiagnosticActor`; `DiagnosticPlan::error`, `::warning`, `::info` | `scenes/diagnostics` | [SP recipes][sp-recipes], [CX Diagnostic][cx-diagnostic] | supported |
| `hover-card` | An IDE tooltip on a range | score `Hover` (`show`, `hide`); imp `HoverActor`; `HoverPlan::new(target).code(…).text(…).below()` | `scenes/diagnostics` | [CX Hover Card][cx-hover-card] | supported |
| `cursor` | A caret and selection | score `Cursor` (`show`, `hide`, `move_to`, `select`); imp `CursorActor` (adds `collapse`); `CursorPlan` | `scenes/diagnostics` | [CX Cursor][cx-cursor] | supported |
| Inlay Hint | Ghost type text that opens room inline | `ide::insert_inlay` or a diff line's `.inlay`; `InlayHint::on(…).show`, `.hide`; score `show_beat`, `hide_beat` | `scenes/diagnostics` | [CX Inlay Hint][cx-inlay-hint], [AR IDE][ar-ide] | supported |
| `pointer` | A hand that leans with its acceleration and points at targets | `PointerRecipePlan { editor_id }` through `scene.actor(id, POINTER_RECIPE, …)`; it needs `x`, `y`, `opacity`, `scale`, and `blur` channels | `scenes/hero/src/lib.rs` | [CX Pointer][cx-pointer] | outside verify; one per editor |

## Diagrams and data

| Recipe | Use it for | Author with | Example | Docs | Status |
| --- | --- | --- | --- | --- | --- |
| `sequence` | A protocol's ordered messages, replayed in the same slots | score `Sequence` (`show`, `hide`, `to`, `animate`, `reveal`, `fade`, `strike`, `participant`); imp `SequenceActor`; `SequenceRowPlan::message`, `::reply`, `::note`, `::end` with `.in_slot`, `.with_aside` | `scenes/pr-walkthrough/src/film.rs` (`behavior`); anchors: `scenes/anchors/src/lib.rs` (`build_sequence`) | [SP recipes][sp-recipes], [CX Sequence Diagram][cx-sequence] | supported; `participant`: no demo |
| `tree` | A JSON payload, config, or plan that unfolds | score `Tree` (`open`, `close`, `reveal`, `scroll_to`, `highlight`, `set`, `show`, `hide`); imp `TreeActor`; `TreePlan::new(…)` | `scenes/tree` | [SP recipes][sp-recipes], [CX Tree][cx-tree] | supported; `scroll_to`: no demo |
| `plot` | Sampled curves such as springs and metrics, with a riding dot and its velocity | score `Plot` (`show`, `draw`, `fade`, `ride`, `stop_ride`, `velocity`, `mark`, `hide`); imp `PlotActor`; `PlotSeriesPlan::sampled`, `::motion` | `scenes/charts` | [SP recipes][sp-recipes], [CX Plot][cx-plot] | supported |
| `lanes` | A plan's channels drawn as tracks | score `Lanes` (`show`, `scrub`, `emphasize`, `hide`); imp `LanesActor`; `LanesPlan::from_scene_plan` | `scenes/charts` | [CX Lanes][cx-lanes] | supported |
| `checklist` | Checks that run, fail, retry, and pass | score `Checklist` (`show`, `hide`, `reveal`, `reveal_item`, `start`, `resolve`); imp `ChecklistActor` (adds `skip`) | `scenes/viz-components/src/lib.rs` (`checklist`) | [SP recipes][sp-recipes], [CX Checklist][cx-checklist] | supported; `reveal_item`: no demo |
| `meter` | A gauge, a countdown ring, or a progress bar | score `Meter` (`show`, `hide`, `set`, `sweep`); imp `MeterActor` (adds `countdown`, `flash`); `MeterPlan::ring`, `::bar`, `::countdown` | `scenes/viz-components/src/lib.rs` (`meters`) | [CX Meter][cx-meter] | supported |
| `bars` | Before and after benchmark numbers | score `Bars` (`show`, `hide`, `reveal_rows`, `set`, `grow`, `sort`, `reveal_deltas`); imp `BarsActor` | `scenes/viz-components/src/lib.rs` (`bars`); `scenes/amdahl` | [CX Benchmark Bars][cx-bars] | supported; `reveal_rows`: no demo |
| `confetti` | A seeded success burst | score `Confetti` (`burst`); imp `ConfettiActor` | `scenes/viz-components/src/lib.rs` (`checklist`) | [CX Confetti][cx-confetti] | supported |

Meters, bars, and checklist counts share the channel-driven [Readout][cx-readout].
Plot, Lanes, and Tree are channel-only, so they also run in `plan present`.

## Text surfaces

| Recipe | Use it for | Author with | Example | Docs | Status |
| --- | --- | --- | --- | --- | --- |
| `terminal` | A CLI session, an agent run, or a build | score `Terminal` (`show`, `hide`, `prompt`, `idle`, `type_command`, `type_at`, `print`, `print_text`, `stream`, `spin`, `resolve`, `highlight`); imp `TerminalActor` (adds `clear`, `scroll_to`) | `scenes/text-surfaces/src/lib.rs` (`build_terminal`) | [SP recipes][sp-recipes], [CX Terminal][cx-terminal], [AR text surfaces][ar-text-surfaces] | supported; `type_at`, `print_text`, `scroll_to`: no demo |
| `chat` | A Slack or iMessage thread that reacts | score `Chat` (`show`, `hide`, `stamp`, `typing`, `say`, `say_text`, `stream`, `react`, `highlight`); imp `ChatActor`; `ChatPlan.style(ChatStyle::Bubbles)` | `scenes/text-surfaces/src/lib.rs` (`build_slack`, `build_messages`) | [CX Chat Thread][cx-chat] | supported |
| `changed-files` | A pull request's file list with a diffstat | score `ChangedFiles` (`show`, `hide`, `reveal`, `reveal_row`, `highlight`, `focus`, `unfocus`); imp `ChangedFilesActor` (adds `scroll_to`) | `scenes/text-surfaces/src/lib.rs` (`build_changed_files`) | [CX Changed Files][cx-changed-files] | supported; export-only when the totals roll; `reveal_row`, `scroll_to`: no demo |
| `lower-third` | The name and role of a speaker or subject | score `LowerThird` (`show`, `hide`); imp `LowerThirdActor` | `scenes/text-surfaces/src/lib.rs` (`build_terminal`) | [CX Lower Third][cx-lower-third] | supported |

## Media, footage, and lip sync

| Feature | Use it for | Author with | Example | Docs | Status |
| --- | --- | --- | --- | --- | --- |
| Narration (`psychopomp-media`) | Speech, dialogue, sound effects, and derived audio, declared in the program and generated once | `Media::open(root)`, `media.say`, `media.dialogue`, `media.sfx`, `audio.derive(Effect::…)`, `media.finish()`; `Reading::new(lead, [(audio.clip(), gap)])`; `audio.place`, `.play`, `.beat` | `scenes/generated-media`; `scenes/psychopomp-intro` | [SP media][sp-media], [CX Generated Resource][cx-generated-resource] | supported (preferred) |
| Narration (`scripts/narrate.ts`) | Older Whisper-timed clips with a manifest | `bun scripts/narrate.ts <script.json> [--draft]`; `Narration::load(dir)`, `narration.reading(lead, [(id, gap)])` | `scenes/pr-walkthrough` | [SP reel][sp-reel] | supported |
| Phrase timing | Keying beats to spoken words | `Spoken::at`, `at_any`, `at_after`, `at_every`, `words`, `start`, `end`; `NarrationClip::split` with `place_range` across segments | `flagship.rs` (`at_any`); `scenes/generated-media` (`at_every`); `scenes/pr-50231/src/permissions.rs` (`split`) | [CX Transcript][cx-transcript] | supported |
| Sound effects | Catalog one-shots as Layer Clips | `sfx::TICK`, `SEND`, `IMPACT`, and the rest of the 17: `.play(scene, id, at, gain_db)`, `.beat(id, gain_db)` | `flagship.rs` | [SP score][sp-score] (sfx bullet) | supported; catalog paths assume the plan is written in `scenes/<name>/` |
| `subtitles` | Burned-in, word-timed captions | score `Subtitles` (`show`, `hide`); imp `SubtitlesActor`; `SubtitlesPlan::from_spoken`, `.spoken`, `.word_by_word`, `.face` | `scenes/viz-components/src/lib.rs` (`subtitles`); word by word: `scenes/balls-with-dots` | [CX Subtitles][cx-subtitles] | supported; export-only |
| `image` | A PNG, JPEG, or WebP, bare or framed, that can be pinned | score `Image` (`fly_in`, `hide`, `move_to`); imp `ImageActor`; `image::media(…)` | `scenes/anchors/src/lib.rs` (`build_stage`) | [CX Image][cx-image] | supported; export-only; `move_to`: no demo |
| `video` | A screen recording in a card with a focus window | score `Video` (`fly_in`, `focus`, `unfocus`, `hide`); imp `VideoActor`; `video::media(…)` | `scenes/video` | [CX Video Card][cx-video-card] | supported; export-only |
| `footage` | Stills, clips, and sequences; collages; freezes, ramps, and stutters | no score handle; imp `FootageActor` (`fly_in`, `toss_in`, `glide`, `to`, `move_to`, `focus`, `unfocus`, `drift`, `treat`, `hide`, `freeze`, `play`, `ramp`, `retime`, `stutter`, `audio`); `FootagePlan`, `Clip`; `footage::layout::{grid, masonry, scatter, pile, filmstrip, by_distance}` | `scenes/footage/src/lib.rs` (`build_wall`, `build_pile`) | [CX Footage][cx-footage], [AR Footage][ar-footage] | supported; export-only; `retime`, `move_to`, `layout::scatter`, `Fit::Contain`: no demo |
| Playheads | Freezing, ramping, stuttering, or scrubbing a clip, on the Stage or as an overlay | `StageActor::footage_playhead(element, &media)` or `FootageActor::playhead()` → `Playhead` (`freeze`, `play`, `ramp`, `retime`, `seek`, `stutter`) | `scenes/footage/src/lib.rs` (`build_stage`); `seek` on an overlay: `scenes/ios-build-111/src/main.rs` | [SP recipes][sp-recipes] (`footage`, Stage `footage`) | supported; export-only; `seek` is outside verify |
| Lip sync | Portraits whose mouths and blinks follow narration | `lipsync::{SpriteSheet, Sprite, visemes, blinks, frames}`; `Sprite::perform` | `scenes/high-council/src/main.rs` | [CX Sprite Sheet][cx-sprite-sheet] | supported; `verify` renders its frames only once `output/high-council/sprites` exists |

## Reel transitions

`ReelPlan::new(id, segments)` validates the reel. The first segment needs a
0 ns transition, so start with `cut`. A transition must fit the time each
neighbour is alone on screen. Docs: [SP transitions][sp-transitions],
[SP reel][sp-reel], [CX Reel][cx-reel]. Motion and audio rules are in
[TECHNIQUES.md](../explainer-motion/TECHNIQUES.md#reel-transitions).

| Style | `ReelSegmentPlan` constructor | Reach for it when | Example | Status |
| --- | --- | --- | --- | --- |
| cut | `cut(plan)` | The first segment, or a hard change | `scenes/transitions/src/lib.rs` | supported |
| crossfade, dip | `crossfaded(plan, ns)`, `dipped(plan, ns)`, `ReelPlan::dipped(id, plans, ns)` | Use a dip between text-dense frames and never cross-fade two editors | transitions; `scenes/pr-walkthrough` (dips) | supported |
| zoom | `zoomed(plan, ns, focus)` | A Stage card opens into its detail or code; `focus` is in the outgoing frame | `flagship.rs` (`client_rect`); transitions (`query`) | supported |
| zoom-out | `zoomed_out(plan, ns, focus)` | Coming back out of that detail; `focus` is in the incoming frame | transitions (`pulled-back`) | supported (showroom only) |
| wipe | `wiped(plan, ns, ReelWipePlan::new(direction).hold(…).labeled(…))` | Spatial before and after | `scenes/compare` | supported |
| push, slide, whip | `pushed`, `slid`, `whipped(plan, ns, direction)` | Frames that sit side by side in one space | transitions | supported |
| iris, ink | `irised(plan, ns, ring)`, `inked(plan, ns)`, `.focused(rect)` | Opening from a point; section breaks | transitions | supported |
| match, match-round | `matched(plan, ns, from, to)`, `matched_round(…)` | An element becomes its counterpart (round for dots and balls) | transitions (`detail`); round: `scenes/balls-with-dots/src/lib.rs` | supported |
| flip, cube | `flipped`, `cubed(plan, ns, direction)` | Flip: a before and after of the same thing. Cube: two frames as faces of one turning cube | transitions | supported |
| glitch, flash, light leak | `glitched`, `flashed`, `leaked(plan, ns)` | Failure (glitch) or section breaks; keep glitches and flashes 0.4 to 0.7 s | transitions | supported |
| J-cut, L-cut | `j_cut(plan, lead)`, `l_cut(plan, tail)` | Speech that bridges the picture cut | none | no demo |

## Interactive presentations

| Feature | Use it for | Author with | Example | Docs | Status |
| --- | --- | --- | --- | --- | --- |
| Presentation Steps | Navigable held destinations in one plan | `scene.presentation_step(id, title, start, hold)`; `scene.steps(prefix, titles, beat, settle)`; `scene.cue_steps(beat)` | `scenes/diagnostics` (five steps); `scenes/effect-succeed-slides` | [SP present][sp-present], [CX Presentation Step][cx-presentation-step] | supported (`verify` checks the plan and frames, not live navigation) |
| Deck | Slides, each a plan with its own steps | `DeckPlan { slides: vec![SlidePlan { title, plan }] }`, `deck.write_with_slides(path)`; `plan present <deck>` | `scenes/interactive-showcase`; `scenes/data-modeling` | [SP deck][sp-deck], [CX Playback][cx-playback] | outside verify |
| Native admission | Checking whether `plan present` will take a plan | — | — | [SP present][sp-present] | Rejects State Channels, any media (narration, sound, video, images, footage), Rolling Numbers, Changed Files whose totals roll, and Subtitles; needs at least one step. No deck uses a Stage root |
| `effect-task` | An Effect computation that is idle, running, succeeds, fails, dies, or retries | `TaskRecipePlan { name, center, initial, events }` with `TaskState`, through `scene.actor(id, TASK_RECIPE, …)` | `scenes/interactive-showcase/src/lib.rs` | [CX Task][cx-task] | outside verify |
| `value-token` | One labeled value or case in a finite teaching diagram | `ValueTokenPlan { label, detail, center, size, font_size, accent }` through `scene.actor(id, VALUE_TOKEN_RECIPE, …)`; `x`, `y`, `opacity`, and `emphasis` channels | `scenes/data-modeling/src/stage.rs` | [CX Value Token][cx-value-token], [README][data-readme] | outside verify |
| State Channels | A discrete value that changes, such as a title's subtitle or a text's content | `scene.state(&actor, property, value)`, `scene.change(&channel, at, value)`; only `title-card` `subtitle` and `text` `content` read them | `scenes/agent-demo` | [CX State Channel][cx-state-channel] | outside verify; export-only |

## Provisional recipes

These are overlays whose payloads live in `psychopomp::component_prototype`.
Declare one with `scene.actor(id, component_prototype::TYPESET, &plan)`. The other
constants are `WIDTH_TEXT`, `COLLECTION`, `CONNECTOR`, `RICH_TEXT`, `HEADER`, and
`VENN`. They await an aesthetic verdict and are not a public API commitment, so
call them provisional even where a scene uses them. Only `prototype-header` runs
in a `verify` scene. Limits, channels, and controls are in the showroom's
[README][proto-readme] and in [SP recipes][sp-recipes].

| Recipe | Payload | Use it for | Example |
| --- | --- | --- | --- |
| `prototype-typeset` | `TypesetPlan` | Large proportional text with stable inline parts | `scenes/component-prototypes/src/lib.rs` (`build_deck`); `scenes/keyed-grid/src/styles.rs` |
| `prototype-collection` | `CollectionPlan` | Typographic items that are inserted, arranged, and removed | `scenes/component-prototypes/src/lib.rs` |
| `prototype-connector` | `ConnectorPlan` | Bezier ink between items of a Typeset or Collection, not Semantic Targets | `scenes/component-prototypes/src/lib.rs` |
| `prototype-width-text` | `TypesetPlan` | Width-revealing type expressions | `scenes/component-prototypes/src/slideshow.rs` |
| `prototype-rich-text` | `RichTextPlan` | Bounded Markdown, without tables, math, HTML, or images | `scenes/component-prototypes/src/slideshow.rs` |
| `prototype-header` | `HeaderPlan` | Header entrances with word stagger and reflection | `scenes/component-prototypes/src/slideshow.rs`; `scenes/balls-with-dots/src/ball.rs` (in verify) |
| `prototype-venn` | `VennPlan` | Two sets whose hatching follows their sampled geometry | `scenes/component-prototypes/src/slideshow.rs` |

## Coverage

These items were checked against the code and `scenes/` when the catalogue was
last updated:

- [x] All 38 recipe names preflight accepts have a row: 4 roots, 4 editor
  attachments, 23 overlays, and 7 `prototype-*` recipes
  (`plan_runtime/preflight.rs` `Plan::new`, `plan_runtime/ide.rs`, and
  `plan_runtime/viz.rs` `accepts`).
- [x] All 13 `StageElement` kinds and every public `StageActor` beat.
- [x] Every `CameraRig` method and `Move`. The score `Camera` lacks `crash`,
  `move_to`, `framing`, `screen_box`, and `sized`.
- [x] All 19 `ReelTransitionStyle`s, plus `cut`.
- [x] All 25 score handles appear beside their imperative actors. Families
  without a score handle are marked.
- [x] Each example names an existing scene and function. *supported* means that
  scene is in `verify.json`.
- [ ] Not catalogued here: per-recipe channels and payload fields ([SP recipes][sp-recipes]),
  score combinators and timing tokens ([SP score][sp-score]), `psychopomp::math`,
  themes, and the `plan serve` protocol.

**No demo.** No Scene Program calls these, so do not cite one as an example:
`CameraRig::crash` and `sized`; `StageActor::lower` (and score `lower`); `j_cut` and `l_cut`;
form `helix` and `knot`; path `curve: "bezier"`; `retime` on a footage actor or
`Playhead`; `footage::layout::scatter`; `Fit::Contain`; `move_to` on text, images,
Rolling Numbers, and footage; `TextActor::show_during`;
`TerminalActor::type_at`, `print_text`, and `scroll_to`;
`ChangedFilesActor::reveal_row` and `scroll_to`; `ChecklistActor::reveal_item`;
`BarsActor::reveal_rows`; `TreeActor::scroll_to`; `SequenceActor::participant`;
`LensPlan::frost`; and the authoring helpers `chain!`, `seq`, `hold_for`,
`play_cue`, `spring_channel`, and `Feel::*` from `psychopomp::score`, and
`row`, `column`, and `spread_x` from `psychopomp::layout`.

## Keeping this catalogue current

When you add or change a recipe, a Stage element or channel, a beat, a camera
shot, or a transition:

1. Add or update its row with its handles (score and imp), a working example, its
   doc section, and its status.
2. Prove a demo before you claim one. Use it in a scene listed in `verify.json`
   for *supported*. Otherwise mark it *outside verify* or *no demo*, and add it to
   the No demo list.
3. Keep *provisional* until a reviewer accepts the look. Passing tests do not
   change the status.
4. A new Stage channel also needs its rest value in the defaults table
   ([AGENTS.md][agents]).
5. Re-run the searches that this page was checked with. In order, they list the
   recipe-name constants (the literal `"title-card"` is the one recipe they
   miss), the `StageActor` and `CameraRig` methods, the transition
   constructors, and the score handle types. The last one finds the scenes that
   call a method:

```sh
grep -rhoE 'pub const [A-Z_]+: &str = "[a-z-]+"' crates/psychopomp/src
awk '/^impl StageActor/,/^}/' crates/psychopomp/src/stage.rs | grep -o 'pub fn [a-z_]*'
awk '/^impl CameraRig/,/^}/' crates/psychopomp/src/stage/camera.rs | grep -o 'pub fn [a-z_]*'
awk '/^impl ReelSegmentPlan/,/^}/' crates/psychopomp/src/plan.rs | grep -o 'pub fn [a-z_]*'
grep -o '^pub struct [A-Za-z]*' crates/psychopomp/src/score.rs
grep -rln --include='*.rs' 'zoomed_out(' scenes/
```

[verify]: https://github.com/kitlangton/psychopomp/blob/main/verify.json
[agents]: https://github.com/kitlangton/psychopomp/blob/main/AGENTS.md
[sp-recipes]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#recipe-payloads-and-channels
[sp-score]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#author-with-the-score-dsl
[sp-handles]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#author-with-typed-handles
[sp-anchors]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#pin-overlays-to-anchors
[sp-transitions]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#transitions
[sp-reel]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#make-a-narrated-explainer-reel
[sp-media]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#declare-narration-and-sound
[sp-present]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#play-as-a-presentation
[sp-deck]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#present-a-deck
[sp-grid]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#present-a-growing-3d-grid
[sp-mask]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#mask-rolling-text
[sp-stability]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#inspect-maximum-stability
[sp-snapshots]: https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#schedule-several-line-order-changes
[cx]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md
[cx-stage]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#stage
[cx-stage-camera]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#stage-camera
[cx-keyed-grid]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#keyed-grid
[cx-stepped-diff]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#stepped-diff
[cx-line-mark]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#line-mark
[cx-inline-reveal]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#inline-reveal
[cx-semantic-target]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#semantic-target
[cx-scalar-plan]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#scalar-plan
[cx-diagnostic]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#diagnostic
[cx-hover-card]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#hover-card
[cx-cursor]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#cursor
[cx-inlay-hint]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#inlay-hint
[cx-pointer]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#pointer
[cx-anchor]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#anchor
[cx-caption]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#caption
[cx-callout]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#callout
[cx-rolling-number]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#rolling-number
[cx-lens]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#lens
[cx-sequence]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#sequence-diagram
[cx-tree]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#tree
[cx-plot]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#plot
[cx-lanes]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#lanes
[cx-readout]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#readout
[cx-checklist]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#checklist
[cx-meter]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#meter
[cx-bars]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#benchmark-bars
[cx-confetti]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#confetti
[cx-terminal]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#terminal
[cx-chat]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#chat-thread
[cx-changed-files]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#changed-files
[cx-lower-third]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#lower-third
[cx-generated-resource]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#generated-resource
[cx-transcript]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#transcript
[cx-subtitles]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#subtitles
[cx-image]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#image
[cx-video-card]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#video-card
[cx-footage]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#footage
[cx-sprite-sheet]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#sprite-sheet
[cx-reel]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#reel
[cx-presentation-step]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#presentation-step
[cx-playback]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#playback
[cx-task]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#task
[cx-value-token]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#value-token
[cx-state-channel]: https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md#state-channel
[ar-stage]: https://github.com/kitlangton/psychopomp/blob/main/ARCHITECTURE.md#stage
[ar-stage-camera]: https://github.com/kitlangton/psychopomp/blob/main/ARCHITECTURE.md#the-stage-camera
[ar-anchors]: https://github.com/kitlangton/psychopomp/blob/main/ARCHITECTURE.md#anchors
[ar-lenses]: https://github.com/kitlangton/psychopomp/blob/main/ARCHITECTURE.md#lenses
[ar-ide]: https://github.com/kitlangton/psychopomp/blob/main/ARCHITECTURE.md#ide-annotations
[ar-highlight]: https://github.com/kitlangton/psychopomp/blob/main/ARCHITECTURE.md#math
[ar-text-surfaces]: https://github.com/kitlangton/psychopomp/blob/main/ARCHITECTURE.md#text-surfaces
[ar-footage]: https://github.com/kitlangton/psychopomp/blob/main/ARCHITECTURE.md#footage
[fx-lightning]: https://github.com/kitlangton/psychopomp/blob/main/EFFECTS.md#lightning
[fx-dissolve]: https://github.com/kitlangton/psychopomp/blob/main/EFFECTS.md#dissolve-scan-and-shield
[proto-readme]: https://github.com/kitlangton/psychopomp/blob/main/scenes/component-prototypes/README.md
[grid-readme]: https://github.com/kitlangton/psychopomp/blob/main/scenes/keyed-grid/README.md
[data-readme]: https://github.com/kitlangton/psychopomp/blob/main/scenes/data-modeling/README.md
