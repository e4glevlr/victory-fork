# Grand Merge Execution Plan — RTX 3060 Ti Edition (`e4glevlr/victory-fork`)

> **Audience:** Autonomous coding agent / sibling session executing this merge from a cold start.  
> **Target Hardware:** NVIDIA RTX 3060 Ti (**8 GB VRAM**, Ampere **Wave32**, native **DP4A**, **1,000,000** view heap cap) · **16 GB System RAM** · Intel i5-12400F (**6C/12T**, AVX2).  
> **Base Branch:** `e4glevlr/victory-fork` `main` @ `8a63f368` — **MUST remain the base** (NEVER rebase onto `trustno1-coder/VIctory-NVIDIA`).  
> **Integration Branch:** `rtx3060ti/grand-merge` (cut from `main` after committing Phase 0 prep).  
> **Approved Scope & Defaults (User-Confirmed 2026-11-26):**
> 1. Execute **Phases 1 → 4** in full, one atomic commit per logical item/phase.
> 2. `VICTORY_TEXTURE_BACK` (`texback.rs`, PR `#999`) MUST be **ON by default** (`=0` disables it).
> 3. Strictly obey [AGENTS.md](../AGENTS.md) (**P0 Stability & Asset Policy → P1 Visual Quality & Wave32 Correctness → P2 Exact FPS & 8 GB / 16 GB Memory Safety**) and [docs/asset-policy.md](asset-policy.md) (**never write the game title anywhere; max file size stays 512 KiB; zero binary assets**).

---

## 0. Current Baseline Audit (Already Landed in `8a63f368` — Do NOT Re-Port)

Before touching any file, verify you do **not** duplicate work already present in `main` (`8a63f368`):
- **Shaders (already in tree):** `VICTORY_WAVE_PAIRS`, `VICTORY_MESH_LINK` (25,682 / 25,953 shaders passing), `#895` (pixel quad reads), `#907` (`[earlydepthstencil]`), `#913` (LDS atomic CAS), `#914` (`globallycoherent` UAV + device fence), `#927` (16-bit buffer loads), `#961` (disable math refactor without FMad), `VICTORY_LDS_EXACT`, `VICTORY_EXACT_F64_SUMS`, `VICTORY_SHADER_LOOP_LIMIT`, `VICTORY_SHADER_OPT` bit 64 = `MERGE` (`crates/victory-shader/src/emit/bitcode/opt.rs:56`).
- **GPU / HLE (already in tree):** `#911` + `#1206` (CBV ≤ 64 KiB cap), `#936` (Shadow SNAPS flush), `#827`, `#822` (commit-aware trim), `#903` (`VICTORY_PLACED_TEX`), `#886` + `#1044` (DirectStorage physical-core pool), `#768`, `#916`, `#896`, `#956`, `#917` (flat `ambient.rs`), `#823` (xmem), `#860`, `#462`, `#1205` (upload fill wait), `#1147`, `#1210` (`ResourceMinLODClamp`), `#1241` (aliased GPU texture copy), `#1214` (DRED2 removal class), `#1227` (`VICTORY_CARD_SYNC_BATCH`), `VICTORY_DRIVER_WATCH`, `crates/victory-gpu/src/over.rs` (Series S dual-plane compose).
- **Pre-existing uncommitted workspace files (commit first in Phase 0):**
  `AGENTS.md`, `docs/3060ti-briefing.md`, `docs/rtx3060ti-merge-plan.md`, `README.md`, `docs/repo-visualization.svg`, `.githooks/pre-commit`, `xtask/src/{main.rs,policy.rs,viz.rs}`.

### Remote Repositories to Fetch (Read-Only Reference)
```bash
git remote add upstream https://github.com/fxceless/victory-fork.git       2>/dev/null || true
git remote add nv       https://github.com/trustno1-coder/VIctory-NVIDIA.git 2>/dev/null || true
git remote add marvisak https://github.com/Marvisak/VIctory.git            2>/dev/null || true
git remote add justr4mzi https://github.com/justr4mzi/VIctory.git          2>/dev/null || true
git fetch upstream nv marvisak justr4mzi
```
For GitHub PRs not yet merged into `upstream/main`, inspect diffs via `gh pr diff <NUM> -R fxceless/victory-fork` or `curl -H "Accept: application/vnd.github.v3.diff" https://api.github.com/repos/fxceless/victory-fork/pulls/<NUM>`.

