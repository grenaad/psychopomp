//! The verify manifest: which Scene Program writes each plan, and the moments
//! of it `psychopomp verify` renders.
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::render::Theme;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Manifest {
    /// The theme of every scene that names none.
    #[serde(default)]
    pub theme: Theme,
    pub scenes: Vec<Scene>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Scene {
    /// Stable name: the baseline's plan file and frame directory.
    pub name: String,
    /// The Cargo package whose binary writes the plan.
    pub package: String,
    /// Arguments to that binary. Scenes that share a package and arguments
    /// share one run.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment for that binary, such as `PSYCHOPOMP_LANG`. Scenes that
    /// differ only here are separate runs.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The plan or reel the program writes, relative to the manifest.
    pub plan: String,
    /// Instants to render, in seconds.
    #[serde(default)]
    pub times: Vec<f64>,
    /// Moments rendered through the export shutter, as video frames are.
    #[serde(default)]
    pub shutter: Vec<f64>,
    #[serde(default)]
    pub theme: Option<Theme>,
    /// Ignored files the frames need, such as generated sprites, relative to
    /// the manifest. Without them the plan is still compared but no frame
    /// renders.
    #[serde(default)]
    pub requires: Vec<String>,
}

impl Manifest {
    pub fn read(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path)
            .with_context(|| format!("read verify manifest {}", path.display()))?;
        Self::parse(&json).with_context(|| format!("verify manifest {}", path.display()))
    }

    pub fn parse(json: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(json)?;
        ensure!(!manifest.scenes.is_empty(), "lists no scenes");
        let mut names = HashSet::new();
        for scene in &manifest.scenes {
            let name = &scene.name;
            ensure!(
                !name.is_empty()
                    && !name.starts_with('.')
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
                "scene name '{name}' must be a plain file name"
            );
            ensure!(names.insert(name), "scene '{name}' is listed twice");
            ensure!(
                !scene.package.is_empty() && !scene.plan.is_empty(),
                "scene '{name}' needs a package and a plan path"
            );
            ensure!(
                !scene.times.is_empty() || !scene.shutter.is_empty(),
                "scene '{name}' renders no frames; give it times or shutter times"
            );
            for list in [&scene.times, &scene.shutter] {
                let mut seen = HashSet::new();
                for &at in list {
                    ensure!(
                        at.is_finite() && at >= 0.0,
                        "scene '{name}' has an invalid time {at}"
                    );
                    ensure!(
                        seen.insert(at.to_bits()),
                        "scene '{name}' lists {at}s twice"
                    );
                }
            }
        }
        Ok(manifest)
    }

    /// The scenes named in `only`, in manifest order; every scene when empty.
    pub fn select(&self, only: &[String]) -> Result<Vec<&Scene>> {
        for name in only {
            if !self.scenes.iter().any(|scene| &scene.name == name) {
                bail!("the manifest has no scene '{name}'");
            }
        }
        Ok(self
            .scenes
            .iter()
            .filter(|scene| only.is_empty() || only.contains(&scene.name))
            .collect())
    }
}

impl Scene {
    pub fn theme(&self, manifest: &Manifest) -> Theme {
        self.theme.unwrap_or(manifest.theme)
    }

    /// The first required path missing under `root`.
    pub fn missing(&self, root: &Path) -> Option<&str> {
        self.requires
            .iter()
            .map(String::as_str)
            .find(|path| !root.join(path).exists())
    }

    /// Each frame to render: its time and whether it goes through the shutter.
    pub fn frames(&self) -> impl Iterator<Item = (f64, bool)> + '_ {
        let instants = self.times.iter().map(|&at| (at, false));
        instants.chain(self.shutter.iter().map(|&at| (at, true)))
    }
}

/// One program run: package, arguments, environment.
pub(super) type Run<'a> = (&'a str, &'a [String], &'a BTreeMap<String, String>);

impl Scene {
    pub fn run(&self) -> Run<'_> {
        (self.package.as_str(), self.args.as_slice(), &self.env)
    }
}

