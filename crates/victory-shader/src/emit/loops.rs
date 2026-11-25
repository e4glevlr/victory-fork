//! The iteration limit of translated loops: the limits by stage, the count each loop header
//! keeps and the test of it on each back edge (`Lower::exit` lowers the branches). A child
//! module of the lowering, so it shares the emitter's state.

use super::{Attr, Bin, Lower, Tag, Term, V};

/// Iterations after which a loop of a program that tests rays against tree
/// nodes is left, unless [`super::super::loop_limits`] says otherwise.
///
/// A ray's walk of a well-formed tree visits some hundreds of nodes at
/// most. The title's shadow-mask rays ran to the compute limit of 4,096: the
/// trees of moving objects are built by the title's builder, which is off,
/// so a walk into one follows whatever its memory holds. At 4,096 the
/// shadow mask took 11.7 ms of the card a frame, at 1,024 2.4 ms (timing
/// runs of the opening of the gameplay, RTX 5090); a ray stopped either way
/// leaves the program as killed, as before.
pub(in crate::emit) const TRACE_LOOP_LIMIT: u32 = 1 << 10;

/// Iterations after which a loop of a pixel program whose end the data
/// decides is left (per entry into it), unless [`super::super::loop_limits`] says
/// otherwise ([`Lower::count_iteration`]). The console runs such a loop as
/// long as its data says; data the host has not mirrored yet (a null view
/// reads zeros) can make that four billion iterations, which hangs the host
/// GPU until the driver resets it. A lane that reaches the limit where the
/// loop branches back on a condition leaves the loop and goes on with the
/// program; one where it jumps back leaves the program as killed (a pixel
/// discards). A loop whose code fixes its count (`trip`) runs it all.
// shortcut: a genuine loop of more iterations than the limit is cut short too.
pub(in crate::emit) const PIXEL_LOOP_LIMIT: u32 = 1 << 7;
/// The same for vertex and mesh programs.
pub(in crate::emit) const VERTEX_LOOP_LIMIT: u32 = 1 << 8;
/// The same for compute programs that test no rays ([`TRACE_LOOP_LIMIT`]).
pub(in crate::emit) const COMPUTE_LOOP_LIMIT: u32 = 1 << 12;

impl Lower<'_> {
    /// A forward edge into loop header `to` (from DXIL block `via`) starts
    /// its count at 0.
    pub(super) fn enter_loop(&mut self, _from: usize, to: usize, via: usize) {
        if let Some((_, incoming)) = self.counters.get_mut(&to) {
            let z = self.s.m.i32(0);
            incoming.push((z, via));
        }
    }

    /// A back edge to header `to` (a back edge keeps its block, no
    /// critical-edge block): the count goes up; the result compares it with
    /// the stage's limit by `p` (below: go on; at or above: stop).
    pub(super) fn count_iteration(&mut self, to: usize, p: u8) -> V {
        let c = self.cur;
        let (phi, _) = self.counters[&to];
        let one = self.s.m.i32(1);
        let next = self.s.m.bin(c, Bin::Add, phi, one);
        self.s.m.tag(phi, Tag::Counter);
        self.s.m.tag(next, Tag::Counter);
        // Per stage unless the switch names one: a loop that never meets its
        // exit (a count read from a buffer the host has not got yet, say)
        // runs to the limit in every lane. A million iterations a pixel
        // hung the GPU for seconds a draw until Windows reset it
        // (PS_Textured_Eight, 2026-11-24, run 12).
        // shortcut: a pixel loop past 128 iterations, a vertex or mesh loop
        // past 256 or a compute loop past 4,096 is cut short. (They were
        // 1,024, 4,096 and 65,536: the opening cutscene then lost the card
        // 14-25 frames in, in a minimap or shadow mesh draw; with every
        // loop cut at 64 it ran 90 frames. Raise them again once shaders
        // stop reading data that is not there yet.)
        let limits = super::super::loop_limits();
        let limit = if self.s.pixel {
            limits.pixel
        } else if self.s.mesh.is_some() || self.s.threads.is_none() {
            limits.vertex
        } else if self.traces {
            limits.trace
        } else {
            limits.compute
        };
        // A loop whose code fixes how often it goes round runs that often, as
        // on the console, whatever the data: the limit is for loops the data
        // ends. (The title's environment BRDF tables integrate 1,024 samples a
        // texel, two or four a pass: at the pixel limit of 128 passes each of
        // the 65,536 texels of the three tables stopped at a half or a quarter
        // of its samples, in all six loops, counted with `VICTORY_LOOP_NOTES=1`
        // in the opening cutscene.)
        let limit = match self.passes.get(&to) {
            Some(&n) => limit.max(n.saturating_add(1)),
            None => limit,
        };
        let limit = self.s.m.i32(limit);
        self.counters
            .get_mut(&to)
            .expect("header")
            .1
            .push((next, c));
        let over = self.s.m.cmp(c, p, next, limit);
        self.s.m.tag(over, Tag::Counter);
        over
    }

    /// The block a lane stopped by its loop's limit leaves through.
    pub(super) fn abort_block(&mut self) -> usize {
        if let Some(a) = self.abort {
            return a;
        }
        let a = self.s.m.new_block();
        if self.s.pixel {
            let t = self.s.m.bool(true);
            self.s
                .op(a, "discard", Attr::NoUnwind, self.s.t.void, 82, &[t]);
        }
        self.s.m.terminate(a, Term::Ret);
        self.abort = Some(a);
        a
    }
}