---

## 1. Critical Hazards & Codebase Adaptation Notes (Verified Against Local Tree)

Read every item carefully before editing:

1. **`VICTORY_DYNAMIC_SLOTS` (`crates/victory-gpu/src/descriptor/host.rs`) — NOT a 1-line edit!**
   - In `VIctory-NVIDIA a9632bc8`, a `dynamic_slots()` helper already existed.
   - In **our** `descriptor/host.rs`, `DYNAMIC` is currently hardcoded at **5 places**:
     - `L1078`: `pub(crate) const DYNAMIC: u32 = 1 << 18;`
     - `L1100`: `let extra = if sampler { 0 } else { DYNAMIC };`
     - `L1191`: `next: Mutex::new((0, vec![false; DYNAMIC as usize])),`
     - `L1192`: `free: std::sync::atomic::AtomicU32::new(DYNAMIC),`
     - `L1344`: `*next = ((first + n) % DYNAMIC as usize) as u32;` (plus check `release_dynamic` / any other `DYNAMIC` reference).
   - **Required adaptation:** Add a `dynamic: u32` field on `HostHeap` computed by a `dynamic_slots(null: u32) -> u32` helper: when `null > 0` (capped heap like NVIDIA 1M), parse `VICTORY_DYNAMIC_SLOTS` and clamp to `DYNAMIC..(null / 2)` (default `DYNAMIC`, recommended `524288` in launcher). Replace all runtime uses of `DYNAMIC` inside `HostHeap` (`extra`, `taken` vec size, `free` atomic init, wrap-around `% self.dynamic as usize`, bounds checks) with `self.dynamic`. Add a unit test for clamping behavior.
2. **`VICTORY_SHADER_OPT` bit 64 collision (`crates/victory-shader/src/emit/bitcode/opt.rs:55-60`):**
   - Bit `64` is ALREADY `pub const MERGE: u32 = 64;` (included in `MODULE` and `ALL`, ON by default).
   - `WIDE` (4-dword scalar buffer load from `fxceless c36971b2` / `VIctory-NVIDIA 200a9cc5`) MUST use **`pub const WIDE: u32 = 128;`**, kept **OUT** of default `ALL` (opt-in via `VICTORY_SHADER_OPT=255` or `...|128`), and included in `shader_cache.rs` keying so cached translations stay distinct.
3. **Port `texback.rs` (`PR #999`) from `fxceless f5c03f3b`, NOT `VIctory-NVIDIA`:**
   - `VIctory-NVIDIA` inherits `7900XTX`'s ~78k-line `victory-gpu` rewrite (`host.rs`/`draw.rs`). `fxceless f5c03f3b` matches our exact `victory-gpu` layout cleanly (~380 lines across 6 files).
   - Flip its default switch in `texback.rs` so `VICTORY_TEXTURE_BACK` is **ON by default** (`std::env::var_os("VICTORY_TEXTURE_BACK").is_none_or(|v| v != "0")`).
4. **Always pair `FX 32be36d5` (`#1237` `VICTORY_LOOKUP_CACHE`) with `FXPR #1245`:**
   - Never land `#1237` without `#1245`'s invalidation at registration and idle sweep.
