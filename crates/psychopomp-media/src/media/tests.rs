use std::{cell::RefCell, fs, path::Path, rc::Rc};

use anyhow::Result;
use psychopomp::author::{PlanBuilder, SECOND, seconds};

use super::*;
use crate::studio::Generated;

/// One job a fake generator was asked for: id, canonical spec, and the
/// request IDs it was stitched after.
type Call = (String, Value, Vec<String>);

#[derive(Clone, Default)]
struct Calls(Rc<RefCell<Vec<Call>>>);

impl Calls {
    fn ids(&self) -> Vec<String> {
        self.0.borrow().iter().map(|(id, ..)| id.clone()).collect()
    }

    fn clear(&self) {
        self.0.borrow_mut().clear();
    }
}

/// Writes each resource's duration as its file, so derivations can read it.
struct Fake(Calls);

impl Generator for Fake {
    fn generate(&mut self, job: &Job<'_>, out: &Path) -> Result<Generated> {
        self.0.0.borrow_mut().push((
            job.id.to_owned(),
            job.spec.canonical(),
            job.previous_requests.clone(),
        ));
        let request = vec![format!("req-{}", job.id)];
        let generated = match job.spec {
            Spec::Speech(speech) => {
                let (words, duration) = words::estimate(&speech.text());
                let paid = speech.backend != Backend::Say;
                Generated {
                    duration,
                    words,
                    requests: if paid { request } else { Vec::new() },
                    credits: paid.then_some(1),
                    api_calls: u32::from(paid),
                }
            }
            Spec::Sound(sound) => Generated {
                duration: sound.duration_nanos - 20_000_000,
                words: Vec::new(),
                requests: request,
                credits: Some(20),
                api_calls: 1,
            },
            Spec::Silence { duration_nanos, .. } => Generated {
                duration: *duration_nanos,
                words: Vec::new(),
                requests: Vec::new(),
                credits: None,
                api_calls: 0,
            },
            Spec::Derive(derive) => {
                let source = fs::read_to_string(job.input.as_ref().unwrap())?.parse()?;
                Generated {
                    duration: derive.effect.duration(source),
                    words: Vec::new(),
                    requests: Vec::new(),
                    credits: None,
                    api_calls: 0,
                }
            }
        };
        fs::write(out, generated.duration.to_string())?;
        Ok(generated)
    }
}

fn open(root: &Path, mode: Mode, calls: &Calls) -> Media {
    Media::with(root, mode, Box::new(Fake(calls.clone()))).unwrap()
}

fn open_python(root: &Path, mode: Mode, calls: &Calls) -> Media {
    let file = crate::lock::lock_file(Some("python")).unwrap();
    Media::with_lock(root, &file, mode, Box::new(Fake(calls.clone()))).unwrap()
}

const HUSH: &str = "[extremely soft ASMR whisper] Oh... I hear you like... balls.";

/// The showroom's declarations: a line, a sound, and a derived variant.
fn declare(media: &Media, hush: &str) -> (Audio, Audio, Audio) {
    let kit = Voice::eleven("kit").v4().stability(0.2);
    let hush = media.say("hush", &kit, hush).unwrap();
    let pop = media
        .sfx(
            "pop",
            "a single soft glassy pop, tiny and dry",
            seconds(0.5),
        )
        .unwrap();
    let demon = hush.derive(Effect::pitch(-6.0)).unwrap();
    (hush, pop, demon)
}

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn the_first_run_generates_and_the_second_calls_nothing() {
    let root = crate::tests::scratch("second-run");
    let calls = Calls::default();
    let media = open(&root, Mode::Apply, &calls);
    let (hush, pop, demon) = declare(&media, HUSH);
    let report = media.finish().unwrap();
    assert_eq!(report.created, ids(&["hush", "hush.pitch(-6)", "pop"]));
    assert_eq!(report.api_calls, 2, "the derivation is local");
    assert_eq!(calls.ids(), ["hush", "pop", "hush.pitch(-6)"]);
    assert_eq!(pop.duration(), 480_000_000);
    assert_eq!(demon.duration(), hush.duration());
    assert_eq!(demon.transcript().words(), hush.transcript().words());
    assert_eq!(hush.path(), Path::new(&format!("media/{}.mp3", hush.key())));
    assert_eq!(
        demon.path().extension().unwrap(),
        "mp3",
        "keeps its source's format"
    );
    let lock = fs::read_to_string(root.join("media.lock.json")).unwrap();

    calls.clear();
    let media = open(&root, Mode::Apply, &calls);
    let (again, ..) = declare(&media, HUSH);
    let report = media.finish().unwrap();
    assert!(calls.ids().is_empty());
    assert_eq!(report.unchanged, ids(&["hush", "hush.pitch(-6)", "pop"]));
    assert_eq!(report.api_calls, 0);
    assert_eq!(again.duration(), hush.duration());
    assert_eq!(again.transcript().words(), hush.transcript().words());
    assert_eq!(
        fs::read_to_string(root.join("media.lock.json")).unwrap(),
        lock,
        "an up-to-date run leaves the lock untouched"
    );
}

