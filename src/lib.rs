//! `local_regex` — a small, fully self-contained regular expression engine.
//!
//! Everything is implemented locally: the pattern parser, the NFA
//! construction (Thompson construction) and the matching engine
//! (Pike-VM style multi-state simulation). No system regex engine, no
//! PCRE, no external services, no third-party crates.
//!
//! Supported syntax:
//!   literal chars, `.`, concatenation, `|`, `*`, `+`, `?`, `(...)`,
//!   character classes `[abc]` `[a-z]` `[^...]`, escapes (`\.` `\\` `\n`
//!   `\t` `\d` `\w` `\s` and their negations outside classes).
//!
//! Matching modes:
//!   * [`Regex::is_full_match`] — the pattern must consume the whole input.
//!   * [`Regex::search`] — finds the leftmost match anywhere in the input.

#![forbid(unsafe_code)]

mod ast;
mod nfa;
mod parser;
mod vm;

pub use ast::{Ast, CharClass, ClassItem, RepeatKind};
pub use nfa::{Inst, Prog};
pub use parser::{parse, ParseError, ParseErrorKind};

/// A compiled regular expression.
pub struct Regex {
    prog: Prog,
}

impl Regex {
    /// Parse `pattern` and compile it into an NFA program.
    pub fn new(pattern: &str) -> Result<Regex, ParseError> {
        let ast = parse(pattern)?;
        Ok(Regex {
            prog: Prog::compile(&ast),
        })
    }

    /// Full match: true iff the pattern matches the *entire* input.
    pub fn is_full_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        vm::full_match(&self.prog, &chars)
    }

    /// Search: returns the leftmost match span as 0-based character
    /// indices `[start, end)`, or `None` if there is no match.
    pub fn search(&self, text: &str) -> Option<(usize, usize)> {
        let chars: Vec<char> = text.chars().collect();
        vm::search(&self.prog, &chars)
    }

    /// Expose the compiled program (useful for inspection and tests).
    pub fn program(&self) -> &[Inst] {
        &self.prog.insts
    }
}