5. **Architecture mismatches already checked (do NOT waste time hunting missing upstream symbols):**
   - `crates/victory-gpu/src/descriptor/host.rs:3075`: `fn fits(texture: &ID3D12Resource, mut format: Option<&mut DXGI_FORMAT>, unordered: bool) -> bool` does NOT take `castable: &[u32]` as in `Marvisak`. Adapt `MV ea488a9f` (`#2462`) directly to our `fits` signature (check `typed_store` on the resolved view/own format for unordered views).
   - `crates/victory-gpu/src/watch.rs` & `crates/victory-gpu/src/resource.rs`: Our tree uses `fxceless`'s `VICTORY_VIEW_GUARD` and alias model rather than `Marvisak`'s `guard_aliases` (`MV 136e39d0`) / `alias_source` (`MV 219878a0`), and `crates/victory-hle/src/modules/xmem.rs:161-188` does not yet have `victory_gpu::title_memory_committed` (`FX d03859a2` / `VICTORY_ABSENT_ONCE`). Inspect our `watch.rs` / `resource.rs` / `xmem.rs` for equivalent bugs; **skip cleanly** if the underlying mechanism does not exist in our tree rather than forcing incompatible plumbing.
6. **Asset policy (`xtask/src/policy.rs`):**
   - Keep `MAX_FILE_SIZE = 512 * 1024` (do NOT raise to 640 KiB as `VIctory-NVIDIA` did).
   - Run `cargo xtask policy` after every phase.
7. **macOS cross-compilation environment (if running on macOS host):**
   - Prefix Windows checks/clippy with `CARGO_FEATURE_PURE=1` (avoids `blake3` searching for `ml64.exe`).
   - Run unit tests on cross-platform crates explicitly: `cargo test -p victory-formats -p victory-shader -p victory-core -p victory-pipeline -p xtask` (full `--workspace` test runs on Windows host).

---

## 2. Phased Implementation Checklist

### Phase 0 — Hygiene & Branch Setup
- [ ] Commit existing uncommitted docs & visualization work cleanly on `main` (or first commit on branch):
  - `AGENTS.md`, `docs/3060ti-briefing.md`, `docs/rtx3060ti-merge-plan.md`, `README.md`, `docs/repo-visualization.svg`, `.githooks/pre-commit`, `xtask/src/{main.rs,policy.rs,viz.rs}`.
  - Confirm `.githooks/pre-commit` is executable (`chmod +x .githooks/pre-commit`) and `git config core.hooksPath` is `.githooks`.
- [ ] Cut branch `git checkout -b rtx3060ti/grand-merge`.
- [ ] Run baseline verification gate (Section 4) and record baseline test count.

---

### Phase 1 — P0 Stability, NVIDIA 1M View Heap & 8 GB VRAM Safety
*Goal: Eliminate NVIDIA descriptor drop-outs, in-flight `LEVELS_ON_DEMAND` descriptor races, and 8 GB bindless texture starvation.*

- [ ] **1.1 `VICTORY_DYNAMIC_SLOTS` (from `NV a9632bc8`, adapted to `HostHeap`):**
  - File: `crates/victory-gpu/src/descriptor/host.rs` (anchors: `L1078`, `L1100`, `L1191`, `L1192`, `L1344`).
  - Add `dynamic_slots(null: u32) -> u32` reading `VICTORY_DYNAMIC_SLOTS`, clamping to `[DYNAMIC, null / 2]` when `null > 0`.
  - Store `dynamic: u32` on `HostHeap` and replace all `DYNAMIC` sizing/modulo/bounds sites with `self.dynamic`.
  - Add unit test verifying clamping below `DYNAMIC`, within bounds, and above `null / 2`.
- [ ] **1.2 `VICTORY_VIEWS_REMADE_QUIET` (from `FX 4679615a`):**
  - Files: `crates/victory-gpu/src/descriptor/host.rs` (`RESHOWN` @ `L220`, `texture_views_remade` @ `L1690`), `crates/victory-gpu/src/host.rs` (`STARTED`/`FINISHED` @ `L77-78` -> add `pub(crate) fn submissions_in_flight() -> u64`).
  - Counts remade slots while submissions are in flight and defers remaking when `VICTORY_VIEWS_REMADE_QUIET=1` (opt-in OFF by default; enable in 3060 Ti profile if `LEVELS_ON_DEMAND` hits device loss).
