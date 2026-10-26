//! The Media Lock: `media.lock.json` beside a Scene Program records, for each
//! declared resource id, the content key it was generated for, its file, exact
//! duration, words, and provenance. It is the state the reconciler diffs
//! declarations against.
use std::{collections::BTreeMap, fs, io, path::Path};

use anyhow::{Context, Result, bail};
use psychopomp::transcript::WordTiming;
use serde::{Deserialize, Serialize};
use serde_json::ser::Formatter;

pub(crate) const LOCK_FILE: &str = "media.lock.json";
/// The content-addressed store, `media/<key>.<ext>`, beside the lock.
pub(crate) const STORE: &str = "media";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Lock {
    pub version: u32,
    pub resources: BTreeMap<String, Entry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub key: String,
    /// Relative to the lock's directory, which is also the plan's.
    pub file: String,
    pub duration_nanos: u64,
    /// The canonical spec the key hashes, kept to explain replacements.
    pub spec: serde_json::Value,
    /// Provider request IDs, for stitching and support.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requests: Vec<String>,
    /// What the provider reported billing (ElevenLabs' `character-cost`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<u64>,
    /// The manifest this entry was adopted from instead of generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adopted: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Word>,
}

/// `[word, startSeconds, endSeconds]`, one line each in the lock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Word(pub String, pub f64, pub f64);

impl From<&WordTiming> for Word {
    fn from(word: &WordTiming) -> Self {
        Self(word.word.clone(), word.start, word.end)
    }
}

impl From<&Word> for WordTiming {
    fn from(Word(word, start, end): &Word) -> Self {
        Self {
            word: word.clone(),
            start: *start,
            end: *end,
        }
    }
}

impl Entry {
    pub fn words(&self) -> Vec<WordTiming> {
        self.words.iter().map(WordTiming::from).collect()
    }
}

impl Lock {
    pub const VERSION: u32 = 1;

    pub fn new() -> Self {
        Self {
            version: Self::VERSION,
            resources: BTreeMap::new(),
        }
    }

    /// The lock in `root`, or an empty one.
    pub fn read(root: &Path) -> Result<Self> {
        Self::read_file(root, LOCK_FILE)
    }

    /// The lock `file` in `root` (`media.lock.json` or a variant's), or an
    /// empty one.
    pub fn read_file(root: &Path, file: &str) -> Result<Self> {
        let path = root.join(file);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        let lock: Self =
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
        if lock.version != Self::VERSION {
            bail!(
                "{} is media lock version {}; this build reads {}",
                path.display(),
                lock.version,
                Self::VERSION
            );
        }
        Ok(lock)
    }

    /// Write atomically, so an interrupted run never truncates paid state.
    pub fn write(&self, root: &Path) -> Result<()> {
        self.write_file(root, LOCK_FILE)
    }

    pub fn write_file(&self, root: &Path, file: &str) -> Result<()> {
        let path = root.join(file);
        let partial = root.join(format!(".{file}.partial"));
        fs::write(&partial, self.to_json())
            .with_context(|| format!("write {}", partial.display()))?;
        fs::rename(&partial, &path).with_context(|| format!("replace {}", path.display()))
    }

    /// Pretty JSON with each word, line, and spec setting on one line.
    pub fn to_json(&self) -> String {
        let mut out = Vec::new();
        let mut serializer =
            serde_json::Serializer::with_formatter(&mut out, LockFormatter::default());
        self.serialize(&mut serializer).expect("locks serialize");
        String::from_utf8(out).expect("JSON is UTF-8") + "\n"
    }

    /// Ids whose entries reference `file`.
    pub fn references(&self, file: &str) -> usize {
        self.resources
            .values()
            .filter(|entry| entry.file == file)
            .count()
    }
}

/// The lock file of `variant`: `media.lock.json` for none, else
/// `media.<variant>.lock.json` (lowercase letters, digits and `-`).
pub(crate) fn lock_file(variant: Option<&str>) -> Result<String> {
    let Some(variant) = variant else {
        return Ok(LOCK_FILE.to_owned());
    };
    if variant.is_empty()
        || !variant
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("media lock variant '{variant}' must be lowercase letters, digits, or '-'");
    }
    Ok(format!("media.{variant}.lock.json"))
}

/// Every lock file in `root` but `own`, sorted: the base lock and each
/// variant's. They share the `media/` store.
pub(crate) fn siblings(root: &Path, own: &str) -> Result<Vec<(String, Lock)>> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for entry in entries {
        let name = entry?.file_name().to_string_lossy().into_owned();
        let variant = name
            .strip_prefix("media.")
            .and_then(|rest| rest.strip_suffix(".lock.json"));
        let is_lock = name == LOCK_FILE || variant.is_some_and(|v| lock_file(Some(v)).is_ok());
        if is_lock && name != own {
            names.push(name);
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|name| Ok((name.clone(), Lock::read_file(root, &name)?)))
        .collect()
}

