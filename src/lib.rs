//! local_regex：完全本地实现的正则表达式匹配引擎。
//!
//! 不依赖系统 regex 引擎或任何外部服务：
//! pattern 解析（parser）、AST（ast）、Thompson NFA 构造（nfa）、
//! 状态集模拟匹配（matcher）全部自行实现。
//!
//! 支持的语法：
//!   普通字符、`.`、连接、`|`、`*`、`+`、`?`、括号、
//!   字符类 `[abc]` `[^abc]` `[a-z]`、转义 `\n \t \r \0` 及元字符转义。
//!
//! 匹配语义：
//!   - `is_full_match`：整个输入必须被完整匹配（锚定）。
//!   - `find` / `is_match`：非锚定搜索，返回最左最长匹配区间（字符下标）。

pub mod ast;
pub mod matcher;
pub mod nfa;
pub mod parser;

pub use parser::ParseError;

/// 编译后的正则表达式。
pub struct Regex {
    nfa: nfa::Nfa,
}

impl Regex {
    /// 解析 pattern 并构造 NFA。
    pub fn new(pattern: &str) -> Result<Regex, ParseError> {
        let ast = parser::parse(pattern)?;
        Ok(Regex {
            nfa: nfa::compile(&ast),
        })
    }

    /// full match：整个输入必须完全匹配。
    pub fn is_full_match(&self, input: &str) -> bool {
        let chars: Vec<char> = input.chars().collect();
        matcher::Matcher::new(&self.nfa).is_full_match(&chars)
    }

    /// search：返回最左最长匹配的 (start, end) 字符下标区间。
    pub fn find(&self, input: &str) -> Option<(usize, usize)> {
        let chars: Vec<char> = input.chars().collect();
        matcher::Matcher::new(&self.nfa).find(&chars)
    }

    /// search 的布尔形式。
    pub fn is_match(&self, input: &str) -> bool {
        self.find(input).is_some()
    }
}
