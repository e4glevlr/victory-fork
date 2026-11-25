//! Per-view dirty-page marking for buffer UAV writes (`SLOT_VIEW_MARKS` /
//! `SLOT_VIEW_WRITES`). A sibling module of `lower.rs` so it shares `Lower`'s
//! state without pushing `lower.rs` past the policy size cap.

use super::Lower;
use crate::emit::bitcode::{Attr, Bin, Tag, V, pred};
use crate::emit::shader::{SLOT_VIEW_MARKS, SLOT_VIEW_WRITES, VIEW_MARK_NONE, VIEW_WRITE_BITS};

/// Byte offset of a buffer UAV write relative to the start of its view.
pub(super) enum WriteOffset {
    /// Already a byte offset from the view's start.
    Byte(V),
    /// Structured buffer element `index` times constant `stride`, plus byte `off`.
    Strided(V, u32, V),
    /// Typed buffer element `index`; element stride is packed into the upper 16
    /// bits of `SLOT_VIEW_MARKS` word 1 so marking needs no `SLOT_STRIDES`.
    Typed(V),
}

impl Lower<'_> {
    /// Marks the 64 KiB piece(s) touched by a write of `bytes` bytes at `offset`
    /// through heap buffer UAV handle `h` in `u<SLOT_VIEW_WRITES>`, when
    /// `self.x.view_marks` is enabled and `h` came from `heap_handle`.
    ///
    /// Must be called inside the write's active-lane guard so only lanes that
    /// execute the write set bits.
    pub(super) fn mark_view_write(&mut self, h: V, offset: WriteOffset, bytes: u32) {
        if !self.x.view_marks || bytes == 0 {
            return;
        }
        let Some(&slot) = self.s.view_index.get(&h) else {
            return;
        };
        let c = self.cur;
        let i32t = self.s.t.i32;
        let iu = self.s.m.undef(i32t);
        let k3 = self.s.m.i32(3);
        let entry_byte = self.s.m.bin(c, Bin::Shl, slot, k3);
        let marks = self.s.root_srv_handle(SLOT_VIEW_MARKS);
        let mask2 = self.s.m.i8(3);
        let align4 = self.s.m.i32(4);
        let loaded = self.s.op(
            c,
            "rawBufferLoad.f32",
            Attr::ReadOnly,
            self.s.t.res_ret,
            139,
            &[marks, entry_byte, iu, mask2, align4],
        );
        let raw0 = self.s.m.extract(c, loaded, 0);
        let raw1 = self.s.m.extract(c, loaded, 1);
        let base = self.as_int(raw0);
        let word1 = self.as_int(raw1);
        let none = self.s.m.i32(VIEW_MARK_NONE);
        let tracked = self.s.m.cmp(c, pred::NE, base, none);
        let at = self.begin_guarded(Some(tracked));
        let c = self.cur;
        let kffff = self.s.m.i32(0xFFFF);
        let phase = self.s.m.bin(c, Bin::And, word1, kffff);
        let byte_off = match offset {
            WriteOffset::Byte(x) => x,
            WriteOffset::Strided(index, stride, off) => {
                let s = self.s.m.i32(stride);
                let scaled = self.s.m.bin(c, Bin::Mul, index, s);
                self.s.m.bin(c, Bin::Add, scaled, off)
            }
            WriteOffset::Typed(index) => {
                let k16 = self.s.m.i32(16);
                let stride = self.s.m.bin(c, Bin::LShr, word1, k16);
                self.s.m.bin(c, Bin::Mul, index, stride)
            }
        };
        let k16 = self.s.m.i32(16);
        let start = self.s.m.bin(c, Bin::Add, phase, byte_off);
        let first_piece = self.s.m.bin(c, Bin::LShr, start, k16);
        let bit0 = self.s.m.bin(c, Bin::Add, base, first_piece);
        self.set_write_bit(bit0);
        if bytes > 1 {
            let c = self.cur;
            let span = self.s.m.i32(bytes - 1);
            let end = self.s.m.bin(c, Bin::Add, start, span);
            let last_piece = self.s.m.bin(c, Bin::LShr, end, k16);
            let crosses = self.s.m.cmp(c, pred::NE, last_piece, first_piece);
            let extra = self.begin_guarded(Some(crosses));
            let c = self.cur;
            let bit1 = self.s.m.bin(c, Bin::Add, base, last_piece);
            self.set_write_bit(bit1);
            self.end_guarded(extra, &[]);
        }
        self.end_guarded(at, &[]);
    }

    /// Sets `bit` (clamped to `VIEW_WRITE_BITS - 1`, the reserved sink bit) in
    /// `u<SLOT_VIEW_WRITES>` via atomic OR.
    fn set_write_bit(&mut self, bit: V) {
        let c = self.cur;
        let i32t = self.s.t.i32;
        let iu = self.s.m.undef(i32t);
        let last = self.s.m.i32(VIEW_WRITE_BITS - 1);
        let clamped = self.s.op(c, "binary.i32", Attr::ReadNone, i32t, 40, &[bit, last]);
        self.s.m.tag(clamped, Tag::Clamp);
        let k5 = self.s.m.i32(5);
        let k2 = self.s.m.i32(2);
        let k31 = self.s.m.i32(31);
        let one = self.s.m.i32(1);
        let word = self.s.m.bin(c, Bin::LShr, clamped, k5);
        let word_byte = self.s.m.bin(c, Bin::Shl, word, k2);
        let shift = self.s.m.bin(c, Bin::And, clamped, k31);
        let mask = self.s.m.bin(c, Bin::Shl, one, shift);
        let writes = self.s.root_uav_handle(SLOT_VIEW_WRITES);
        let or = self.s.m.i32(2);
        self.s
            .op(c, "atomicBinOp.i32", Attr::NoUnwind, i32t, 78, &[writes, or, word_byte, iu, iu, mask]);
    }
}
