//! Small TypeScript/JavaScript and Python highlighters that compile one source
//! line into styled spans for the editor recipe. Roles follow the Presentation
//! Theme syntax mapping: keywords, function calls as the accent, capitalized
//! names as types, strings, muted comments, and numbers. They are line-local
//! approximations for explainers, not parsers: template literals, triple-quoted
//! strings, and block comments that span lines are not tracked.
//!
//! [`Syntax`] picks one by file name: `.py` and `.pyi` are Python, and every
//! other name keeps the TypeScript highlighter, which C# films have always used.
use crate::code::{StyledSpan, SyntaxStyle};

/// Which highlighter colors a line.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Syntax {
    /// [`typescript`]: TypeScript and JavaScript, and the C-like languages
    /// (C#) that were always shown through it.
    #[default]
    TypeScript,
    /// [`python`].
    Python,
}

impl Syntax {
    /// By extension: `.py` and `.pyi` are Python; anything else, including a
    /// tab label without an extension, keeps [`Syntax::TypeScript`].
    pub fn for_file(name: &str) -> Self {
        let extension = name.rsplit_once('.').map(|(_, extension)| extension);
        match extension {
            Some("py" | "pyi") => Self::Python,
            _ => Self::TypeScript,
        }
    }

    /// One line's styled spans.
    pub fn spans(self, line: &str) -> Vec<StyledSpan> {
        match self {
            Self::TypeScript => typescript(line),
            Self::Python => python(line),
        }
    }

    /// The byte offset where the line's comment starts (`//` or `#`), outside
    /// strings, as the highlighter itself splits it.
    pub fn line_comment(self, line: &str) -> Option<usize> {
        let chars = line.char_indices().collect::<Vec<_>>();
        let mut index = 0;
        while index < chars.len() {
            let c = chars[index].1;
            match self {
                Self::TypeScript => {
                    if c == '/' && chars.get(index + 1).is_some_and(|(_, n)| *n == '/') {
                        return Some(chars[index].0);
                    }
                    if matches!(c, '"' | '\'' | '`') {
                        index = quoted_end(&chars, index, false);
                        continue;
                    }
                }
                Self::Python => {
                    if c == '#' {
                        return Some(chars[index].0);
                    }
                    if matches!(c, '"' | '\'') {
                        index = quoted_end(&chars, index, true);
                        continue;
                    }
                }
            }
            index += 1;
        }
        None
    }
}

const KEYWORDS: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "of",
    "return",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];

/// Muted gray: neutral, so every theme maps it to its muted text color.
pub const COMMENT: SyntaxStyle = SyntaxStyle::Rgb(128, 128, 128);
/// Numbers keep one warm color in every theme.
pub const NUMBER: SyntaxStyle = SyntaxStyle::Rgb(245, 167, 66);