#[test]
fn a_changed_line_replaces_it_and_what_derives_from_it() {
    let root = crate::tests::scratch("replace");
    let calls = Calls::default();
    let media = open(&root, Mode::Apply, &calls);
    declare(&media, HUSH);
    media.finish().unwrap();

    calls.clear();
    let media = open(&root, Mode::Apply, &calls);
    declare(&media, "[soft] Oh... I hear you like... cubes.");
    let report = media.finish().unwrap();
    assert_eq!(report.replaced, ids(&["hush", "hush.pitch(-6)"]));
    assert_eq!(report.unchanged, ids(&["pop"]));
    assert_eq!(calls.ids(), ["hush", "hush.pitch(-6)"]);
    assert_eq!(report.api_calls, 1);
}

#[test]
fn undeclared_entries_are_orphans_until_pruned() {
    let root = crate::tests::scratch("prune");
    let calls = Calls::default();
    let media = open(&root, Mode::Apply, &calls);
    let (_, pop, _) = declare(&media, HUSH);
    media.finish().unwrap();
    let kit = Voice::eleven("kit").v4().stability(0.2);

    let media = open(&root, Mode::Apply, &calls);
    media.say("hush", &kit, HUSH).unwrap();
    let report = media.finish().unwrap();
    assert_eq!(report.orphans, ids(&["hush.pitch(-6)", "pop"]));
    assert!(root.join(pop.path()).exists(), "only prune deletes");

    calls.clear();
    let media = open(&root, Mode::Prune, &calls);
    media.say("hush", &kit, HUSH).unwrap();
    media.finish().unwrap();
    assert!(calls.ids().is_empty());
    assert!(!root.join(pop.path()).exists());
    let lock = crate::lock::Lock::read(&root).unwrap();
    assert_eq!(lock.resources.keys().collect::<Vec<_>>(), ["hush"]);
}

#[test]
fn an_offline_plan_estimates_and_never_calls_the_backend() {
    let root = crate::tests::scratch("plan");
    let calls = Calls::default();
    let media = open(&root, Mode::Plan, &calls);
    let (hush, pop, demon) = declare(&media, HUSH);
    assert!(hush.is_estimated() && pop.is_estimated() && demon.is_estimated());
    // The scene still runs: phrase lookups work on the estimated clock.
    let mut scene = PlanBuilder::new("plan", 10 * SECOND);
    let said = hush.place(&mut scene, SECOND);
    assert!(said.at("balls") > SECOND);
    assert_eq!(pop.duration(), seconds(0.5));
    let error = media.finish().unwrap_err().to_string();
    assert!(
        error.contains("3 media resource(s) need generating"),
        "{error}"
    );
    assert!(error.contains("ElevenLabs characters"), "{error}");
    assert!(calls.ids().is_empty());
    assert!(!root.join("media.lock.json").exists());

    let media = open(&root, Mode::Prune, &calls);
    declare(&media, HUSH);
    assert!(
        media.finish().is_err(),
        "prune refuses while audio is missing"
    );
    assert!(calls.ids().is_empty());
}

#[test]
fn drafts_stand_in_until_a_real_run_replaces_them() {
    let root = crate::tests::scratch("draft");
    let calls = Calls::default();
    let media = open(&root, Mode::Draft, &calls);
    let (hush, pop, _) = declare(&media, HUSH);
    let report = media.finish().unwrap();
    assert_eq!(report.api_calls, 0);
    let specs = calls.0.borrow().clone();
    assert_eq!(specs[0].1["backend"], "say");
    assert_eq!(specs[1].1["kind"], "silence");
    assert_eq!(pop.duration(), seconds(0.5));

    calls.clear();
    let media = open(&root, Mode::Draft, &calls);
    let (again, ..) = declare(&media, HUSH);
    media.finish().unwrap();
    assert!(calls.ids().is_empty(), "drafts are up to date for drafting");
    assert_eq!(again.key(), hush.key());

    let media = open(&root, Mode::Apply, &calls);
    declare(&media, HUSH);
    let report = media.finish().unwrap();
    assert_eq!(report.replaced, ids(&["hush", "hush.pitch(-6)", "pop"]));
    assert_eq!(report.api_calls, 2);
}

