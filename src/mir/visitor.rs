// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::block::{BasicBlock, BasicBlockData};
use crate::mir::body::{Body, LocalDecl};
use crate::mir::operand::{Constant, Operand};
use crate::mir::place::{Local, Place, PlaceContext, PlaceElem};
use crate::mir::rvalue::Rvalue;
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::terminator::{Terminator, TerminatorKind};

/// A visitor trait for traversing the MIR.
pub trait Visitor {
    fn visit_body(&mut self, body: &Body) {
        for (i, decl) in body.local_decls.iter().enumerate() {
            self.visit_local_decl(Local(i), decl);
        }
        for (i, block) in body.basic_blocks.iter().enumerate() {
            self.visit_basic_block(BasicBlock(i), block);
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
                self.visit_place(place, PlaceContext::StorageLive, block);
            }
            StatementKind::StorageDead(place) => {
                self.visit_place(place, PlaceContext::StorageDead, block);
            }
            StatementKind::Nop => {}
        }
    }

    fn visit_assign(&mut self, block: BasicBlock, place: &Place, rvalue: &Rvalue) {
        self.visit_place(place, PlaceContext::MutatingUse, block);
        self.visit_rvalue(rvalue, block);
    }

    fn visit_terminator(&mut self, block: BasicBlock, terminator: &Terminator) {
        match &terminator.kind {
            TerminatorKind::Goto { .. } => {}
            TerminatorKind::SwitchInt { discr, .. } => {
                self.visit_operand(discr, block);
            }
            TerminatorKind::Return | TerminatorKind::Unreachable => {}
            TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } => {
                self.visit_operand(func, block);
                for arg in args {
                    self.visit_operand(arg, block);
                }
                self.visit_place(destination, PlaceContext::MutatingUse, block);
            }
            TerminatorKind::GpuLaunch {
                kernel,
                grid,
                block: grid_block,
                destination,
                ..
            } => {
                self.visit_operand(kernel, block);
                self.visit_operand(grid, block);
                self.visit_operand(grid_block, block);
                self.visit_place(destination, PlaceContext::MutatingUse, block);
            }
        }
    }

    fn visit_rvalue(&mut self, rvalue: &Rvalue, location: BasicBlock) {
        match rvalue {
            Rvalue::Use(op) => self.visit_operand(op, location),
            Rvalue::Ref(place) => self.visit_place(place, PlaceContext::NonMutatingUse, location),
            Rvalue::BinaryOp(_, lhs, rhs) => {
                self.visit_operand(lhs, location);
                self.visit_operand(rhs, location);
            }
            Rvalue::UnaryOp(_, val) => self.visit_operand(val, location),
            Rvalue::Cast(op, _) => self.visit_operand(op, location),
            Rvalue::Len(place) => self.visit_place(place, PlaceContext::NonMutatingUse, location),
            Rvalue::GpuIntrinsic(_) => {}
            Rvalue::Aggregate(_, ops) => {
                for op in ops {
                    self.visit_operand(op, location);
                }
            }
            Rvalue::Phi(args) => {
                for (op, _) in args {
                    self.visit_operand(op, location);
                }
            }
        }
    }

    fn visit_operand(&mut self, operand: &Operand, location: BasicBlock) {
        match operand {
            Operand::Copy(place) => self.visit_place(place, PlaceContext::NonMutatingUse, location),
            Operand::Move(place) => self.visit_place(place, PlaceContext::NonMutatingUse, location), // Move is also a use
            Operand::Constant(constant) => self.visit_constant(constant, location),
        }
    }

    fn visit_place(&mut self, place: &Place, context: PlaceContext, location: BasicBlock) {
        self.visit_local(place.local, context, location);
        self.visit_projection(place, context, location);
    }

    fn visit_projection(&mut self, place: &Place, _context: PlaceContext, location: BasicBlock) {
        for elem in &place.projection {
            if let PlaceElem::Index(local) = elem {
                self.visit_local(*local, PlaceContext::NonMutatingUse, location);
            }
        }
    }

    fn visit_local(&mut self, _local: Local, _context: PlaceContext, _location: BasicBlock) {}

    fn visit_local_decl(&mut self, _local: Local, _decl: &LocalDecl) {}

    fn visit_constant(&mut self, _constant: &Constant, _location: BasicBlock) {}
}