pub fn typescript(line: &str) -> Vec<StyledSpan> {
    let chars = line.char_indices().collect::<Vec<_>>();
    let mut spans: Vec<StyledSpan> = Vec::new();
    let push = |spans: &mut Vec<StyledSpan>, text: &str, style: SyntaxStyle| {
        if text.is_empty() {
            return;
        }
        match spans.last_mut() {
            Some(last) if same(last.style, style) => last.text.push_str(text),
            _ => spans.push(StyledSpan::new(text, style)),
        }
    };
    let slice = |from: usize, to: usize| {
        let start = chars.get(from).map_or(line.len(), |(i, _)| *i);
        let end = chars.get(to).map_or(line.len(), |(i, _)| *i);
        &line[start..end]
    };
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index].1;
        if c == '/' && chars.get(index + 1).is_some_and(|(_, n)| *n == '/') {
            push(&mut spans, slice(index, chars.len()), COMMENT);
            break;
        }
        if matches!(c, '"' | '\'' | '`') {
            let mut end = index + 1;
            while end < chars.len() && chars[end].1 != c {
                end += if chars[end].1 == '\\' { 2 } else { 1 };
            }
            let end = (end + 1).min(chars.len());
            push(&mut spans, slice(index, end), SyntaxStyle::String);
            index = end;
            continue;
        }
        if c.is_ascii_digit() {
            let mut end = index + 1;
            while end < chars.len()
                && (chars[end].1.is_ascii_alphanumeric() || matches!(chars[end].1, '_' | '.'))
            {
                end += 1;
            }
            push(&mut spans, slice(index, end), NUMBER);
            index = end;
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            let mut end = index + 1;
            while end < chars.len()
                && (chars[end].1.is_alphanumeric() || matches!(chars[end].1, '_' | '$'))
            {
                end += 1;
            }
            let word = slice(index, end);
            let called = chars[end..]
                .iter()
                .map(|(_, c)| *c)
                .find(|c| !c.is_whitespace())
                == Some('(');
            let member = index > 0 && chars[index - 1].1 == '.';
            let style = if KEYWORDS.contains(&word) && !member {
                SyntaxStyle::Keyword
            } else if called {
                SyntaxStyle::Accent
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                SyntaxStyle::Type
            } else {
                SyntaxStyle::Plain
            };
            push(&mut spans, word, style);
            index = end;
            continue;
        }
        push(&mut spans, slice(index, index + 1), SyntaxStyle::Plain);
        index += 1;
    }
    spans
}

fn same(a: SyntaxStyle, b: SyntaxStyle) -> bool {
    use SyntaxStyle::*;
    match (a, b) {
        (Plain, Plain)
        | (Keyword, Keyword)
        | (Type, Type)
        | (String, String)
        | (Accent, Accent) => true,
        (Rgb(r1, g1, b1), Rgb(r2, g2, b2)) => (r1, g1, b1) == (r2, g2, b2),
        _ => false,
    }
}

const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// One Python line. Strings (with `r`, `b`, `u`, `f` prefixes, single,
/// double or triple quotes; f-strings are wholly strings), `#` comments,
/// numbers, a leading `@decorator` in the accent, keywords, and the soft
/// keywords `match`, `case` and `type` where they open a statement. As in
/// [`typescript`], a called name is the accent and a capitalized one a type;
/// a name after `class` is a type. `self` and `cls` are plain names.
pub fn python(line: &str) -> Vec<StyledSpan> {
    let chars = line.char_indices().collect::<Vec<_>>();
    let mut spans: Vec<StyledSpan> = Vec::new();
    let slice = |from: usize, to: usize| {
        let start = chars.get(from).map_or(line.len(), |(i, _)| *i);
        let end = chars.get(to).map_or(line.len(), |(i, _)| *i);
        &line[start..end]
    };
    let first = chars
        .iter()
        .position(|(_, c)| !c.is_whitespace())
        .unwrap_or(chars.len());
    // The statement's code without its comment, for the soft keywords.
    let code = line[..Syntax::Python.line_comment(line).unwrap_or(line.len())].trim_end();
    let mut previous: Option<&str> = None;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index].1;
        if c == '#' {
            push(&mut spans, slice(index, chars.len()), COMMENT);
            break;
        }
        if c == '@' && index == first {
            let mut end = index + 1;
            while end < chars.len()
                && (chars[end].1.is_alphanumeric() || matches!(chars[end].1, '_' | '.'))
            {
                end += 1;
            }
            push(&mut spans, slice(index, end), SyntaxStyle::Accent);
            previous = None;
            index = end;
            continue;
        }
        if let Some(quote) = string_prefix(&chars, index) {
            let end = quoted_end(&chars, quote, true);
            push(&mut spans, slice(index, end), SyntaxStyle::String);
            previous = None;
            index = end;
            continue;
        }
        if c.is_ascii_digit()
            || (c == '.'
                && chars
                    .get(index + 1)
                    .is_some_and(|(_, n)| n.is_ascii_digit()))
        {
            let mut end = index + 1;
            while end < chars.len() {
                let n = chars[end].1;
                let exponent = matches!(n, '+' | '-')
                    && matches!(chars[end - 1].1, 'e' | 'E')
                    && !slice(index, end).starts_with("0x");
                if n.is_ascii_alphanumeric() || matches!(n, '_' | '.') || exponent {
                    end += 1;
                } else {
                    break;
                }
            }
            push(&mut spans, slice(index, end), NUMBER);
            previous = None;
            index = end;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut end = index + 1;
            while end < chars.len() && (chars[end].1.is_alphanumeric() || chars[end].1 == '_') {
                end += 1;
            }
            let word = slice(index, end);
            let next = chars[end..]
                .iter()
                .map(|(_, c)| *c)
                .find(|c| !c.is_whitespace());
            let called = next == Some('(');
            let member = index > 0 && chars[index - 1].1 == '.';
            let soft = index == first
                && match word {
                    "match" | "case" => {
                        code.ends_with(':') && !matches!(next, Some('=' | '.' | ':' | ',' | ')'))
                    }
                    "type" => {
                        chars.get(end).is_some_and(|(_, c)| c.is_whitespace())
                            && next.is_some_and(|c| c.is_alphabetic() || c == '_')
                    }
                    _ => false,
                };
            let style = if !member && (PYTHON_KEYWORDS.contains(&word) || soft) {
                SyntaxStyle::Keyword
            } else if !member && previous == Some("class") {
                SyntaxStyle::Type
            } else if called {
                SyntaxStyle::Accent
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                SyntaxStyle::Type
            } else {
                SyntaxStyle::Plain
            };
            push(&mut spans, word, style);
            previous = Some(word);
            index = end;
            continue;
        }
        if !c.is_whitespace() {
            previous = None;
        }
        push(&mut spans, slice(index, index + 1), SyntaxStyle::Plain);
        index += 1;
    }
    spans
}

