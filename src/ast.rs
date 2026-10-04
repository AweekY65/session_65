//! Abstract syntax tree for the supported regex syntax.

/// A parsed regular expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    /// Matches the empty string (produced by `()`, `a|`, empty pattern, ...).
    Empty,
    /// A single literal character.
    Literal(char),
    /// `.` — matches any single character (including `\n`).
    AnyChar,
    /// A character class such as `[a-z]`, `[^0-9]` or shorthand `\d`.
    Class(CharClass),
    /// Concatenation of sub-expressions.
    Concat(Vec<Ast>),
    /// Alternation `a|b|c`.
    Alt(Vec<Ast>),
    /// A postfix repetition operator applied to a sub-expression.
    Repeat {
        node: Box<Ast>,
        kind: RepeatKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatKind {
    /// `*` — zero or more.
    Star,
    /// `+` — one or more.
    Plus,
    /// `?` — zero or one.
    Quest,
}

/// A character class: a set of characters / ranges, optionally negated.
#[derive(Debug, Clone, PartialEq)]
pub struct CharClass {
    pub negated: bool,
    pub items: Vec<ClassItem>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClassItem {
    Ch(char),
    Range(char, char),
}

impl CharClass {
    pub fn matches(&self, c: char) -> bool {
        let hit = self.items.iter().any(|item| match *item {
            ClassItem::Ch(x) => x == c,
            ClassItem::Range(lo, hi) => lo <= c && c <= hi,
        });
        hit != self.negated
    }
}