/// A visitor trait for traversing and modifying the MIR.
pub trait MutVisitor {
    fn visit_body(&mut self, body: &mut Body) {
        for (i, decl) in body.local_decls.iter_mut().enumerate() {
            self.visit_local_decl(Local(i), decl);
        }
        for (i, block) in body.basic_blocks.iter_mut().enumerate() {
            self.visit_basic_block(BasicBlock(i), block);
        }
    }

    fn visit_basic_block(&mut self, block: BasicBlock, data: &mut BasicBlockData) {
        for statement in &mut data.statements {
            self.visit_statement(block, statement);
        }
        if let Some(terminator) = &mut data.terminator {
            self.visit_terminator(block, terminator);
        }
    }

    fn visit_statement(&mut self, block: BasicBlock, statement: &mut Statement) {
        match &mut statement.kind {
            StatementKind::Assign(place, rvalue) => {
                self.visit_assign(block, place, rvalue);
            }
            StatementKind::StorageLive(place) => {
                self.visit_place(place, PlaceContext::StorageLive, block);
            }
            StatementKind::StorageDead(place) => {
                self.visit_place(place, PlaceContext::StorageDead, block);
            }
            StatementKind::Nop => {}
        }
    }

    fn visit_assign(&mut self, block: BasicBlock, place: &mut Place, rvalue: &mut Rvalue) {
        self.visit_place(place, PlaceContext::MutatingUse, block);
        self.visit_rvalue(rvalue, block);
    }

    fn visit_terminator(&mut self, block: BasicBlock, terminator: &mut Terminator) {
        match &mut terminator.kind {
            TerminatorKind::Goto { .. } => {}
            TerminatorKind::SwitchInt { discr, .. } => {
                self.visit_operand(discr, block);
            }
            TerminatorKind::Return | TerminatorKind::Unreachable => {}
            TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } => {
                self.visit_operand(func, block);
                for arg in args {
                    self.visit_operand(arg, block);
                }
                self.visit_place(destination, PlaceContext::MutatingUse, block);
            }
            TerminatorKind::GpuLaunch {
                kernel,
                grid,
                block: grid_block,
                destination,
                ..
            } => {
                self.visit_operand(kernel, block);
                self.visit_operand(grid, block);
                self.visit_operand(grid_block, block);
                self.visit_place(destination, PlaceContext::MutatingUse, block);
            }
        }
    }

    fn visit_rvalue(&mut self, rvalue: &mut Rvalue, location: BasicBlock) {
        match rvalue {
            Rvalue::Use(op) => self.visit_operand(op, location),
            Rvalue::Ref(place) => self.visit_place(place, PlaceContext::NonMutatingUse, location),
            Rvalue::BinaryOp(_, lhs, rhs) => {
                self.visit_operand(lhs, location);
                self.visit_operand(rhs, location);
            }
            Rvalue::UnaryOp(_, val) => self.visit_operand(val, location),
            Rvalue::Cast(op, _) => self.visit_operand(op, location),
            Rvalue::Len(place) => self.visit_place(place, PlaceContext::NonMutatingUse, location),
            Rvalue::GpuIntrinsic(_) => {}
            Rvalue::Aggregate(_, ops) => {
                for op in ops {
                    self.visit_operand(op, location);
                }
            }
            Rvalue::Phi(args) => {
                for (op, _) in args {
                    self.visit_operand(op, location);
                }
            }
        }
    }

    fn visit_operand(&mut self, operand: &mut Operand, location: BasicBlock) {
        match operand {
            Operand::Copy(place) => self.visit_place(place, PlaceContext::NonMutatingUse, location),
            Operand::Move(place) => self.visit_place(place, PlaceContext::NonMutatingUse, location),
            Operand::Constant(constant) => self.visit_constant(constant, location),
        }
    }

    fn visit_place(&mut self, place: &mut Place, context: PlaceContext, location: BasicBlock) {
        self.visit_local(&mut place.local, context, location);
        self.visit_projection(place, context, location);
    }

    fn visit_projection(
        &mut self,
        place: &mut Place,
        _context: PlaceContext,
        location: BasicBlock,
    ) {
        for elem in &mut place.projection {
            if let PlaceElem::Index(local) = elem {
                self.visit_local(local, PlaceContext::NonMutatingUse, location);
            }
        }
    }

    fn visit_local(&mut self, _local: &mut Local, _context: PlaceContext, _location: BasicBlock) {}

    fn visit_local_decl(&mut self, _local: Local, _decl: &mut LocalDecl) {}

    fn visit_constant(&mut self, _constant: &mut Constant, _location: BasicBlock) {}
}
