//! Standalone terminal test program for the local regex engine.
//!
//! Usage:
//!   cargo run --bin verify_regex                 run the full test suite
//!   cargo run --bin verify_regex -- PATTERN TEXT evaluate one pattern
//!
//! Prints one PASS/FAIL line per case and exits non-zero on any failure.

use local_regex::{ParseErrorKind, Regex};
use std::process::ExitCode;
use std::time::Instant;

struct Stats {
    pass: usize,
    fail: usize,
}

impl Stats {
    fn report(&mut self, ok: bool, label: &str) {
        if ok {
            self.pass += 1;
            println!("PASS  {}", label);
        } else {
            self.fail += 1;
            println!("FAIL  {}", label);
        }
    }
}


/// Abbreviate long strings so test labels stay readable.
fn short(s: &str) -> String {
    const MAX: usize = 40;
    if s.chars().count() <= MAX {
        format!("{:?}", s)
    } else {
        let prefix: String = s.chars().take(MAX).collect();
        format!("{:?}...<{} chars total>", prefix, s.chars().count())
    }
}

fn check_full(stats: &mut Stats, pattern: &str, input: &str, expected: bool) {
    let label = format!("full_match({}, {}) == {}", short(pattern), short(input), expected);
    match Regex::new(pattern) {
        Ok(re) => stats.report(re.is_full_match(input) == expected, &label),
        Err(e) => stats.report(false, &format!("{}  [unexpected parse error: {}]", label, e)),
    }
}

fn check_search(stats: &mut Stats, pattern: &str, input: &str, expected: Option<(usize, usize)>) {
    let label = format!("search({}, {}) == {:?}", short(pattern), short(input), expected);
    match Regex::new(pattern) {
        Ok(re) => stats.report(re.search(input) == expected, &label),
        Err(e) => stats.report(false, &format!("{}  [unexpected parse error: {}]", label, e)),
    }
}

fn check_err(stats: &mut Stats, pattern: &str, kind: ParseErrorKind, pos: usize) {
    let label = format!("parse({}) == error {:?} at pos {}", short(pattern), kind, pos);
    match Regex::new(pattern) {
        Ok(_) => stats.report(false, &format!("{}  [pattern compiled successfully]", label)),
        Err(e) => stats.report(e.kind == kind && e.pos == pos, &format!("{}  [got {}]", label, e)),
    }
}

fn section(title: &str) {
    println!("\n== {} ==", title);
}