- [ ] **1.3 Descriptor & UAV Safety Guards (from `Marvisak`):**
  - `MV ea488a9f` (`#2462`) → `crates/victory-gpu/src/descriptor/host.rs` (`fn fits` @ `L3075`): reject typed UAVs in formats the host cannot store to (e.g., `B5G5R5A1_UNORM` on NVIDIA removes the device).
  - `MV 78ed12eb` (`#2449`) → `crates/victory-gpu/src/descriptor.rs` (`create_cbv` @ `L985`): null `desc` writes `View::Constant(0, 0)` instead of leaving a stale view + port unit test `null_cbv_clears_a_stale_view`.
  - Audit `crates/victory-gpu/src/watch.rs` & `crates/victory-gpu/src/resource.rs` against `MV 136e39d0` (`#2455`), `MV 219878a0`, and `FX d03859a2` (`#1185`); port only if equivalent paths exist in our tree, otherwise document skip in commit message.
- [ ] **1.4 Bindless Texture Fair Scheduling & Relook Backoff (`FXPR #1246`):**
  - Files: `crates/victory-gpu/src/descriptor/host.rs` (`TextureQueue::due` @ `L719`), `crates/victory-gpu/src/draw.rs` (`L2009`: `4000 / took` → `VICTORY_RELOOK_MS` default `20_000`, `.clamp(16, 1024)`).
  - Interleave looked-at and fresh texture slots (`VICTORY_FAIR_DUE=0` restores old order) + port unit test `the_texture_queue_serves_looked_at_and_fresh_slots_in_turns`.
- [ ] **Gate:** Run full Verification Gate (Section 4) → commit `Phase 1: P0 NVIDIA heap, VRAM & descriptor safety`.

---

### Phase 2 — P1 Visual Fidelity, Water Physics & Wave32 / DP4A Shaders
*Goal: Fix ocean/boat physics, RT format & integer-blend mismatches, depth-bounds passes, native Ampere DP4A, and remaining Wave32 shader edge cases.*

- [ ] **2.1 Water / Compute Texture CPU Readback (`FX f5c03f3b` / PR `#999` — DEFAULT ON):**
  - Files: `crates/victory-gpu/src/{texback.rs,lib.rs,host.rs,resource.rs,draw.rs,texture.rs}` (~+380 lines).
  - Port cleanly from `fxceless f5c03f3b` (NOT `VIctory-NVIDIA`).
  - Set default ON per user decision: `VICTORY_TEXTURE_BACK=0` opts out; unset or `1` enables it.
  - Ensure `victory_formats::texture::Layout::tile` (if needed by `texback.rs`) is ported cleanly with unit tests.
- [ ] **2.2 Target-Format PSO Variants & Integer RT No-Blend (`FX a35c5573` + `FX 025b042a`):**
  - Files: `crates/victory-gpu/src/{draw.rs,pipeline.rs,pipeline/host.rs,pipeline/make.rs}` (~+472/-5).
  - `VICTORY_TARGET_VARIANTS` ON by default (`=0` disables): creates format-matched host PSO when bound RT format differs from pipeline declaration.
  - Disables blending on integer render targets (`025b042a`).
- [ ] **2.3 Depth Bounds Test to Host D3D12 (`MV 842b0cb7` / `#158`):**
  - Files: `crates/victory-gpu/src/{draw.rs,host.rs,list.rs,pipeline.rs,pipeline/host.rs,...}` (~+65 lines).
  - Check adapter `DepthBoundsTestSupported` feature flag (RTX 3060 Ti supports it natively; guard gracefully for WARP/older adapters).
- [ ] **2.4 Native Ampere DP4A Lowering (`FXPR #808`):**
  - Files: `crates/victory-shader/src/emit/{dot.rs,bitcode.rs,lower.rs,valu_tests.rs}` (+639/-343) + remove `#[ignore]` on `byte_dot_products_match_the_hardware` in `crates/victory-shader/src/emit/semantics_tests.rs:2308`.
  - Lowers `v_dot4_i32_i8` / `v_dot4c_i32_i8` to DXIL `dot4AddPacked.i32` (100% bit-exact, hardware-accelerated on RTX 3060 Ti).
