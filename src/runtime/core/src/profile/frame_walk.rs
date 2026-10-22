// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Walking a sampled thread's frame-pointer chain from inside a signal
//! handler.
//!
//! A frame record is two words at the frame pointer: the caller's frame
//! pointer, then the return address into the caller. The walk reads nothing
//! it has not first proven to lie inside the stack it was told about, so a
//! sample taken in code that keeps no frame record, or mid-prologue, ends the
//! walk early rather than reading through a stray pointer.

/// The most frames one sample records, the sampled instruction included.
pub(crate) const MAX_FRAMES: usize = 64;

/// The alignment every frame pointer the ABI lays down keeps.
#[cfg(target_arch = "aarch64")]
const FRAME_ALIGNMENT: usize = 16;
#[cfg(not(target_arch = "aarch64"))]
const FRAME_ALIGNMENT: usize = 8;

/// The bytes of a frame record: the saved frame pointer and the return
/// address.
const FRAME_RECORD_BYTES: usize = 2 * std::mem::size_of::<usize>();

/// The bits of a code address that address memory. On AArch64 the bits above
/// them may carry a pointer-authentication code in a saved return address.
#[cfg(target_arch = "aarch64")]
const ADDRESS_MASK: usize = 0x0000_ffff_ffff_ffff;
#[cfg(not(target_arch = "aarch64"))]
const ADDRESS_MASK: usize = usize::MAX;

/// The registers a sample starts its walk from. `lr` is the link register
/// on AArch64, and 0 where there is none.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Registers {
    pub pc: usize,
    pub fp: usize,
    pub sp: usize,
    pub lr: usize,
}

/// The code of the compiled function a sample interrupted: its bytes, and the
/// interrupted instruction's offset into them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LeafCode<'c> {
    pub bytes: &'c [u8],
    pub offset: usize,
}

/// What a walk starts from: the interrupted registers, and the code of the
/// compiled function they were interrupted in, when they were.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample<'c> {
    pub registers: Registers,
    pub leaf: Option<LeafCode<'c>>,
}

/// Where a function interrupted while it has no frame record of its own —
/// before its prologue has set the frame pointer, or at its `ret` after the
/// epilogue restored the caller's — keeps the address it returns to. The
/// frame pointer then still names the caller's record, which holds the
/// caller's own return address, so without this the caller drops out of the
/// stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnframedReturn {
    /// In the stack word this many bytes above the stack pointer.
    OnStack(usize),
    /// In the link register.
    InLinkRegister,
}

/// x86-64 `push rbp`, `mov rbp, rsp`, `pop rbp` and `ret`: the frame setup
/// and teardown Cranelift emits around every function that keeps a frame.
const X86_64_PUSH_RBP: u8 = 0x55;
const X86_64_MOV_RBP_RSP: [u8; 3] = [0x48, 0x89, 0xe5];
const X86_64_POP_RBP: u8 = 0x5d;
const X86_64_RET: u8 = 0xc3;

/// Where the x86-64 function `code`, interrupted at `offset`, keeps its
/// return address when it has no frame record: at the push, on top of the
/// stack; between the push and the move, one word up; at a `ret` that follows
/// `pop rbp`, on top of the stack again. Read off the instruction bytes, so
/// a function laid out some other way is left to the frame-pointer walk.
fn x86_64_unframed_return(code: &[u8], offset: usize) -> Option<UnframedReturn> {
    let opens_its_frame =
        code.first() == Some(&X86_64_PUSH_RBP) && code.get(1..4) == Some(&X86_64_MOV_RBP_RSP[..]);
    match offset {
        0 if opens_its_frame => Some(UnframedReturn::OnStack(0)),
        1 if opens_its_frame => Some(UnframedReturn::OnStack(8)),
        _ => {
            let ret = code.get(offset) == Some(&X86_64_RET);
            let after_pop =
                offset.checked_sub(1).and_then(|before| code.get(before)) == Some(&X86_64_POP_RBP);
            (ret && after_pop).then_some(UnframedReturn::OnStack(0))
        }
    }
}

