//! Recursive-descent parser: pattern string -> AST.
//!
//! Grammar (precedence from loosest to tightest):
//!   alt    := concat ('|' concat)*
//!   concat := repeat*
//!   repeat := atom ('*' | '+' | '?')?        // at most one quantifier per atom
//!   atom   := '(' alt ')' | '.' | class | escape | literal
//!
//! All positions reported in errors are 0-based character indices.

use crate::ast::{Ast, CharClass, ClassItem, RepeatKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// `(` without a matching `)`; position points at the `(`.
    UnclosedParen,
    /// `)` without a matching `(`; position points at the `)`.
    UnexpectedParen,
    /// A quantifier (`*`/`+`/`?`) with nothing to repeat.
    MissingOperand,
    /// A quantifier directly following another quantifier, e.g. `a**`.
    StackedQuantifier,
    /// `[` without a matching `]`; position points at the `[`.
    UnclosedClass,
    /// `[]` or `[^]` — a class with no items.
    EmptyClass,
    /// A range whose lower bound is greater than its upper bound, e.g. `[z-a]`,
    /// or a range endpoint that is a shorthand class such as `[\d-z]`.
    BadRange,
    /// Pattern ends with a dangling `\`.
    TrailingBackslash,
    /// Unknown escape sequence, e.g. `\q`.
    InvalidEscape(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 0-based character index in the pattern.
    pub pos: usize,
    pub kind: ParseErrorKind,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match &self.kind {
            ParseErrorKind::UnclosedParen => "unclosed '('".to_string(),
            ParseErrorKind::UnexpectedParen => "unmatched ')'".to_string(),
            ParseErrorKind::MissingOperand => "quantifier has nothing to repeat".to_string(),
            ParseErrorKind::StackedQuantifier => {
                "quantifier follows another quantifier".to_string()
            }
            ParseErrorKind::UnclosedClass => "unclosed '['".to_string(),
            ParseErrorKind::EmptyClass => "empty character class".to_string(),
            ParseErrorKind::BadRange => "invalid character class range".to_string(),
            ParseErrorKind::TrailingBackslash => "dangling '\\' at end of pattern".to_string(),
            ParseErrorKind::InvalidEscape(c) => format!("invalid escape sequence '\\{}'", c),
        };
        write!(f, "parse error at position {}: {}", self.pos, msg)
    }
}

impl std::error::Error for ParseError {}

