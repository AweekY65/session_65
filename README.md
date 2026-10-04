# local_regex — 本地正则表达式匹配引擎

一个完全本地、零依赖的 Rust 正则表达式引擎。pattern 解析、NFA 构造与匹配核心全部自行实现：

- **不调用**系统 regex 引擎、PCRE 或任何外部服务；
- **零第三方 crate 依赖**（`Cargo.toml` 中没有任何 dependencies）；
- 所有 pattern、自动机、输入数据与测试用例只存在于本地文件或内存中。

## 支持的语法

| 语法 | 含义 |
| --- | --- |
| `a` | 普通字符（字面量） |
| `.` | 任意单个字符（含 `\n`） |
| `ab` | 连接 |
| `a\|b` | 交替 |
| `a*` `a+` `a?` | 重复（贪婪）；每个原子最多跟一个量词，`a**` 报错 |
| `( ... )` | 分组，可嵌套，如 `(a*b?)+` |
| `[abc]` `[a-z]` `[^0-9]` | 字符类；`[a-]`/`[-a]` 中 `-` 为字面量；`[\d]` 等简写可出现在类中 |
| `\d` `\w` `\s` `\D` `\W` `\S` | 简写字符类（取反形式不允许出现在 `[...]` 内） |
| `\.` `\\` `\*` 等 | 非字母数字字符转义后按字面量匹配 |
| `\n` `\t` `\r` `\f` `\v` `\0` | 控制字符转义 |

优先级（由松到紧）：交替 `|` < 连接 < 量词 `* + ?` < 分组/原子。

## 匹配模式

两种模式语义明确区分：

- **Full match** — `Regex::is_full_match(text)`：pattern 必须消耗**整个**输入。
- **Search** — `Regex::search(text)`：在输入任意位置寻找匹配，返回
  `Some((start, end))`（0 起始的字符下标，左闭右开）。语义为
  **leftmost-first**（Perl 风格）：起始位置最靠左的匹配获胜；同一起始位置时，
  结果与贪婪回溯引擎找到的第一个匹配一致（量词贪婪、交替分支按书写顺序优先）。
  空匹配合法：`a*` 在 `"bbb"` 上报告 `(0, 0)`。

## 实现结构

```
src/
├── ast.rs    — AST 定义（Literal / AnyChar / Class / Concat / Alt / Repeat / Empty）
├── parser.rs — 递归下降 parser，pattern 字符串 -> AST
├── nfa.rs    — Thompson construction，AST -> NFA 指令程序
├── vm.rs     — Pike VM 多状态模拟（匹配核心）
├── lib.rs    — Regex 公开 API
└── bin/verify_regex.rs — 独立终端测试程序
```

**解析**：递归下降，文法为 `alt := concat ('|' concat)*`，
`concat := repeat*`，`repeat := atom quantifier?`。所有解析错误都携带
0 起始的字符位置，包括：未闭合括号 `(`、多余 `)`、量词缺少操作数（`*abc`）、
量词叠用（`a**`）、未闭合字符类 `[abc`、空字符类 `[]`、非法区间 `[z-a]`、
末尾悬空 `\`、非法转义 `\q`。

**NFA 构造（Thompson construction）**：AST 编译为扁平指令序列
`Char / Any / Class / Split / Jmp / Match`。epsilon 转移由 `Split`（二选一）
与 `Jmp`（无条件跳转）表示；空字符串、空分支（`a|`、`()`）编译为纯 epsilon
路径。程序只有一个接受态（末尾的 `Match`）。

**匹配（Pike VM）**：不回溯，而是维护每个输入位置上所有可达 NFA 状态的
**集合**，线程按优先级排序（复现 leftmost-first 语义），每个线程携带起始位置
以报告匹配区间。epsilon 闭包用显式栈迭代计算，并用代际标记去重——
`(a*)*` 这类 epsilon 环不会死循环，深嵌套 pattern 也不会爆栈。

## 复杂度

设 NFA 程序大小为 `m`（与 pattern 长度成正比），输入长度为 `n`：

- 时间：**O(m · n)** 最坏情况，与 pattern 结构无关——不存在指数级回溯，
  `(a+)+b`、`(a|a)*`、`a?^N a^N` 等"灾难性" pattern 都在多项式时间内完成；
- 空间：**O(m)**（两个线程集合 + 访问标记）；
- 编译：**O(m)**。

## 测试

不使用 `cargo test`，而是通过独立测试程序逐项输出 PASS/FAIL：

```bash
cargo run --bin verify_regex            # 运行完整测试套件
cargo run --release --bin verify_regex  # 优化构建（长输入性能测试更快）
```

覆盖内容：运算符优先级、字面量与 `.`、空 pattern 与空匹配、字符类（区间/
取反/转义/简写）、重复与嵌套量词、full match 与 search 语义区分、转义字符、
长输入与性能（20 万字符输入、经典回溯杀手 pattern）、带位置的非法 pattern
报错，以及一组固定参考结果对照表（`REFERENCE`）。全部通过时退出码为 0，
任一失败则为 1。

也可以临时评估单个 pattern：

```bash
cargo run --bin verify_regex -- '(a|b)*abb' 'xxababb'
# pattern:     "(a|b)*abb"
# input:       "xxababb"
# full match:  false
# search:      match at [2, 7)
```

## 库用法

```rust
use local_regex::Regex;

let re = Regex::new(r"\d+(\.\d+)?")?;   // 解析 + 编译，错误带位置
assert!(re.is_full_match("3.14"));      // full match
assert!(!re.is_full_match("x3.14"));
assert_eq!(re.search("pi=3.14;"), Some((3, 7))); // search
```