- [ ] **2.5 Wave32 / Shader Correctness Batch (isolated inside `crates/victory-shader/`):**
  - `FXPR #1041` (`resource::track` reverse postorder, +45/-1).
  - `FXPR #1042` (packed 16-bit integer saturating clamp, +149/-2).
  - `MV f3578e38` (`emit/wave.rs`: f16 inline float constants in whole-wave `v_add_f16`, +47).
  - `MV e815cef2` (`emit/lower.rs`, `emit/memory_tests.rs`: last-byte lookup for straddling memory writes, +241/-37).
  - `MV 366a9786` (`emit/lower.rs`: guarded scalar writes kill stale SGPRs in `dead_on_edge`, +45/-3).
  - `MV 120233cf` (`emit/lower.rs`: ignore guarded or modified `v_mov_b32` in `Lower::konst`, +92/-8).
  - `JR 6ba165b2` (`#1030`: inactive compute swizzle sources return 0, +86).
  - `JR 6c9a9032` (`#1026`: fuse & flush sources in legacy f32 MAD/MAC, +75/-8).
  - `JR 640262ee` (`#1024`: lower `image_get_lod` on cube textures, +93/-3).
  - `JR a15e95d8` (`#1019`: preserve signaling NaNs in typed buffer loads, +56/-4).
  - *Strict rule:* Port **only** shader logic & unit tests from `justr4mzi`; ignore all RX 6700 XT defaults.
- [ ] **Gate:** Run full Verification Gate (Section 4) — verify `victory-shader` tests (including `byte_dot_products_match_the_hardware`) all pass → commit `Phase 2: P1 water readback, RT variants, depth bounds & Wave32/DP4A shaders`.

---

### Phase 3 — P2 Exact CPU/GPU Speedups & 16 GB RAM Protection
*Goal: Cut CPU/D3D12 submission overhead on 6C/12T and cap RAM under 16 GB with zero pixel loss.*

- [ ] **3.1 Belobug / ZJOU Zero-RAM Driver Dedups (from `VIctory-NVIDIA`, ~110 lines total):**
  - `NV ca1d519f`: `VICTORY_BIND_CACHE` (ON unless `=0`) — skip redundant `OMSetRenderTargets` in `descriptor.rs` & `draw.rs`.
  - `NV b32fb997`: `VICTORY_DECLARE_ONCE` (ON unless `=0`) — declare shader-written range once until next title memory write in `host.rs` & `host/place.rs` (adapt to our `host.rs` if `host/place.rs` layout differs).
  - `NV ab86ef7f`: skip re-keeping & re-binding identical pipeline state within the same command list (`draw.rs`, `host.rs`).
  - `NV c96525db`: copy unaligned 256-byte root CBV to scratch once per list instead of per draw (`draw.rs`, `host.rs`).
- [ ] **3.2 Thread-Local Buffer Lookup Cache (`FX 32be36d5` `#1237` + `FXPR #1245` PAIRED):**
  - Files: `crates/victory-gpu/src/{host.rs,shadow.rs}` (~+232/-25).
  - `VICTORY_LOOKUP_CACHE=1|alt` (keep OFF by default per `fxceless`, enable in `scripts/run-3060ti.ps1`).
  - Must include `#1245`'s invalidation on buffer registration & idle sweep (plus `#1245` unit tests).
- [ ] **3.3 16 GB RAM Protection & Descriptor Mirror (`FXPR #1240` selective exact subset):**
  - Inspect `FXPR #1240` carefully; port **`VICTORY_RAM_TRIM_MB`** (low-RAM pool trimmer preventing Windows commit error `1455` on 16 GB machines) and any exact batched descriptor mirror improvements that don't duplicate 3.1's `VICTORY_BIND_CACHE`.
  - If porting `VICTORY_TEXTURE_MAX_DIM`, keep it **strictly opt-in (OFF by default)** as an emergency 8 GB open-world fallback only.
- [ ] **3.4 Readback & Changed-Run Micro-Optimizations (`FXPR #1230`, `#1232`, `#1233`):**
  - Inspect each PR diff cleanly against our `shadow.rs`/`host.rs` (already has `#1227` and `#1205`).
  - Port contiguous GPU readback coalescing (`#1230`), in-place write interval compaction (`#1232`), and opt-in AVX2 changed-run detection (`#1233`, using `is_x86_feature_detected!("avx2")` safe runtime check + unit tests matching scalar output!).