/// AArch64 `stp x29, x30, [sp, #-16]!`, `mov x29, sp`,
/// `ldp x29, x30, [sp], #16` and `ret`: Cranelift's frame setup and teardown.
const AARCH64_STP_FRAME: u32 = 0xa9bf_7bfd;
const AARCH64_MOV_FP_SP: u32 = 0x9100_03fd;
const AARCH64_LDP_FRAME: u32 = 0xa8c1_7bfd;
const AARCH64_RET: u32 = 0xd65f_03c0;

/// The AArch64 instruction at byte `offset` of `code`, if one starts there.
fn aarch64_instruction(code: &[u8], offset: usize) -> Option<u32> {
    if !offset.is_multiple_of(4) {
        return None;
    }
    let bytes = code.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Where the AArch64 function `code`, interrupted at `offset`, keeps its
/// return address when it has no frame record: in the link register, before
/// the `mov x29, sp` that sets its frame and at a `ret` that follows the
/// `ldp` restoring the caller's.
fn aarch64_unframed_return(code: &[u8], offset: usize) -> Option<UnframedReturn> {
    let opens_its_frame = aarch64_instruction(code, 0) == Some(AARCH64_STP_FRAME)
        && aarch64_instruction(code, 4) == Some(AARCH64_MOV_FP_SP);
    let in_frame_setup = opens_its_frame && (offset == 0 || offset == 4);
    let at_final_ret = aarch64_instruction(code, offset) == Some(AARCH64_RET)
        && offset
            .checked_sub(4)
            .and_then(|before| aarch64_instruction(code, before))
            == Some(AARCH64_LDP_FRAME);
    (in_frame_setup || at_final_ret).then_some(UnframedReturn::InLinkRegister)
}

/// Where `leaf`, on this target, keeps its return address if it has no
/// frame record. Both readers are compiled everywhere, so each is checked
/// and tested on any host; the target picks one.
fn unframed_return(leaf: LeafCode<'_>) -> Option<UnframedReturn> {
    if cfg!(target_arch = "aarch64") {
        aarch64_unframed_return(leaf.bytes, leaf.offset)
    } else if cfg!(target_arch = "x86_64") {
        x86_64_unframed_return(leaf.bytes, leaf.offset)
    } else {
        None
    }
}

/// The return address of a leaf interrupted with no frame record, read where
/// its code says it is: the link register, or a stack word that is aligned
/// and lies inside `stack` at or above the stack pointer.
fn unframed_caller(
    sample: Sample<'_>,
    stack: StackBounds,
    read_word: &impl Fn(usize) -> usize,
) -> Option<usize> {
    let registers = sample.registers;
    let return_address = match unframed_return(sample.leaf?)? {
        UnframedReturn::InLinkRegister => registers.lr,
        UnframedReturn::OnStack(above) => {
            let slot = registers.sp.checked_add(above)?;
            let word = std::mem::size_of::<usize>();
            let is_readable = slot.is_multiple_of(word)
                && slot >= stack.low
                && slot.checked_add(word)? <= stack.high;
            if !is_readable {
                return None;
            }
            read_word(slot)
        }
    };
    let return_address = code_address(return_address);
    (return_address != 0).then(|| return_address - 1)
}

/// The stack a walk may read: from `low` up to, not including, `high`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StackBounds {
    pub low: usize,
    pub high: usize,
}

impl StackBounds {
    /// Whether `address` lies in the stack.
    pub(crate) fn contains(&self, address: usize) -> bool {
        (self.low..self.high).contains(&address)
    }
}

/// `address` with any authentication bits cleared.
pub(crate) fn code_address(address: usize) -> usize {
    address & ADDRESS_MASK
}

