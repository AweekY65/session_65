//! NFA 模拟匹配器（Thompson's algorithm / pike VM 的确定性变体）。
//!
//! 每一步并行维护“当前活跃状态集合”，对长度为 n 的输入、
//! 指令数为 m 的自动机，时间复杂度 O(n * m)，空间 O(m)，
//! 不存在指数级回溯。

use crate::nfa::{Inst, Nfa};

pub struct Matcher<'a> {
    nfa: &'a Nfa,
    /// mark[pc] == gen 表示该 pc 已在当前状态集合中（代际戳去重）。
    mark: Vec<u32>,
    gen: u32,
    /// 每个 pc 对应的线程起始下标（仅 search 模式使用）。
    starts: Vec<usize>,
}

/// 一组活跃状态：指令 pc 列表。
struct StateSet {
    pcs: Vec<usize>,
}

impl<'a> Matcher<'a> {
    pub fn new(nfa: &'a Nfa) -> Self {
        let n = nfa.insts.len();
        Matcher {
            nfa,
            mark: vec![0; n],
            gen: 0,
            starts: vec![0; n],
        }
    }

    /// 把 pc 及其 epsilon 闭包加入集合；`start` 为该线程的起始下标。
    /// 同一 pc 保留最小的 start（保证 search 结果左偏）。
    fn add_state(&mut self, set: &mut StateSet, pc: usize, start: usize) {
        // 显式栈避免递归，深层嵌套 pattern 也不会爆栈。
        let mut stack = vec![(pc, start)];
        while let Some((pc, start)) = stack.pop() {
            if self.mark[pc] == self.gen {
                // 已知状态以更小的起始下标再次到达：更新并重新扩展
                // epsilon 后继，保证下游记录的 start 也最小（左偏语义）。
                // 每次重新扩展都严格减小 starts[pc]，必然终止。
                if start >= self.starts[pc] {
                    continue;
                }
                self.starts[pc] = start;
                match &self.nfa.insts[pc] {
                    Inst::Split(a, b) => {
                        stack.push((*a, start));
                        stack.push((*b, start));
                    }
                    Inst::Jmp(x) => stack.push((*x, start)),
                    // 消费/接受状态已在集合中，更新 starts 即可。
                    _ => {}
                }
                continue;
            }
            self.mark[pc] = self.gen;
            self.starts[pc] = start;
            match &self.nfa.insts[pc] {
                Inst::Split(a, b) => {
                    stack.push((*a, start));
                    stack.push((*b, start));
                }
                Inst::Jmp(x) => stack.push((*x, start)),
                Inst::Char(_) | Inst::Any | Inst::Class { .. } | Inst::Match => {
                    set.pcs.push(pc);
                }
            }
        }
    }

    fn begin_set(&mut self, set: &mut StateSet) {
        set.pcs.clear();
        self.gen += 1;
    }

    fn inst_matches(inst: &Inst, c: char) -> bool {
        match inst {
            Inst::Char(x) => *x == c,
            Inst::Any => true,
            Inst::Class { negated, ranges } => {
                let inside = ranges.iter().any(|&(lo, hi)| lo <= c && c <= hi);
                inside != *negated
            }
            _ => false,
        }
    }

    /// full match：整个输入必须被 pattern 完整消费。
    pub fn is_full_match(&mut self, input: &[char]) -> bool {
        let mut clist = StateSet { pcs: Vec::new() };
        let mut nlist = StateSet { pcs: Vec::new() };
        self.begin_set(&mut clist);
        let start = self.nfa.start;
        self.add_state(&mut clist, start, 0);
        for (i, &c) in input.iter().enumerate() {
            let _ = i;
            self.begin_set(&mut nlist);
            for k in 0..clist.pcs.len() {
                let pc = clist.pcs[k];
                let st = self.starts[pc];
                if Self::inst_matches(&self.nfa.insts[pc], c) {
                    self.add_state(&mut nlist, pc + 1, st);
                }
            }
            std::mem::swap(&mut clist, &mut nlist);
            if clist.pcs.is_empty() {
                return false; // 状态集已空，提前退出
            }
        }
        clist
            .pcs
            .iter()
            .any(|&pc| self.nfa.insts[pc] == Inst::Match)
    }

    /// search：返回最左最长匹配 (start, end)，下标为字符下标，end 为开区间。
    /// 即所有匹配中起始下标最小者；起始相同时取结束下标最大者
    /// （leftmost-longest，POSIX 语义）。
    pub fn find(&mut self, input: &[char]) -> Option<(usize, usize)> {
        let mut clist = StateSet { pcs: Vec::new() };
        let mut nlist = StateSet { pcs: Vec::new() };
        let mut best: Option<(usize, usize)> = None;
        self.begin_set(&mut clist);
        let start_pc = self.nfa.start;
        self.add_state(&mut clist, start_pc, 0);
        self.update_best(&clist, 0, &mut best);
        for (i, &c) in input.iter().enumerate() {
            self.begin_set(&mut nlist);
            for k in 0..clist.pcs.len() {
                let pc = clist.pcs[k];
                let st = self.starts[pc];
                if Self::inst_matches(&self.nfa.insts[pc], c) {
                    self.add_state(&mut nlist, pc + 1, st);
                }
            }
            // 非锚定：每个位置都允许开启新线程。
            self.add_state(&mut nlist, start_pc, i + 1);
            std::mem::swap(&mut clist, &mut nlist);
            self.update_best(&clist, i + 1, &mut best);
        }
        best
    }

    fn update_best(&self, set: &StateSet, end: usize, best: &mut Option<(usize, usize)>) {
        if let Some(s) = self.min_match_start(set) {
            let better = match *best {
                None => true,
                Some((bs, be)) => s < bs || (s == bs && end > be),
            };
            if better {
                *best = Some((s, end));
            }
        }
    }

    fn min_match_start(&self, set: &StateSet) -> Option<usize> {
        set.pcs
            .iter()
            .filter(|&&pc| self.nfa.insts[pc] == Inst::Match)
            .map(|&pc| self.starts[pc])
            .min()
    }
}
