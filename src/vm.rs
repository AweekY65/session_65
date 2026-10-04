//! NFA simulation (Pike VM / Thompson's multi-state simulation).
//!
//! Instead of backtracking, the engine keeps the *set* of NFA states that
//! are reachable after consuming each input character. Every input position
//! is processed in O(program size), so the worst-case running time is
//! O(program size * input length) — no exponential blow-up, regardless of
//! nesting like `(a*)*` or ambiguous patterns like `(a|a)*`.
//!
//! Threads are kept in priority order (the order in which Thompson
//! construction emits alternatives: first alternation branch first, greedy
//! quantifiers prefer the body). This lets `search` reproduce
//! leftmost-first, Perl-like semantics without any backtracking.

use crate::nfa::{Inst, Prog};

/// A deduplicated, priority-ordered set of threads for one input position.
/// `dense` holds (pc, start_position) pairs for character-consuming and
/// match states, highest priority first. `visited` marks every pc already
/// expanded this step so epsilon cycles (e.g. from `(a*)*`) terminate.
struct ThreadList {
    dense: Vec<(usize, usize)>,
    visited: Vec<u32>,
    gen: u32,
}

impl ThreadList {
    fn new(capacity: usize) -> ThreadList {
        ThreadList {
            dense: Vec::with_capacity(capacity),
            visited: vec![0; capacity],
            // Starts at 1 so the zero-initialized `visited` marks nothing.
            gen: 1,
        }
    }

    fn clear(&mut self) {
        self.gen += 1;
        self.dense.clear();
    }

    fn is_visited(&self, pc: usize) -> bool {
        self.visited[pc] == self.gen
    }

    fn mark(&mut self, pc: usize) {
        self.visited[pc] = self.gen;
    }
}

/// Add `pc` and its epsilon closure to `list`, in priority order (depth
/// first, first alternative first). Iterative (explicit stack), so deeply
/// nested patterns cannot overflow the call stack.
fn add_thread(prog: &Prog, list: &mut ThreadList, pc: usize, start: usize) {
    let mut stack = vec![(pc, start)];
    while let Some((pc, start)) = stack.pop() {
        if list.is_visited(pc) {
            continue;
        }
        list.mark(pc);
        match prog.insts[pc] {
            Inst::Jmp(next) => stack.push((next, start)),
            Inst::Split(a, b) => {
                // `a` has higher priority: push it last so it is popped first.
                stack.push((b, start));
                stack.push((a, start));
            }
            // Char / Any / Class / Match are real states of the thread set.
            _ => list.dense.push((pc, start)),
        }
    }
}

/// Advance every thread in `clist` over input character `c`, writing the
/// resulting set into `nlist`.
fn step(prog: &Prog, clist: &ThreadList, nlist: &mut ThreadList, c: char) {
    for &(pc, start) in &clist.dense {
        let consumes = match &prog.insts[pc] {
            Inst::Char(x) => *x == c,
            Inst::Any => true,
            Inst::Class(class) => class.matches(c),
            _ => false,
        };
        if consumes {
            add_thread(prog, nlist, pc + 1, start);
        }
    }
}

/// Full match: the whole input must be consumed by a path from the start
/// state to the accepting state.
pub fn full_match(prog: &Prog, text: &[char]) -> bool {
    let mut clist = ThreadList::new(prog.len());
    let mut nlist = ThreadList::new(prog.len());
    add_thread(prog, &mut clist, 0, 0);
    for &c in text {
        step(prog, &clist, &mut nlist, c);
        std::mem::swap(&mut clist, &mut nlist);
        nlist.clear();
    }
    clist
        .dense
        .iter()
        .any(|&(pc, _)| matches!(prog.insts[pc], Inst::Match))
}

/// Search: find a match anywhere in the input.
///
/// Returns `Some((start, end))` as 0-based character indices, `[start, end)`.
///
/// Semantics: leftmost-first (Perl-like). The match starts at the leftmost
/// position that can match; among the matches starting there, the one a
/// greedy backtracking engine would find first wins (quantifiers are
/// greedy, earlier alternation branches are preferred). Empty matches are
/// possible: pattern `a*` on "bbb" reports `(0, 0)`.
pub fn search(prog: &Prog, text: &[char]) -> Option<(usize, usize)> {
    let mut clist = ThreadList::new(prog.len());
    let mut nlist = ThreadList::new(prog.len());
    let mut matched: Option<(usize, usize)> = None;
    for i in 0..=text.len() {
        // Seed a new start thread at position i with the lowest priority,
        // but only while no match has been found (leftmost start wins).
        if matched.is_none() {
            add_thread(prog, &mut clist, 0, i);
        }
        // If a Match thread is present, the highest-priority one is the
        // best match so far; cut all lower-priority threads, exactly like
        // a backtracking engine committing to a found match.
        if let Some(idx) = clist
            .dense
            .iter()
            .position(|&(pc, _)| matches!(prog.insts[pc], Inst::Match))
        {
            let (_, start) = clist.dense[idx];
            matched = Some((start, i));
            clist.dense.truncate(idx);
        }
        if clist.dense.is_empty() || i == text.len() {
            break;
        }
        step(prog, &clist, &mut nlist, text[i]);
        std::mem::swap(&mut clist, &mut nlist);
        nlist.clear();
    }
    matched
}
