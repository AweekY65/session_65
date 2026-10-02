# local_regex

完全本地实现的正则表达式匹配引擎（Rust）。pattern 解析、AST、NFA 构造与匹配核心全部自行实现，**不调用系统 regex 引擎、PCRE 或任何外部服务**，所有 pattern、自动机、输入数据与测试用例仅存在于本地文件与内存中。无第三方依赖。

## 支持的语法

| 语法 | 含义 |
|---|---|
| `a` | 普通字符 |
| `.` | 任意单个字符 |
| `ab` | 连接 |
| `a\|b` | 分支（alternation） |
| `a*` `a+` `a?` | 零次或多次 / 一次或多次 / 零次或一次 |
| `(abc)` | 分组 |
| `[abc]` `[^abc]` `[a-z]` `[a-zA-Z0-9]` | 字符类（支持取反与区间） |
| `\n` `\t` `\r` `\0` | 控制字符转义 |
| `\.` `\*` `\\` `\[` 等 | 元字符转义为其字面值 |

优先级（高 → 低）：量词 `* + ?` > 连接 > 分支 `|`。

补充说明：

- 量词可堆叠/嵌套，如 `a**`、`(a*)*`、`(a+)+b`，均正确处理。
- 空分支与空分组合法，匹配空串：`(a|)`、`()`、`a*` 均可匹配 `""`。
- 字符类中：`]` 作为首个字符时是字面值（如 `[]a]`）；`-` 在首尾时是字面值（如 `[-a]`、`[a-]`）；类内支持转义（如 `[\]]`）。
- 对未知字母转义（如 `\q`）报错，而非静默吞掉。
- `^`、`$` 等锚点**未实现**，按普通字符处理。

## 匹配语义

两种模式语义明确区分：

- **full match**（`Regex::is_full_match`）：整个输入必须被 pattern 完整消费（锚定语义）。
- **search**（`Regex::find` / `Regex::is_match`）：非锚定搜索，返回最左最长匹配区间 `(start, end)`（字符下标，end 开区间，leftmost-longest / POSIX 语义）。

## 实现结构

```
src/
├── lib.rs      # 公共 API：Regex::new / is_full_match / find / is_match
├── ast.rs      # AST 定义
├── parser.rs   # 递归下降解析器（pattern → AST）
├── nfa.rs      # Thompson construction（AST → NFA 指令序列）
├── matcher.rs  # NFA 状态集模拟（匹配核心）
└── bin/
    └── verify_regex.rs  # 独立终端测试程序
```

**解析**：递归下降，文法为 `alt := concat ('|' concat)*`，`concat := repeat*`，`repeat := atom ('*'|'+'|'?')*`。非法括号、非法字符类（未闭合、区间逆序）、缺失操作数（如 `*abc`、`a|+`）、行尾反斜杠等均产生**带字符位置**的 `ParseError`。

**NFA 构造**：Thompson construction。指令集为 `Char / Any / Class / Split / Jmp / Match`；`Split` 与 `Jmp` 即 epsilon transition，编译期以 fragment + 出口回填（patch）方式组合，空串编译为一条 nop 跳转。

**匹配**：Thompson 状态集模拟（pike VM 的确定性变体）。每一步并行维护活跃状态集合，用代际戳去重，epsilon 闭包用显式栈计算（不会递归爆栈）。search 模式在每个输入位置注入新起始线程，并为每个状态记录最小起始下标以实现 leftmost-longest。

## 复杂度

设 pattern 编译出的指令数为 `m`，输入长度为 `n`：

- 编译：O(m)
- 匹配：**O(n · m)** 时间，O(m) 空间
- 不存在指数级回溯：`(a+)+b` 这类对回溯引擎灾难性的 pattern 在本引擎中仍以线性时间完成（测试中有实测计时）。

## 运行测试

不使用 `cargo test`，通过独立测试可执行程序逐项输出 PASS/FAIL：

```bash
cargo run --bin verify_regex          # debug 模式
cargo run --release --bin verify_regex  # release 模式（性能测试更快）
```

覆盖内容：优先级、空匹配/epsilon、字符类、重复与嵌套量词、转义、full match 与 search 语义区分、长输入（10 万字符）、抗指数回溯性能、非法 pattern 的位置化报错，以及 26 组固定参考结果对照表。全部通过时退出码为 0，任一失败则以非零码退出。

## 使用示例

```rust
use local_regex::Regex;

let re = Regex::new("[a-z]+@[a-z]+\\.[a-z]+").unwrap();
assert!(re.is_full_match("user@example.com"));
assert_eq!(re.find("mail: user@example.com!"), Some((6, 22)));
```
