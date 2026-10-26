//! The language a film's code is shown in. One Scene Program can tell the
//! same story in C# (the default) or Python; `PSYCHOPOMP_LANG` picks one per
//! run. The C# film keeps its ids, files and media lock; the Python film
//! suffixes them with `-python` and keeps its own lock beside the C# one,
//! sharing the voice cache, so the two never overwrite each other.
//!
//! ```ignore
//! let lang = Lang::current();
//! let reel = ReelPlan::new(lang.id("abstraction"), segments)?; // abstraction-python
//! let file = lang.pick("Payments.cs", "payments.py");
//! let media = Media::open_variant(root, lang.variant())?;      // media.python.lock.json
//! ```
use std::{env, sync::OnceLock};

use anyhow::{Result, bail};

use crate::highlight::Syntax;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Lang {
    #[default]
    CSharp,
    Python,
}

impl Lang {
    /// The environment variable that picks a language: `csharp` (default) or
    /// `python`.
    pub const VAR: &'static str = "PSYCHOPOMP_LANG";

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "" | "csharp" => Ok(Self::CSharp),
            "python" => Ok(Self::Python),
            other => bail!("{}={other}: use csharp or python", Self::VAR),
        }
    }

    pub fn from_env() -> Result<Self> {
        match env::var(Self::VAR) {
            Ok(value) => Self::parse(&value),
            Err(env::VarError::NotPresent) => Ok(Self::CSharp),
            Err(error) => bail!("{}: {error}", Self::VAR),
        }
    }

    /// This run's language, read once. Panics on an unknown value, before any
    /// plan is written.
    pub fn current() -> Self {
        static CURRENT: OnceLock<Lang> = OnceLock::new();
        *CURRENT.get_or_init(|| Self::from_env().unwrap_or_else(|error| panic!("{error:#}")))
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::CSharp => "csharp",
            Self::Python => "python",
        }
    }

    /// The media lock variant: none for C#, whose lock is `media.lock.json`.
    pub fn variant(self) -> Option<&'static str> {
        match self {
            Self::CSharp => None,
            Self::Python => Some("python"),
        }
    }

    /// Appended to plan ids and output names: `""` or `"-python"`.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::CSharp => "",
            Self::Python => "-python",
        }
    }

    /// `base` with this language's suffix.
    pub fn id(self, base: &str) -> String {
        format!("{base}{}", self.suffix())
    }

    /// The C# or the Python value.
    pub fn pick<T>(self, csharp: T, python: T) -> T {
        match self {
            Self::CSharp => csharp,
            Self::Python => python,
        }
    }

    /// The highlighter for this language's code. C# keeps the TypeScript
    /// highlighter it has always been rendered with.
    pub fn syntax(self) -> Syntax {
        match self {
            Self::CSharp => Syntax::TypeScript,
            Self::Python => Syntax::Python,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csharp_is_the_unsuffixed_default() {
        assert_eq!(Lang::default(), Lang::CSharp);
        assert_eq!(Lang::parse("").unwrap(), Lang::CSharp);
        assert_eq!(Lang::parse("csharp").unwrap(), Lang::CSharp);
        assert_eq!(Lang::CSharp.id("abstraction"), "abstraction");
        assert_eq!(Lang::CSharp.variant(), None);
        assert_eq!(Lang::CSharp.syntax(), Syntax::TypeScript);
    }

    #[test]
    fn python_suffixes_ids_and_has_its_own_lock_variant() {
        let python = Lang::parse("python").unwrap();
        assert_eq!(python.id("abstraction"), "abstraction-python");
        assert_eq!(python.variant(), Some("python"));
        assert_eq!(python.pick("Payments.cs", "payments.py"), "payments.py");
        assert_eq!(
            python.syntax(),
            Syntax::for_file(python.pick("a.cs", "a.py"))
        );
        let error = Lang::parse("Python").unwrap_err().to_string();
        assert!(error.contains("PSYCHOPOMP_LANG=Python"), "{error}");
    }
}