- [ ] **3.5 Opt-in 4-Dword Scalar Buffer Loads (`WIDE` on Bit `128`):**
  - File: `crates/victory-shader/src/emit/bitcode/opt.rs` (port logic from `fxceless c36971b2` / `NV 200a9cc5`).
  - Define `pub const WIDE: u32 = 128;` (NEVER bit `64`, which is `MERGE`).
  - Keep OFF in default `ALL` (`ALL` stays `MODULE | FIRST`); opt-in via `VICTORY_SHADER_OPT=255` and ensure `shader_cache.rs` already hashes the full `VICTORY_SHADER_OPT` bitmask.
- [ ] **Gate:** Run full Verification Gate (Section 4) → commit `Phase 3: P2 exact CPU/GPU dedups, lookup cache & 16GB RAM trimmer`.

---

### Phase 4 — Opt-in Visual Polish, Safe Streaming, Loader Hooks & 3060 Ti Launcher
*Goal: Add high-value opt-in capabilities (all OFF by default per `AGENTS.md`) and a turnkey RTX 3060 Ti launcher.*

- [ ] **4.1 Dynamic SH1 Indirect Light Stand-in (`crates/victory-gpu/src/ambient.rs`):**
  - Reference: `trustno1-coder/VIctory-NVIDIA` `crates/victory-gpu/src/ambient.rs` (20 KB self-contained HLSL/D3D12 compute pipeline reading `CS_CacheScreenSpaceRadiance` 64×36 → depth-aware 160×90 SH1 + `VICTORY_AMBIENT_SHADE` crevice AO + `VICTORY_AMBIENT_AIR` indoor fog guard).
  - **Contract:** Keep `VICTORY_AMBIENT` **OFF by default**. Support BOTH existing flat mode (`VICTORY_AMBIENT=0.6`) and upgraded dynamic SH1 mode (`VICTORY_AMBIENT=sh` or `auto`) so A/B testing is effortless until real RT bounce light lands.
- [ ] **4.2 Selective Loader Opt-in Hooks (`NV afd84de1`):**
  - Files: `crates/victory-loader/src/{hooks.rs,lib.rs,windows.rs}` (~+277 lines).
  - Port `VICTORY_NO_WATCHDOG=1` (disables 30s freeze kill during shader compilation on 6C/12T) and `VICTORY_OWN_UPSCALER=1` (temporal upscaler for Series S mode). Keep `VICTORY_RENDER_SCALE` strictly opt-in (OFF by default).
  - Verify `hooks.rs` complies with `docs/asset-policy.md` (small byte-pattern signatures & offsets only, zero proprietary code/strings, zero game title names!).
- [ ] **4.3 Safe Texture Streaming Evaluation (`NV 350c6864` — `VICTORY_SHOW_AFTER` / `VICTORY_KEEP_SHOWN`):**
  - Inspect `NV 350c6864` diff carefully against our `texture.rs`, `queue2.rs`, and `descriptor/host.rs`.
  - **Port rule:** Adapt ONLY if it integrates cleanly (< ~250 lines, opt-in behind `VICTORY_SHOW_AFTER=1`) without dragging `7900XTX`'s `shadow.rs` rewrite. If tightly coupled to `7900XTX` internals, document findings in `docs/3060ti-briefing.md` and defer to a dedicated follow-up PR.
- [ ] **4.4 Turnkey RTX 3060 Ti Launcher & Docs Sync:**
  - Create `scripts/run-3060ti.ps1` with `-Profile Quality` (default, Series X native) and `-Profile Survival8GB` (adds `VICTORY_TEXTURE_DROP_MIPS=1` or `-SeriesS` + `VICTORY_OWN_UPSCALER=1` for dense open-world VRAM overflow).
  - Update `docs/3060ti-briefing.md` with newly landed switches & test results.
  - Refresh `docs/repo-visualization.svg` via `cargo xtask viz`.
