//! Semantic color roles shared by explainer recipes such as captions and
//! sequence diagrams. Status tones keep fixed colors in every Presentation Theme;
//! the others follow the theme palette. The syntax tones (`Keyword`, `Type`,
//! `String`, `Call`, `Number`) paint code drawn as Stage labels in exactly the
//! editor's syntax colors; `highlight::Syntax::label_spans` produces them.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tone {
    #[default]
    Plain,
    /// A request or neutral action (blue).
    Request,
    Success,
    Error,
    Warning,
    Muted,
    /// The theme accent, for the one thing to follow.
    Accent,
    /// Code: a keyword, in the theme's keyword color.
    Keyword,
    /// Code: a type name, in the theme's type color.
    Type,
    /// Code: a string literal, in the theme's string color.
    String,
    /// Code: a called name, in the editor's call color (the theme accent).
    Call,
    /// Code: a number, in the editor's warm number color.
    Number,
}

impl Tone {
    pub fn is_default(&self) -> bool {
        *self == Self::Plain
    }
}
