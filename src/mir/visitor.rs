// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::mir::block::{BasicBlock, BasicBlockData};
use crate::mir::body::{Body, LocalDecl};
use crate::mir::operand::{Constant, Operand};
use crate::mir::place::{Local, Place, PlaceContext};
use crate::mir::rvalue::Rvalue;
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::terminator::{Terminator, TerminatorKind};

/// A visitor trait for traversing the MIR.
pub trait Visitor {
    fn visit_body(&mut self, body: &Body) {
        for (i, block) in body.basic_blocks.iter().enumerate() {
            self.visit_basic_block(BasicBlock(i), block);
        }
        for (i, decl) in body.local_decls.iter().enumerate() {
            self.visit_local_decl(Local(i), decl);
        }
    }

    fn visit_basic_block(&mut self, block: BasicBlock, data: &BasicBlockData) {
        for statement in &data.statements {
            self.visit_statement(block, statement);
        }
        if let Some(terminator) = &data.terminator {
            self.visit_terminator(block, terminator);
        }
    }

    fn visit_statement(&mut self, block: BasicBlock, statement: &Statement) {
        match &statement.kind {
            StatementKind::Assign(place, rvalue) => {
                self.visit_assign(block, place, rvalue);
            }
            StatementKind::StorageLive(place) => {
                self.visit_place(place, PlaceContext::StorageLive);
            }
            StatementKind::StorageDead(place) => {
                self.visit_place(place, PlaceContext::StorageDead);
            }
            StatementKind::Nop => {}
        }
    }

    fn visit_assign(&mut self, _block: BasicBlock, place: &Place, rvalue: &Rvalue) {
        self.visit_place(place, PlaceContext::MutatingUse);
        self.visit_rvalue(rvalue);
    }

    fn visit_terminator(&mut self, _block: BasicBlock, terminator: &Terminator) {
        match &terminator.kind {
            TerminatorKind::Goto { target: _ } => {}
            TerminatorKind::SwitchInt {
                discr,
                targets: _,
                otherwise: _,
            } => {
                self.visit_operand(discr);
            }
            TerminatorKind::Return => {}
            TerminatorKind::Unreachable => {}
            TerminatorKind::Call {
                func,
                args,
                destination,
                target: _,
            } => {
                self.visit_operand(func);
                for arg in args {
                    self.visit_operand(arg);
                }
                self.visit_place(destination, PlaceContext::MutatingUse);
            }

            TerminatorKind::GpuLaunch {
                kernel,
                grid,
                block,
                destination,
                target: _,
            } => {
                self.visit_operand(kernel);
                self.visit_operand(grid);
                self.visit_operand(block);
                self.visit_place(destination, PlaceContext::MutatingUse);
            }
        }
    }

    fn visit_rvalue(&mut self, rvalue: &Rvalue) {
        match rvalue {
            Rvalue::Use(op) => self.visit_operand(op),
            Rvalue::Ref(place) => self.visit_place(place, PlaceContext::NonMutatingUse),
            Rvalue::BinaryOp(_, lhs, rhs) => {
                self.visit_operand(lhs);
                self.visit_operand(rhs);
            }
            Rvalue::UnaryOp(_, val) => self.visit_operand(val),
            Rvalue::Cast(op, _) => self.visit_operand(op),
            Rvalue::Len(place) => self.visit_place(place, PlaceContext::NonMutatingUse),
            Rvalue::GpuIntrinsic(_) => {}
        }
    }

    fn visit_operand(&mut self, operand: &Operand) {
        match operand {
            Operand::Copy(place) => self.visit_place(place, PlaceContext::NonMutatingUse),
            Operand::Move(place) => self.visit_place(place, PlaceContext::NonMutatingUse), // Move is also a use
            Operand::Constant(constant) => self.visit_constant(constant),
        }
    }

    fn visit_place(&mut self, place: &Place, _context: PlaceContext) {
        self.visit_local(place.local);
    }

    fn visit_local(&mut self, _local: Local) {}

    fn visit_local_decl(&mut self, _local: Local, _decl: &LocalDecl) {}

    fn visit_constant(&mut self, _constant: &Constant) {}
}
