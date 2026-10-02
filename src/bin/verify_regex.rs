//! 独立终端测试程序：逐项输出 PASS/FAIL，任一失败则以非零码退出。
//! 运行方式：cargo run --bin verify_regex

use local_regex::Regex;
use std::time::Instant;

static mut PASS: usize = 0;
static mut FAIL: usize = 0;

fn report(name: &str, ok: bool, detail: &str) {
    unsafe {
        if ok {
            PASS += 1;
            println!("[PASS] {}", name);
        } else {
            FAIL += 1;
            println!("[FAIL] {} -- {}", name, detail);
        }
    }
}

/// 检查 full match 结果。
fn full(name: &str, pattern: &str, input: &str, expect: bool) {
    match Regex::new(pattern) {
        Ok(re) => {
            let got = re.is_full_match(input);
            report(
                name,
                got == expect,
                &format!(
                    "pattern={:?} input={:?} full: expect {}, got {}",
                    pattern, input, expect, got
                ),
            );
        }
        Err(e) => report(name, false, &format!("unexpected parse error: {}", e)),
    }
}

/// 检查 search（find）结果。
fn find(name: &str, pattern: &str, input: &str, expect: Option<(usize, usize)>) {
    match Regex::new(pattern) {
        Ok(re) => {
            let got = re.find(input);
            report(
                name,
                got == expect,
                &format!(
                    "pattern={:?} input={:?} find: expect {:?}, got {:?}",
                    pattern, input, expect, got
                ),
            );
        }
        Err(e) => report(name, false, &format!("unexpected parse error: {}", e)),
    }
}

/// 检查 pattern 应当解析失败，且错误位置正确。
fn perr(name: &str, pattern: &str, expect_pos: usize) {
    match Regex::new(pattern) {
        Ok(_) => report(
            name,
            false,
            &format!("pattern={:?} should fail to parse but succeeded", pattern),
        ),
        Err(e) => report(
            name,
            e.pos == expect_pos,
            &format!(
                "pattern={:?} error pos: expect {}, got {} ({})",
                pattern, expect_pos, e.pos, e.message
            ),
        ),
    }
}

