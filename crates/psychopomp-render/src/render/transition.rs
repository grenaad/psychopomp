//! Composited reel transitions: pixels for pushes, slides, whips, irises,
//! matched zooms, card flips, cubes, ink, glitches, flashes, and light leaks.
//! Each combines the outgoing and incoming frames of one temporal sample in
//! linear light; the poses come from `psychopomp::plan::transition`, so a
//! sample depends only on its time. Work is split across threads by rows and
//! is deterministic.
use psychopomp::plan::{ReelTransitionStyle, TransitionPhase};

use super::HeadlessRenderer;

mod light;
mod reveal;
mod travel;
mod turn;

/// The time between a transition frame's temporal samples. A moving layer is
/// smeared across this gap, so its samples join into one continuous streak.
const SAMPLE_GAP: f32 =
    (crate::exposure::SHUTTER_SECONDS / crate::exposure::TRANSITION_TEMPORAL_SAMPLES as f64) as f32;

/// Theme colors a transition paints with, in linear light.
#[derive(Clone, Copy)]
struct Paint {
    background: [f32; 3],
    raised: [f32; 3],
    ink: [f32; 3],
    accent: [f32; 3],
}

/// The two frames of one sample, tightly packed sRGB RGBA.
#[derive(Clone, Copy)]
struct Frames<'a> {
    outgoing: &'a [u8],
    incoming: &'a [u8],
    width: usize,
    height: usize,
}

impl HeadlessRenderer {
    /// Replace `outgoing` with the transition `phase` between it and `incoming`.
    pub(crate) fn composite_transition(
        &mut self,
        outgoing: &mut [u8],
        incoming: &[u8],
        phase: TransitionPhase,
    ) {
        let palette = self.theme.palette();
        let paint = Paint {
            background: linear_rgb(self.theme.background([1, 2, 4])),
            raised: linear_rgb(palette.raised),
            ink: linear_rgb(palette.text),
            accent: linear_rgb(palette.accent),
        };
        let frames = Frames {
            outgoing,
            incoming,
            width: self.spec.width as usize,
            height: self.spec.height as usize,
        };
        use ReelTransitionStyle::*;
        let pixels = match phase.style {
            Push(direction) => travel::travel(frames, phase, direction, travel::Kind::Push, paint),
            Slide(direction) => {
                travel::travel(frames, phase, direction, travel::Kind::Slide, paint)
            }
            Whip(direction) => travel::travel(frames, phase, direction, travel::Kind::Whip, paint),
            Iris { ring } => reveal::iris(frames, phase, ring, paint),
            Ink => reveal::ink(frames, phase, paint),
            Match(target) => turn::matched(frames, phase, target, false, paint),
            MatchRound(target) => turn::matched(frames, phase, target, true, paint),
            Flip(direction) => turn::flip(frames, phase, direction, paint),
            Cube(direction) => turn::cube(frames, phase, direction, paint),
            Glitch => light::glitch(frames, phase, paint),
            Flash => light::flash(frames, phase, paint),
            LightLeak => light::leak(frames, phase, paint),
            // Mixed by weight in the reel runtime, never composited here.
            Crossfade | Dip | Zoom | ZoomOut | Wipe | JCut | LCut => return,
        };
        outgoing.copy_from_slice(&pixels);
    }
}

struct Tables {
    to_linear: [f32; 256],
    to_srgb: Vec<u8>,
}

fn tables() -> &'static Tables {
    static TABLES: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| Tables {
        to_linear: std::array::from_fn(|value| {
            let encoded = value as f32 / 255.0;
            if encoded <= 0.04045 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            }
        }),
        to_srgb: (0..65536)
            .map(|value| {
                let linear = value as f32 / 65535.0;
                let encoded = if linear <= 0.003_130_8 {
                    linear * 12.92
                } else {
                    1.055 * linear.powf(1.0 / 2.4) - 0.055
                };
                (encoded * 255.0).round() as u8
            })
            .collect(),
    })
}

fn linear_rgb(rgb: [u8; 3]) -> [f32; 3] {
    let to_linear = &tables().to_linear;
    rgb.map(|value| to_linear[value as usize])
}

/// One sRGB RGBA pixel as linear RGB.
fn linear(pixels: &[u8], index: usize) -> [f32; 3] {
    let to_linear = &tables().to_linear;
    let at = index * 4;
    [
        to_linear[pixels[at] as usize],
        to_linear[pixels[at + 1] as usize],
        to_linear[pixels[at + 2] as usize],
    ]
}

