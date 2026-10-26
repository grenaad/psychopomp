//! The reconciler. Each declaration (`say`, `dialogue`, `sfx`, `derive`)
//! lowers to a canonical spec and content key, compares it with the Media
//! Lock, and resolves at once: up to date (`=`), created (`+`), or replaced
//! (`~`). The returned [`Audio`] carries the real duration and words, so the
//! choreography after it is timed to what was actually generated. `finish`
//! reports what the lock holds that nothing declared (`-`, orphans).
use std::{
    cell::RefCell,
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use anyhow::{Context, Result, bail};
use psychopomp::{
    author::PlanBuilder,
    composition::Time,
    narration::{NarrationClip, Spoken},
    plan::{MediaKindPlan, MediaPlan, MediaRolePlan},
    transcript::{Transcript, WordTiming},
};
use serde_json::Value;

use crate::{
    lock::{self, Entry, Lock, STORE, Word},
    spec::{
        Backend, DERIVE_POST, DeriveSpec, Effect, Line, Route, SAY_DEFAULT, Said, Sound, Spec,
        Voice,
    },
    studio::{Generator, Job, Studio},
    words,
};

/// How a run treats resources the lock does not already hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Generate what is missing or changed (the default).
    Apply,
    /// Report the delta with estimated cost and call nothing. Missing audio
    /// gets estimated timings so the whole program runs; `finish` then fails.
    Plan,
    /// Voice missing or changed speech with macOS `say` and sound effects as
    /// silence, to time a scene before spending credits. Real audio already
    /// in the lock is kept.
    Draft,
    /// Call nothing; once nothing is missing, delete orphaned entries and
    /// unreferenced files in `media/`.
    Prune,
}

impl Mode {
    /// The environment variable that selects a mode: `apply` (default),
    /// `plan`, `draft`, or `prune`.
    pub const VAR: &'static str = "PSYCHOPOMP_MEDIA";

    pub fn from_env() -> Result<Self> {
        match env::var(Self::VAR).as_deref() {
            Err(_) | Ok("" | "apply") => Ok(Self::Apply),
            Ok("plan") => Ok(Self::Plan),
            Ok("draft") => Ok(Self::Draft),
            Ok("prune") => Ok(Self::Prune),
            Ok(other) => bail!("{}={other}: use apply, plan, draft, or prune", Self::VAR),
        }
    }

    fn offline(self) -> bool {
        matches!(self, Self::Plan | Self::Prune)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Apply => "media",
            Self::Plan => "media plan",
            Self::Draft => "media draft",
            Self::Prune => "media prune",
        }
    }
}

/// One resource's place in the delta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// `+`: no entry for this id.
    Create,
    /// `~`: the entry was generated for a different key.
    Replace,
    /// `=`: the lock holds this key and its file exists.
    Same,
}

impl Change {
    fn sign(self) -> char {
        match self {
            Self::Create => '+',
            Self::Replace => '~',
            Self::Same => '=',
        }
    }
}

/// What a run did, from [`Media::finish`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub created: Vec<String>,
    pub replaced: Vec<String>,
    pub unchanged: Vec<String>,
    /// Locked ids nothing declared.
    pub orphans: Vec<String>,
    /// Requests to paid providers this run.
    pub api_calls: u32,
}

/// Generated audio for one Scene Program: declarations reconciled against
/// `media.lock.json` and the `media/` store in `root`, the directory the plan
/// is written to, so stored paths are the plan's media paths.
///
/// ```ignore
/// let media = Media::open(env!("CARGO_MANIFEST_DIR"))?;
/// let kit = Voice::eleven(KIT).v4().stability(0.2);
/// let hush = media.say("hush", &kit, "[soft ASMR whisper] Oh... I hear you like... balls.")?;
/// let pop = media.sfx("pop", "a single soft glassy pop, tiny and dry", seconds(0.5))?;
/// let demon = hush.derive(Effect::pitch(-6.0))?;
/// media.finish()?;
/// ```
#[derive(Clone)]
pub struct Media(Rc<RefCell<State>>);

struct State {
    root: PathBuf,
    mode: Mode,
    /// This run's lock file: `media.lock.json`, or a variant's.
    lock_file: String,
    lock: Lock,
    /// The other locks beside it, read-only: their audio is reused by key,
    /// and their files are never stray.
    siblings: Vec<(String, Lock)>,
    dirty: bool,
    resolved: BTreeMap<String, Resolved>,
    generator: Box<dyn Generator>,
    say_voice: String,
    api_calls: u32,
    finished: bool,
    /// A declaration failed, so the program is exiting without `finish`.
    failed: bool,
}