/// Record into `frames` the sampled instruction and, through the frame
/// records above it, each caller's return address less one, so the address
/// falls in the call rather than on the instruction after it. Returns how
/// many frames were written. A leaf caught with no frame record of its own
/// first has its caller added from where its code keeps the return address.
///
/// A frame pointer is followed only while it is aligned, lies at or above
/// both the sampled stack pointer and `stack.low`, leaves room for its record
/// below `stack.high`, and sits above the one before it. The first one that
/// does not, a null one, or a null return address ends the walk.
/// `read_word` is only ever asked for an address those checks admitted.
pub(crate) fn walk_frames(
    sample: Sample<'_>,
    stack: StackBounds,
    read_word: impl Fn(usize) -> usize,
    frames: &mut [usize; MAX_FRAMES],
) -> usize {
    let registers = sample.registers;
    frames[0] = code_address(registers.pc);
    let mut count = 1;
    if let Some(caller) = unframed_caller(sample, stack, &read_word) {
        frames[1] = caller;
        count = 2;
    }
    let mut lowest = registers.sp.max(stack.low);
    let record_limit = stack.high.saturating_sub(FRAME_RECORD_BYTES);
    let mut fp = registers.fp;
    while count < MAX_FRAMES {
        let is_followable =
            fp != 0 && fp.is_multiple_of(FRAME_ALIGNMENT) && (lowest..record_limit).contains(&fp);
        if !is_followable {
            break;
        }
        let return_address = code_address(read_word(fp + std::mem::size_of::<usize>()));
        if return_address == 0 {
            break;
        }
        frames[count] = return_address - 1;
        count += 1;
        lowest = fp + 1;
        fp = read_word(fp);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORD: usize = std::mem::size_of::<usize>();

    /// A stack of words standing at a fixed, aligned base address, read by
    /// address the way the handler reads real memory.
    struct SyntheticStack {
        base: usize,
        words: Vec<usize>,
    }

    impl SyntheticStack {
        fn new(words: usize) -> Self {
            Self {
                base: 0x10_0000,
                words: vec![0; words],
            }
        }

        fn address(&self, word: usize) -> usize {
            self.base + word * WORD
        }

        fn bounds(&self) -> StackBounds {
            StackBounds {
                low: self.base,
                high: self.address(self.words.len()),
            }
        }

        /// Lay a frame record at word `at`: the caller's frame pointer, then
        /// the return address.
        fn frame(&mut self, at: usize, caller_fp: usize, return_address: usize) {
            self.words[at] = caller_fp;
            self.words[at + 1] = return_address;
        }

        fn read(&self, address: usize) -> usize {
            assert!(
                self.bounds().contains(address) && address.is_multiple_of(WORD),
                "the walk read {address:#x}, outside the stack it was given"
            );
            self.words[(address - self.base) / WORD]
        }

        fn walk(&self, registers: Registers) -> Vec<usize> {
            self.walk_leaf(registers, None)
        }

        fn walk_leaf(&self, registers: Registers, leaf: Option<LeafCode<'_>>) -> Vec<usize> {
            let mut frames = [0; MAX_FRAMES];
            let sample = Sample { registers, leaf };
            let count = walk_frames(sample, self.bounds(), |a| self.read(a), &mut frames);
            frames[..count].to_vec()
        }
    }

    /// The frame pointer of a record laid at word `at`, aligned for the
    /// target: synthetic frames sit at multiples of four words.
    fn at(stack: &SyntheticStack, word: usize) -> usize {
        stack.address(word)
    }

    #[test]
    fn a_chain_of_records_yields_the_pc_then_each_return_address_less_one() {
        let mut stack = SyntheticStack::new(64);
        let (inner, middle, outer) = (at(&stack, 4), at(&stack, 16), at(&stack, 32));
        stack.frame(4, middle, 0x2001);
        stack.frame(16, outer, 0x3001);
        stack.frame(32, 0, 0x4001);
        let frames = stack.walk(Registers {
            pc: 0x1000,
            fp: inner,
            sp: at(&stack, 2),
            lr: 0,
        });
        assert_eq!(frames, vec![0x1000, 0x2000, 0x3000, 0x4000]);
    }

    #[test]
    fn a_misaligned_frame_pointer_ends_the_walk() {
        let mut stack = SyntheticStack::new(64);
        let inner = at(&stack, 4);
        stack.frame(4, at(&stack, 16) + WORD / 2, 0x2001);
        let frames = stack.walk(Registers {
            pc: 0x1000,
            fp: inner,
            sp: inner,
            lr: 0,
        });
        assert_eq!(frames, vec![0x1000, 0x2000]);
    }

    #[test]
    fn a_frame_pointer_that_does_not_climb_ends_the_walk() {
        let mut stack = SyntheticStack::new(64);
        let (inner, upper) = (at(&stack, 4), at(&stack, 16));
        stack.frame(4, upper, 0x2001);
        stack.frame(16, inner, 0x3001);
        let frames = stack.walk(Registers {
            pc: 0x1000,
            fp: inner,
            sp: inner,
            lr: 0,
        });
        assert_eq!(frames, vec![0x1000, 0x2000, 0x3000]);
    }

    #[test]
    fn a_frame_pointer_below_the_stack_pointer_or_outside_the_stack_is_not_read() {
        let mut stack = SyntheticStack::new(64);
        let below_sp = at(&stack, 4);
        stack.frame(4, 0, 0x2001);
        let frames = stack.walk(Registers {
            pc: 0x1000,
            fp: below_sp,
            sp: at(&stack, 8),
            lr: 0,
        });
        assert_eq!(frames, vec![0x1000]);

        let past_the_top = stack.bounds().high;
        let frames = stack.walk(Registers {
            pc: 0x1000,
            fp: past_the_top,
            sp: at(&stack, 0),
            lr: 0,
        });
        assert_eq!(frames, vec![0x1000]);

        let record_overhangs_the_top = stack.bounds().high - WORD;
        let frames = stack.walk(Registers {
            pc: 0x1000,
            fp: record_overhangs_the_top & !(FRAME_ALIGNMENT - 1),
            sp: at(&stack, 0),
            lr: 0,
        });
        assert_eq!(frames.len(), 1, "{frames:x?}");
    }

    #[test]
    fn a_null_frame_pointer_or_return_address_ends_the_walk() {
        let mut stack = SyntheticStack::new(64);
        let frames = stack.walk(Registers {
            pc: 0x1000,
            fp: 0,
            sp: at(&stack, 0),
            lr: 0,
        });
        assert_eq!(frames, vec![0x1000]);

        let inner = at(&stack, 4);
        stack.frame(4, at(&stack, 16), 0);
        let frames = stack.walk(Registers {
            pc: 0x1000,
            fp: inner,
            sp: inner,
            lr: 0,
        });
        assert_eq!(frames, vec![0x1000]);
    }

    #[test]
    fn a_walk_stops_at_the_frame_limit() {
        let records = MAX_FRAMES + 8;
        let mut stack = SyntheticStack::new(4 * records + 8);
        for record in 0..records {
            let word = 4 * record;
            let caller = at(&stack, word + 4);
            stack.frame(word, caller, 0x1_0001 + record);
        }
        let frames = stack.walk(Registers {
            pc: 0x1000,
            fp: at(&stack, 0),
            sp: at(&stack, 0),
            lr: 0,
        });
        assert_eq!(frames.len(), MAX_FRAMES);
        assert_eq!(frames[1], 0x1_0000);
        assert_eq!(frames[MAX_FRAMES - 1], 0x1_0000 + MAX_FRAMES - 2);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn authentication_bits_are_cleared_from_every_address() {
        let mut stack = SyntheticStack::new(64);
        let inner = at(&stack, 4);
        stack.frame(4, 0, 0x002d_0000_0000_2001);
        let frames = stack.walk(Registers {
            pc: 0x0080_0000_0000_1000,
            fp: inner,
            sp: inner,
            lr: 0,
        });
        assert_eq!(frames, vec![0x1000, 0x2000]);
    }

    /// `push rbp; mov rbp, rsp; nop; mov rsp, rbp; pop rbp; ret`, the frame
    /// Cranelift lays on x86-64 around a one-byte body.
    const X86_64_FUNCTION: [u8; 10] = [0x55, 0x48, 0x89, 0xe5, 0x90, 0x48, 0x89, 0xec, 0x5d, 0xc3];

    #[test]
    fn x86_64_names_where_an_unframed_leaf_keeps_its_return_address() {
        let at = |offset| x86_64_unframed_return(&X86_64_FUNCTION, offset);
        assert_eq!(at(0), Some(UnframedReturn::OnStack(0)), "before the push");
        assert_eq!(at(1), Some(UnframedReturn::OnStack(8)), "after the push");
        assert_eq!(at(9), Some(UnframedReturn::OnStack(0)), "at the ret");
        assert_eq!(at(4), None, "in the body the frame is set up");
        assert_eq!(at(8), None, "before the pop the frame still stands");
        assert_eq!(at(10), None, "past the end of the code");
        let unframed = [0x90, 0xc3];
        assert_eq!(
            x86_64_unframed_return(&unframed, 0),
            None,
            "no push, no frame setup"
        );
        assert_eq!(
            x86_64_unframed_return(&unframed, 1),
            None,
            "a ret after no pop"
        );
    }

    /// `stp x29, x30, [sp, #-16]!; mov x29, sp; nop; ldp x29, x30, [sp], #16;
    /// ret`, the frame Cranelift lays on AArch64 around a one-instruction body.
    const AARCH64_FUNCTION: [u32; 5] = [0xa9bf7bfd, 0x910003fd, 0xd503201f, 0xa8c17bfd, 0xd65f03c0];

    fn aarch64_bytes() -> Vec<u8> {
        AARCH64_FUNCTION
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    #[test]
    fn aarch64_names_where_an_unframed_leaf_keeps_its_return_address() {
        let code = aarch64_bytes();
        let at = |offset| aarch64_unframed_return(&code, offset);
        assert_eq!(
            at(0),
            Some(UnframedReturn::InLinkRegister),
            "before the stp"
        );
        assert_eq!(
            at(4),
            Some(UnframedReturn::InLinkRegister),
            "before the mov"
        );
        assert_eq!(at(16), Some(UnframedReturn::InLinkRegister), "at the ret");
        assert_eq!(at(8), None, "in the body the frame is set up");
        assert_eq!(at(12), None, "before the ldp the frame still stands");
        assert_eq!(at(2), None, "inside an instruction");
        assert_eq!(at(20), None, "past the end of the code");
        let unframed: Vec<u8> = [0xd503201fu32, 0xd65f03c0]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        assert_eq!(aarch64_unframed_return(&unframed, 0), None);
        assert_eq!(aarch64_unframed_return(&unframed, 4), None);
    }

    /// The host's function, and the offsets of its first instruction, of the
    /// one after the frame is set up, and of its body.
    fn host_function() -> (Vec<u8>, usize, usize) {
        if cfg!(target_arch = "aarch64") {
            (aarch64_bytes(), 0, 8)
        } else {
            (X86_64_FUNCTION.to_vec(), 0, 4)
        }
    }

    #[test]
    fn a_leaf_caught_before_its_frame_is_set_up_still_names_its_caller() {
        let mut stack = SyntheticStack::new(64);
        let (sp, caller_fp, outer_fp) = (at(&stack, 2), at(&stack, 8), at(&stack, 16));
        stack.words[2] = 0x2001;
        stack.frame(8, outer_fp, 0x3001);
        stack.frame(16, 0, 0x4001);
        let (code, entry, body) = host_function();
        let registers = Registers {
            pc: 0x1000,
            fp: caller_fp,
            sp,
            lr: 0x2001,
        };
        let entered = stack.walk_leaf(
            registers,
            Some(LeafCode {
                bytes: &code,
                offset: entry,
            }),
        );
        assert_eq!(entered, vec![0x1000, 0x2000, 0x3000, 0x4000]);
        let in_body = stack.walk_leaf(
            registers,
            Some(LeafCode {
                bytes: &code,
                offset: body,
            }),
        );
        assert_eq!(
            in_body,
            vec![0x1000, 0x3000, 0x4000],
            "a framed body adds no caller"
        );
    }

    #[cfg(not(target_arch = "aarch64"))]
    #[test]
    fn a_return_address_slot_outside_the_stack_is_not_read() {
        let stack = SyntheticStack::new(8);
        let (code, entry, _) = host_function();
        let registers = Registers {
            pc: 0x1000,
            fp: 0,
            sp: stack.bounds().high,
            lr: 0,
        };
        let frames = stack.walk_leaf(
            registers,
            Some(LeafCode {
                bytes: &code,
                offset: entry,
            }),
        );
        assert_eq!(frames, vec![0x1000]);
    }
}
