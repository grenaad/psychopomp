# Techniques

Sizes in the source diagram recipes are diagram pixels on 44 px cards; multiply
by about 1.4 for 110 to 124 px cards in a 1080p frame. Psychopomp implements
these in [`crates/psychopomp/src/stage.rs`](https://github.com/kitlangton/psychopomp/blob/main/crates/psychopomp/src/stage.rs) (`packet`, `StageActor`) and
[`crates/psychopomp-render/src/render/stage.rs`](https://github.com/kitlangton/psychopomp/blob/main/crates/psychopomp-render/src/render/stage.rs). The larger HDR Stage uses the
restrained calibration immediately below.

## Current Stage calibration

- **Packet travel:** `smootherstep(t) = 6t⁵ − 15t⁴ + 10t³`; peak speed is 1.875
  times the average, versus cubic in-out's 3. Velocity and acceleration meet the
  hold continuously. Invert it monotonically to compute trail crossing times.
  Use `StageActor::glide` (or a `Move::Glide` camera shot) between resting
  compositions; use spring retargeting when an interruption must retain momentum.
- **Camera:** zero-bounce springs (`Move::Spring`), usually 1.0–1.8 seconds
  (opening dolly 2.2–2.6). Retain quintic glides for precisely timed resting
  moves; compare the same framing before choosing the feel. Write shots with
  the `CameraRig` (`stage.camera()`): `establish` (start 300–400 px back),
  `frame` with 140–170 px padding for a group (about 430 for one card held
  near 2×), `follow` a packet on a 0.6–0.8 s catch, `focus_on` with
  `aperture` 0.4–1.0, `orbit` 0.3–0.6 rad on a 3+ s glide, `dolly_zoom` about
  500 px on a 2.4 s glide at an impact (give it near and far depth cues to
  stretch), a 0.6 s `whip`, `handheld` near 1 only on quiet beats, and a
  slow glide `push_in` or tight `frame` on the resolution.
- **Orb contact:** intersect the packet path with the visible shell, invert its
  travel curve for contact age, then send a geodesic wave at 3.6 rad/s. A 25 ms
  attack and 480 ms exponential decay drive local light; a short 120 ms contact
  dimple gives it weight. A hemisphere-masked continuous trace connects the lit
  particles. Ports and the overall outline remain stable.
- **Rigid panels:** scale 1.035 → 1, spring duration 0.6 / bounce 0.12; y 16 → 0,
  duration 0.55 / bounce 0.16. Opacity 180 ms, blur 3 → 0 over 300 ms. Content
  follows after 65 ms on its own 0.36 / zero-bounce spring. No entrance flash.
  (`StageActor::settle_in`; `SpringPlan::PANEL` and `CONTENT` name these feels.)
- **Contact:** surge peak 0.45; cable bow 10 px, builds over 90 ms then settles
  on a 0.42 / bounce 0.28 spring. Brief flow establishes the connection, then
  returns to stillness.
- **Connections:** no default frame sweep. Fixed-size sockets resolve over 300 ms
  with smootherstep, then the wire draws on `cubic-bezier(.45, 0, .2, 1)` and
  holds quietly. Circular targets need an opaque silhouette: extend the wire
  beneath it, depth-sort packets behind it, and fade packet labels before entry.
  Measured packet labels stop short of connected bodies (`fit_between_ports`).
- **Hero entrance:** scale 0.58 → 1 on a 0.85 s / 0.2-bounce spring, blur 11 → 0
  on its own 0.7 s zero-bounce spring, angular offset −1.8 → 0 radians over
  1.25 s cubic-out (`StageActor::orb_in(.., OrbEntrance::HERO)`). Never animate
  a multiplier of absolute time for spin.
- **Material:** card flashes lift ink and rim without tinting the substrate.
  An overhead key gives the neutral rim depth; local packet reflections and
  socket floods supply color. Orb pulses only change illumination, never radius.
- **Staging:** finer, slower orb particles; small camera excursions; no standing
  neon halos. Reduce the per-primitive light sources before reducing global bloom:
  turning down bloom alone does not remove emissive halos authored into primitives.

## Hype register

For deliberately over-the-top films (see [`scenes/psychopomp-intro`](https://github.com/kitlangton/psychopomp/tree/main/scenes/psychopomp-intro)), keep the
quiet rules for the sweet beats and let the loud ones escalate:

- **Build:** ramp `camera.quake` linearly between shouted words (0.45 → 1.0 →
  1.5 → 2.0) under a riser that ends on the peak word; jolts still add on top.
- **Slam:** shouted text scales 1.7 → 1 on a 0.3 s / 0.3-bounce spring with a
  jolt, a 0.14 `post.zoom` streak, chroma, and a short 0.4 `post.flash`.
- **Cut to silence:** a full `post.flash` on the peak hides setting the quake to
  0 and fading everything but the hero; the sweet line lands in the stillness.

## Procedural destruction

Research foundations: [Quilez, domain warping](https://iquilezles.org/articles/warp/)
for organic density, and [GPU Gems 3, chapter 30](https://developer.nvidia.com/gpugems/gpugems3/part-v-physics-simulation/chapter-30-real-time-simulation-and-rendering-3d-fluids)
for volume rendering. Psychopomp uses an analytic procedural appearance rather
than the chapter's frame-integrated fluid simulation, preserving arbitrary-time
sampling and rewind.

- **One age:** −1 intact; 0 begins; 5.2 seconds gone. Compress the shell for
  120 ms, then release it decisively. Reverse age for the rewind.
  Preserve the intact material and occlusion at age zero. A 55 ms ignition
  handoff avoids switching every particle's size and brightness on one frame.
- **Volume:** raymarch a bounded expanding, rising density field. Low-frequency
  lobes establish silhouette; domain-warped four-octave noise sculpts folds and
  fissures. Front-to-back Beer-Lambert absorption makes cool smoke obscure the
  hot interior. Fixed quadrature avoids temporal noise crawl.
- **Bounds:** give density compact support before ray/culling bounds, or the volume
  develops straight-cut lobes. Keep the quad large enough for late smoke lift.
- **Temperature:** a brief white-yellow ignition, orange fissures, red embers,
  then nonemissive smoke. Vary heat spatially: a uniformly emissive sphere loses
  structure under highlight rolloff. Keep dark folds between bright regions.
  Cast local reflections on nearby rims; far edges and card bodies remain dark.
- **Pressure:** an inward screen-space pinch, then an expanding bipolar radial
  displacement of the existing image. Add very little ring emission; a large
  bright donut competes with the combustion.
- **Debris:** closed-form constant gravity plus linear drag; distribute speed,
  cooling, and lifetime with stable seeds. Short trails on a subset of embers
  communicate speed while most remain small points. Never accumulate positions
  from previous frames.
- **Review:** inspect 40 ms contact strips, fire-to-smoke transition, late fade,
  and reverse reconstruction. Verify labels after JSON round-trip too: centered
  text that silently reloads left-aligned destroys visual grouping.

## Beat families

Read each family as **trigger → motion → contact → settle**: what causes the
beat, what moves, what receives it, and how it comes to rest. The bullets are
labelled by source:

- **Built in** is engine behavior. Change it in Psychopomp, not in a scene.
- **Recommended** is house practice taken from the cited scenes. Retune it by eye
  instead of copying its numbers.

Every beat here writes clocks or eased channels, so any frame samples in any
order, and easing a clock backward reverses its effect (`materialize`,
`unburst`). Handles and status are in the `psychopomp` skill's
[FEATURES.md](../psychopomp/FEATURES.md).

### Path relay

- **Trigger → motion → contact → settle:** a path that has finished drawing on
  carries one packet. `connect` draws a path without a port pop.
  `relay(packet, at, seconds)` sends the packet through every stop; each stop
  lights as the packet lands, and the trail cools behind it on the packet's own
  clock.
- **Built in:** each leg is a whole packet life: a 0.34 s gather, a minimum-jerk
  flight, and a 0.72 s landing ring. The legs share `seconds` equally. The next
  leg gathers on the stop's far side `packet::RELAY` (0.12 s) after the previous
  one lands, and every element reached takes `land`. `relay` returns each arrival;
  the score span ends at the last. A packet can be sent again only after
  `packet::lifetime` has passed.
- **Recommended:** let each stop's built-in `land` be its reaction, and key what
  follows to the returned arrival. Carry the whole route on one packet element so
  its identity survives the stops.
- **Reference:** `scenes/stage-forms/src/main.rs`: the write relays 0.5 s after
  its arrow makes contact, and the store morphs 0.3 s after the final arrival.

### Particle morph

- **Trigger → motion:** an arrival or a spoken change.
  `morph(form, at, index, seconds)` eases the fractional `morph` channel to a
  whole shape index on smootherstep, and returns when every point has landed.
- **Built in:** point `i` is the same particle in every shape. Each point leaves
  at a seed-staggered moment, bows slightly outward, and lands exactly on the next
  shape. The channel blends neighbouring shapes, so easing from 0 to 2 passes
  through shape 1. Wires attach to the sampled, turned silhouette. `land` pulses a
  form, which takes no surface ripple. `solid` 0 makes a Hollow form that hides
  nothing.
- **Recommended:** order the shapes as the story visits them, and let one motion
  read at a time: tumble (eased `pitch` or `roll`) only after the morph lands.
- **Reference:** `scenes/stage-forms/src/main.rs`: plane, cube, then sphere, with
  the tumble 0.2 s after the cube forms.

### Status, spinners, and timers

- **Trigger:** the event that changes the state, such as an arrival, a verdict,
  or a phrase.
- **Built in:**
  - A card's `status` channel is a fractional index, so easing it shows every
    entry in between. `swap_status(card, at, [from, to], s)` cross-fades straight
    from one to the other. `swap_labels([from, to], at, gap)` fades one label out
    over 0.15 s and fades the other in `gap` later.
  - `clock("<card>.spinner", at)` starts a critically damped spinner motor, and
    `clock("<card>.release", at)` lets it coast to rest.
    `resolve_spinner(card, started, done)` starts the mark at the motor's next
    top-right handoff after `done`. The mark draws in 0.46 s, and the beat returns
    when it is drawn. Its flash then cools over 1.3 s (`effects::spinner`).
  - Checklist items and terminal tasks reuse the same spinner.
    `ChecklistActor::resolve` waits for the handoff, `skip` lets the motor coast,
    and `start` on a resolved item is a retry.
  - `ring_timer(ring, at, s, sweep)` fades the ring in over 0.3 s and eases its
    sweep on smootherstep. `MeterActor::countdown` sweeps a closed ring linearly
    to zero under a Readout, and flashes at each threshold and at zero.
- **Recommended:**
  - Change a status on its cause. The effects showroom flips `build` 0.3 s after
    the zap's contact.
  - Resolve a spinner with `resolve_spinner`, never by setting `mark` at the
    phrase, and put the mark's sound on the returned time.
  - Show one readable label at a time. `swap_labels` with a gap does this; the
    flagship uses 250 ms.
  - When the ring stands for a clock that viewers read, use the meter countdown.
    `ring_timer` eases in and out, so it reads as a sweep that settles.
  - Before a replay, set `spinner` and `release` back to -1 and start the motor
    again.
- **Reference:** `scenes/pr-walkthrough/src/flagship.rs` (`stage_film`);
  `scenes/viz-components/src/lib.rs` (`checklist`, `meters`);
  `scenes/opencode-jr-architecture/src/session.rs` (`ring_timer`).

### Charge, lightning, shields, and hum

- **Trigger → motion → contact:** charge gathers on the source, the bolt is the
  path, and the receiver absorbs the hit.
- **Built in:**
  - `charge(element, at, intensity, s)` eases a card's or orb's crackle (0 to
    1.5) on a minimum-jerk curve and lights the rim it crawls along. A duration
    of zero applies it at once.
  - `zap(bolt, at)` rolls a fresh seed and runs a 75 ms stepped leader, then 1 to
    8 strokes (3 by default) at seeded 45 to 110 ms intervals. It returns the
    first stroke's contact. Strokes flash both contacts, throw sparks, and flood
    the receiver. Each stroke's light reaches zero 0.6 s after it. A struck orb
    takes a surface impact and a struck shield ripples; the clock runs on until
    those reactions finish.
  - `raise` and `lower(shield, at, s)` switch the shield's cells on or off in
    seeded order, and raising first declares the shield down. Packets crossing
    into a shield ripple it.
  - `hum(bolt, at, intensity, s)` re-strikes 24 times a second; an intensity of 0
    stops it.
- **Recommended:**
  - Build the charge before the strike. The showroom ramps it over 1.4 s and
    strikes just after it peaks.
  - At the returned contact, discharge the source with `charge(.., 0.0, 0.0)`
    and put `land` or `jolt` on the receiver.
  - Raise a shield before the attack, never on the hit, and let its ripple be the
    contact.
  - Treat hum as sustained traffic: pair it with a low charge on the receiver,
    then ease both to 0 so the frame goes quiet again.
- **Reference:** `scenes/effects-showroom/src/lib.rs` (`strike`, `shield`,
  `hum`).

### Dissolve, materialize, and scan

- **Trigger → contact:** a replacement forms where the old card burned away. A
  scan verifies it, and the scan's end is the contact.
- **Built in:**
  - `dissolve(card, at)` returns when the card is gone, after 1.1 s
    (`dissolve::BURN`). Its ash keeps drifting and is cold by 2.05 s
    (`dissolve::DURATION`).
  - `materialize(card, at, s)` holds the card as cold ash until `at`, plays the
    clock back on a decelerating curve, and returns when the card is whole.
  - `scan(card, at, s)` sweeps a line that is invisible at both ends and lights
    the rim it crosses.
- **Recommended:** declare the replacement's `dissolve` at `dissolve::DURATION`
  so it waits as ash, and materialize it after the old card is gone; the
  showroom waits 0.35 s. Flip the status and `land` as the scan finishes; the
  showroom does so 0.15 s and 0.1 s before its end.
- **Reference:** `scenes/effects-showroom/src/lib.rs` (`replace`).

### Anchored overlays and glass

- **Trigger → motion:** retarget an overlay when the thing it describes moves.
  Start a caption's `move_to` at the packet's launch (the time passed to `send`)
  so the caption travels beside the packet.
- **Built in:**
  - Anchors resolve from the root's sampled geometry at every temporal sample.
    Pinned captions, callouts, numbers, text, images, footage, and lenses
    therefore ride camera moves, jolts, line insertions, and panel zooms without
    lag. They follow position, not perspective scale, and nothing is parented.
  - `move_to(anchor)` springs every `anchor.<id>` weight on `SpringPlan::MOVE`,
    so a redirect in mid-glide keeps its velocity.
  - A lens condenses in on a 0.5 s spring with a slight overshoot and leaves on
    `EXIT`. It moves, slides, magnifies, resizes, and refocuses on `MOVE`.
  - `CaptionPlan::glass()` makes a chip refract the scene behind it.
- **Recommended:** pin an overlay instead of chasing a moving card with x and y
  writes. Glide a lens between anchors rather than hiding and re-showing it. When
  the glass would cover its subject, float it beside the subject with `slide`
  plus the opposite `focus`.
- **Reference:** `scenes/anchors/src/lib.rs` (`build_stage`: the request's
  caption travels with the packet, then redirects mid-glide);
  `scenes/loupe/src/lib.rs` (`build_editor`, `build_stage`).

### Stable code

- **Trigger → contact:** a step lands on its phrase, and annotations react once
  the room has opened.
- **Built in:**
  - A Stepped Diff keeps, adds, or removes Stable Lines step by step. Removed
    lines turn red `warning` before their step, and added lines carry an added
    Line Mark. A pure insertion or removal first holds blank rows, so moving code
    never crosses entering code.
  - Keyed snapshots move lines on a 0.45 s zero-bounce profile.
  - Inline Reveals and Inlay Hints open room inside a line while the retained
    parts keep their identity.
  - Annotations measure their Semantic Target at every sample and ride its
    motion.
- **Recommended:** follow
  [AGENTS.md "Stable code between steps"](https://github.com/kitlangton/psychopomp/blob/main/AGENTS.md#stable-code-between-steps):
  reveal only what changes; a retained part may move but never disappears and
  reappears. Move a caret or clear a diagnostic after its line has moved. In the
  diagnostics showroom the fix lands at 7.8 s, the caret moves 0.55 s later, and
  the error rides its line down before clearing 1.35 s after the fix.
- **Verify:** run `plan steps <plan>` to see changed parts and retained-line
  motion. For a deck, step forward, backward, skipped, and in a rapid
  A → B → A → C in `plan present`.
- **Reference:** `scenes/diagnostics/src/lib.rs`;
  `scenes/pr-walkthrough/src/flagship.rs` (`stable_code`).

### Camera handoffs

- **Built in:**
  - Each `CameraRig` shot starts from the pose written so far and writes only
    the channels whose destination changes.
  - `Move::Spring` keeps its velocity when the next shot interrupts it, and
    `Move::Glide` is an exact minimum-jerk move between rests.
  - `follow` blends `camera.track.<id>` weights that are resolved at every
    sample, and `release` holds where the followed thing comes to rest.
  - Jolts, `camera.quake`, and handheld sway add on top of the authored pose.
- **Recommended:**
  - Catch a packet before it launches, and hand back after it lands. The camera
    showroom follows 0.2 s before launch on a 0.7 s spring and releases 0.2 s
    after landing on a 1.0 s spring.
  - Level the lens as a whip starts. The showroom resets yaw, pitch, roll, zoom,
    and pivot on the whip's own 0.6 s glide.
  - Fade handheld sway out as a precise push-in begins.
  - Settle the camera before a reel zoom. The focus rectangle is projected from a
    resting pose, and `CameraRig::pose` ignores follows, jolts, and sway.
- **Reference:** `scenes/camera/src/main.rs`;
  `scenes/pr-walkthrough/src/flagship.rs` (the closing camera and
  `client_rect`).

### Reel transitions

- **Built in:**
  - A transition overlaps the incoming segment's first `transitionNanos` with the
    outgoing segment's last. Each segment keeps its own clock.
  - Both segments' audio plays through the overlap, summed and then limited, with
    no ducking.
  - A zoom magnifies the outgoing frame until `focus` fills its width, while the
    incoming frame grows out of `focus` as a rounded card that fades in while
    still small. The zoom curve is fixed (`ReelZoom::at`).
  - A zoom-out is the same zoom played backward, with `focus` in the incoming
    frame.
  - A J-cut starts the incoming segment `lead` early, so it is heard while its
    picture stays hidden until the cut. An L-cut cuts the picture at once while
    the outgoing segment is still heard for `tail`, and its last picture is never
    shown.
  - Transition progress joins the sample key, so transitions are motion-blurred.
- **Choose:** follow the guidance in
  [SCENE_PLANS.md "Transitions"](https://github.com/kitlangton/psychopomp/blob/main/SCENE_PLANS.md#transitions).
  Zoom into a card's detail, then zoom out back to the same composition over the
  same duration. Use a J-cut or L-cut when speech should bridge the cut.
- **Zoom-out focus:** reuse the zoom-in rectangle only when the incoming frame
  shows the card at the same projected geometry. The transitions showroom's two
  `system` frames qualify: they share one layout under the default camera.
  Otherwise, project the card from the incoming frame's opening pose, with
  `stage::Camera::at(..).project_rect` or with `CameraRig::framing` followed by
  `project_rect`. Keep the incoming frame at rest from time zero.
- **J and L cuts:** open a J-cut's segment on sound, and put its first visual beat
  at or after `lead`. In an L-cut, give the outgoing segment's last `tail` to
  narration only, with no visual beat. Across every other overlap, keep to one
  voice: end the outgoing clip before the overlap or start the incoming clip
  after it.
- **Reference:** `scenes/transitions/src/lib.rs` (`zoomed`, `zoomed_out`);
  `scenes/pr-walkthrough/src/flagship.rs` (`client_rect`). No scene uses a J-cut
  or L-cut yet.

## Source-diagram recipes

## Send

One clock per packet: `age` in seconds since dispatch, and its flight time.

| Phase | Duration | Behavior |
|---|---|---|
| Gather | 340 ms, `g = cubicOut(age / 0.34)` | Dot radius `4g`, opacity `g^1.5`. A soft disc shrinks `4 + 14(1 − g)` px onto the port at opacity `0.5 sin(πg)`. The reflection ramps up as `g^1.5`. |
| Flight | per route (0.4 to 1.6 s) | Minimum-jerk quintic by arc length. A solid near-white dot of radius 4, with no halo. |
| Landing | 720 ms, `q = time / 0.72` | See the ring. The reflection fades as `(1 − q)²`. |

- **Ring** (the dot becomes the ring): `opening = smoothstep(q / 0.24)`, fully
  open at 173 ms. Radius `2 + 2·opening + 9.1·(1 − (1 − q)²)`, stroke `4 → 1.5`,
  opacity `(1 − 0.734·opening)·(1 − q)^2.52`. A 2 px ring with a 4 px stroke
  looks exactly like the dot, so the handoff is seamless. It is a splash, not a
  shockwave.
- **Trail**: each point dims with the time since the dot crossed it:
  `heat = 0.7·(1 − since / cooling)^1.7`, with cooling 240 ms to 1.1 s.
  It is long mid-flight, short near the ends, and keeps cooling after landing.
  Stroke 1.6 px against a 1 px wire.
- **Reflection** (the border lighting up as the dot approaches): lights only the
  borders of nearby frames, never fills. Radius 80 px, strength 0.6, falloff
  through 1, 0.65, 0.16, 0 at 0, 0.3, 0.7, 1 of the radius.
- **Ember** (where it left): 2.2 s. Radius `0.1 + 0.7√t` of a 210 px size, core
  `0.6·0.26·min(t / 0.04, 1)·(1 − t)^0.9`, falling off as `(1 − r / R)^2.4`.
- **Flood** (where it arrived): 1.2 s. Travel `1 − (1 − t)^4`, width
  `0.07 + 0.6·√travel` of the card size, fade `e^(−0.9t)·(1 − smoothstep((t − 0.65) / 0.35))`,
  a Gaussian from the socket soft-capped near 5 % visible wash. The border catches
  40 % of it.

## Flashes and impacts

| Use | Curve |
|---|---|
| Name or result flash | `(1 − t / 1.2)²`, immediate attack |
| Settle-in or added row | `e^(−t / 0.234)·(1 − (t / 0.75)²)` |
| Press | `e^(−t / 0.25)·(1 − (t / 0.9)²)` |
| Inner rule, then outer border 0.14 s later | rise `sin(age / 0.18 · π/2)`, decay `(1 − (age − 0.18) / 1.1)²` |

## Pacing and motion tokens

React 0.3 s, step 0.8 s, settle 1.2 s, read 1 s. Rows ripple 0.12 s apart
(`author::stagger(rows, at, author::STAGGER, ..)`). A reply launches no earlier
than `stage::reply_after(arrival)`: its 340 ms gather waits for the request,
plus an 80 ms reaction.
Rewind: hold 0.15 s, then 1.25 s on `cubic-bezier(.65, 0, .25, 1)`, blurred by
its speed.

| Token (`SpringPlan` / `score::Feel`) | Parameters | Use |
|---|---|---|
| `PANEL` | `0.6 s`, bounce `0.12` | Rigid panel body settling into place (`StageActor::settle_in`, `window::settle_in`) |
| `CONTENT` | `0.36 s`, bounce `0.0` | Ink following its container (`author::CONTENT_LAG` = `65 ms` behind), or overlay opacity entrance |
| `ENTER` | `0.45 s`, bounce `0.0` | Spatial entrance and persistent layout room opening (caption/text rise, tree fold/scroll, code line motion, chat message slot) |
| `EXIT` | `0.24 s`, bounce `0.0` | Clean exit fade in place — simpler and faster than entry |
| `MOVE` | `0.6 s`, bounce `0.0` | Point-to-point spatial translation and anchor weight glide (`.with_thresholds(1e-5, 1e-5)`) |
| `SNAP` | `0.3 s`, bounce `0.0` | Quick state change, focus shift, or status cross-fade |
| `POP` | `0.32 s`, bounce `0.18` | Small floating UI pop (hover card, reaction pill, delta chip) |
| `CAMERA` | `1.6 s`, bounce `0.0` | Camera move with weight and a natural tail |
| `LIVELY` | `0.85 s`, bounce `0.2` | Hero orb landing with deliberate overshoot |
| `Ease::DRAW` | `cubic-bezier(0.45, 0, 0.2, 1)` | Wire, leader, accent bar, and axis draw-on (`author::DRAW_SECONDS` = `0.42 s`) |
| `Ease::GLIDE` | `smootherstep` | Minimum-jerk travel between resting holds |