/// A declaration's resolution: real, or estimated by an offline plan.
#[derive(Clone)]
struct Resolved {
    /// The declared spec's key.
    declared: String,
    /// The key of the audio in hand: the declared one, or a draft stand-in's.
    key: String,
    change: Change,
    file: String,
    duration: u64,
    words: Vec<WordTiming>,
    /// Only for estimates: the spec that would be generated.
    pending: Option<Spec>,
}

/// What a declaration was made from, for estimates and derived timings.
enum Source {
    Text { text: String, requests: Vec<String> },
    Length(u64),
    Derived { from: Audio, effect: Effect },
}

impl Media {
    /// Reconcile against `root`, in the mode `PSYCHOPOMP_MEDIA` selects, with
    /// the real providers (ElevenLabs, Fish Audio, `say`, ffmpeg, Whisper).
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_variant(root, None)
    }

    /// [`Media::open`] for one variant of a scene (a film told in another
    /// language): its own lock, `media.<variant>.lock.json`, beside the base
    /// `media.lock.json`, sharing the `media/` store. A declaration whose key
    /// another lock already holds reuses that audio for free, so only lines
    /// whose text differs are generated; neither variant ever writes the
    /// other's lock, and prune keeps every file a sibling lock references.
    /// `None` is the base lock.
    pub fn open_variant(root: impl AsRef<Path>, variant: Option<&str>) -> Result<Self> {
        let root = root.as_ref();
        Self::with_lock(
            root,
            &lock::lock_file(variant)?,
            Mode::from_env()?,
            Box::new(Studio::new(root)),
        )
    }

    #[cfg(test)]
    pub(crate) fn with(root: &Path, mode: Mode, generator: Box<dyn Generator>) -> Result<Self> {
        Self::with_lock(root, lock::LOCK_FILE, mode, generator)
    }

    pub(crate) fn with_lock(
        root: &Path,
        lock_file: &str,
        mode: Mode,
        generator: Box<dyn Generator>,
    ) -> Result<Self> {
        let lock = Lock::read_file(root, lock_file)?;
        let siblings = lock::siblings(root, lock_file)?;
        eprintln!("{}: {}", mode.label(), root.join(lock_file).display());
        Ok(Self(Rc::new(RefCell::new(State {
            root: root.to_owned(),
            mode,
            lock_file: lock_file.to_owned(),
            lock,
            siblings,
            dirty: false,
            resolved: BTreeMap::new(),
            generator,
            say_voice: env::var("SAY_VOICE").unwrap_or_else(|_| SAY_DEFAULT.to_owned()),
            api_calls: 0,
            finished: false,
            failed: false,
        }))))
    }

    /// `line` in `voice`, as one Text to Speech request. ElevenLabs words come
    /// from its character alignment, others' from Whisper.
    pub fn say(&self, id: &str, voice: &Voice, line: impl Into<Line>) -> Result<Audio> {
        let line = line.into();
        let (previous, requests) = self.stitch(&line, voice)?;
        let spec = voice.speech(
            Route::Speech,
            vec![Said {
                voice: voice.id().to_owned(),
                text: line.text.clone(),
            }],
            previous,
        )?;
        self.declare(
            id,
            Spec::Speech(spec),
            Source::Text {
                text: line.text,
                requests,
            },
        )
    }

    /// One ElevenLabs Text to Dialogue performance of `lines`, each in its
    /// own voice. Every voice shares one model, settings, and alignment.
    pub fn dialogue<'a>(
        &self,
        id: &str,
        lines: impl IntoIterator<Item = (&'a Voice, &'a str)>,
    ) -> Result<Audio> {
        let lines = lines.into_iter().collect::<Vec<_>>();
        let Some(&(lead, _)) = lines.first() else {
            bail!("dialogue '{id}' has no lines");
        };
        if let Some((other, _)) = lines.iter().find(|(voice, _)| !lead.shares_request(voice)) {
            bail!(
                "dialogue '{id}': voice '{}' has a different model, settings, or alignment than '{}'; one request applies one set",
                other.id(),
                lead.id()
            );
        }
        let characters = lines
            .iter()
            .map(|(_, text)| text.chars().count())
            .sum::<usize>();
        if lead.backend() == Backend::ElevenLabs && characters > 2_000 {
            bail!("dialogue '{id}' has {characters} characters; split it at 2,000");
        }
        let said = lines
            .iter()
            .map(|(voice, text)| Said {
                voice: voice.id().to_owned(),
                text: (*text).to_owned(),
            })
            .collect();
        let spec = lead.speech(Route::Dialogue, said, Vec::new())?;
        let text = spec.text();
        self.declare(
            id,
            Spec::Speech(spec),
            Source::Text {
                text,
                requests: Vec::new(),
            },
        )
    }

    /// An ElevenLabs sound effect of about `duration` (0.5 to 30 seconds),
    /// with its leading silence trimmed, peak-matched, and its tail faded.
    pub fn sfx(&self, id: &str, sound: impl Into<Sound>, duration: u64) -> Result<Audio> {
        let spec = sound.into().spec(duration)?;
        self.declare(id, Spec::Sound(spec), Source::Length(duration))
    }

    /// Report the delta, including orphans; fail an offline run that found
    /// missing audio; in prune mode, delete orphans. Call it after the last
    /// declaration and before writing the plan.
    pub fn finish(&self) -> Result<Report> {
        self.0.borrow_mut().finish()
    }

    fn stitch(&self, line: &Line, voice: &Voice) -> Result<(Vec<String>, Vec<String>)> {
        let Some((id, key)) = &line.after else {
            return Ok((Vec::new(), Vec::new()));
        };
        if voice.backend() != Backend::ElevenLabs {
            bail!("only ElevenLabs stitches a line after '{id}'");
        }
        let state = self.0.borrow();
        let requests = state
            .lock
            .resources
            .get(id)
            .filter(|entry| &entry.key == key)
            .map(|entry| entry.requests.clone())
            .unwrap_or_default();
        let last = requests.len().saturating_sub(3);
        Ok((vec![key.clone()], requests[last..].to_vec()))
    }

    fn derive(&self, from: &Audio, effect: Effect) -> Result<Audio> {
        effect.check()?;
        let spec = Spec::Derive(DeriveSpec {
            source: from.key.clone(),
            effect,
            post: DERIVE_POST.to_owned(),
        });
        let id = format!("{}.{}", from.id(), effect.label());
        self.declare(
            &id,
            spec,
            Source::Derived {
                from: from.clone(),
                effect,
            },
        )
    }

    fn declare(&self, id: &str, spec: Spec, source: Source) -> Result<Audio> {
        if id.is_empty() || id.contains(|c: char| c.is_whitespace() || matches!(c, '/' | '@')) {
            bail!("media id '{id}' must be non-empty, without spaces, '/', or '@'");
        }
        let key = spec.key();
        let resolved = {
            let mut state = self.0.borrow_mut();
            if state.finished {
                bail!("media '{id}' was declared after finish()");
            }
            match state.resolved.get(id) {
                Some(existing) if existing.declared == key => existing.clone(),
                Some(_) => bail!("media '{id}' is declared twice with different content"),
                None => {
                    let resolved = state.resolve(id, &spec, key, &source);
                    state.failed |= resolved.is_err();
                    let resolved = resolved?;
                    state.resolved.insert(id.to_owned(), resolved.clone());
                    resolved
                }
            }
        };
        Audio::new(self.clone(), id, resolved)
    }
}