/// Append `text` in `style`, joining the previous span when it has the same.
fn push(spans: &mut Vec<StyledSpan>, text: &str, style: SyntaxStyle) {
    if text.is_empty() {
        return;
    }
    match spans.last_mut() {
        Some(last) if same(last.style, style) => last.text.push_str(text),
        _ => spans.push(StyledSpan::new(text, style)),
    }
}

/// When a Python string starts at `index` (its prefix letters, if any, then a
/// quote), the index of its opening quote.
fn string_prefix(chars: &[(usize, char)], index: usize) -> Option<usize> {
    if index > 0 && (chars[index - 1].1.is_alphanumeric() || chars[index - 1].1 == '_') {
        return None;
    }
    let mut quote = index;
    while quote < chars.len()
        && quote - index < 2
        && matches!(
            chars[quote].1,
            'r' | 'R' | 'b' | 'B' | 'u' | 'U' | 'f' | 'F'
        )
    {
        quote += 1;
    }
    chars
        .get(quote)
        .filter(|(_, c)| matches!(c, '"' | '\''))
        .map(|_| quote)
}

/// The index just past the string whose opening quote is at `quote`, or the
/// line's end when it does not close on this line. A backslash escapes the
/// next character; `triple` lets `"""` and `'''` close only on their own kind.
fn quoted_end(chars: &[(usize, char)], quote: usize, triple: bool) -> usize {
    let q = chars[quote].1;
    let tripled = triple && (1..3).all(|k| chars.get(quote + k).is_some_and(|(_, c)| *c == q));
    let mut end = quote + if tripled { 3 } else { 1 };
    while end < chars.len() {
        let c = chars[end].1;
        if c == '\\' {
            end += 2;
            continue;
        }
        if c == q && (!tripled || (1..3).all(|k| chars.get(end + k).is_some_and(|(_, n)| *n == q)))
        {
            return (end + if tripled { 3 } else { 1 }).min(chars.len());
        }
        end += 1;
    }
    chars.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(line: &str) -> Vec<(String, &'static str)> {
        named(typescript(line))
    }

    fn py(line: &str) -> Vec<(String, &'static str)> {
        named(python(line))
    }

    fn named(spans: Vec<StyledSpan>) -> Vec<(String, &'static str)> {
        spans
            .into_iter()
            .map(|span| {
                let name = match span.style {
                    SyntaxStyle::Plain => "plain",
                    SyntaxStyle::Keyword => "keyword",
                    SyntaxStyle::Type => "type",
                    SyntaxStyle::String => "string",
                    SyntaxStyle::Accent => "accent",
                    SyntaxStyle::Rgb(128, 128, 128) => "comment",
                    SyntaxStyle::Rgb(..) => "number",
                };
                (span.text, name)
            })
            .collect()
    }

    #[test]
    fn roles_follow_the_theme_syntax_mapping() {
        assert_eq!(
            styles(r#"const failure = new Error("port in use") // why"#),
            vec![
                ("const".into(), "keyword"),
                (" failure = ".into(), "plain"),
                ("new".into(), "keyword"),
                (" ".into(), "plain"),
                ("Error".into(), "accent"),
                ("(".into(), "plain"),
                ("\"port in use\"".into(), "string"),
                (") ".into(), "plain"),
                ("// why".into(), "comment"),
            ]
        );
        assert_eq!(
            styles("yield* Effect.sleep(100)"),
            vec![
                ("yield".into(), "keyword"),
                ("* ".into(), "plain"),
                ("Effect".into(), "type"),
                (".".into(), "plain"),
                ("sleep".into(), "accent"),
                ("(".into(), "plain"),
                ("100".into(), "number"),
                (")".into(), "plain"),
            ]
        );
    }

    #[test]
    fn member_names_are_not_keywords_and_text_is_preserved() {
        let line = r#"  if (error.type === "a\"b") return x.default"#;
        let spans = typescript(line);
        assert_eq!(
            spans.iter().map(|s| s.text.as_str()).collect::<String>(),
            line
        );
        let last = spans.last().unwrap();
        assert!(last.text.ends_with("x.default") && matches!(last.style, SyntaxStyle::Plain));
        assert!(styles(line).contains(&("\"a\\\"b\"".into(), "string")));
    }

    #[test]
    fn python_roles_follow_the_same_mapping() {
        assert_eq!(
            py("    def pay(self, order: Order) -> Receipt:  # charge it"),
            vec![
                ("    ".into(), "plain"),
                ("def".into(), "keyword"),
                (" ".into(), "plain"),
                ("pay".into(), "accent"),
                ("(self, order: ".into(), "plain"),
                ("Order".into(), "type"),
                (") -> ".into(), "plain"),
                ("Receipt".into(), "type"),
                (":  ".into(), "plain"),
                ("# charge it".into(), "comment"),
            ]
        );
        assert_eq!(
            py("class StripePayment(PaymentGateway):"),
            vec![
                ("class".into(), "keyword"),
                (" ".into(), "plain"),
                ("StripePayment".into(), "type"),
                ("(".into(), "plain"),
                ("PaymentGateway".into(), "type"),
                ("):".into(), "plain"),
            ]
        );
        assert_eq!(
            py("@dataclass(frozen=True)"),
            vec![
                ("@dataclass".into(), "accent"),
                ("(frozen=".into(), "plain"),
                ("True".into(), "keyword"),
                (")".into(), "plain"),
            ]
        );
        assert_eq!(
            py("        return None if x is not None else self.total"),
            vec![
                ("        ".into(), "plain"),
                ("return".into(), "keyword"),
                (" ".into(), "plain"),
                ("None".into(), "keyword"),
                (" ".into(), "plain"),
                ("if".into(), "keyword"),
                (" x ".into(), "plain"),
                ("is".into(), "keyword"),
                (" ".into(), "plain"),
                ("not".into(), "keyword"),
                (" ".into(), "plain"),
                ("None".into(), "keyword"),
                (" ".into(), "plain"),
                ("else".into(), "keyword"),
                (" self.total".into(), "plain"),
            ]
        );
        assert_eq!(
            py("stripe.charge(cents=1_000, rate=0.5, tiny=1e-3, mask=0x1F)")
                .into_iter()
                .filter(|(_, style)| *style == "number")
                .map(|(text, _)| text)
                .collect::<Vec<_>>(),
            ["1_000", "0.5", "1e-3", "0x1F"]
        );
    }

    #[test]
    fn python_strings_hide_comments_and_keep_their_prefixes() {
        for (line, string) in [
            (
                r#"raise ValueError(f"order {order.id} is not placed")"#,
                r#"f"order {order.id} is not placed""#,
            ),
            (r"pattern = r'\d+#'", r"r'\d+#'"),
            (
                r#"    """Pay one order. # not a comment"""  # a comment"#,
                r#""""Pay one order. # not a comment""""#,
            ),
            (r"note = 'it\'s paid'", r"'it\'s paid'"),
            (r#"data = b"\x00""#, r#"b"\x00""#),
        ] {
            let spans = py(line);
            assert!(
                spans.contains(&(string.into(), "string")),
                "{line}: {spans:?}"
            );
            assert_eq!(
                spans
                    .iter()
                    .map(|(text, _)| text.as_str())
                    .collect::<String>(),
                line
            );
        }
        let line = r#"url = "https://pay.example/#top"  # the anchor"#;
        assert_eq!(Syntax::Python.line_comment(line), line.find("# the"));
        assert_eq!(
            py(line).last().unwrap(),
            &("# the anchor".into(), "comment")
        );
        assert_eq!(Syntax::Python.line_comment("a = '#'"), None);
        assert_eq!(
            py("prefix_f = f").last().unwrap().1,
            "plain",
            "f alone is a name"
        );
    }

    #[test]
    fn python_soft_keywords_open_statements_only() {
        let keyword = |line: &str, word: &str| py(line).contains(&(word.to_owned(), "keyword"));
        assert!(keyword("match order.state:", "match"));
        assert!(keyword("    case Paid(receipt=r):  # done", "case"));
        assert!(keyword("    case _:", "case"));
        assert!(keyword("type Money = Decimal", "type"));
        assert!(!keyword("match = pattern.match(text)", "match"));
        assert!(!keyword("kind = type(order)", "type"));
        assert!(!keyword("type(order)", "type"));
        assert!(!keyword("order.match(x):", "match"));
        assert_eq!(py("x = order.case")[0], ("x = order.case".into(), "plain"));
    }

    #[test]
    fn the_file_name_picks_the_highlighter_and_csharp_keeps_typescript() {
        assert_eq!(Syntax::for_file("payments.py"), Syntax::Python);
        assert_eq!(Syntax::for_file("stubs/ports.pyi"), Syntax::Python);
        for name in [
            "Payments.cs",
            "a.ts",
            "used everywhere",
            "README",
            "py",
            "copy.py.txt",
        ] {
            assert_eq!(Syntax::for_file(name), Syntax::TypeScript, "{name}");
        }
        for line in [
            "public PaymentReceipt Pay(PaymentRequest request) // charge",
            r#"var url = "https://a//b"; // after"#,
            "    { if (State != expected) throw new InvalidOperationException(); }",
        ] {
            let via = Syntax::for_file("Payments.cs").spans(line);
            assert_eq!(
                serde_json::to_value(&via).unwrap(),
                serde_json::to_value(typescript(line)).unwrap()
            );
        }
        let line = r#"var url = "https://a//b"; // after"#;
        assert_eq!(Syntax::TypeScript.line_comment(line), line.find("// after"));
        assert_eq!(Syntax::TypeScript.line_comment("// all"), Some(0));
        assert_eq!(Syntax::TypeScript.line_comment("a / b"), None);
    }
}