/// Parse a pattern into an AST.
pub fn parse(pattern: &str) -> Result<Ast, ParseError> {
    let mut parser = Parser {
        chars: pattern.chars().collect(),
        pos: 0,
    };
    let ast = parser.parse_alt()?;
    if parser.pos < parser.chars.len() {
        // The only token that can stop parsing here is an unmatched ')'.
        return Err(ParseError {
            pos: parser.pos,
            kind: ParseErrorKind::UnexpectedParen,
        });
    }
    Ok(ast)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

/// A single item parsed inside a `[...]` class: either one char or a
/// shorthand expansion such as `\d`.
enum ClassAtom {
    Ch(char),
    Items(Vec<ClassItem>),
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn err(&self, pos: usize, kind: ParseErrorKind) -> ParseError {
        ParseError { pos, kind }
    }

    fn parse_alt(&mut self) -> Result<Ast, ParseError> {
        let mut branches = vec![self.parse_concat()?];
        while self.peek() == Some('|') {
            self.pos += 1;
            branches.push(self.parse_concat()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap())
        } else {
            Ok(Ast::Alt(branches))
        }
    }

    fn parse_concat(&mut self) -> Result<Ast, ParseError> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == ')' || c == '|' {
                break;
            }
            items.push(self.parse_repeat()?);
        }
        match items.len() {
            0 => Ok(Ast::Empty),
            1 => Ok(items.pop().unwrap()),
            _ => Ok(Ast::Concat(items)),
        }
    }

    fn parse_repeat(&mut self) -> Result<Ast, ParseError> {
        let mut node = self.parse_atom()?;
        let mut quantified = false;
        while let Some(c) = self.peek() {
            let kind = match c {
                '*' => RepeatKind::Star,
                '+' => RepeatKind::Plus,
                '?' => RepeatKind::Quest,
                _ => break,
            };
            if quantified {
                return Err(self.err(self.pos, ParseErrorKind::StackedQuantifier));
            }
            quantified = true;
            self.pos += 1;
            node = Ast::Repeat {
                node: Box::new(node),
                kind,
            };
        }
        Ok(node)
    }

    fn parse_atom(&mut self) -> Result<Ast, ParseError> {
        let pos = self.pos;
        match self.next() {
            Some('(') => {
                let inner = self.parse_alt()?;
                match self.next() {
                    Some(')') => Ok(inner),
                    _ => Err(self.err(pos, ParseErrorKind::UnclosedParen)),
                }
            }
            Some('.') => Ok(Ast::AnyChar),
            Some('[') => self.parse_class(pos),
            Some('\\') => self.parse_escape(pos),
            Some('*') | Some('+') | Some('?') => {
                Err(self.err(pos, ParseErrorKind::MissingOperand))
            }
            Some(c) => Ok(Ast::Literal(c)),
            None => unreachable!("parse_concat only calls parse_atom when a char remains"),
        }
    }

    fn parse_escape(&mut self, pos: usize) -> Result<Ast, ParseError> {
        match self.next() {
            None => Err(self.err(pos, ParseErrorKind::TrailingBackslash)),
            Some(c) => {
                if let Some(class) = shorthand_class(c) {
                    Ok(Ast::Class(class))
                } else if let Some(lit) = escape_literal(c) {
                    Ok(Ast::Literal(lit))
                } else if c.is_alphanumeric() {
                    Err(self.err(pos, ParseErrorKind::InvalidEscape(c)))
                } else {
                    // Any non-alphanumeric char may be escaped literally: \. \\ \* ...
                    Ok(Ast::Literal(c))
                }
            }
        }
    }

    fn parse_class(&mut self, pos: usize) -> Result<Ast, ParseError> {
        let mut negated = false;
        if self.peek() == Some('^') {
            negated = true;
            self.pos += 1;
        }
        let mut items: Vec<ClassItem> = Vec::new();
        loop {
            match self.peek() {
                None => return Err(self.err(pos, ParseErrorKind::UnclosedClass)),
                Some(']') => {
                    self.pos += 1;
                    break;
                }
                _ => {}
            }
            let lo_pos = self.pos;
            let lo = self.parse_class_char(pos)?;
            // A '-' forms a range only when another char (not ']') follows it;
            // otherwise it is a literal '-', e.g. `[a-]` and `[-a]`.
            if self.peek() == Some('-') && self.peek_at(1).map_or(false, |c| c != ']') {
                self.pos += 1; // consume '-'
                let hi = self.parse_class_char(pos)?;
                match (lo, hi) {
                    (ClassAtom::Ch(a), ClassAtom::Ch(b)) => {
                        if a > b {
                            return Err(self.err(lo_pos, ParseErrorKind::BadRange));
                        }
                        items.push(ClassItem::Range(a, b));
                    }
                    // Shorthand classes like \d cannot be range endpoints.
                    _ => return Err(self.err(lo_pos, ParseErrorKind::BadRange)),
                }
            } else {
                match lo {
                    ClassAtom::Ch(c) => items.push(ClassItem::Ch(c)),
                    ClassAtom::Items(its) => items.extend(its),
                }
            }
        }
        if items.is_empty() {
            return Err(self.err(pos, ParseErrorKind::EmptyClass));
        }
        Ok(Ast::Class(CharClass { negated, items }))
    }

    fn parse_class_char(&mut self, class_pos: usize) -> Result<ClassAtom, ParseError> {
        let pos = self.pos;
        match self.next() {
            None => Err(self.err(class_pos, ParseErrorKind::UnclosedClass)),
            Some('\\') => match self.next() {
                None => Err(self.err(pos, ParseErrorKind::TrailingBackslash)),
                Some(c) => {
                    if let Some(class) = shorthand_class(c) {
                        if class.negated {
                            // Negated shorthands (\D, \W, \S) are not allowed
                            // inside a class; escaping the letter is ambiguous.
                            return Err(self.err(pos, ParseErrorKind::InvalidEscape(c)));
                        }
                        Ok(ClassAtom::Items(class.items))
                    } else if let Some(lit) = escape_literal(c) {
                        Ok(ClassAtom::Ch(lit))
                    } else if c.is_alphanumeric() {
                        Err(self.err(pos, ParseErrorKind::InvalidEscape(c)))
                    } else {
                        Ok(ClassAtom::Ch(c))
                    }
                }
            },
            Some(c) => Ok(ClassAtom::Ch(c)),
        }
    }
}

/// Shorthand classes: `\d` `\w` `\s` and their negations `\D` `\W` `\S`.
fn shorthand_class(c: char) -> Option<CharClass> {
    let (negated, items): (bool, Vec<ClassItem>) = match c {
        'd' => (false, vec![ClassItem::Range('0', '9')]),
        'D' => (true, vec![ClassItem::Range('0', '9')]),
        'w' => (
            false,
            vec![
                ClassItem::Range('0', '9'),
                ClassItem::Range('A', 'Z'),
                ClassItem::Ch('_'),
                ClassItem::Range('a', 'z'),
            ],
        ),
        'W' => (
            true,
            vec![
                ClassItem::Range('0', '9'),
                ClassItem::Range('A', 'Z'),
                ClassItem::Ch('_'),
                ClassItem::Range('a', 'z'),
            ],
        ),
        's' => (
            false,
            vec![
                ClassItem::Ch(' '),
                ClassItem::Ch('\t'),
                ClassItem::Ch('\n'),
                ClassItem::Ch('\r'),
                ClassItem::Ch('\u{0B}'),
                ClassItem::Ch('\u{0C}'),
            ],
        ),
        'S' => (
            true,
            vec![
                ClassItem::Ch(' '),
                ClassItem::Ch('\t'),
                ClassItem::Ch('\n'),
                ClassItem::Ch('\r'),
                ClassItem::Ch('\u{0B}'),
                ClassItem::Ch('\u{0C}'),
            ],
        ),
        _ => return None,
    };
    Some(CharClass { negated, items })
}

/// Control-character escapes usable both inside and outside `[...]`.
fn escape_literal(c: char) -> Option<char> {
    match c {
        'n' => Some('\n'),
        't' => Some('\t'),
        'r' => Some('\r'),
        'f' => Some('\u{0C}'),
        'v' => Some('\u{0B}'),
        '0' => Some('\0'),
        _ => None,
    }
}