impl State {
    fn exists(&self, file: &str) -> bool {
        self.root.join(file).is_file()
    }

    fn resolve(&mut self, id: &str, spec: &Spec, key: String, source: &Source) -> Result<Resolved> {
        let draft = match self.mode {
            Mode::Draft => spec.draft(&self.say_voice),
            _ => None,
        };
        let accepts = |entry: &Entry| {
            entry.key == key || draft.as_ref().is_some_and(|draft| entry.key == draft.key())
        };
        let entry = self.lock.resources.get(id).cloned();
        if let Some(entry) = entry
            .as_ref()
            .filter(|entry| accepts(entry) && self.exists(&entry.file))
        {
            let note = match (&entry.adopted, entry.key == key) {
                (_, false) => "  (draft)",
                (Some(_), _) => "  (adopted)",
                _ => "",
            };
            eprintln!("  = {id}  {:.2}s{note}", entry.duration_nanos as f64 / 1e9);
            return Ok(Resolved::locked(key, entry, Change::Same));
        }
        let own = self
            .lock
            .resources
            .iter()
            .map(|(other, entry)| (other, entry, None));
        let siblings = self.siblings.iter().flat_map(|(file, lock)| {
            lock.resources
                .iter()
                .map(move |(other, entry)| (other, entry, Some(file)))
        });
        let reused = own
            .chain(siblings)
            .find(|(_, entry, _)| accepts(entry) && self.exists(&entry.file))
            .map(|(other, entry, file)| (other.clone(), entry.clone(), file.cloned()));
        if let Some((other, found, file)) = reused {
            eprintln!(
                "  = {id}  {:.2}s  (same audio as '{other}'{})",
                found.duration_nanos as f64 / 1e9,
                file.map(|file| format!(" in {file}")).unwrap_or_default()
            );
            self.lock.resources.insert(id.to_owned(), found.clone());
            self.dirty = true;
            return Ok(Resolved::locked(key, &found, Change::Same));
        }

        let change = match entry {
            Some(_) => Change::Replace,
            None => Change::Create,
        };
        let why = entry
            .map(|entry| {
                let mut changed = Vec::new();
                diff(&entry.spec, &spec.canonical(), String::new(), &mut changed);
                format!("  changed {}", changed.join(", "))
            })
            .unwrap_or_default();
        let target = draft.as_ref().unwrap_or(spec);
        eprintln!(
            "  {} {id}  {}{why}{}",
            change.sign(),
            spec.summary(),
            if draft.is_some() { "  → draft" } else { "" }
        );
        if self.mode.offline() {
            let (words, duration) = source.estimate();
            let extension = source.extension(spec);
            return Ok(Resolved {
                declared: key.clone(),
                file: format!("{STORE}/{key}.{extension}"),
                key,
                change,
                duration,
                words,
                pending: Some(spec.clone()),
            });
        }
        let mut resolved = self.generate(id, target, source)?;
        resolved.declared = key;
        resolved.change = change;
        Ok(resolved)
    }