/// Two-space pretty JSON, except containers nested `INLINE` deep, which stay
/// on one line: `["balls,", 2.06, 2.5]` rather than five lines per word.
#[derive(Default)]
struct LockFormatter {
    level: usize,
    has_value: bool,
}

const INLINE: usize = 5;

impl LockFormatter {
    fn open<W: ?Sized + io::Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        self.level += 1;
        self.has_value = false;
        writer.write_all(bracket)
    }

    fn close<W: ?Sized + io::Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        let inline = self.level >= INLINE;
        self.level -= 1;
        if self.has_value && !inline {
            writer.write_all(b"\n")?;
            self.indent(writer)?;
        }
        writer.write_all(bracket)
    }

    fn item<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if self.level >= INLINE {
            return writer.write_all(if first { b"" } else { b", " });
        }
        writer.write_all(if first { b"\n" } else { b",\n" })?;
        self.indent(writer)
    }

    fn indent<W: ?Sized + io::Write>(&self, writer: &mut W) -> io::Result<()> {
        (0..self.level).try_for_each(|_| writer.write_all(b"  "))
    }
}

impl Formatter for LockFormatter {
    fn begin_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"[")
    }

    fn end_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"]")
    }

    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.item(writer, first)
    }

    fn end_array_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    fn begin_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"{")
    }

    fn end_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"}")
    }

    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.item(writer, first)
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }

    fn end_object_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Entry {
        Entry {
            key: "0123456789abcdef".into(),
            file: "media/0123456789abcdef.mp3".into(),
            duration_nanos: 2_500_000_000,
            spec: serde_json::json!({
                "kind": "speech",
                "settings": { "stability": 0.2 },
                "lines": [{ "voice": "v", "text": "Hi, balls." }]
            }),
            requests: vec!["req-1".into()],
            credits: Some(10),
            adopted: None,
            words: vec![
                Word("Hi,".into(), 0.0, 0.44),
                Word("balls.".into(), 0.5, 1.0),
            ],
        }
    }

    #[test]
    fn locks_round_trip_with_one_word_per_line() {
        let mut lock = Lock::new();
        lock.resources.insert("hush".into(), entry());
        let json = lock.to_json();
        assert!(json.contains("\n        [\"Hi,\", 0.0, 0.44],\n"), "{json}");
        assert!(
            json.contains("\"settings\": {\"stability\": 0.2}"),
            "{json}"
        );
        assert!(!json.contains("adopted"), "{json}");
        let back: Lock = serde_json::from_str(&json).unwrap();
        assert_eq!(back, lock);
        assert_eq!(back.to_json(), json, "formatting is stable");
    }

    #[test]
    fn whisper_times_survive_the_lock_bit_for_bit() {
        // Long-digit values as narrate.ts wrote them; adoption must not move them.
        let mut lock = Lock::new();
        let mut adopted = entry();
        adopted.words = [
            "10.780000000000001",
            "24.459999999999997",
            "0.7000000000000008",
        ]
        .iter()
        .map(|text| {
            let seconds: f64 = serde_json::from_str(text).unwrap();
            Word((*text).into(), seconds, seconds)
        })
        .collect();
        lock.resources.insert("hush".into(), adopted.clone());
        let back: Lock = serde_json::from_str(&lock.to_json()).unwrap();
        for (Word(_, a, _), Word(_, b, _)) in
            adopted.words.iter().zip(&back.resources["hush"].words)
        {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn a_missing_lock_is_empty_and_a_written_one_reads_back() {
        let root = crate::tests::scratch("lock-round-trip");
        assert_eq!(Lock::read(&root).unwrap(), Lock::new());
        let mut lock = Lock::new();
        lock.resources.insert("hush".into(), entry());
        lock.write(&root).unwrap();
        assert_eq!(Lock::read(&root).unwrap(), lock);
        assert_eq!(lock.references("media/0123456789abcdef.mp3"), 1);
    }

    #[test]
    fn variants_have_their_own_lock_file_beside_the_base_one() {
        assert_eq!(lock_file(None).unwrap(), "media.lock.json");
        assert_eq!(lock_file(Some("python")).unwrap(), "media.python.lock.json");
        assert!(lock_file(Some("")).is_err());
        assert!(lock_file(Some("../x")).is_err());
        let root = crate::tests::scratch("lock-siblings");
        let mut lock = Lock::new();
        lock.resources.insert("hush".into(), entry());
        lock.write(&root).unwrap();
        lock.write_file(&root, "media.python.lock.json").unwrap();
        fs::write(root.join("media.lock.json.bak"), "{}").unwrap();
        let names = |own| {
            siblings(&root, own)
                .unwrap()
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("media.lock.json"), ["media.python.lock.json"]);
        assert_eq!(names("media.python.lock.json"), ["media.lock.json"]);
    }
}