/// Write linear RGB as an opaque sRGB pixel.
fn put(pixel: &mut [u8; 4], rgb: [f32; 3]) {
    let to_srgb = &tables().to_srgb;
    for (channel, value) in rgb.into_iter().enumerate() {
        pixel[channel] = to_srgb[(value.clamp(0.0, 1.0) * 65535.0).round() as usize];
    }
    pixel[3] = 255;
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|channel| a[channel] + (b[channel] - a[channel]) * t)
}

fn scale(rgb: [f32; 3], factor: f32) -> [f32; 3] {
    rgb.map(|value| value * factor)
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|channel| a[channel] + b[channel])
}

fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, |count| count.get().min(16))
}

/// Paint `length`-long runs of `T` in parallel: `paint(index, run)` fills the
/// run with that index. The split depends only on the machine's thread
/// count, and every run is painted the same way however it is split.
fn par_runs<T: Send>(items: &mut [T], length: usize, paint: impl Fn(usize, &mut [T]) + Sync) {
    let runs = items.len() / length.max(1);
    let per_thread = runs.div_ceil(threads()).max(1);
    std::thread::scope(|scope| {
        for (block, chunk) in items.chunks_mut(per_thread * length).enumerate() {
            let paint = &paint;
            scope.spawn(move || {
                for (offset, run) in chunk.chunks_mut(length).enumerate() {
                    paint(block * per_thread + offset, run);
                }
            });
        }
    });
}

/// A frame of `width` x `height` RGBA, painted row by row in parallel.
fn paint_rows(
    width: usize,
    height: usize,
    paint: impl Fn(usize, &mut [[u8; 4]]) + Sync,
) -> Vec<u8> {
    let mut pixels = vec![[0_u8; 4]; width * height];
    par_runs(&mut pixels, width, paint);
    pixels.into_flattened()
}

/// A frame in linear light with half-size levels below it, so a minified
/// sample averages the pixels it covers rather than aliasing.
struct Mips {
    levels: Vec<Level>,
}

struct Level {
    width: usize,
    height: usize,
    pixels: Vec<[f32; 3]>,
}

impl Mips {
    /// `levels` in all, the first at full size.
    fn new(pixels: &[u8], width: usize, height: usize, levels: usize) -> Self {
        let mut full = vec![[0.0_f32; 3]; width * height];
        par_runs(&mut full, width, |y, row| {
            for (x, pixel) in row.iter_mut().enumerate() {
                *pixel = linear(pixels, y * width + x);
            }
        });
        let mut chain = vec![Level {
            width,
            height,
            pixels: full,
        }];
        while chain.len() < levels.max(1) {
            let above = chain.last().unwrap();
            let (width, height) = (above.width.div_ceil(2), above.height.div_ceil(2));
            if above.width < 2 || above.height < 2 {
                break;
            }
            let mut pixels = vec![[0.0_f32; 3]; width * height];
            par_runs(&mut pixels, width, |y, row| {
                for (x, pixel) in row.iter_mut().enumerate() {
                    let at = |dx: usize, dy: usize| {
                        let sx = (x * 2 + dx).min(above.width - 1);
                        let sy = (y * 2 + dy).min(above.height - 1);
                        above.pixels[sy * above.width + sx]
                    };
                    let sum = add(add(at(0, 0), at(1, 0)), add(at(0, 1), at(1, 1)));
                    *pixel = scale(sum, 0.25);
                }
            });
            chain.push(Level {
                width,
                height,
                pixels,
            });
        }
        Self { levels: chain }
    }

    /// The color around the full-size point (`x`, `y`) (pixel centers at
    /// half-integers), averaged over `footprint` full-size pixels.
    fn sample(&self, x: f32, y: f32, footprint: f32) -> [f32; 3] {
        let lod = footprint
            .max(1.0)
            .log2()
            .min((self.levels.len() - 1) as f32);
        let level = lod.floor() as usize;
        let near = self.levels[level].bilinear(x, y, level);
        let blend = lod - level as f32;
        if blend <= 0.0 || level + 1 >= self.levels.len() {
            return near;
        }
        mix(
            near,
            self.levels[level + 1].bilinear(x, y, level + 1),
            blend,
        )
    }
}

