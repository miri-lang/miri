// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The samples a profiled run took, kept in one buffer allocated before the
//! first sample, which a signal handler appends to without a lock or an
//! allocation.
//!
//! A sample is a header word then its frames. A writer claims its words with
//! one atomic add, writes the header with the frame count, the frames, and
//! then the header again with its complete bit, so a reader that finds a
//! header knows how far to step over it even while its frames are still
//! being written. A claim that would run past the end is refused and
//! counted, and leaves the cursor where it was, so the claims taken are one
//! contiguous run from the start of the buffer and the cursor never passes
//! its end.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// The header bit set once every frame of a sample is written.
const COMPLETE: u64 = 1;

// The buffer is allocated as zeroed `u64`s and read as `AtomicU64`s.
const _: () = assert!(std::mem::align_of::<u64>() == std::mem::align_of::<AtomicU64>());
const _: () = assert!(std::mem::size_of::<u64>() == std::mem::size_of::<AtomicU64>());

/// A fixed-capacity buffer of samples.
pub(crate) struct SampleBuffer {
    words: Box<[AtomicU64]>,
    /// The first word no claim has taken yet; never past the capacity.
    cursor: AtomicUsize,
    /// The samples refused for want of room.
    dropped: AtomicU64,
}

impl SampleBuffer {
    /// A buffer of `capacity` words. The words are taken zeroed from the
    /// allocator, which for a large buffer maps pages that occupy no memory
    /// until a sample is written into them.
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        let words = vec![0u64; capacity].into_boxed_slice();
        let words = Box::into_raw(words) as *mut [AtomicU64];
        // SAFETY: `AtomicU64` has the size, alignment (asserted above) and
        // bit validity of `u64`, so the allocation holds `capacity` valid
        // `AtomicU64`s and is freed with the layout it was allocated with.
        let words = unsafe { Box::from_raw(words) };
        Self {
            words,
            cursor: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// Append one sample of `frames`, or count it as dropped when the buffer
    /// has no room. Safe to call from a signal handler: it allocates nothing,
    /// takes no lock and never blocks. An empty sample is not recorded.
    pub(crate) fn push(&self, frames: &[usize]) -> bool {
        if frames.is_empty() {
            return false;
        }
        let words = frames.len() + 1;
        let capacity = self.words.len();
        let claimed = self
            .cursor
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |start| {
                start.checked_add(words).filter(|&end| end <= capacity)
            });
        let Some(slots) = claimed
            .ok()
            .and_then(|start| self.words.get(start..start + words))
        else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        };
        let header = (frames.len() as u64) << 1;
        slots[0].store(header, Ordering::Relaxed);
        for (slot, &frame) in slots[1..].iter().zip(frames) {
            slot.store(frame as u64, Ordering::Relaxed);
        }
        slots[0].store(header | COMPLETE, Ordering::Release);
        true
    }

    /// The samples refused for want of room.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Hand every complete sample's frames to `visit`, in the order they
    /// were taken. A sample still being written when this runs is counted
    /// in the returned number and not visited.
    pub(crate) fn for_each_sample(&self, mut visit: impl FnMut(&[usize])) -> u64 {
        let end = self.cursor.load(Ordering::Acquire).min(self.words.len());
        let mut frames = Vec::new();
        let mut incomplete = 0;
        let mut position = 0;
        while position < end {
            let header = self.words[position].load(Ordering::Acquire);
            let count = (header >> 1) as usize;
            if count == 0 {
                // Claimed, but interrupted before even its length was written:
                // nothing says where the next sample starts.
                incomplete += 1;
                break;
            }
            let body = position + 1..position + 1 + count;
            if header & COMPLETE == 0 {
                incomplete += 1;
            } else if let Some(words) = self.words.get(body) {
                frames.clear();
                frames.extend(
                    words
                        .iter()
                        .map(|word| word.load(Ordering::Relaxed) as usize),
                );
                visit(&frames);
            }
            position += count + 1;
        }
        incomplete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(buffer: &SampleBuffer) -> Vec<Vec<usize>> {
        let mut seen = Vec::new();
        buffer.for_each_sample(|frames| seen.push(frames.to_vec()));
        seen
    }

    #[test]
    fn samples_come_back_in_the_order_they_were_taken() {
        let buffer = SampleBuffer::with_capacity(16);
        assert!(buffer.push(&[1, 2, 3]));
        assert!(buffer.push(&[4]));
        assert_eq!(samples(&buffer), vec![vec![1, 2, 3], vec![4]]);
        assert_eq!(buffer.dropped(), 0);
    }

    #[test]
    fn a_sample_that_does_not_fit_is_dropped_and_a_smaller_one_still_fits() {
        let buffer = SampleBuffer::with_capacity(7);
        assert!(buffer.push(&[1, 2, 3]));
        assert!(!buffer.push(&[4, 5, 6]), "four words do not fit in three");
        assert!(buffer.push(&[7]), "two words still fit in three");
        assert_eq!(samples(&buffer), vec![vec![1, 2, 3], vec![7]]);
        assert_eq!(buffer.dropped(), 1);
    }

    #[test]
    fn a_sample_filling_the_buffer_exactly_is_kept() {
        let buffer = SampleBuffer::with_capacity(4);
        assert!(buffer.push(&[1, 2, 3]));
        assert_eq!(samples(&buffer), vec![vec![1, 2, 3]]);
        assert_eq!(buffer.dropped(), 0);
    }

    #[test]
    fn a_sample_still_being_written_is_counted_and_skipped() {
        let buffer = SampleBuffer::with_capacity(16);
        assert!(buffer.push(&[1, 2]));
        // A writer interrupted between its claim and its complete header.
        let start = buffer.cursor.fetch_add(3, Ordering::Relaxed);
        buffer.words[start].store(2 << 1, Ordering::Relaxed);
        assert!(buffer.push(&[9]));
        let mut seen = Vec::new();
        let incomplete = buffer.for_each_sample(|frames| seen.push(frames.to_vec()));
        assert_eq!(seen, vec![vec![1, 2], vec![9]]);
        assert_eq!(incomplete, 1);
    }

    #[test]
    fn an_empty_sample_is_not_recorded() {
        let buffer = SampleBuffer::with_capacity(4);
        assert!(!buffer.push(&[]));
        assert!(samples(&buffer).is_empty());
        assert_eq!(buffer.dropped(), 0);
    }

    #[test]
    fn a_refused_claim_leaves_the_cursor_inside_the_buffer() {
        let buffer = SampleBuffer::with_capacity(6);
        assert!(buffer.push(&[1, 2]));
        for _ in 0..1000 {
            assert!(!buffer.push(&[3, 4, 5]));
        }
        assert_eq!(buffer.cursor.load(Ordering::Relaxed), 3);
        assert!(buffer.push(&[6, 7]), "a sample that still fits is kept");
        assert_eq!(buffer.cursor.load(Ordering::Relaxed), 6);
        assert_eq!(samples(&buffer), vec![vec![1, 2], vec![6, 7]]);
        assert_eq!(buffer.dropped(), 1000);
    }
}
