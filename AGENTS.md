# Project Engineering Rules — Visual Quality First, Then FPS

## Supreme Goal (Non-Negotiable Priority Order)

1. **P0 — Stability & Policy Compliance**
   - Never violate [docs/asset-policy.md](docs/asset-policy.md) (never name the title in the tree; never commit game assets, dumps, screenshots, or derived binaries).
   - Never trade GPU stability (driver hangs / TDR / device removal / silent memory corruption) for speed.

2. **P1 (Supreme Goal) — Clean, Accurate, Artifact-Free Rendering**
   - **Visual correctness ALWAYS comes before FPS.** A fast frame that looks broken is a regression.
   - Zero tolerance for avoidable visual regressions: black/missing walls or interiors, missing/corrupted skin/hair/character shading, NaN explosions/fireflies, mosaic/block artifacts, texture flickering/stale frames, broken depth bounds, wrong water/fog/lighting, or missing HUD/UI.
   - **Shader correctness first (target GPU: NVIDIA Ampere Wave32 / RTX 3060 Ti):**
     - Prioritize fixing untranslated or driver-refused shaders, DXIL validator failures, input/output signature & interpolation mismatches, Wave64→Wave32 lowering correctness, LDS barrier/ordering correctness, and NaN/precision hazards before applying speed shortcuts.
     - Keep shader compiler optimizations (`VICTORY_SHADER_OPT`) **exact** by default. Any lossy approximation, `shortcut:`, or unsafe math/sync relaxation MUST be opt-in behind an environment variable (off by default) and clearly document its visual trade-off.
   - **Texture, lighting & memory-sync fidelity:**
     - Preserve synchronization correctness across aliased physical pages, descriptor heaps, staged texture uploads, and in-flight GPU buffers so streamed geometry/textures arrive cleanly without race conditions or pop-in corruption.
     - Prefer exact VRAM savings (e.g., allocating only loaded mip levels via `VICTORY_LEVELS_ON_DEMAND`) over quality-degrading fallbacks (e.g., dropping high-res mips via `VICTORY_TEXTURE_DROP_MIPS`). Use quality-reducing switches only as an explicit opt-in fallback when VRAM overflow would otherwise break rendering.

3. **P2 — Performance & FPS Optimization (Only After Visual Cleanliness Is Verified)**
   - Target hardware profile: **NVIDIA RTX 3060 Ti (8 GB VRAM, Ampere Wave32) · 16 GB System RAM · Intel i5-12400F (6C/12T)**.
   - Optimize only changes that **preserve identical or cleaner visual output**, prioritizing bottlenecks in this order:
     1. **8 GB VRAM ceiling:** Avoid allocating VRAM for unloaded mip levels; reserve room for hot buffers and render targets; prevent spilling into shared system RAM over PCIe.
     2. **16 GB RAM ceiling:** Reuse command lists and staging buffers; trim idle caches under physical RAM / Windows commit pressure; eliminate redundant copies.
     3. **GPU & copy-back latency:** Eliminate CPU/GPU pipeline stalls and redundant readbacks (`VICTORY_RELAY`, `VICTORY_ASYNC_SUBMIT`, `VICTORY_QUEUE2`, `VICTORY_CARD_FETCH_INFLIGHT`, source-keyed warm shader cache, skipping unused RT resolve passes while BVH builder is off, Wave32-friendly exact DXIL lowering such as `dot4AddPacked`).
     4. **CPU overhead (6C/12T):** Reduce per-frame hashing, lock contention, and unnecessary kernel syscalls without weakening memory-sync guarantees.

## Porting, Testing & Verification Discipline

- **Scan before porting:** Before pulling any upstream commit, community PR, or fork patch, inspect its diff for (a) visual side effects / artifact risks, (b) NVIDIA Wave32 compatibility, and (c) conflict risk against the current branch.
- **Gate risky experiments:** Any change that could alter rendering output or GPU stability must sit behind an env switch (defaulting to the visually clean path) for A/B visual + frametime comparison.
- **Verify every change:** Before committing, verify Windows compilation (`cargo check --workspace --target x86_64-pc-windows-msvc`), diagnostics build, `cargo clippy`, relevant unit tests (`victory-formats`, `victory-shader`, `victory-core`, `victory-pipeline`), and `cargo xtask policy`.
- **Measure, don't guess:** Base every FPS optimization on real profiling signals (`VICTORY_GPU_TIMING=1`, `VICTORY_PROFILE=1`, `VICTORY_SHADER_CENSUS=1`, frame overlay/logs) rather than speculation.
