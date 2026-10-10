---
name: psychopomp
description: >
  Make explainer reels and deterministic motion graphics with Psychopomp:
  walkthrough videos of pull requests, code changes, protocols, or lifecycles,
  with GPU Stage choreography, callouts, rolling numbers, sequence diagrams,
  animated diffs, and optional ElevenLabs or Fish Audio narration.
---

# Psychopomp explainer reels

An **explainer reel** is several independently authored Scene Plans joined on one
clock: behavior stories on the GPU Stage (orbs, cards, beams, packets, camera,
bloom), pinned Callouts, Rolling Numbers, changes as animated Stepped Diffs,
captions in a terminal voice, and optional spoken narration. When narrated,
every visual beat waits for a phrase in the transcript, so re-voicing re-times
the film automatically.

The working examples are in [`scenes/pr-walkthrough`](https://github.com/kitlangton/psychopomp/tree/main/scenes/pr-walkthrough): [`src/flagship.rs`](https://github.com/kitlangton/psychopomp/blob/main/scenes/pr-walkthrough/src/flagship.rs) (#50825 as a
Stage film that zooms into its code), [`src/stop_stage.rs`](https://github.com/kitlangton/psychopomp/blob/main/scenes/pr-walkthrough/src/stop_stage.rs) (#50042 combining Stage,
Callouts, Rolling Number, and a Zoom into an annotated code diff), and [`src/lib.rs`](https://github.com/kitlangton/psychopomp/blob/main/scenes/pr-walkthrough/src/lib.rs)
(the five-PR reel with sequence diagrams). Copy their shape; do not start from a
blank crate. Component payloads, channels, and commands live in [`SCENE_PLANS.md`](https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#make-a-narrated-explainer-reel)
("Make A Narrated Explainer Reel") and terms in [`CONTEXT.md`](https://github.com/kitlangton/psychopomp/blob/main/CONTEXT.md). Both are long:
read "Make A Narrated Explainer Reel" before authoring, then open other sections
when a feature you use needs them. Engineering rules are in [`AGENTS.md`](https://github.com/kitlangton/psychopomp/blob/main/AGENTS.md).

## Core model

```text
Scene Program ── score Beats or *Actor calls ──▶ PlanBuilder ──▶ Scene Plan (JSON)
  ──▶ strict preflight (recipes, references, channel names)
  ──▶ channels compile to tracks ──▶ temporal samples per frame ──▶ PNG or MP4
```

- **Programs emit plans.** A Scene Program is ordinary Rust that emits a Scene
  Plan, Deck, or Reel. The plan is compiled output: change the program and run
  it again rather than editing the JSON.
- **Beats and spans.** `PlanBuilder` is the only `mut`. Score handles return
  `Beat`s, which compose with `.then`, `.also`, `.with`, `.on_end`, `.after`,
  `.early`, `stagger`, and `at!`. A beat's `Span` ends at its primary action
  (ready to wire, contact, arrival), not when its springs settle. Chain later
  beats from spans and let the settling overlap.
- **One root.** A plan has at most one root: `stage`, `editor`, `title-card`, and
  `keyed-grid` exclude one another, and preflight rejects two. The Stage is one
  of these roots, the 2.5D GPU diagram, not another word for the scene.
  Captions, callouts, charts, text surfaces, and footage are overlays that work
  over any root, or over none. Diagnostics, hover cards, cursors, and the pointer
  need an editor root.
- **Channels.** A Continuous Channel's `set`, `spring`, and `ease` events compile
  to a Property Track, and a later spring starts from the sampled position and
  velocity. State Channels are discrete values: a title card's `subtitle` and a
  text's `content`. Channel names are strict, so a typo fails preflight. Every
  Stage channel rests at one default.
- **Anchors.** A pinned overlay names a target: a point, a Stage element, an
  editor Semantic Target, or a Sequence participant or row. The renderer resolves
  it from sampled geometry at every temporal sample, and `anchor.<id>` weights
  blend between targets. Nothing is parented: a pinned overlay follows its
  target's position, not its scale.
- **Time.** Plan time is integer nanoseconds, and every frame is a pure function
  of it. Each frame averages several temporal samples, so motion blur comes from
  real movement.
- **Reel or Deck.** A Reel delivers independently authored plans on one clock.
  Each segment keeps its own local time from zero, at most two segments are
  visible at once, and a transition overlaps neighbours, sound included. A Deck,
  or one plan's Presentation Steps, is navigated live with `plan present`, which
  retargets from the sampled motion. A plan with media, State Channels, Rolling
  Numbers, or Subtitles is export-only; FEATURES.md has the full rule.

## Choose features

[FEATURES.md](FEATURES.md) maps each need to its feature. Every row gives the
score or imperative handle, a working scene to copy, the doc section with the
payload and channels, and its status: supported, outside `verify`, provisional,
or no demo. Read the rows the film needs, then open only those sections. The
`explainer-motion` skill's [TECHNIQUES.md](../explainer-motion/TECHNIQUES.md#beat-families)
walks each beat family from trigger to settle.

## Steps

1. **Establish the facts.** For each change, read the actual diff and PR description
   (`gh pr diff`, `gh pr view`) and write one sentence each for the broken behavior,
   the fixed behavior, and the change. Done when every claim you will narrate is
   traceable to code, a test, or a reproduced run; mark anything you could not verify
   and keep it out of the script.

2. **Write the script** as `scenes/<name>/narration/script.json`, one clip per part:
   intro, then per change `<slug>-before`, `<slug>-after`, `<slug>-code`, then outro.
   Follow [STORY.md](STORY.md) when writing it. Done when each clip is under about 30
   seconds spoken and every planned visual beat has a distinct anchor phrase in it.

3. **Voice it (optional).** Draft first with no credentials (`--draft` uses macOS
   `say`), or synthesize final narration with **ElevenLabs** (`engine: "elevenlabs"`,
   default model `eleven_v4`) or **Fish Audio** (`engine: "fish"`):
   ```json
   {
     "engine": "elevenlabs",
     "model": "eleven_v4",
     "voice": "<elevenlabs-voice-id>",
     "settings": { "stability": 0.5, "similarity": 0.7 },
     "clips": [
       { "id": "change-before", "text": "[warm, conversational voice] ..." }
     ]
   }
   ```
   ```sh
   # Draft (no API key required)
   bun scripts/narrate.ts scenes/<name>/narration/script.json --draft

   # ElevenLabs (requires ELEVENLABS_API_KEY and script.voice)
   ELEVENLABS_API_KEY=... bun scripts/narrate.ts scenes/<name>/narration/script.json

   # Fish Audio (requires FISH_AUDIO_API_KEY)
   FISH_AUDIO_API_KEY=... bun scripts/narrate.ts scenes/<name>/narration/script.json
   ```
   [`scripts/narrate.ts`](https://github.com/kitlangton/psychopomp/blob/main/scripts/narrate.ts) normalizes loudness, transcribes word timings with Whisper,
   and writes `<id>.mp3`, `<id>.words.json`, and `narration.json`. When switching
   voices or engines, rebuild the Scene Plan against the new word timings.

   **Or declare the narration in the Scene Program** with `psychopomp-media`
   ([`SCENE_PLANS.md`](https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#declare-narration-and-sound), "Declare Narration And Sound"): `media.say(id, &voice, text)`,
   `media.dialogue`, `media.sfx`, and `audio.derive(Effect::pitch(..))`, then
   `media.finish()?`. Each line is generated once and recorded in
   `media.lock.json`; later runs call nothing unless a declaration changed, and
   ElevenLabs words come from its character alignment with the script's spelling.
   Check the cost first with `PSYCHOPOMP_MEDIA=plan cargo run -p <crate>` (zero
   API calls), time with `PSYCHOPOMP_MEDIA=draft`, and clean up with
   `PSYCHOPOMP_MEDIA=prune`. Move a narrate.ts scene with
   `cargo run -p psychopomp-media -- adopt scenes/<name>/narration` (no
   regeneration). Done when a plain run reports every resource `=`.

   **Final timing pass.** Before trusting the anchors, confirm that the word
   timings match the speech.
   - `psychopomp-media` times ElevenLabs voices from the provider's character
     alignment of the *script*. A skipped, repeated, or improvised word, or a
     direction read aloud, leaves timings that disagree with what was said.
     Whisper times `narrate.ts` clips and Fish and `say` voices from the audio
     itself.
   - Render each narrated cue with audio and check that its beats land on the
     spoken words.
   - If an ElevenLabs voice drifts, time it with `.whisper()`. That changes its
     Resource Key and regenerates the line, so preview the cost with
     `PSYCHOPOMP_MEDIA=plan`.
   - `narrate.ts` always requests `mp3_44100_192` from ElevenLabs. An account
     whose tier refuses that format needs `psychopomp-media` with
     `Voice::eleven(id).format("mp3_44100_128")`. The format is also part of the
     key.

4. **Author the Scene Program** in `scenes/<name>` (the workspace picks it up; add
   it to [`verify.json`](https://github.com/kitlangton/psychopomp/blob/main/verify.json) with a few key times).
   Use `psychopomp::score` ([`SCENE_PLANS.md`](https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#author-with-the-score-dsl), "Author With The Score DSL") so
   `PlanBuilder` is the only `mut` binding and every actor (`Stage`, `Camera`,
   `Caption`, `Callout`, `RollingNumber`, `Checklist`, `Meter`, `Bars`,
   `Subtitles`, `Confetti`, `Terminal`, `Chat`, `ChangedFiles`, `LowerThird`,
   `Lens`) is an immutable value whose methods return composable `Beat`s
   (`.then`, `.also`, `.with`, `.on_end`, `.after`, `.early`, `stagger`, `each`,
   `at!(scene, time => ...)`). Schedule clips with `Narration::reading`, build
   elements with `StageElement::card|orb|beam|packet|label|ring` and relative
   `psychopomp::layout::Placement` envelopes on `StagePost::RESTRAINED`, frame
   with `Caption::header|chip|footer`, and play sounds with `sfx::*.beat(id, gain_db)`
   (or `audio.beat(gain_db)` for `psychopomp-media`). Load the `explainer-motion`
   skill and apply it to every beat: cards `settle_in`, beams `connect`,
   messages `send`, replies wait for `.reply()`, impacts `hit`, the hero `orb_in`,
   the fix's `rewind`; use `Callout` to pin annotations to Stage elements or
   Editor code ranges and `RollingNumber` for live counters/timers; drive camera
   moves through `stage.camera()` (`establish`, `frame`, `follow`, `release`,
   `orbit`, `whip`, `dolly_zoom`). Reach for the visualization and text overlays
   instead of hand-building them from Stage labels and rings: `Checklist` for
   checks that run and resolve, `Meter` (`MeterPlan::countdown`) for a timeout
   ring, `Bars` for before/after numbers, `Confetti` for a success beat, and
   `Subtitles` (`SubtitlesPlan::from_spoken`) to burn in word-timed captions.
   Changes are `Diff`s of `keep`, `add(step)`, and `remove(step)` lines, each step
   keyed to a phrase. Done when `cargo run -p <crate>` writes the reel and
   `cargo run --release -- plan validate <reel>` reports valid. A missing phrase panics
   with its clip: change the anchor to words the transcript actually contains.

5. **Review before rendering.** Get segment spans from `plan inspect <reel>`, then
   contact-sheet each segment at its before, switch, after, and code beats:
   ```sh
   bun scripts/sheet.ts <reel> t1,t2,... --theme neutral --shutter --out output/sheet.jpg
   ```
   When tuning the look, set `PSYCHOPOMP_SHADER_DIR=crates/psychopomp-render/src/render`
   and edit [`stage.wgsl`](https://github.com/kitlangton/psychopomp/blob/main/crates/psychopomp-render/src/render/stage.wgsl)/[`stage_post.wgsl`](https://github.com/kitlangton/psychopomp/blob/main/crates/psychopomp-render/src/render/stage_post.wgsl): every frame and sheet picks up shader edits
   without a Rust rebuild.
   Then render one behavior segment with audio (`plan render <reel> out.mp4 --cue <scene-id>`)
   or a 2–3 second window (`--range a..b`) and inspect frames extracted during
   motion. While reviewing, check:
   - **Labels.** In a feature tour or showroom, a chip or caption names each
     feature while it plays, as in `scenes/transitions` and
     `scenes/effects-showroom`.
   - **Holds.** A quiet hold keeps the result on screen and readable while the
     narration lands. Avoid stretches of empty background or frames with nothing
     to read, such as a long lead, a lingering dip, or a segment that fades out
     before it ends.
   - **Text at full scale.** Sheets tile whole frames at half resolution, so read
     the text in `plan frame` PNGs or in `sheet.ts --crop x,y,w,h` tiles.
   - **Motion.** Watch normal-speed windows and contact frames 40 ms or less
     apart, as the `explainer-motion` skill's "Designing a beat" describes.
     Stills cannot prove that motion is smooth.
   - **Transitions with audio.** Render each transition window with its sound.
     Both segments' audio plays through every overlap, so two voices must never
     collide. A J-cut hides the incoming picture for its lead, and an L-cut never
     shows the outgoing segment's last `tail` of picture.

   Done when every segment has been looked at, no text overlaps, clips, or reads
   against the wrong chip, and you can say which of these checks you ran.

6. **Render and verify** the whole reel once, after the sheets and short windows
   pass. Run it in the foreground with a long timeout and never end a turn while it
   is still running. Progress goes to stderr once per second of output. Keep it
   visible and logged with `tee`, and set `pipefail` so the pipeline exits with the
   render's status:
   ```sh
   set -o pipefail
   cargo run --release -- plan render <reel> output/<name>.mp4 --theme neutral 2>&1 | tee output/<name>.render.log
   ```
   Do not pipe a render into `tail`, which shows nothing until the render ends.
   Done when `ffprobe` shows 1920x1080, 60 fps, AAC audio, and a duration matching
   `plan inspect`, and loudness is near -16 LUFS with peaks under -1 dBFS. Those
   numbers prove the track exists, not that it is right. Report audio as reviewed
   only after you have listened to, or transcribed, the transitions and anchor
   phrases. A non-zero mean level is not a review.

7. **Deliver** the MP4 path. Commit the scene, narration assets, and any engine
   changes in Psychopomp with conventional messages.

## Improving Psychopomp

When a reel needs something the engine cannot do, add it to Psychopomp rather than
working around it in one scene: plan types and validation in [`crates/psychopomp`](https://github.com/kitlangton/psychopomp/tree/main/crates/psychopomp),
strict-channel preflight in `plan_runtime`, pixels in `render`, GPU-free tests plus
an `#[ignore]` GPU test, and the docs [`AGENTS.md`](https://github.com/kitlangton/psychopomp/blob/main/AGENTS.md) asks you to keep current, including
the feature's row in [FEATURES.md](FEATURES.md) ("Keeping this catalogue current"). Run
`cargo run --release -- verify baseline` before the change and `verify compare
--expect <scenes you changed>` after it to prove every other showroom and film is
untouched.

Build from small reusable pieces. Interpolation, easing, curves, shape ports, and
connectors belong in `psychopomp::math` (organized like pmndrs `math`: core `lerp`/
`remap`/`smoothstep`, `easing`, `curve`, `shapes`, `random`; glam vectors). Extend it
instead of inlining math in a renderer or scene, and split renderers into one small
helper per element, as [`render/stage.rs`](https://github.com/kitlangton/psychopomp/blob/main/crates/psychopomp-render/src/render/stage.rs) does. Keep
`cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
and `cargo test --workspace` green.
