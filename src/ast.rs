//! 正则表达式的抽象语法树（AST）定义。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ast {
    /// 空串（例如空分支 `a|` 或空括号 `()`）。
    Empty,
    /// 普通字符。
    Literal(char),
    /// `.` 任意单个字符。
    AnyChar,
    /// 字符类，如 `[a-z]`、`[^0-9]`。
    Class { negated: bool, ranges: Vec<(char, char)> },
    /// 连接。
    Concat(Vec<Ast>),
    ///  alternation `|`。
    Alt(Vec<Ast>),
    /// `*` 零次或多次。
    Star(Box<Ast>),
    /// `+` 一次或多次。
    Plus(Box<Ast>),
    /// `?` 零次或一次。
    Quest(Box<Ast>),
}
