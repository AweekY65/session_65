//! 递归下降解析器：把 pattern 字符串解析成 AST。
//!
//! 文法（优先级从低到高）：
//!   alt    := concat ('|' concat)*
//!   concat := repeat*
//!   repeat := atom ('*' | '+' | '?')*
//!   atom   := '(' alt ')' | '[' class ']' | '.' | escape | literal

use crate::ast::Ast;
use std::fmt;

/// 带位置（字符下标，从 0 开始）的解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub pos: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parse error at position {}: {}", self.pos, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(pattern: &str) -> Result<Ast, ParseError> {
    let mut p = Parser {
        chars: pattern.chars().collect(),
        pos: 0,
    };
    let ast = p.parse_alt()?;
    if p.pos < p.chars.len() {
        return Err(ParseError {
            pos: p.pos,
            message: format!("unexpected '{}'", p.chars[p.pos]),
        });
    }
    Ok(ast)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn err<T>(&self, pos: usize, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            pos,
            message: message.into(),
        })
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
            if c == '|' || c == ')' {
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
        // 允许嵌套/堆叠量词，如 (a*)*、a*?、a**。
        loop {
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    node = Ast::Star(Box::new(node));
                }
                Some('+') => {
                    self.pos += 1;
                    node = Ast::Plus(Box::new(node));
                }
                Some('?') => {
                    self.pos += 1;
                    node = Ast::Quest(Box::new(node));
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn parse_atom(&mut self) -> Result<Ast, ParseError> {
        let pos = self.pos;
        match self.bump() {
            None => self.err(pos, "unexpected end of pattern"),
            Some('(') => {
                let inner = self.parse_alt()?;
                match self.bump() {
                    Some(')') => Ok(inner),
                    _ => self.err(pos, "unclosed '('"),
                }
            }
            Some(')') => self.err(pos, "unmatched ')'"),
            Some(c @ ('*' | '+' | '?')) => {
                self.err(pos, format!("missing operand for '{}'", c))
            }
            Some('.') => Ok(Ast::AnyChar),
            Some('[') => self.parse_class(pos),
            Some('\\') => Ok(Ast::Literal(self.parse_escape(pos)?)),
            Some(c) => Ok(Ast::Literal(c)),
        }
    }

    /// 解析 `\` 之后的转义字符。`slash_pos` 是反斜杠的位置（用于报错）。
    fn parse_escape(&mut self, slash_pos: usize) -> Result<char, ParseError> {
        match self.bump() {
            None => self.err(slash_pos, "trailing '\\' at end of pattern"),
            Some('n') => Ok('\n'),
            Some('t') => Ok('\t'),
            Some('r') => Ok('\r'),
            Some('0') => Ok('\0'),
            Some(c) if c.is_ascii_alphanumeric() => {
                self.err(slash_pos, format!("unknown escape '\\{}'", c))
            }
            // 其他非字母数字字符转义后就是其字面含义，如 \. \* \[ \\。
            Some(c) => Ok(c),
        }
    }

    /// 解析字符类主体，`open_pos` 是 `[` 的位置。
    /// 支持：`[abc]`、`[^abc]`、`[a-z]`、类内转义、`]` 作为首字符时为字面值、
    /// `-` 在首尾时为字面值。
    fn parse_class(&mut self, open_pos: usize) -> Result<Ast, ParseError> {
        let negated = if self.peek() == Some('^') {
            self.pos += 1;
            true
        } else {
            false
        };
        let mut ranges: Vec<(char, char)> = Vec::new();
        let mut first = true;
        loop {
            let pos = self.pos;
            match self.bump() {
                None => return self.err(open_pos, "unterminated character class '['"),
                Some(']') if !first => break,
                Some(c) => {
                    let lo = if c == '\\' { self.parse_escape(pos)? } else { c };
                    // 形如 a-z 的区间；`-` 后面紧跟 `]` 时 `-` 是字面值。
                    if self.peek() == Some('-') && self.peek_at(1).is_some_and(|n| n != ']') {
                        self.pos += 1; // 消费 '-'
                        let hi_pos = self.pos;
                        let hi_raw = self.bump().unwrap();
                        let hi = if hi_raw == '\\' {
                            self.parse_escape(hi_pos)?
                        } else {
                            hi_raw
                        };
                        if lo > hi {
                            return self.err(
                                pos,
                                format!("invalid range '{}-{}': start greater than end", lo, hi),
                            );
                        }
                        ranges.push((lo, hi));
                    } else {
                        ranges.push((lo, lo));
                    }
                    first = false;
                }
            }
        }
        if ranges.is_empty() {
            return self.err(open_pos, "empty character class");
        }
        Ok(Ast::Class { negated, ranges })
    }
}