fn main() {
    println!("== local_regex 验证测试 ==\n");

    // ---------- 1. 优先级 ----------
    println!("-- 优先级 --");
    full("prec: concat over alt", "ab|c", "ab", true);
    full("prec: alt right branch", "ab|c", "c", true);
    full("prec: alt not greedy concat", "ab|c", "ac", false);
    full("prec: star binds to atom", "ab*", "abbb", true);
    full("prec: star binds to atom (zero)", "ab*", "a", true);
    full("prec: star not over concat", "ab*", "abab", false);
    full("prec: group then star", "(ab)*", "ababab", true);
    full("prec: plus binds to atom", "ab+", "abbb", true);
    full("prec: plus requires one", "ab+", "a", false);
    full("prec: quest binds to atom", "ab?", "ab", true);
    full("prec: quest zero", "ab?", "a", true);
    full("prec: alt inside group", "a(b|c)d", "acd", true);
    full("prec: alt inside group 2", "a(b|c)d", "abd", true);
    full("prec: alt inside group 3", "a(b|c)d", "ad", false);

    // ---------- 2. 空匹配 / epsilon ----------
    println!("-- 空匹配与 epsilon --");
    full("empty: a* on empty", "a*", "", true);
    full("empty: a+ on empty", "a+", "", false);
    full("empty: empty group", "()", "", true);
    full("empty: empty alt branch", "(a|)", "", true);
    full("empty: empty alt branch 2", "(a|)", "a", true);
    full("empty: nested empty", "(a*(b|c*)?)*", "", true);
    full("empty: a? on empty", "a?", "", true);
    find("empty: find a* in bbb", "a*", "bbb", Some((0, 0)));
    find("empty: find b* in ab", "b*", "ab", Some((0, 0)));
    find("empty: find (x|) in zz", "(x|)", "zz", Some((0, 0)));

    // ---------- 3. 字符类 ----------
    println!("-- 字符类 --");
    full("class: simple set", "[abc]+", "cab", true);
    full("class: simple set reject", "[abc]+", "cabd", false);
    full("class: range", "[a-z]+", "hello", true);
    full("class: range reject upper", "[a-z]+", "Hello", false);
    full("class: multi ranges", "[a-zA-Z0-9]+", "Ab3z9", true);
    full("class: negated", "[^0-9]+", "abc", true);
    full("class: negated reject", "[^0-9]+", "abc1", false);
    full("class: negated set", "[^abc]+", "xyz", true);
    full("class: dash literal last", "[a-]+", "a-a", true);
    full("class: dash literal first", "[-a]+", "-a-", true);
    full("class: rbracket literal first", "[]a]+", "]a]", true);
    full("class: escaped rbracket", "[\\]]+", "]]", true);
    full("class: escaped dash", "[a\\-z]+", "a-z", true);
    full("class: caret literal mid", "[a^]+", "a^a", true);
    full("class: range with escape", "[\\t-\\r]+", "\t\n\r", true);
    full("class: digits then text", "[0-9]+[a-z]+", "123abc", true);

    // ---------- 4. 重复与嵌套量词 ----------
    println!("-- 重复与嵌套量词 --");
    full("rep: star", "a*", "aaaa", true);
    full("rep: plus", "a+", "aaaa", true);
    full("rep: plus empty fail", "a+", "", false);
    full("rep: quest", "a?", "a", true);
    full("rep: quest too many", "a?", "aa", false);
    full("rep: group star", "(ab)*", "ababab", true);
    full("rep: nested star", "(a*)*", "aaaa", true);
    full("rep: nested star empty", "(a*)*", "", true);
    full("rep: nested plus in star", "(a+)*", "aaaa", true);
    full("rep: star of plus with tail", "(a+)+b", "aaab", true);
    full("rep: stacked quantifier a**", "a**", "aaa", true);
    full("rep: stacked quantifier a*?", "a*?", "aaa", true);
    full("rep: stacked quantifier (a*)+", "(a*)+", "aaa", true);
    full("rep: alt star", "(a|b)*", "abba", true);
    full("rep: alt star reject", "(a|b)*", "abbac", false);

    // ---------- 5. 转义与任意字符 ----------
    println!("-- 转义与任意字符 --");
    full("esc: literal dot", "a\\.c", "a.c", true);
    full("esc: literal dot reject", "a\\.c", "abc", false);
    full("esc: literal star", "a\\*", "a*", true);
    full("esc: literal backslash", "a\\\\b", "a\\b", true);
    full("esc: literal paren", "\\(a\\)", "(a)", true);
    full("esc: newline", "a\\nb", "a\nb", true);
    full("esc: tab", "\\t+", "\t\t", true);
    full("esc: literal plus", "a\\+b", "a+b", true);
    full("esc: literal pipe", "a\\|b", "a|b", true);
    full("esc: literal question", "a\\?", "a?", true);
    full("dot: any char", "a.c", "abc", true);
    full("dot: any char digit", "a.c", "a5c", true);
    full("dot: needs a char", "a.c", "ac", false);
    full("dot: dot star", "a.*c", "aXYZc", true);

    // ---------- 6. full match 与 search 语义区分 ----------
    println!("-- full match 与 search 语义 --");
    full("mode: full requires whole", "abc", "xabcy", false);
    find("mode: search finds substring", "abc", "xabcy", Some((1, 4)));
    find("mode: search leftmost", "a+", "baaab", Some((1, 4)));
    find("mode: search no match", "xyz", "abcdef", None);
    find("mode: search alt", "cat|dog", "my dog ran", Some((3, 6)));
    find("mode: search class", "[0-9]+", "abc123def", Some((3, 6)));
    find("mode: anchored-like pattern", "^", "^", Some((0, 1))); // ^ 无特殊含义，是普通字符
    full("mode: caret is literal", "^a", "^a", true);

    // ---------- 7. 长输入与性能（无指数回溯） ----------
    println!("-- 长输入与性能 --");
    let long_ab: String = "ab".repeat(50_000);
    full("perf: (a|b)* 100k chars", "(a|b)*", &long_ab, true);

    let long_tail = format!("{}abb", "a".repeat(100_000));
    full("perf: (a|b)*abb on 100k", "(a|b)*abb", &long_tail, true);
    find("perf: search in 100k", "abb", &long_tail, Some((100_000, 100_003)));

    // 对回溯引擎是灾难性的 pattern；NFA 模拟应在线性时间内完成。
    let evil: String = "a".repeat(3_000);
    let t0 = Instant::now();
    let re = Regex::new("(a+)+b").unwrap();
    let r1 = re.is_full_match(&evil);
    let d1 = t0.elapsed();
    report(
        "perf: (a+)+b on 3000 a's (no catastrophic backtracking)",
        !r1 && d1.as_secs() < 5,
        &format!("result={} elapsed={:?}", r1, d1),
    );
    println!("       (elapsed: {:?})", d1);

    let t0 = Instant::now();
    let re = Regex::new("(a|a)*b").unwrap();
    let r2 = re.is_full_match(&evil);
    let d2 = t0.elapsed();
    report(
        "perf: (a|a)*b on 3000 a's",
        !r2 && d2.as_secs() < 5,
        &format!("result={} elapsed={:?}", r2, d2),
    );
    println!("       (elapsed: {:?})", d2);

    // ---------- 8. 非法 pattern（带位置错误） ----------
    println!("-- 非法 pattern --");
    perr("err: unclosed paren", "(", 0);
    perr("err: unclosed paren nested", "a(b(c", 3);
    perr("err: unmatched close paren", "a)", 1);
    perr("err: close paren first", ")", 0);
    perr("err: missing operand star", "*abc", 0);
    perr("err: missing operand plus", "a|+", 2);
    perr("err: missing operand quest after group open", "(?a)", 1);
    perr("err: unterminated class", "[abc", 0);
    perr("err: unterminated class after prefix", "ab[0-9", 2);
    perr("err: invalid range", "[z-a]", 1);
    perr("err: trailing backslash", "ab\\", 2);
    perr("err: unknown escape", "a\\q", 1);

    // ---------- 9. 固定参考结果对照表 ----------
    // 期望值手工推导并固定于此，作为回归基准。
    println!("-- 固定参考结果对照 --");
    let reference: &[(&str, &str, bool)] = &[
        ("a|b", "a", true),
        ("a|b", "b", true),
        ("a|b", "ab", false),
        ("(a|b)(c|d)", "ad", true),
        ("(a|b)(c|d)", "ac", true),
        ("(a|b)(c|d)", "ab", false),
        ("a*b", "aaab", true),
        ("a*b", "b", true),
        ("a*b", "aaa", false),
        ("(ab)+", "abab", true),
        ("(ab)+", "aba", false),
        ("[a-c][0-9]", "b7", true),
        ("[a-c][0-9]", "d7", false),
        ("[^a]+", "bbb", true),
        ("[^a]+", "bba", false),
        ("a.c", "axc", true),
        ("a\\.c", "axc", false),
        ("(a|b)*abb", "ababb", true),
        ("(a|b)*abb", "abab", false),
        ("x?y", "y", true),
        ("x?y", "xy", true),
        ("x?y", "xxy", false),
        ("(a*)(b*)", "aabb", true),
        ("(a*)(b*)", "abba", false),
        ("[a-z]+@[a-z]+\\.[a-z]+", "user@example.com", true),
        ("[a-z]+@[a-z]+\\.[a-z]+", "user@example", false),
    ];
    for (i, (pat, input, expect)) in reference.iter().enumerate() {
        full(&format!("ref #{:02}: {:?} vs {:?}", i + 1, pat, input), pat, input, *expect);
    }

    // ---------- 汇总 ----------
    let (pass, fail) = unsafe { (PASS, FAIL) };
    println!("\n== 结果: {} PASS, {} FAIL ==", pass, fail);
    if fail > 0 {
        std::process::exit(1);
    }
}