/// The distinct program runs `scenes` need, in first-use order.
pub(super) fn runs<'a>(scenes: &[&'a Scene]) -> Vec<Run<'a>> {
    let mut runs: Vec<Run<'a>> = Vec::new();
    for scene in scenes {
        let run = scene.run();
        if !runs.contains(&run) {
            runs.push(run);
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::{Manifest, runs};
    use crate::render::Theme;

    const MANIFEST: &str = r#"{
        "theme": "neutral",
        "scenes": [
            { "name": "hello", "package": "psychopomp-hello", "plan": "target/hello.json",
              "times": [0.5, 2], "shutter": [1.25] },
            { "name": "reel", "package": "films", "plan": "scenes/films/reel.json",
              "times": [3], "theme": "original" },
            { "name": "cut", "package": "films", "args": ["cut"], "plan": "scenes/films/cut.json",
              "shutter": [1] },
            { "name": "reel-b", "package": "films", "plan": "scenes/films/b.json", "times": [1],
              "requires": ["output/films/sprites"] },
            { "name": "reel-python", "package": "films", "env": { "PSYCHOPOMP_LANG": "python" },
              "plan": "scenes/films/reel-python.json", "times": [1] }
        ]
    }"#;

    #[test]
    fn scenes_parse_with_defaults_and_overrides() {
        let manifest = Manifest::parse(MANIFEST).unwrap();
        let [hello, reel, cut, _, python] = &manifest.scenes[..] else {
            panic!("five scenes")
        };
        assert!(reel.env.is_empty());
        assert_eq!(python.env["PSYCHOPOMP_LANG"], "python");
        assert_eq!(hello.theme(&manifest), Theme::Neutral);
        assert_eq!(reel.theme(&manifest), Theme::Original);
        assert!(reel.args.is_empty() && reel.shutter.is_empty());
        assert_eq!(cut.args, ["cut"]);
        assert_eq!(
            hello.frames().collect::<Vec<_>>(),
            [(0.5, false), (2.0, false), (1.25, true)]
        );
        let theme = Manifest::parse(
            r#"{ "scenes": [{ "name": "a", "package": "p", "plan": "a.json", "times": [0] }] }"#,
        )
        .unwrap()
        .theme;
        assert_eq!(theme, Theme::Original, "the renderer's default theme");
    }

    #[test]
    fn required_paths_are_checked_under_the_root() {
        let manifest = Manifest::parse(MANIFEST).unwrap();
        let root = std::env::temp_dir().join(format!("verify-requires-{}", std::process::id()));
        assert_eq!(
            manifest.scenes[3].missing(&root),
            Some("output/films/sprites")
        );
        std::fs::create_dir_all(root.join("output/films/sprites")).unwrap();
        assert_eq!(manifest.scenes[3].missing(&root), None);
        assert_eq!(manifest.scenes[0].missing(&root), None, "nothing required");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn scenes_sharing_a_package_arguments_and_environment_share_one_run() {
        let manifest = Manifest::parse(MANIFEST).unwrap();
        let all = manifest.select(&[]).unwrap();
        let runs = runs(&all)
            .into_iter()
            .map(|(package, args, env)| (package, args.join(" "), env.len()))
            .collect::<Vec<_>>();
        assert_eq!(
            runs,
            [
                ("psychopomp-hello", String::new(), 0),
                ("films", String::new(), 0),
                ("films", "cut".to_owned(), 0),
                ("films", String::new(), 1),
            ]
        );
    }

    #[test]
    fn selection_keeps_manifest_order_and_rejects_unknown_names() {
        let manifest = Manifest::parse(MANIFEST).unwrap();
        let only = ["cut".to_owned(), "hello".to_owned()];
        let names = manifest
            .select(&only)
            .unwrap()
            .iter()
            .map(|scene| scene.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["hello", "cut"]);
        let error = manifest.select(&["nope".to_owned()]).unwrap_err();
        assert!(error.to_string().contains("no scene 'nope'"));
    }

    #[test]
    fn invalid_manifests_are_rejected_with_the_scene_named() {
        let scene = |fields: &str| {
            format!(
                r#"{{ "scenes": [{{ "name": "a", "package": "p", "plan": "a.json", {fields} }}] }}"#
            )
        };
        for (manifest, message) in [
            (r#"{ "scenes": [] }"#.to_owned(), "lists no scenes"),
            (scene(r#""times": []"#), "renders no frames"),
            (scene(r#""times": [-1]"#), "invalid time"),
            (scene(r#""times": [1, 1]"#), "lists 1s twice"),
            (
                scene(r#""times": [1], "theme": "sepia""#),
                "unknown variant",
            ),
            (scene(r#""times": [1], "shuter": [1]"#), "unknown field"),
            (
                r#"{ "scenes": [{ "name": "../a", "package": "p", "plan": "a", "times": [0] }] }"#
                    .to_owned(),
                "plain file name",
            ),
            (
                r#"{ "scenes": [
                    { "name": "a", "package": "p", "plan": "a", "times": [0] },
                    { "name": "a", "package": "q", "plan": "b", "times": [0] }
                ] }"#
                    .to_owned(),
                "listed twice",
            ),
        ] {
            let error = format!("{:#}", Manifest::parse(&manifest).unwrap_err());
            assert!(error.contains(message), "{manifest}: {error}");
        }
    }

    #[test]
    fn the_checked_in_manifest_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../verify.json");
        let manifest = Manifest::read(path.as_ref()).unwrap();
        assert!(manifest.scenes.len() >= 20);
    }
}