    fn generate(&mut self, id: &str, spec: &Spec, source: &Source) -> Result<Resolved> {
        let key = spec.key();
        let file = format!("{STORE}/{key}.{}", source.extension(spec));
        fs::create_dir_all(self.root.join(STORE))?;
        let (input, requests) = match source {
            Source::Derived { from, .. } => (Some(self.root.join(from.path())), Vec::new()),
            Source::Text { requests, .. } => (None, requests.clone()),
            Source::Length(_) => (None, Vec::new()),
        };
        let job = Job {
            id,
            spec,
            input,
            previous_requests: requests,
        };
        let generated = self
            .generator
            .generate(&job, &self.root.join(&file))
            .with_context(|| format!("generate media '{id}'"))?;
        self.api_calls += generated.api_calls;
        let words = match source {
            Source::Derived { from, effect } => {
                effect.retime(from.transcript().words(), from.duration())
            }
            _ => generated.words,
        };
        let words = words::clamped(words, generated.duration);
        eprintln!(
            "      {:.2}s{}",
            generated.duration as f64 / 1e9,
            match generated.requests.first() {
                Some(request) => format!("  request {request}"),
                None => String::new(),
            }
        );
        let entry = Entry {
            key: key.clone(),
            file: file.clone(),
            duration_nanos: generated.duration,
            spec: spec.canonical(),
            requests: generated.requests,
            credits: generated.credits,
            adopted: None,
            words: words.iter().map(Word::from).collect(),
        };
        self.lock.resources.insert(id.to_owned(), entry);
        // Checkpoint: a later failure must not lose paid generations.
        self.lock.write_file(&self.root, &self.lock_file)?;
        self.dirty = false;
        Ok(Resolved {
            declared: key.clone(),
            key,
            change: Change::Create,
            file,
            duration: generated.duration,
            words,
            pending: None,
        })
    }