impl Level {
    fn bilinear(&self, x: f32, y: f32, depth: usize) -> [f32; 3] {
        let factor = (1_u32 << depth) as f32;
        let sx = (x / factor - 0.5).clamp(0.0, (self.width - 1) as f32);
        let sy = (y / factor - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
        let at = |px: usize, py: usize| self.pixels[py * self.width + px];
        mix(
            mix(at(x0, y0), at(x1, y0), fx),
            mix(at(x0, y1), at(x1, y1), fx),
            fy,
        )
    }
}

#[cfg(test)]
mod tests {
    use psychopomp::plan::WipeDirection;

    use super::*;

    const WIDTH: usize = 96;
    const HEIGHT: usize = 54;

    fn solid(value: u8) -> Vec<u8> {
        [value, value, value, 255].repeat(WIDTH * HEIGHT)
    }

    fn paint() -> Paint {
        Paint {
            background: linear_rgb([8, 8, 7]),
            raised: linear_rgb([34, 34, 33]),
            ink: linear_rgb([226, 223, 217]),
            accent: linear_rgb([224, 179, 90]),
        }
    }

    /// Composite `style` at `progress` of a one-second transition from a
    /// dark frame (40) to a light one (200).
    fn composite(style: ReelTransitionStyle, progress: f32, focus: Option<[f32; 4]>) -> Vec<u8> {
        let (outgoing, incoming) = (solid(40), solid(200));
        let frames = Frames {
            outgoing: &outgoing,
            incoming: &incoming,
            width: WIDTH,
            height: HEIGHT,
        };
        let phase = TransitionPhase {
            style,
            progress,
            seconds: 1.0,
            focus,
        };
        use ReelTransitionStyle::*;
        match style {
            Push(direction) => {
                travel::travel(frames, phase, direction, travel::Kind::Push, paint())
            }
            Slide(direction) => {
                travel::travel(frames, phase, direction, travel::Kind::Slide, paint())
            }
            Whip(direction) => {
                travel::travel(frames, phase, direction, travel::Kind::Whip, paint())
            }
            Iris { ring } => reveal::iris(frames, phase, ring, paint()),
            Ink => reveal::ink(frames, phase, paint()),
            Match(target) => turn::matched(frames, phase, target, false, paint()),
            MatchRound(target) => turn::matched(frames, phase, target, true, paint()),
            Flip(direction) => turn::flip(frames, phase, direction, paint()),
            Cube(direction) => turn::cube(frames, phase, direction, paint()),
            Glitch => light::glitch(frames, phase, paint()),
            Flash => light::flash(frames, phase, paint()),
            LightLeak => light::leak(frames, phase, paint()),
            _ => unreachable!("not composited"),
        }
    }

    fn at(pixels: &[u8], x: usize, y: usize) -> u8 {
        pixels[(y * WIDTH + x) * 4]
    }

    #[test]
    fn a_push_carries_both_frames_by_its_travel() {
        // Halfway, a leftward push shows the outgoing frame on the left.
        let pixels = composite(ReelTransitionStyle::Push(WipeDirection::Left), 0.5, None);
        assert_eq!(at(&pixels, 4, 27), 40);
        assert_eq!(at(&pixels, 90, 27), 200);
        let pixels = composite(ReelTransitionStyle::Push(WipeDirection::Down), 0.5, None);
        assert_eq!(
            at(&pixels, 48, 50),
            40,
            "a downward push leaves the outgoing below"
        );
        assert_eq!(at(&pixels, 48, 3), 200);
    }

    #[test]
    fn every_transition_starts_on_the_outgoing_frame_and_ends_on_the_incoming() {
        use ReelTransitionStyle::*;
        let left = WipeDirection::Left;
        let card = Some([40.0, 20.0, 16.0, 10.0]);
        for (style, focus) in [
            (Push(left), None),
            (Slide(WipeDirection::Up), None),
            (Whip(WipeDirection::Right), None),
            (Iris { ring: false }, card),
            (Ink, None),
            (Match([10.0, 10.0, 60.0, 30.0]), card),
            (MatchRound([10.0, 10.0, 40.0, 40.0]), card),
            (Flip(left), None),
            (Cube(left), None),
            (Glitch, None),
            (LightLeak, None),
        ] {
            let start = composite(style, 1e-4, focus);
            let end = composite(style, 1.0 - 1e-4, focus);
            let near = |pixels: &[u8], value: u8| {
                pixels
                    .chunks(4)
                    .all(|pixel| pixel[..3].iter().all(|c| c.abs_diff(value) <= 3))
            };
            assert!(near(&start, 40), "{style:?} starts on the outgoing frame");
            if !near(&end, 200) {
                let bad = end
                    .chunks(4)
                    .enumerate()
                    .filter(|(_, pixel)| pixel[0].abs_diff(200) > 3)
                    .map(|(index, pixel)| (index % WIDTH, index / WIDTH, pixel[0]))
                    .take(8)
                    .collect::<Vec<_>>();
                panic!("{style:?} ends on the incoming frame: {bad:?}");
            }
        }
    }