- [ ] **Gate:** Run full Verification Gate (Section 4) → commit `Phase 4: opt-in SH ambient, loader hooks & RTX 3060 Ti launcher`.

---

## 3. Explicit Exclusion List (Do NOT Port)

- ❌ `VICTORY_THIN` (`list/thin.rs`), `VICTORY_LAND` (`land.rs`), `replay/`, `timeline/` (~78k diff, 40–46 GB private RAM — fatal on 16 GB RAM).
- ❌ `VICTORY_BVH_MISS=1` / `VICTORY_BVH_NO_HIT=1` / `VICTORY_BVH_HOLD=1` as defaults (destroys indirect lighting — violates P1).
- ❌ `VICTORY_RENDER_SCALE=0.5` as default (soft 960×540 blur — violates P1).
- ❌ Raising `MAX_FILE_SIZE` from `512 KiB` to `640 KiB` in `xtask/src/policy.rs`.
- ❌ Any AMD RX 6700 XT / 7900 XTX hardware-specific default overrides.

---

## 4. Mandatory Verification Gate (Run After EVERY Phase)

Every single command MUST exit `0` before moving to the next phase:

```bash
# 1. Windows MSVC release + diagnostics typecheck (works on macOS with CARGO_FEATURE_PURE=1)
CARGO_FEATURE_PURE=1 cargo check --workspace --target x86_64-pc-windows-msvc
CARGO_FEATURE_PURE=1 cargo check -p victory --features diagnostics --target x86_64-pc-windows-msvc

# 2. Zero-warning Windows clippy (matches CI -D warnings)
CARGO_FEATURE_PURE=1 cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings

# 3. Cross-platform unit tests (run full `cargo test --workspace` on Windows)
cargo test -p victory-formats -p victory-shader -p victory-core -p victory-pipeline -p xtask

# 4. Asset policy (Rule #1) & live README visualization refresh
cargo xtask policy && cargo xtask viz
```

### Target Windows RTX 3060 Ti Sign-off Profile (`scripts/run-3060ti.ps1 -Profile Quality`)
```powershell
$env:VICTORY_LEVELS_ON_DEMAND = "1"      # Exact VRAM saving (-3.1 GB)
$env:VICTORY_PLACED_TEX       = "1"      # 64 MiB placed heaps (#903)
$env:VICTORY_DYNAMIC_SLOTS    = "524288" # Stops dropped draws under NVIDIA 1M view cap
$env:VICTORY_TEXTURE_BACK     = "1"      # Default ON (water/boat physics readback)
$env:VICTORY_CARD_SYNC_BATCH  = "1"      # 3-step send-and-hash batching (#1227)
$env:VICTORY_ASYNC_SUBMIT     = "1"      # Dedicated submit thread
$env:VICTORY_QUEUE2           = "1"      # Copy queue for readbacks
$env:VICTORY_LOOKUP_CACHE     = "1"      # Thread-local buffer cache (#1237 + #1245)
$env:VICTORY_BIND_CACHE       = "1"      # Skip duplicate RT binds
$env:VICTORY_DECLARE_ONCE     = "1"      # Dedup shader-write declarations
$env:VICTORY_RAM_TRIM_MB      = "12288"  # Protects 16 GB RAM from Windows commit error 1455
$env:VICTORY_NO_WATCHDOG      = "1"      # Prevents 30s compile watchdog kill
# Optional until real RT bounce light lands:
# $env:VICTORY_AMBIENT        = "sh"     # Dynamic SH1 indirect light stand-in
```

### Final Acceptance Criteria
- Zero regressions in visual cleanliness (first-room walls, character faces/mouths, glass depth order, HUD glyphs, ocean/boat buoyancy).
- `victory-shader` tests pass with `byte_dot_products_match_the_hardware` un-ignored and passing.
- `cargo xtask policy` passes with `512 KiB` limit intact and zero forbidden title references.
- On Windows RTX 3060 Ti: VRAM stays under `7.5 GB`, Windows Committed RAM stays under `15 GB`, dynamic descriptor rejections drop to ~`0`, and frametime improves measurably over `8a63f368` baseline via `scripts/bench.ps1`.
