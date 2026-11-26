# Cross-Session Handoff & Engineering Briefing — RTX 3060 Ti (8 GB) / 16 GB RAM / i5-12400F

> **Snapshot date:** 2026-11-25 22:25 ICT  
> **Workspace:** `victory-fork` (based on `fxceless/victory-fork`, branch `e4glevlr/shader-optimization-3060ti-status`)  
> **Binding rules:** Always follow [AGENTS.md](../AGENTS.md) and [docs/asset-policy.md](asset-policy.md). Never name the title in tracked files and never commit assets, dumps, screenshots, or derived binaries.

---

## 1. Non-Negotiable Priority Order (Read First)

1. **P0 — Stability & Policy:** Zero driver hangs / TDR / device removal / memory corruption; `cargo xtask policy` must always pass.
2. **P1 (Supreme Goal) — Clean, Accurate, Artifact-Free Rendering:**
   - **Visual correctness ALWAYS comes before FPS.** Never accept a speedup that causes black interiors/walls, broken faces/skin/hair, glass/depth errors, NaN explosions, mosaic blocks, flickering textures, or missing HUD.
   - Fix untranslated/refused shaders, DXIL validation errors, Wave64→Wave32 lowering, LDS ordering, memory sync, and lighting/RT correctness first.
   - Keep default shader optimizations (`VICTORY_SHADER_OPT`) **exact**. Any lossy shortcut must be opt-in (off by default).
   - Prefer **exact VRAM savings** (`VICTORY_LEVELS_ON_DEMAND=1`, placed texture heaps) over quality-reducing fallbacks (`VICTORY_TEXTURE_DROP_MIPS`).
3. **P2 — Performance & FPS (Target Hardware):**
   - **GPU:** NVIDIA RTX 3060 Ti (**8 GB VRAM**, Ampere **Wave32**, native FP64, native DP4A `dot4AddPacked`, native ROVs).
   - **RAM:** 16 GB System RAM (requires Windows Pagefile ≥ 24–32 GB on NVMe until memory footprint drops below 16 GB).
   - **CPU:** Intel Core i5-12400F (6C/12T).

---

## 2. Current Branch State vs Upstream (`fxceless/victory-fork`)

### Local branch (`e4glevlr/shader-optimization-3060ti-status` @ `9b48051`)
Built on `fxceless/victory-fork` `310c6eb` with selective ports from `JoshuuDev/victory-amd`:
- CLI `victory warm` (shader + compute pipeline cache pre-warm while preserving `early` list).
- Skip ~1,000 `CS_RaytracingResolve` compute pipelines when `VICTORY_BVH_BUILDER` is off (`VICTORY_RT_RESOLVE=1` re-enables; `VICTORY_ACTIVATE_RAYS=0` skips `CS_ScreenProbes_ActivateRays`).
- Source-keyed shader cache (`victory_shader::sources()` instead of `current_exe()`).
- Smaller-mip texture detiling & staging (`Layout::bytes_from`, `upload_from`, `first_mip`).
- Split 1D indirect compute dispatches past 65,535 groups (`bound_split`, `USER_GROUP_BASE`).