#[test]
fn every_spoken_word_can_cue_a_beat() {
    let root = crate::tests::scratch("chant");
    let calls = Calls::default();
    let media = open(&root, Mode::Apply, &calls);
    let chant = media
        .say(
            "chant",
            &Voice::fish("fish"),
            "[chanting] Balls! balls, BALLS.",
        )
        .unwrap();
    let pop = media.sfx("pop", "a pop", seconds(0.5)).unwrap();
    media.finish().unwrap();

    let mut scene = PlanBuilder::new("chant", 10 * SECOND);
    let said = chant.place(&mut scene, SECOND);
    let beats = said.at_every("balls");
    assert_eq!(beats.len(), 3);
    for &at in &beats {
        pop.play(&mut scene, at, -18.0);
    }
    let plan = scene.finish().unwrap();
    assert_eq!(plan.media.len(), 4);
    assert_eq!(plan.media[0].id, "narration-chant");
    assert_eq!(plan.media[1].timeline_start_nanos, beats[0]);
    assert_eq!(plan.media[1].path, pop.path());
    assert!(matches!(plan.media[1].role, MediaRolePlan::Layer));
}

#[test]
fn a_stitched_line_follows_its_predecessor() {
    let root = crate::tests::scratch("stitch");
    let calls = Calls::default();
    let kit = Voice::eleven("kit");
    let media = open(&root, Mode::Apply, &calls);
    let first = media.say("first", &kit, "One.").unwrap();
    let second = media
        .say("second", &kit, Line::new("Two.").after(&first))
        .unwrap();
    media.finish().unwrap();
    let jobs = calls.0.borrow().clone();
    assert_eq!(jobs[1].1["previous"][0], first.key());
    assert_eq!(jobs[1].2, ["req-first"]);
    assert_ne!(
        second.key(),
        open(&root, Mode::Plan, &Calls::default())
            .say("second", &kit, "Two.")
            .unwrap()
            .key(),
        "stitching is part of the key"
    );
    assert!(
        media
            .say("third", &Voice::fish("f"), Line::new("x").after(&first))
            .is_err(),
        "only ElevenLabs stitches"
    );
}

#[test]
fn declarations_are_checked() {
    let root = crate::tests::scratch("checks");
    let calls = Calls::default();
    let media = open(&root, Mode::Apply, &calls);
    let kit = Voice::eleven("kit");
    media.say("line", &kit, "One.").unwrap();
    media.say("line", &kit, "One.").unwrap();
    assert!(
        media.say("line", &kit, "Two.").is_err(),
        "one id, one content"
    );
    assert!(media.say("bad id", &kit, "One.").is_err());
    let calm = kit.clone().stability(0.9);
    assert!(
        media
            .dialogue("duet", [(&kit, "Hi."), (&calm, "Hey.")])
            .is_err(),
        "one request, one set of settings"
    );
    let other = Voice::eleven("other");
    let duet = media
        .dialogue("duet", [(&kit, "Hi."), (&other, "Hey.")])
        .unwrap();
    assert_eq!(duet.transcript().words().len(), 2);
    assert_eq!(calls.ids(), ["line", "duet"]);
    media.finish().unwrap();
    assert!(media.say("late", &kit, "One.").is_err(), "after finish");
}