    #[test]
    fn reveals_open_from_their_focus_and_ink_covers_its_share() {
        let focus = Some([8.0, 8.0, 8.0, 8.0]);
        let pixels = composite(ReelTransitionStyle::Iris { ring: false }, 0.3, focus);
        assert!(at(&pixels, 12, 12) > 190, "the iris center is open");
        assert!(at(&pixels, 95, 53) < 45, "the far corner is not");
        let pixels = composite(ReelTransitionStyle::Ink, 0.5, None);
        let inked = pixels.chunks(4).filter(|pixel| pixel[0] > 120).count();
        let share = inked as f32 / (WIDTH * HEIGHT) as f32;
        assert!(
            (share - 0.5).abs() < 0.08,
            "half the frame is inked ({share})"
        );
    }

    #[test]
    fn a_flash_whites_out_over_the_cut() {
        use psychopomp::plan::transition::FLASH_PEAK;
        let peak = composite(ReelTransitionStyle::Flash, FLASH_PEAK, None);
        assert!(peak.chunks(4).all(|pixel| pixel[0] > 230));
        let after = composite(ReelTransitionStyle::Flash, 0.9, None);
        assert!(
            at(&after, 48, 27) >= 200,
            "the incoming frame is under the decay"
        );
    }

    #[test]
    fn linear_light_round_trips_every_byte() {
        for value in 0..=255_u8 {
            let mut pixel = [0_u8; 4];
            put(&mut pixel, linear_rgb([value; 3]));
            assert_eq!(pixel, [value, value, value, 255]);
        }
    }

    #[test]
    fn parallel_runs_cover_every_index_once() {
        let mut items = vec![usize::MAX; 37 * 5];
        par_runs(&mut items, 5, |index, run| run.fill(index));
        for (index, run) in items.chunks(5).enumerate() {
            assert!(run.iter().all(|value| *value == index));
        }
    }

    #[test]
    fn mips_average_what_a_minified_sample_covers() {
        // A one-pixel checkerboard averages to mid-gray two levels down.
        let (width, height) = (8, 8);
        let mut pixels = vec![255_u8; width * height * 4];
        for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if (index % width + index / width) % 2 == 0 {
                pixel[..3].fill(0);
            }
        }
        let mips = Mips::new(&pixels, width, height, 3);
        assert_eq!(mips.levels.len(), 3);
        let sharp = mips.sample(0.5, 0.5, 1.0);
        assert_eq!(sharp, [0.0; 3]);
        let soft = mips.sample(4.0, 4.0, 4.0);
        assert!(
            soft.iter().all(|value| (value - 0.5).abs() < 1e-5),
            "{soft:?}"
        );
    }
}

#[cfg(test)]
mod gpu_tests {
    use psychopomp::plan::{ReelTransitionStyle, TransitionPhase, WipeDirection};

    use crate::render::{HeadlessRenderer, RenderSpec};

    #[test]
    #[ignore = "requires a headless GPU; a composited push replaces the outgoing frame"]
    fn a_push_composites_into_the_outgoing_frame() {
        let mut renderer = pollster::block_on(HeadlessRenderer::new(RenderSpec {
            width: 1920,
            height: 1080,
            file_name: "transition-proof".into(),
        }))
        .unwrap();
        let mut outgoing = [40_u8, 40, 40, 255].repeat(1920 * 1080);
        let incoming = [200_u8, 200, 200, 255].repeat(1920 * 1080);
        let phase = TransitionPhase {
            style: ReelTransitionStyle::Push(WipeDirection::Left),
            progress: 0.5,
            seconds: 1.0,
            focus: None,
        };
        renderer.composite_transition(&mut outgoing, &incoming, phase);
        let at = |x: usize| outgoing[(540 * 1920 + x) * 4];
        assert_eq!((at(100), at(1800)), (40, 200));
    }
}