### Upstream `fxceless/victory-fork` (`main` @ `0b0074e`) — 5 major commits ahead!
Upstream `main` has landed huge upgrades since `310c6eb`:
- [`8dd30ab`](https://github.com/fxceless/victory-fork/commit/8dd30ab472697c47b141c273c2a00f8ce1c00520): `VICTORY_VIEW_GUARD` (aliased physical page sync), safe kernel reads, `VICTORY_LEVELS_ON_DEMAND=1`, `translate --warm all`, source-keyed shader cache.
- [`a26859f`](https://github.com/fxceless/victory-fork/commit/a26859fc5f512e0db0d7522d3da821c038378ab7): First room renders properly (`#305` byte-copied descriptor sync), `VICTORY_CARD_ROOM_FOR_BUFFERS` (CPU compare `550–710 MB → 25 MB/frame`), **`VICTORY_LEVELS_ON_DEMAND=1` verified on GPU saving ~3.1 GB VRAM with zero quality loss**, +14 shaders.
- [`9376fd6`](https://github.com/fxceless/victory-fork/commit/9376fd60c0ba0e76a74c39b49b1407dbac0a64c7): Cheap descriptor fingerprint (`70–220 ms → ~3 ms/frame` CPU), skip failing write-watch calls (`#670`), `VICTORY_CARD_FETCH_INFLIGHT=1` (`#648`, relay fetch wait `181 → 0 ms`).
- [`f85101b`](https://github.com/fxceless/victory-fork/commit/f85101b6095e8cf3cecf8f0647cc7f5428daa177): `VICTORY_ASYNC_SUBMIT=1`, `VICTORY_QUEUE2=1` (COPY queue fetches), safer indirect draw windows (`#220`), reuse of 512 host lists & texture staging buffers, `VICTORY_PROFILE=1`.
- [`a253706`](https://github.com/fxceless/victory-fork/commit/a25370661ed1343e2f2d5e79669ec8db4e60dc68) & [`0b0074e`](https://github.com/fxceless/victory-fork/commit/0b0074e0b2765e574c622e9a3de566f20f04135b): Removed 402 illegal D3D12 self-copies, `VICTORY_DRIVER_WATCH` (pre-TDR auto-disable of draw windows), `VICTORY_COPY_PLAIN=1` fixed, `VICTORY_FRAME_LOG` / `VICTORY_CARD_METER`, `RwLock` heap sync, fast-core pinning. Measured **7–8 FPS in first rooms, 10–12 FPS in lighter views** on RTX 4070.

> **Step 0 for the next implementation session:** Rebase `e4glevlr/shader-optimization-3060ti-status` onto `fxceless/victory-fork` `0b0074e` (or latest `upstream/main`). Drop the pieces now natively in `main` (`warm`, source cache key, smaller-mip staging) and carry forward only our unique wins (`CS_RaytracingResolve` skip + `bound_split` for >65,535 x-groups, unless `#993`/`#834` replaces it).

---

## 3. Audited PR & Patch Roadmap (Strictly Ordered P0 → P1 → P2)

### Tier A — P0 Stability Fixes (NVIDIA & Runtime Safety)
| PR | What it fixes | Why it matters on 3060 Ti |
| :--- | :--- | :--- |
| **[#911](https://github.com/fxceless/victory-fork/pull/911)** | Clamp CBV size to ≤ 64 KiB | Prevents **NVIDIA device removal** on oversized CBVs. |
| **[#827](https://github.com/fxceless/victory-fork/pull/827)** | Fix world-entry crash from `9376fd6` table-slot skip | Critical stability guard after rebasing onto `0b0074e`. |
| **[#141](https://github.com/fxceless/victory-fork/pull/141)** | Wait before overwriting in-flight texture staging under `VICTORY_RELAY` | Eliminates dynamic UI/subtitle texture flicker & races. |
| **[#936](https://github.com/fxceless/victory-fork/pull/936)** | Flush `SNAPS` before dropping `Shadow` | Prevents use-after-free crash. |
| **[#856](https://github.com/fxceless/victory-fork/pull/856)** | Load D3D12 Agility SDK on Windows 10 | Required **only** if target PC runs Windows 10 (skip on Win 11). |

### Tier B — P1 Lighting & Live Ray Tracing (Eliminating Black Shadows on NVIDIA Ampere)
| PR | What it does | Notes |
| :--- | :--- | :--- |
| **[#840](https://github.com/fxceless/victory-fork/pull/840)** | Fix `DispatchX` flag `0x20` (`USE_THREAD_DIMENSIONS`) | Fixes BVH builder launching thread-count as group-count (structure overrun). |
| **[#823](https://github.com/fxceless/victory-fork/pull/823)** | NVIDIA 32-lane: 4 refused computes + **full BVH builder pipelines** + contiguous xmem views | Unblocks BVH building on RTX 3060 Ti Wave32. |
| **[#905](https://github.com/fxceless/victory-fork/pull/905)** / **[#980](https://github.com/fxceless/victory-fork/pull/980)** | Live Ray Tracing on `main` (tested on **RTX 3080 Ampere**); `#980` combines with hot VRAM memory (-1/3 copies) | Core fix for dark interiors/shadowed faces. Keep gated behind env flag until verified on 8 GB VRAM. |
| **[#916](https://github.com/fxceless/victory-fork/pull/916)** | Only address-storing shaders mark title-memory copies written | Eliminates **12 GB/frame** of CPU compare overhead when RT is on. |
| **[#917](https://github.com/fxceless/victory-fork/pull/917)** | `VICTORY_AMBIENT` (off by default) | Opt-in constant ambient fallback while RT is disabled. |

### Tier C — P1 Clean Graphics, Anti-Artifact & NVIDIA Wave32 Shader Correctness
**Visual / Geometry / Animation Fixes:**
- **[#927](https://github.com/fxceless/victory-fork/pull/927)** — 16-bit buffer accesses preserve bits (**character faces & mouths now animate** instead of evaluating to zero).
- **[#907](https://github.com/fxceless/victory-fork/pull/907)** — Pixel shaders declare `[earlydepthstencil]` (**fixes glass prepass rendering over characters** + free early-Z speedup).
- **[#919](https://github.com/fxceless/victory-fork/pull/919)** — Mask mesh primitive vertex indices to 9 bits (prevents exploded/corrupted triangles).
- **[#895](https://github.com/fxceless/victory-fork/pull/895)** — Keep pixel helper lanes out of wave-first loops & quad permutes (prevents polygon-edge artifacts).
- **[#956](https://github.com/fxceless/victory-fork/pull/956)** — Texture recheck & full-read for small textures (fixes missing HUD glyphs).
- **[#896](https://github.com/fxceless/victory-fork/pull/896)** — Preserve texture refresh notifications during streaming bursts (prevents missing/black textures).
- **[#922](https://github.com/fxceless/victory-fork/pull/922)** — Disable blending on integer render targets (fixes refused PSOs).

**Shader Translation Coverage & Wave32 Accuracy (NVIDIA RTX 3060 Ti):**
- **[#699](https://github.com/fxceless/victory-fork/pull/699)** — Signature row order & single interpolation mode (**+142 shaders pass DXIL validator**).
- **[#718](https://github.com/fxceless/victory-fork/pull/718)** — Sampler chosen via control flow between two immediate samplers (**+40 shaders**).
- **[#854](https://github.com/fxceless/victory-fork/pull/854)** → **[#993](https://github.com/fxceless/victory-fork/pull/993)** — Paired Wave64 on 32-lane GPUs (**+230 shaders**), oversized mesh+pixel linking, real geometry shaders, exact console loops, >65,535 x-group dispatches.
- **[#988](https://github.com/fxceless/victory-fork/pull/988)** — Narrow shaders that cannot observe wave size directly to native **Wave32** (cleaner & faster on Ampere).
- **[#695](https://github.com/fxceless/victory-fork/pull/695)** — `VICTORY_WAVE_WIDE_ONLY` for targeted shaders (rescues 2 `water_shading` computes on NVIDIA without global `VICTORY_WAVE_WIDE=1`).
- **[#500](https://github.com/fxceless/victory-fork/pull/500)** — `VICTORY_LDS_EXACT=1` per-dword LDS ordering on 32-lane GPUs (rescues prologue compute `369cbe906ea72764`).
- **[#913](https://github.com/fxceless/victory-fork/pull/913)** (atomic CAS for LDS byte/half writes), **[#914](https://github.com/fxceless/victory-fork/pull/914)** (`globallycoherent` UAV + device fence), **[#837](https://github.com/fxceless/victory-fork/pull/837)** (`VICTORY_NAN_GUARD`), **[#961](https://github.com/fxceless/victory-fork/pull/961)** (disable math refactoring when FMad absent), **[#934](https://github.com/fxceless/victory-fork/pull/934)** (`v_interp_p2_f16`), **[#948](https://github.com/fxceless/victory-fork/pull/948)** (VOP3 unsigned clamp), **[#734](https://github.com/fxceless/victory-fork/pull/734)** (`VOP3 CMPX` writes `EXEC`).

### Tier D — P2 Exact VRAM 8 GB / RAM 16 GB / FPS (Apply After P1 Is Verified Clean)
- **8 GB VRAM (exact, zero quality loss first):**
  - `VICTORY_LEVELS_ON_DEMAND=1` (already in `0b0074e`, saves ~3.1 GB VRAM).
  - **[#903](https://github.com/fxceless/victory-fork/pull/903)** — `VICTORY_PLACED_TEX=1`: suballocate placed textures from 64 MiB heaps (cuts D3D12 fragmentation without losing resolution).
  - **[#691](https://github.com/fxceless/victory-fork/pull/691)** — 1 MiB window grid + tails (saves ~751 MB windows, ~732 MB shared mem, ~821 MB RAM on 8 GB cards).
- **8 GB VRAM (opt-in fallbacks ONLY if VRAM still overflows):**
  - **[#610](https://github.com/fxceless/victory-fork/pull/610)** — `VICTORY_TEXTURE_DROP_MIPS=N` (off by default; `N=1` cuts 75% texture VRAM if needed).
  - **[#188](https://github.com/fxceless/victory-fork/pull/188)** — Series S mode (`--console series-s`, 720p) fixed on 8 GB NVIDIA cards.
- **16 GB RAM & i5-12400F CPU:**
  - **[#822](https://github.com/fxceless/victory-fork/pull/822)** — Trim idle upload/staging caches when system commit runs low.
  - **[#886](https://github.com/fxceless/victory-fork/pull/886)** — DirectStorage spare staging across queues.
  - **[#768](https://github.com/fxceless/victory-fork/pull/768)** (`VICTORY_WIDE_TEXTURES=1`), **[#892](https://github.com/fxceless/victory-fork/pull/892)** (dedicated HUD/screen pipeline workers), **[#733](https://github.com/fxceless/victory-fork/pull/733)** (avoid full queue sort), **[#842](https://github.com/fxceless/victory-fork/pull/842)** / **[#857](https://github.com/fxceless/victory-fork/pull/857)** (coalesced write-backs).
- **GPU FPS (exact, preserving identical output):**
  - **[#808](https://github.com/fxceless/victory-fork/pull/808)** — Lower packed dot products to DXIL `dot4AddPacked` + `fmad` (hardware-accelerated DP4A on RTX 3060 Ti Ampere).
  - **[#992](https://github.com/fxceless/victory-fork/pull/992)** — 64-thread workgroup `CARD_COMPARE` (`0.2–0.8 ms → 0.002–0.11 ms`).
  - **[#462](https://github.com/fxceless/victory-fork/pull/462)** + **[#144](https://github.com/fxceless/victory-fork/pull/144)** — GPU-filled / direct shadow-bound table CBVs (saves ~67 ms/frame relay wait).
  - **[#860](https://github.com/fxceless/victory-fork/pull/860)** — Prevent false trips of `0b0074e`/`f85101b` fill-ahead latch.
- **Diagnostics / Profiling:**
  - **[#717](https://github.com/fxceless/victory-fork/pull/717)** (F3 frame-breakdown overlay + `overlay.log`), **[#687](https://github.com/fxceless/victory-fork/pull/687)** (missing-texture checkerboard, PIX captures, slow shader compile log), **[#817](https://github.com/fxceless/victory-fork/pull/817)** (`translate-diff` DXIL comparison between builds).

---

## 4. Explicit Do-Not-Port / Conflict List

- **Skip `#688`:** Directly conflicts with `#648` (`VICTORY_CARD_FETCH_INFLIGHT`, already merged in `9376fd6`) and targets `VICTORY_RELAY=1` instead of default `VICTORY_RELAY=2`.
- **Skip `#653` / `#716` / `#767`:** Massive 85-file upstream merges; wait for `fxceless/victory-fork` to land officially rather than rebasing manually.
- **Skip `Marvisak/VIctory` `cd94d35` (`view_marks`) for now:** Diverged deeply from Hyphen's `CARD_COMPARE` / `shadow.rs` / `host.rs` architecture.
- **Skip `#843`:** Software FP64 emulation for Intel Arc A770 (RTX 3060 Ti has native FP64).
- **Never enable `VICTORY_BVH_HOLD`:** Confirmed to break indirect lighting (violates P1).
- **Watch out for `#868` (leave write-combining memory on card):** Opt-in speed experiment (`-24%` frame time); verify zero visual or gameplay sync regressions before using.

---

## 5. Active Ecosystem Forks Worth Tracking

| Repo / Branch | Key Highlights |
| :--- | :--- |
| [`fxceless/victory-fork`](https://github.com/fxceless/victory-fork) (`main`) | Primary base (Hyphen). Latest: `0b0074e` (7–12 FPS on RTX 4070, `VICTORY_LEVELS_ON_DEMAND=1`). |
| [`JoshuuDev/victory-amd`](https://github.com/JoshuuDev/victory-amd) (`victory-amd`) | Fork of Hyphen; 16 MB sharded remap locks (`003beef`), `VICTORY_FRAMETIME_DUMP`, shader fixes (`9dad973`). |
| [`PastiiJay/klbf-lab-beta`](https://github.com/PastiiJay/klbf-lab-beta) (`klbf-lab` / `klbf-amd`) | Integration test builds: 77-min open-world playtest (~5.2–9.8 FPS), **water physics fix confirmed on NVIDIA RTX 5070 Ti** (`a4f12f5`). |
| [`JMAN730/project-flamingo`](https://github.com/JMAN730/project-flamingo) (`main`) | Largest `Marvisak/VIctory` fork: OOM auto mip-drop (`db4e468`), shader fixes (`gather4-c-lz`, `pixel-sgpr-inputs`). |
| [`Xeltax/VIcky`](https://github.com/Xeltax/VIcky) (`fx-heapmap-8d`) | Integration branch validating PR `#188` Series S mode on NVIDIA (~5.7–6.0 FPS). |
| [`triippywhale/victory-fork`](https://github.com/triippywhale/victory-fork) (`main`) | POPS lowered to Rasterizer-Ordered UAVs (ROVs — native on Ampere). |
| [`justr4mzi/VIctory`](https://github.com/justr4mzi/VIctory) (`main`) | Combines `9376fd6` + `#800–#838` + `evict_placed_in` + unaligned CBV / placed 3D UAV mirroring. |

---

## 6. Verification & Recommended Run Profile (RTX 3060 Ti 8 GB / 16 GB RAM / i5-12400F)

### Build & Verify Commands (macOS cross-check or Windows native)
```bash
# On macOS (cross-check Windows D3D12/HLE code without ml64.exe):
CARGO_FEATURE_PURE=1 cargo check --workspace --target x86_64-pc-windows-msvc
CARGO_FEATURE_PURE=1 cargo check -p victory --features diagnostics --target x86_64-pc-windows-msvc
CARGO_FEATURE_PURE=1 cargo clippy --workspace --target x86_64-pc-windows-msvc

# Cross-platform unit tests + asset policy:
cargo test -p victory-formats -p victory-shader -p victory-core -p victory-pipeline
cargo xtask policy
```

### Recommended Clean-First Windows Run Profile (after rebasing onto `0b0074e`)
```powershell
# 1. Build release (or diag for profiling)
cargo build --release -p victory

# 2. Pre-warm shader cache (cap threads to 8 on 16 GB RAM / i5-12400F)
$env:VICTORY_ACTIVATE_RAYS = "0"
target\release\victory.exe translate --warm all --jobs 8

# 3. Run clean-first profile (exact 8 GB VRAM savings, zero mip quality loss)
$env:VICTORY_LEVELS_ON_DEMAND = "1"
$env:VICTORY_WINDOW_KB = "1024"
$env:VICTORY_WINDOW_TAILS = "1"
$env:VICTORY_CARD_BUFFERS_MB = "512"
$env:VICTORY_TEXTURE_ROOM_PERCENT = "80"
$env:VICTORY_ASYNC_SUBMIT = "1"
$env:VICTORY_QUEUE2 = "1"
$env:VICTORY_COPY_PLAIN = "1"
$env:VICTORY_INDIRECT_WINDOW = "1"
target\release\victory.exe run <game_dir>\<title.exe> --execute
```
*(Ensure Windows Pagefile is ≥ 24–32 GB on an NVMe SSD to avoid `error 1455` on 16 GB RAM.)*

---

## 7. Grand Merge (`rtx3060ti/grand-merge`, from `main` @ `8a63f368`) — What Landed

Executed from [docs/rtx3060ti-merge-plan.md](rtx3060ti-merge-plan.md), one commit per phase, each after the full gate (Windows-target check, diagnostics check, clippy `-D warnings` default and all features, cross-platform tests, `cargo xtask policy`, `cargo xtask viz`). Windows-only tests (victory-gpu, `semantics_tests`, `valu_tests`) were built and linted on macOS, **not run**: pc-home was offline. Run `cargo test --workspace` on Windows before relying on any of this.

### Switches (default in **bold**)

| Switch | Default | What |
|---|---|---|
| `VICTORY_TEXTURE_BACK` | **on** (`0` off) | water textures shaders write are read back into title memory (#999; boats float) |
| `VICTORY_TARGET_VARIANTS` | **on** (`0` off) | a pipeline made for the render-target formats actually bound |
| `VICTORY_FAIR_DUE` | **on** (`0` off) | texture queue serves looked-at and fresh slots in turns (#1246) |
| `VICTORY_RELOOK_MS` | **20000** | re-look interval for slots without memory (#1246) |
| `VICTORY_BIND_CACHE` | **on** (`0` off) | same render targets are not set again |
| `VICTORY_DECLARE_ONCE` | **on** (`0` off) | a shader-written range is declared once until the list writes title memory |
| `VICTORY_DYNAMIC_SLOTS` | off | more dynamic slots on NVIDIA's capped view heap (clamped to half the heap) |
| `VICTORY_VIEWS_REMADE_QUIET` | off | levels-on-demand view slots remade only with no submission in flight |
| `VICTORY_LOOKUP_CACHE` | off (`1`, `alt`) | per-thread buffer lookup cache (#1237 + #1245) |
| `VICTORY_RAM_TRIM_MB` | off | below that much free RAM or commit, reuse pools are emptied |
| `VICTORY_READBACK_COALESCE` | off | contiguous readback pieces in one copy (#1230) |
| `VICTORY_CHANGED_RUNS_AVX2` | off | AVX2 changed-run detection (#1233) |
| `VICTORY_SHADER_OPT` bit 128 (`WIDE`) | off (`255` on) | scalar buffer loads four dwords a call |
| `VICTORY_AMBIENT` | off | `0.6`: flat stand-in (as before); `sh`/`auto`/`sh:<share>`: estimated SH1 stand-in; `constant:<l>` |
| `VICTORY_NO_WATCHDOG`, `VICTORY_OWN_UPSCALER`, `VICTORY_RENDER_SCALE` | off | loader code patches for the 1.0.2.1 build only (hash and bytes checked) |

New shader lowering (always on, exact): DP4A `dot4AddPacked` for `v_dot4*`/`v_dot2*` (#808), packed 16-bit saturating clamp (#1042), reverse-postorder `resource::track` (#1041), f16 inline constants in wave sums, `image_get_lod` on cubes, NaN payloads in typed half loads, inactive `ds_swizzle` sources read 0 (console wave32 only).

### Deliberately not ported (see the phase commit messages for detail)

- `#1026` legacy MAD/MAC fused with flushed sources: unmerged in both upstreams; the semantics reference here and in Marvisak/VIctory still takes it unfused. Needs a hardware measurement first.
- `#1240`'s `VICTORY_TEXTURE_MAX_DIM` (drops detail), its staging zero-fill skip (stale bytes where tiles are mapped) and its batched descriptor mirror (a rewrite of `mirror()`, which differs here).
- `NV 350c6864` (`VICTORY_UPLOAD_AHEAD`, with `SHOW_AFTER`/`KEEP_SHOWN` around it): 654 lines needing `popwatch.rs`, `texture/ahead.rs` and `crate::ab`, none of which exist here; nearly every hunk fails. Deferred to its own PR.
- Already in this tree, so not ported again: depth bounds (`c96b4f2b`), integer targets unblended (`e92dbb41`), a null view's alias source.
- Not applicable here: Marvisak `e815cef2`, `366a9786`, `120233cf`, `136e39d0`; fxceless `d03859a2`.

### Launcher

`scripts/run-3060ti.ps1` (`-Profile Quality` default, `-Profile Survival8GB`, `-SeriesS`, `-DryRun`). Two deviations from the plan's sign-off profile:
- `VICTORY_DYNAMIC_SLOTS` is not set by default (`-DynamicSlots 524288`): shaders clamp view indices only to the heap's null slot, so a view picked by index past the slots kept in place reads a dynamic slot. Use it only if the log shows "no free dynamic descriptor slots", and compare the picture.
- `VICTORY_RAM_TRIM_MB` is `2048`, not `12288`: it is a threshold of *available* memory, and a 16 GB machine running the title is nearly always below 12 GB available, which would empty the pools every 500 ms.

### Still to verify on the RTX 3060 Ti

1. `cargo test --workspace` (incl. `byte_dot_products_match_the_hardware`, the new victory-gpu and shader tests).
2. A/B with `scripts/bench.ps1` against `8a63f368`: default switches vs `-Switches 'VICTORY_BIND_CACHE=0,VICTORY_DECLARE_ONCE=0,VICTORY_TARGET_VARIANTS=0'`.
3. Visual checks: first-room walls, faces/mouths, glass depth order, HUD glyphs, ocean/boat buoyancy (texback default on).