    fn finish(&mut self) -> Result<Report> {
        if self.finished {
            bail!("media finish() was already called");
        }
        self.finished = true;
        let mut report = Report {
            api_calls: self.api_calls,
            ..Report::default()
        };
        for (id, resolved) in &self.resolved {
            match resolved.change {
                Change::Create => report.created.push(id.clone()),
                Change::Replace => report.replaced.push(id.clone()),
                Change::Same => report.unchanged.push(id.clone()),
            }
        }
        report.orphans = self
            .lock
            .resources
            .keys()
            .filter(|id| !self.resolved.contains_key(*id))
            .cloned()
            .collect();
        for id in &report.orphans {
            eprintln!(
                "  - {id}  orphan{}",
                if self.mode == Mode::Prune {
                    ""
                } else {
                    "  (PSYCHOPOMP_MEDIA=prune removes it)"
                }
            );
        }
        let pending = self
            .resolved
            .values()
            .filter_map(|resolved| resolved.pending.as_ref())
            .collect::<Vec<_>>();
        if self.mode == Mode::Prune && pending.is_empty() {
            for id in &report.orphans {
                self.lock.resources.remove(id);
                self.dirty = true;
            }
            for file in self.stray_files()? {
                fs::remove_file(self.root.join(&file)).with_context(|| format!("remove {file}"))?;
                eprintln!("  - {file}  deleted");
            }
        } else {
            let stray = self.stray_files()?.len();
            if stray > 0 {
                eprintln!(
                    "  {stray} unreferenced file(s) in {STORE}/ (PSYCHOPOMP_MEDIA=prune deletes them)"
                );
            }
        }
        if self.dirty {
            self.lock.write_file(&self.root, &self.lock_file)?;
            self.dirty = false;
        }
        eprintln!(
            "{}: {} to create, {} to replace, {} up to date, {} orphan(s); {} API call(s)",
            self.mode.label(),
            report.created.len(),
            report.replaced.len(),
            report.unchanged.len(),
            report.orphans.len(),
            report.api_calls
        );
        if !pending.is_empty() {
            bail!(
                "{} media resource(s) need generating{}; nothing was called. Run without {}={} to generate them",
                pending.len(),
                cost(&pending),
                Mode::VAR,
                if self.mode == Mode::Plan {
                    "plan"
                } else {
                    "prune"
                }
            );
        }
        Ok(report)
    }

    /// Files in `media/` that no entry of this lock or a sibling references.
    fn stray_files(&self) -> Result<Vec<String>> {
        let store = self.root.join(STORE);
        let Ok(entries) = fs::read_dir(&store) else {
            return Ok(Vec::new());
        };
        let mut stray = Vec::new();
        for entry in entries {
            let name = entry?.file_name().to_string_lossy().into_owned();
            let file = format!("{STORE}/{name}");
            let referenced = self.lock.references(&file) > 0
                || self
                    .siblings
                    .iter()
                    .any(|(_, lock)| lock.references(&file) > 0);
            if !name.starts_with('.') && !referenced {
                stray.push(file);
            }
        }
        stray.sort();
        Ok(stray)
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if !self.finished && !self.failed && !std::thread::panicking() {
            eprintln!(
                "media: finish() was never called; orphans and offline misses went unreported"
            );
        }
    }
}

/// What generating `pending` would bill.
fn cost(pending: &[&Spec]) -> String {
    let mut characters = BTreeMap::<&str, usize>::new();
    let mut sound = 0u64;
    for spec in pending {
        match spec {
            Spec::Sound(sound_spec) => sound += sound_spec.duration_nanos,
            spec => {
                if let Some(provider) = spec.provider() {
                    *characters.entry(provider.name()).or_default() += spec.characters();
                }
            }
        }
    }
    let mut parts = characters
        .iter()
        .map(|(provider, count)| format!("{count} {provider} characters"))
        .collect::<Vec<_>>();
    if sound > 0 {
        parts.push(format!(
            "{:.1}s of ElevenLabs sound effects",
            sound as f64 / 1e9
        ));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" (about {})", parts.join(" and "))
    }
}

/// Paths where two canonical specs differ, such as `lines[0].text`.
fn diff(old: &Value, new: &Value, path: String, out: &mut Vec<String>) {
    if old == new {
        return;
    }
    let depth = path.matches(['.', '[']).count();
    match (old, new) {
        (Value::Object(a), Value::Object(b)) if depth < 3 => {
            let mut keys = a.keys().chain(b.keys()).collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                diff(
                    a.get(key).unwrap_or(&Value::Null),
                    b.get(key).unwrap_or(&Value::Null),
                    child,
                    out,
                );
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() && depth < 3 => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                diff(a, b, format!("{path}[{index}]"), out);
            }
        }
        _ => out.push(if path.is_empty() {
            "kind".to_owned()
        } else {
            path
        }),
    }
}

