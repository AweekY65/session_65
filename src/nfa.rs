//! Thompson construction: compile an AST into an NFA program.
//!
//! The NFA is a flat vector of instructions. Epsilon transitions are
//! represented by `Split` and `Jmp`; character-consuming states are
//! `Char`, `Any` and `Class`; `Match` is the single accepting state.

use crate::ast::{Ast, CharClass, RepeatKind};

#[derive(Debug, Clone)]
pub enum Inst {
    /// Consume exactly this character.
    Char(char),
    /// Consume any single character.
    Any,
    /// Consume one character matched by the class.
    Class(CharClass),
    /// Epsilon transition: both targets may be followed.
    Split(usize, usize),
    /// Unconditional epsilon jump.
    Jmp(usize),
    /// Accepting state.
    Match,
}

/// A compiled NFA program. State 0 is the start state; the last
/// instruction is always the single `Match` state.
pub struct Prog {
    pub insts: Vec<Inst>,
}

impl Prog {
    pub fn compile(ast: &Ast) -> Prog {
        let mut insts = Vec::new();
        compile_ast(ast, &mut insts);
        insts.push(Inst::Match);
        Prog { insts }
    }

    pub fn len(&self) -> usize {
        self.insts.len()
    }
}

fn compile_ast(ast: &Ast, insts: &mut Vec<Inst>) {
    match ast {
        Ast::Empty => {
            // No instructions: control falls through (pure epsilon).
        }
        Ast::Literal(c) => insts.push(Inst::Char(*c)),
        Ast::AnyChar => insts.push(Inst::Any),
        Ast::Class(class) => insts.push(Inst::Class(class.clone())),
        Ast::Concat(items) => {
            for item in items {
                compile_ast(item, insts);
            }
        }
        Ast::Alt(branches) => compile_alt(branches, insts),
        Ast::Repeat { node, kind } => match kind {
            // L: Split(L+1, End)
            // L+1: <body>
            //      Jmp(L)
            // End:
            RepeatKind::Star => {
                let split = insts.len();
                insts.push(Inst::Split(0, 0)); // patched below
                compile_ast(node, insts);
                insts.push(Inst::Jmp(split));
                let end = insts.len();
                insts[split] = Inst::Split(split + 1, end);
            }
            // L: <body>
            //    Split(L, End)
            // End:
            RepeatKind::Plus => {
                let body = insts.len();
                compile_ast(node, insts);
                let split = insts.len();
                insts.push(Inst::Split(body, split + 1));
            }
            // S: Split(S+1, End)
            // S+1: <body>
            // End:
            RepeatKind::Quest => {
                let split = insts.len();
                insts.push(Inst::Split(0, 0)); // patched below
                compile_ast(node, insts);
                let end = insts.len();
                insts[split] = Inst::Split(split + 1, end);
            }
        },
    }
}

fn compile_alt(branches: &[Ast], insts: &mut Vec<Inst>) {
    // Chain of splits:
    //   S1: Split(B1, S2)   B1: <branch1> Jmp(End)
    //   S2: Split(B2, S3)   B2: <branch2> Jmp(End)
    //   ...                 Bn: <branchN>
    //   End:
    let mut jmp_holes = Vec::new();
    for (i, branch) in branches.iter().enumerate() {
        if i + 1 < branches.len() {
            let split = insts.len();
            insts.push(Inst::Split(0, 0)); // second target patched below
            compile_ast(branch, insts);
            jmp_holes.push(insts.len());
            insts.push(Inst::Jmp(0)); // target patched to End below
            let next_split = insts.len();
            insts[split] = Inst::Split(split + 1, next_split);
        } else {
            compile_ast(branch, insts);
        }
    }
    let end = insts.len();
    for hole in jmp_holes {
        insts[hole] = Inst::Jmp(end);
    }
}