/// Fixed reference table: (pattern, input, expected full match, expected search span).
/// These expected values are fixed hand-verified reference results.
const REFERENCE: &[(&str, &str, bool, Option<(usize, usize)>)] = &[
    ("abc", "abc", true, Some((0, 3))),
    ("abc", "abcd", false, Some((0, 3))),
    ("abc", "xabcy", false, Some((1, 4))),
    ("abc", "abd", false, None),
    ("a.c", "axc", true, Some((0, 3))),
    ("a|b", "a", true, Some((0, 1))),
    ("a|b", "b", true, Some((0, 1))),
    ("a|b", "c", false, None),
    ("(ab|cd)+", "abcdab", true, Some((0, 6))),
    ("colou?r", "color", true, Some((0, 5))),
    ("colou?r", "colour", true, Some((0, 6))),
    ("a*", "", true, Some((0, 0))),
    ("a*", "bbb", false, Some((0, 0))),
    ("a+", "", false, None),
    ("a+", "baa", false, Some((1, 3))),
    ("[0-9]+", "id=4096;", false, Some((3, 7))),
    ("[^aeiou]+", "bcdfg", true, Some((0, 5))),
    ("[^aeiou]+", "aeiou", false, None),
    ("\\d+\\.\\d+", "3.14", true, Some((0, 4))),
    ("\\w+@\\w+\\.com", "mail me at bob@example.com!", false, Some((11, 26))),
    ("(a|b)*abb", "ababb", true, Some((0, 5))),
    ("(a|b)*abb", "aabba", false, Some((0, 4))),
    ("", "", true, Some((0, 0))),
    ("", "abc", false, Some((0, 0))),
    ("(a*)*", "aaa", true, Some((0, 3))),
    ("(a+)+b", "aaab", true, Some((0, 4))),
    ("(a+)+b", "aaaa", false, None),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 2 {
        return run_one(&args[0], &args[1]);
    }

    let mut stats = Stats { pass: 0, fail: 0 };

    section("precedence: alternation < concatenation < quantifier < group");
    check_full(&mut stats, "ab|c", "ab", true);
    check_full(&mut stats, "ab|c", "c", true);
    check_full(&mut stats, "ab|c", "ac", false);
    check_full(&mut stats, "a(b|c)", "ac", true);
    check_full(&mut stats, "a(b|c)", "ab", true);
    check_full(&mut stats, "ab*", "a", true);
    check_full(&mut stats, "ab*", "abbb", true);
    check_full(&mut stats, "(ab)*", "ababab", true);
    check_full(&mut stats, "(ab)*", "aba", false);
    check_full(&mut stats, "a|b*", "", true); // b* branch matches empty

    section("literal characters and '.'");
    check_full(&mut stats, "hello", "hello", true);
    check_full(&mut stats, "hello", "hell", false);
    check_full(&mut stats, "a.c", "abc", true);
    check_full(&mut stats, "a.c", "a\nc", true); // '.' matches newline here
    check_full(&mut stats, "a.c", "ac", false);
    check_full(&mut stats, ".", "x", true);
    check_full(&mut stats, ".", "", false);
    check_full(&mut stats, "...", "abc", true);
    check_full(&mut stats, "...", "ab", false);

    section("empty pattern and empty matches");
    check_full(&mut stats, "", "", true);
    check_full(&mut stats, "", "a", false);
    check_full(&mut stats, "()", "", true);
    check_full(&mut stats, "(a|)", "", true);
    check_full(&mut stats, "(a|)", "a", true);
    check_full(&mut stats, "a*", "", true);
    check_full(&mut stats, "a?", "", true);
    check_full(&mut stats, "a+", "", false);
    check_search(&mut stats, "", "abc", Some((0, 0)));
    check_search(&mut stats, "a*", "bbb", Some((0, 0)));
    check_search(&mut stats, "()", "xyz", Some((0, 0)));

    section("character classes");
    check_full(&mut stats, "[abc]+", "cab", true);
    check_full(&mut stats, "[abc]+", "cabd", false);
    check_full(&mut stats, "[a-z]+", "hello", true);
    check_full(&mut stats, "[a-z]+", "Hello", false);
    check_full(&mut stats, "[^0-9]+", "abc", true);
    check_full(&mut stats, "[^0-9]+", "ab1", false);
    check_full(&mut stats, "[a-]", "-", true); // trailing '-' is literal
    check_full(&mut stats, "[-a]", "-", true); // leading '-' is literal
    check_full(&mut stats, "[\\]]", "]", true); // escaped ']' inside class
    check_full(&mut stats, "[\\d]+", "123", true); // shorthand inside class
    check_full(&mut stats, "[a-cx-z]+", "abcxyz", true);
    check_full(&mut stats, "[a-cx-z]+", "abcw", false);
    check_full(&mut stats, "\\d+", "42", true);
    check_full(&mut stats, "\\d+", "4x2", false);
    check_full(&mut stats, "\\w+", "foo_bar9", true);
    check_full(&mut stats, "\\s+", " \t\n", true);
    check_full(&mut stats, "\\D+", "abc", true);
    check_full(&mut stats, "[0-9a-f]+", "deadbeef", true);

    section("repetition and nested quantifiers");
    check_full(&mut stats, "a+", "aaa", true);
    check_full(&mut stats, "a?", "a", true);
    check_full(&mut stats, "a?", "aa", false);
    check_full(&mut stats, "(a+)+b", "aaab", true);
    check_full(&mut stats, "(a+)+b", "aaa", false);
    check_full(&mut stats, "(a*b?)+", "aabba", true);
    check_full(&mut stats, "(ab?)*", "abaab", true);
    check_full(&mut stats, "(a*)*", "aaa", true); // epsilon-cycle safety
    check_full(&mut stats, "(a*)*", "", true);
    check_full(&mut stats, "(a*)+", "", true);
    check_full(&mut stats, "((a|b)c)*", "acbcac", true);
    check_full(&mut stats, "((a|b)c)*", "acbca", false);

    section("full match vs search semantics");
    check_full(&mut stats, "abc", "xxabcxx", false);
    check_search(&mut stats, "abc", "xxabcxx", Some((2, 5)));
    check_search(&mut stats, "a+", "bbaab", Some((2, 4)));
    check_search(&mut stats, "\\d+", "abc123x", Some((3, 6)));
    check_search(&mut stats, "xyz", "abc", None);
    check_search(&mut stats, "b", "abcabc", Some((1, 2))); // leftmost
    check_search(&mut stats, "a|ab", "xab", Some((1, 2))); // first alternative wins

    section("escapes");
    check_full(&mut stats, "a\\.b", "a.b", true);
    check_full(&mut stats, "a\\.b", "axb", false);
    check_full(&mut stats, "\\\\", "\\", true);
    check_full(&mut stats, "\\n", "\n", true);
    check_full(&mut stats, "\\t", "\t", true);
    check_full(&mut stats, "\\*", "*", true);
    check_full(&mut stats, "\\(", "(", true);
    check_full(&mut stats, "a\\+b", "a+b", true);

    section("long inputs and performance (no exponential backtracking)");
    let t0 = Instant::now();
    // Classic backtracking killer: a?^N a^N against a^N.
    let n = 30;
    let pattern = format!("{}{}", "a?".repeat(n), "a".repeat(n));
    let input = "a".repeat(n);
    check_full(&mut stats, &pattern, &input, true);
    // Ambiguous alternation under a star over a long input.
    let long_a = "a".repeat(200_000);
    check_full(&mut stats, "(a|a)*", &long_a, true);
    // Nested quantifier that does NOT match; must still terminate fast.
    let almost = format!("{}c", "a".repeat(5_000));
    check_full(&mut stats, "(a+)+b", &almost, false);
    // Long search input.
    let haystack = format!("{}needle{}", "x".repeat(100_000), "y".repeat(100_000));
    check_search(&mut stats, "needle", &haystack, Some((100_000, 100_006)));
    let elapsed = t0.elapsed();
    println!("      (performance group finished in {:?})", elapsed);
    stats.report(
        elapsed.as_secs() < 10,
        &format!("performance group under 10s (took {:?})", elapsed),
    );

    section("illegal patterns: errors carry positions");
    check_err(&mut stats, "(", ParseErrorKind::UnclosedParen, 0);
    check_err(&mut stats, "(a|b", ParseErrorKind::UnclosedParen, 0);
    check_err(&mut stats, "a)", ParseErrorKind::UnexpectedParen, 1);
    check_err(&mut stats, "*abc", ParseErrorKind::MissingOperand, 0);
    check_err(&mut stats, "a|*", ParseErrorKind::MissingOperand, 2);
    check_err(&mut stats, "(*)", ParseErrorKind::MissingOperand, 1);
    check_err(&mut stats, "a**", ParseErrorKind::StackedQuantifier, 2);
    check_err(&mut stats, "a*+", ParseErrorKind::StackedQuantifier, 2);
    check_err(&mut stats, "[abc", ParseErrorKind::UnclosedClass, 0);
    check_err(&mut stats, "[]", ParseErrorKind::EmptyClass, 0);
    check_err(&mut stats, "[^]", ParseErrorKind::EmptyClass, 0);
    check_err(&mut stats, "[z-a]", ParseErrorKind::BadRange, 1);
    check_err(&mut stats, "abc\\", ParseErrorKind::TrailingBackslash, 3);
    check_err(&mut stats, "\\q", ParseErrorKind::InvalidEscape('q'), 0);
    check_err(&mut stats, "[\\D]", ParseErrorKind::InvalidEscape('D'), 1);

    section("fixed reference results");
    for (pattern, input, full, search) in REFERENCE {
        check_full(&mut stats, pattern, input, *full);
        check_search(&mut stats, pattern, input, *search);
    }

    println!(
        "\nSUMMARY: {} passed, {} failed, {} total",
        stats.pass,
        stats.fail,
        stats.pass + stats.fail
    );
    if stats.fail == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Quick ad-hoc evaluation: `verify_regex PATTERN TEXT`.
fn run_one(pattern: &str, text: &str) -> ExitCode {
    match Regex::new(pattern) {
        Ok(re) => {
            println!("pattern:     {:?}", pattern);
            println!("input:       {:?}", text);
            println!("full match:  {}", re.is_full_match(text));
            match re.search(text) {
                Some((s, e)) => println!("search:      match at [{}, {})", s, e),
                None => println!("search:      no match"),
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{}", e);
            ExitCode::FAILURE
        }
    }
}