impl Resolved {
    fn locked(declared: String, entry: &Entry, change: Change) -> Self {
        Self {
            declared,
            key: entry.key.clone(),
            change,
            file: entry.file.clone(),
            duration: entry.duration_nanos,
            words: entry.words(),
            pending: None,
        }
    }
}

impl Source {
    fn estimate(&self) -> (Vec<WordTiming>, u64) {
        match self {
            Self::Text { text, .. } => words::estimate(text),
            Self::Length(duration) => (Vec::new(), *duration),
            Self::Derived { from, effect } => (
                effect.retime(from.transcript().words(), from.duration()),
                effect.duration(from.duration()),
            ),
        }
    }

    fn extension(&self, spec: &Spec) -> String {
        match self {
            Self::Derived { from, .. } => from
                .path()
                .extension()
                .map_or("wav".to_owned(), |ext| ext.to_string_lossy().into_owned()),
            _ => spec.extension().unwrap_or("wav").to_owned(),
        }
    }
}

/// One generated resource: a file in the scene with its exact duration and
/// word timings. Place speech as a Script Clip to look up its words on the
/// plan clock; play anything as a Layer Clip.
///
/// ```ignore
/// let said = hush.place(&mut scene, SECOND);
/// for at in said.at_every("balls") {
///     pop.play(&mut scene, at, -18.0);
/// }
/// demon.play(&mut scene, said.end(), -6.0);
/// ```
#[derive(Clone)]
pub struct Audio {
    media: Media,
    key: String,
    clip: NarrationClip,
    estimated: bool,
}

impl Audio {
    fn new(media: Media, id: &str, resolved: Resolved) -> Result<Self> {
        let transcript = Transcript::new(resolved.words)
            .with_context(|| format!("media '{id}' has invalid word timings"))?;
        Ok(Self {
            media,
            key: resolved.key,
            clip: NarrationClip::new(id, resolved.file, resolved.duration, transcript),
            estimated: resolved.pending.is_some(),
        })
    }

    pub fn id(&self) -> &str {
        self.clip.id()
    }

    /// The content key of the audio in hand.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Relative to the scene root, as the plan references it.
    pub fn path(&self) -> &Path {
        self.clip.path()
    }

    /// Exact decoded length in nanoseconds.
    pub fn duration(&self) -> u64 {
        self.clip.duration()
    }

    /// Word timings in source time.
    pub fn transcript(&self) -> &Transcript {
        self.clip.transcript()
    }

    /// The same audio as a narration clip.
    pub fn clip(&self) -> &NarrationClip {
        &self.clip
    }

    /// Whether these timings are an offline plan's guess.
    pub fn is_estimated(&self) -> bool {
        self.estimated
    }

    /// Place as a Script Clip (`narration-<id>`) starting at `start`;
    /// phrase lookups on the result return plan-clock times.
    pub fn place(&self, scene: &mut PlanBuilder, start: u64) -> Spoken<'_> {
        self.clip.place(scene, start)
    }

    /// Play the whole file as a Layer Clip at `at`, `gain_db` relative to the file.
    pub fn play(&self, scene: &mut PlanBuilder, at: u64, gain_db: f32) {
        let duration = self.duration();
        scene.media(MediaPlan {
            id: format!("{}@{}", self.id(), Time::from_nanos(at)),
            path: self.path().to_owned(),
            kind: MediaKindPlan::Audio,
            role: MediaRolePlan::Layer,
            source_start_nanos: 0,
            source_end_nanos: duration,
            timeline_start_nanos: at,
            timeline_end_nanos: at + duration,
            gain_db,
        });
    }

    /// Return a composable [`psychopomp::score::Beat`] that plays this audio resource
    /// as a Layer Clip at its cue time.
    pub fn beat(&self, gain_db: f32) -> impl psychopomp::score::Beat + '_ {
        psychopomp::score::impulse(move |scene, at| {
            self.play(scene, at, gain_db);
        })
    }

    /// A new resource, `<id>.<effect>`, made from this one with ffmpeg. Its
    /// key follows this one's, so regenerating the source regenerates it.
    pub fn derive(&self, effect: Effect) -> Result<Audio> {
        self.media.derive(self, effect)
    }
}

#[cfg(test)]
mod tests;
