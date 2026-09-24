// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which host arrays a device effect brings current or leaves lagging.
//!
//! A handle's device buffer can be mirrored by more than one host array: two
//! bindings carry one handle when one adopted the other's — a move out of a
//! borrowed parameter, or a binding bound to a reduction's output buffer. Each
//! of them holds a host array of its own, so a readback into one leaves the
//! other exactly as far behind as it was. The residency analyses therefore
//! track each pair of a handle and a binding carrying it — a mirror — rather
//! than the handle alone.

use super::{gpu_handle, DeviceEffect};
use crate::mir::{Body, Local};
use std::collections::BTreeMap;

/// One host array mirroring one device buffer: the handle, and the binding
/// whose host array it is.
pub(crate) type Mirror = (u64, Local);

/// What one terminator does to the mirrors of the handles it names.
#[derive(Debug, Default)]
pub(crate) struct MirrorChanges {
    /// Mirrors whose host array now holds what the device buffer holds.
    pub current: Vec<Mirror>,
    /// Mirrors whose host array may now lag the device buffer.
    pub lagging: Vec<Mirror>,
}

/// Every gpu binding of a body, grouped by the device handle it carries.
pub(crate) struct HandleCarriers(BTreeMap<u64, Vec<Local>>);

impl HandleCarriers {
    pub(crate) fn of(body: &Body) -> Self {
        let mut carriers: BTreeMap<u64, Vec<Local>> = BTreeMap::new();
        for local in (0..body.local_decls.len()).map(Local) {
            if let Some(handle) = gpu_handle(body, local) {
                carriers.entry(handle.0).or_default().push(local);
            }
        }
        HandleCarriers(carriers)
    }

    /// The bindings carrying `handle`.
    pub(crate) fn carrying(&self, handle: u64) -> &[Local] {
        self.0.get(&handle).map_or(&[], Vec::as_slice)
    }

    /// Whether more than one binding carries `handle`.
    pub(crate) fn is_shared(&self, handle: u64) -> bool {
        self.carrying(handle).len() > 1
    }

    fn mirrors(&self, handle: u64) -> impl Iterator<Item = Mirror> + '_ {
        self.carrying(handle)
            .iter()
            .map(move |&local| (handle, local))
    }

    /// The mirrors that lag on entry to the body: every one of a parameter's
    /// handle, whose buffer belongs to a caller that may have launched on it.
    /// A binding the body declares starts current — until a launch touches it
    /// the handle has no device buffer, and its host value is the only copy.
    pub(crate) fn lagging_at_body_entry(&self, body: &Body) -> Vec<Mirror> {
        (1..=body.arg_count)
            .filter(|&index| index < body.local_decls.len())
            .filter_map(|index| gpu_handle(body, Local(index)))
            .flat_map(|handle| self.mirrors(handle.0))
            .collect()
    }

    /// What `effect` does to the mirrors of the handles it names.
    ///
    /// A readback into a binding brings only that binding's host array current.
    /// A scalar reads back through a one-element array no binding carries, and
    /// its result is copied into the scalar the readback was emitted for, which
    /// the call does not name — so every mirror of the handle counts as current.
    ///
    /// An upload brings the binding it assigns current, but a handle another
    /// binding also carries now holds a value that binding's host array does
    /// not: the upload is then, to every mirror, as good as a launch.
    pub(crate) fn changes(&self, effect: &DeviceEffect<'_>) -> MirrorChanges {
        match *effect {
            DeviceEffect::Activates(handle) => self.all_current(handle),
            DeviceEffect::ReadsBack(handle, Some(local))
                if self.carrying(handle).contains(&local) =>
            {
                MirrorChanges {
                    current: vec![(handle, local)],
                    lagging: Vec::new(),
                }
            }
            DeviceEffect::ReadsBack(handle, _) => self.all_current(handle),
            DeviceEffect::Uploads(handle) if self.is_shared(handle) => MirrorChanges {
                current: Vec::new(),
                lagging: self.mirrors(handle).collect(),
            },
            DeviceEffect::Uploads(handle) => self.all_current(handle),
            DeviceEffect::Transfers { from, to } => MirrorChanges {
                current: self.mirrors(from).collect(),
                lagging: self.mirrors(to).collect(),
            },
            DeviceEffect::Launches(handles) => MirrorChanges {
                current: Vec::new(),
                lagging: handles
                    .iter()
                    .flatten()
                    .flat_map(|handle| self.mirrors(handle.0))
                    .collect(),
            },
            DeviceEffect::Nothing => MirrorChanges::default(),
        }
    }

    fn all_current(&self, handle: u64) -> MirrorChanges {
        MirrorChanges {
            current: self.mirrors(handle).collect(),
            lagging: Vec::new(),
        }
    }
}