#[test]
fn adopted_narration_is_up_to_date_without_regenerating() {
    let root = crate::tests::scratch("adopt");
    let narration = root.join("narration");
    fs::create_dir_all(&narration).unwrap();
    let text = "[warm] Hi. This is Psychopomp.";
    let script = r#"{ "engine": "elevenlabs", "model": "eleven_v4", "voice": "kit",
        "settings": { "stability": 0.2, "similarity": 0.65 }, "clips": [{ "id": "hush", "text": "TEXT" }] }"#
        .replace("TEXT", text);
    fs::write(narration.join("script.json"), &script).unwrap();
    let hash =
        crate::adopt::narrate_hash(&serde_json::from_str(&script).unwrap(), "elevenlabs", text);
    fs::write(
        narration.join("narration.json"),
        format!(
            r#"{{ "clips": [{{ "id": "hush", "file": "hush.mp3", "words": "hush.words.json",
            "durationNanos": 2000000000, "textHash": "{hash}", "engine": "elevenlabs",
            "model": "eleven_v4", "requestId": "abc" }}] }}"#
        ),
    )
    .unwrap();
    fs::write(
        narration.join("hush.words.json"),
        r#"{ "wordTimings": [{ "word": "Hi,", "start": 0.1, "end": 0.4 }] }"#,
    )
    .unwrap();
    fs::write(narration.join("hush.mp3"), b"mp3").unwrap();
    assert_eq!(crate::adopt(&narration, &root).unwrap(), ["hush"]);

    let calls = Calls::default();
    let media = open(&root, Mode::Plan, &calls);
    let kit = Voice::eleven("kit")
        .v4()
        .stability(0.2)
        .similarity(0.65)
        .whisper();
    let hush = media.dialogue("hush", [(&kit, text)]).unwrap();
    let report = media.finish().unwrap();
    assert_eq!(report.unchanged, ids(&["hush"]));
    assert!(calls.ids().is_empty());
    assert_eq!(hush.path(), Path::new("narration/hush.mp3"));
    assert_eq!(hush.duration(), 2_000_000_000);
    let mut scene = PlanBuilder::new("adopted", 4 * SECOND);
    assert_eq!(hush.place(&mut scene, SECOND).at("hi"), 1_100_000_000);

    // narrate.ts recorded the hash of other text: that audio says something else.
    fs::remove_file(root.join("media.lock.json")).unwrap();
    fs::write(
        narration.join("script.json"),
        script.replace("Hi.", "Hello."),
    )
    .unwrap();
    let error = crate::adopt(&narration, &root).unwrap_err().to_string();
    assert!(error.contains("different text"), "{error}");
}

#[test]
fn a_variant_reuses_the_base_audio_and_never_writes_the_base_lock() {
    let root = crate::tests::scratch("variant");
    let calls = Calls::default();
    let media = open(&root, Mode::Apply, &calls);
    let (hush, ..) = declare(&media, HUSH);
    media.finish().unwrap();
    let base = fs::read_to_string(root.join("media.lock.json")).unwrap();

    // The same lines in the variant: all reused, nothing generated.
    calls.clear();
    let media = open_python(&root, Mode::Apply, &calls);
    let (again, ..) = declare(&media, HUSH);
    let report = media.finish().unwrap();
    assert!(calls.ids().is_empty());
    assert_eq!(report.api_calls, 0);
    assert_eq!(report.unchanged, ids(&["hush", "hush.pitch(-6)", "pop"]));
    assert!(report.orphans.is_empty());
    assert_eq!(again.path(), hush.path(), "one shared file");
    assert!(root.join("media.python.lock.json").exists());

    // One line differs: only it (and what derives from it) is generated,
    // into the variant's lock.
    let media = open_python(&root, Mode::Apply, &calls);
    let (changed, ..) = declare(&media, "[soft] Oh... I hear you like... snakes.");
    let report = media.finish().unwrap();
    assert_eq!(calls.ids(), ["hush", "hush.pitch(-6)"]);
    assert_eq!(report.replaced, ids(&["hush", "hush.pitch(-6)"]));
    assert_eq!(report.unchanged, ids(&["pop"]));
    assert_eq!(
        fs::read_to_string(root.join("media.lock.json")).unwrap(),
        base,
        "the base lock is never written by a variant"
    );

    // The base still resolves its own line, and prune in either keeps the
    // other's files.
    calls.clear();
    let media = open(&root, Mode::Prune, &calls);
    declare(&media, HUSH);
    let report = media.finish().unwrap();
    assert!(calls.ids().is_empty() && report.orphans.is_empty());
    assert!(
        root.join(changed.path()).exists(),
        "the variant's file stays"
    );
    let media = open_python(&root, Mode::Prune, &calls);
    declare(&media, "[soft] Oh... I hear you like... snakes.");
    media.finish().unwrap();
    assert!(root.join(hush.path()).exists(), "the base's file stays");
    assert_eq!(
        fs::read_to_string(root.join("media.lock.json")).unwrap(),
        base
    );
}
