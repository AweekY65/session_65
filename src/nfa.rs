//! Thompson construction：把 AST 编译成 NFA 指令序列。
//!
//! 指令集：
//!   Char / Any / Class  —— 消费一个字符，成功后隐式走向 pc+1
//!   Split(a, b)         —— epsilon 分叉，同时走向 a 和 b
//!   Jmp(x)              —— epsilon 跳转到 x
//!   Match               —— 接受状态

use crate::ast::Ast;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inst {
    Char(char),
    Any,
    Class { negated: bool, ranges: Vec<(char, char)> },
    Split(usize, usize),
    Jmp(usize),
    Match,
}

#[derive(Debug, Clone)]
pub struct Nfa {
    pub insts: Vec<Inst>,
    pub start: usize,
}

/// 指向某条指令中待回填目标位置的句柄。
#[derive(Debug, Clone, Copy)]
enum Hole {
    SplitA(usize),
    SplitB(usize),
    Jmp(usize),
}

/// 编译片段：入口 pc + 一组待回填到“下一条指令”的出口。
struct Frag {
    start: usize,
    outs: Vec<Hole>,
}

pub fn compile(ast: &Ast) -> Nfa {
    let mut c = Compiler { insts: Vec::new() };
    let frag = c.emit(ast);
    let match_pc = c.push(Inst::Match);
    c.patch_all(&frag.outs, match_pc);
    Nfa {
        insts: c.insts,
        start: frag.start,
    }
}

struct Compiler {
    insts: Vec<Inst>,
}

impl Compiler {
    fn push(&mut self, inst: Inst) -> usize {
        self.insts.push(inst);
        self.insts.len() - 1
    }

    fn patch(&mut self, hole: Hole, target: usize) {
        match hole {
            Hole::SplitA(pc) => match &mut self.insts[pc] {
                Inst::Split(a, _) => *a = target,
                _ => unreachable!(),
            },
            Hole::SplitB(pc) => match &mut self.insts[pc] {
                Inst::Split(_, b) => *b = target,
                _ => unreachable!(),
            },
            Hole::Jmp(pc) => match &mut self.insts[pc] {
                Inst::Jmp(x) => *x = target,
                _ => unreachable!(),
            },
        }
    }

    fn patch_all(&mut self, holes: &[Hole], target: usize) {
        for &h in holes {
            self.patch(h, target);
        }
    }

    fn emit(&mut self, ast: &Ast) -> Frag {
        match ast {
            // 空串编译成一条 nop 跳转，出口即自身占位符。
            Ast::Empty => {
                let pc = self.push(Inst::Jmp(0));
                Frag {
                    start: pc,
                    outs: vec![Hole::Jmp(pc)],
                }
            }
            Ast::Literal(ch) => {
                let pc = self.push(Inst::Char(*ch));
                Frag {
                    start: pc,
                    outs: vec![],
                }
            }
            Ast::AnyChar => {
                let pc = self.push(Inst::Any);
                Frag {
                    start: pc,
                    outs: vec![],
                }
            }
            Ast::Class { negated, ranges } => {
                let pc = self.push(Inst::Class {
                    negated: *negated,
                    ranges: ranges.clone(),
                });
                Frag {
                    start: pc,
                    outs: vec![],
                }
            }
            Ast::Concat(items) => {
                // 依次拼接：前一个片段的出口回填为后一个片段的入口。
                // Char/Any/Class 靠 pc+1 自然下落，无需回填。
                let mut iter = items.iter();
                let first = match iter.next() {
                    Some(a) => self.emit(a),
                    None => return self.emit(&Ast::Empty),
                };
                let start = first.start;
                let mut outs = first.outs;
                for a in iter {
                    let next = self.emit(a);
                    self.patch_all(&outs, next.start);
                    outs = next.outs;
                }
                Frag { start, outs }
            }
            Ast::Alt(branches) => {
                // 为前 n-1 个分支各生成一条 Split，链式指向下一个分支；
                // 除最后一个分支外，每个分支末尾补一条跳到汇合点的 Jmp。
                let n = branches.len();
                let split_pcs: Vec<usize> =
                    (0..n.saturating_sub(1)).map(|_| self.push(Inst::Split(0, 0))).collect();
                let mut outs: Vec<Hole> = Vec::new();
                for (i, branch) in branches.iter().enumerate() {
                    let frag = self.emit(branch);
                    if i > 0 {
                        self.patch(Hole::SplitB(split_pcs[i - 1]), frag.start);
                    }
                    if i < n - 1 {
                        self.patch(Hole::SplitA(split_pcs[i]), frag.start);
                        let jmp = self.push(Inst::Jmp(0));
                        self.patch_all(&frag.outs, jmp);
                        outs.push(Hole::Jmp(jmp));
                    } else {
                        outs.extend(frag.outs);
                    }
                }
                let start = if split_pcs.is_empty() {
                    // 单分支（理论上 parse 已折叠，防御性处理）。
                    0
                } else {
                    split_pcs[0]
                };
                Frag { start, outs }
            }
            Ast::Star(inner) => {
                // L1: Split(L2, L3)
                // L2: body
                //     Jmp L1
                // L3:
                let split = self.push(Inst::Split(0, 0));
                let body = self.emit(inner);
                let jmp = self.push(Inst::Jmp(split));
                self.patch(Hole::SplitA(split), body.start);
                self.patch_all(&body.outs, jmp);
                Frag {
                    start: split,
                    outs: vec![Hole::SplitB(split)],
                }
            }
            Ast::Plus(inner) => {
                // L1: body
                //     Split(L1, L3)
                // L3:
                let body = self.emit(inner);
                let split = self.push(Inst::Split(body.start, 0));
                self.patch_all(&body.outs, split);
                Frag {
                    start: body.start,
                    outs: vec![Hole::SplitB(split)],
                }
            }
            Ast::Quest(inner) => {
                // Split(L1, L2)
                // L1: body
                // L2:
                let split = self.push(Inst::Split(0, 0));
                let body = self.emit(inner);
                self.patch(Hole::SplitA(split), body.start);
                let mut outs = vec![Hole::SplitB(split)];
                outs.extend(body.outs);
                Frag {
                    start: split,
                    outs,
                }
            }
        }
    }
}
