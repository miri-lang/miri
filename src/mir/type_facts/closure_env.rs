// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a closure's environment holds, where, and what releasing it releases.
//!
//! A closure value is a counted block shaped like a class instance: its
//! payload begins with a [`ClosureHeader`] — the function a call enters and
//! the drop thunk of its environment — and each capture follows the header
//! in a slot of its own. Its counted captures are the members that drop
//! thunk releases, so the thunk is planned like every other one, as a
//! [`DropPlan`] with no hook.
//!
//! One [`ClosureEnv`] is built for every closure body, keyed by the closure
//! the body records it is the body of, from the types that body declares its
//! captures at. It does not depend on any body still building a value of the
//! closure: an optimisation may remove every creator while the body is still
//! emitted. The store that builds the closure, the loads that open its body
//! and the thunk that drops it all read the same [`CaptureSlot`], so none of
//! them decides where a capture lives, how wide it is or whether it is held
//! as its bytes.
//!
//! [`ClosureHeader`]: crate::abi::layout::ClosureHeader

use super::drop_plan::{CountedSlot, DropPlan, DroppedMembers};
use super::{DropHookReach, DropInstantiationRefusal};
use crate::ast::types::TypeKind;
use crate::mir::option_layout::LayoutScope;
use crate::mir::rc::is_slot_counted;
use crate::mir::symbol::Symbol;
use crate::mir::Body;
use crate::type_checker::value_class::{
    held_value_layout, value_class, ScalarKind, ValueClass, ValueLayout, WORD_BYTES,
};
use std::collections::{HashMap, HashSet};

/// Where each capture lives in a closure's payload, past its header, and how
/// many bytes the captures span.
///
/// Each capture takes a slot as wide as its value, rounded up to whole words:
/// a float keeps its own bits, a 128-bit value both of its words, and a value
/// held as its bytes all of them, inline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvLayout {
    /// Each capture's slot, in capture order.
    pub slots: Vec<CaptureSlot>,
    /// The bytes the captures span, from the end of the header.
    pub captures_bytes: u32,
}

/// One capture's place in a closure's payload and how it is held there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSlot {
    /// The byte offset of the capture from the end of the closure's header.
    pub offset: u32,
    /// How the capture is held in its slot.
    pub held: CaptureHeld,
}

/// How a capture is held in its slot of a closure's environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureHeld {
    /// A scalar, at its own width and register class.
    Scalar(ScalarKind),
    /// One pointer-width word: a counted pointer, an open type parameter, or
    /// an untracked word.
    Word,
    /// A bitwise value held as its bytes, copied in and out whole.
    Bytes(ValueLayout),
}

impl CaptureHeld {
    /// How a capture of `kind`, read in `scope`, is held: the value-class
    /// authority's answer, which decides how every binding of `kind` is held.
    fn of(kind: &TypeKind, scope: LayoutScope<'_>) -> Self {
        if let Some(layout) = held_value_layout(kind, scope) {
            return Self::Bytes(layout);
        }
        match value_class(kind, scope) {
            ValueClass::Scalar(scalar) => Self::Scalar(scalar),
            ValueClass::Bitwise(_) | ValueClass::Word(_) | ValueClass::Managed => Self::Word,
        }
    }

    /// The bytes the capture spans before its slot is rounded to whole words.
    fn bytes(&self) -> u32 {
        match self {
            Self::Scalar(scalar) => scalar.size(),
            Self::Word => WORD_BYTES,
            Self::Bytes(layout) => layout.size,
        }
    }
}

/// One closure's environment: where each capture lives and what its drop
/// thunk releases.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosureEnv {
    /// Where each capture lives in the payload, and how it is held.
    pub layout: EnvLayout,
    /// What the environment's drop thunk does: no hook, and the counted
    /// captures, numbered by their position among the captures.
    pub plan: DropPlan,
    /// The type parameters the closure body leaves open, which its captures
    /// are read at.
    pub open_params: HashSet<String>,
    /// The type parameters the closure body's substitution bound.
    pub bound_params: HashSet<String>,
}

impl ClosureEnv {
    /// The environment of the closure whose body is `body`.
    fn of(body: &Body, reach: &DropHookReach<'_>) -> Result<Self, DropInstantiationRefusal> {
        let scope = body.layout_scope();
        let captures = (body.captures.env_capture_locals.iter())
            .map(|local| {
                (body.local_decls.get(local.0))
                    .map(|decl| &decl.ty)
                    .ok_or_else(|| {
                        DropInstantiationRefusal::ClosureEnvironment(format!(
                            "capture `_{}` of a closure body has no declaration",
                            local.0
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let counted: Vec<CountedSlot> = (captures.iter().enumerate())
            .filter(|(_, ty)| is_slot_counted(&ty.kind, scope))
            .map(|(index, ty)| CountedSlot {
                index,
                ty: (*ty).clone(),
            })
            .collect();
        let may_recurse = counted.iter().any(|slot| reach.may_reach_closure(&slot.ty));
        Ok(Self {
            layout: EnvLayout::of(captures.iter().map(|ty| &ty.kind), scope)?,
            plan: DropPlan {
                hook: None,
                members: DroppedMembers::Fields(counted),
                may_recurse,
            },
            open_params: body.generics.open_params.clone(),
            bound_params: body.generics.bound_params.clone(),
        })
    }
}

impl EnvLayout {
    /// The layout of captures of `kinds`, in order, read in `scope`.
    // TODO: each slot is rounded to fixed 8-byte words (`WORD_BYTES`), which is
    // correct only for 64-bit targets. A 32-bit target needs every capture slot
    // measured in the target's pointer size instead, read from the target the
    // program is compiled for.
    fn of<'k>(
        kinds: impl IntoIterator<Item = &'k TypeKind>,
        scope: LayoutScope<'_>,
    ) -> Result<Self, DropInstantiationRefusal> {
        let mut next: u32 = 0;
        let slots = kinds
            .into_iter()
            .map(|kind| {
                let held = CaptureHeld::of(kind, scope);
                let offset = next;
                next = (held.bytes().div_ceil(WORD_BYTES).max(1))
                    .checked_mul(WORD_BYTES)
                    .and_then(|span| next.checked_add(span))
                    .ok_or_else(|| {
                        DropInstantiationRefusal::ClosureEnvironment(
                            "a closure's captures span more bytes than an offset holds".to_string(),
                        )
                    })?;
                Ok(CaptureSlot { offset, held })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            slots,
            captures_bytes: next,
        })
    }
}

/// The environment of every closure one of `bodies` is the body of, keyed by
/// the closure the body records. A body that loads captures but records no
/// closure is refused: its environment could not be named.
pub(super) fn closure_envs<'b>(
    bodies: impl IntoIterator<Item = &'b Body>,
    reach: &DropHookReach<'_>,
) -> Result<HashMap<Symbol, ClosureEnv>, DropInstantiationRefusal> {
    bodies
        .into_iter()
        .filter_map(|body| match &body.captures.closure {
            Some(closure) => Some(ClosureEnv::of(body, reach).map(|env| (closure.clone(), env))),
            None if body.captures.env_capture_locals.is_empty() => None,
            None => Some(Err(DropInstantiationRefusal::ClosureEnvironment(
                "a body loads captures but records no closure it is the body of".to_string(),
            ))),
        })
        .collect()
}
