# Frame time: what was measured and changed (2026-11-25)

No game files are on the test PC, so the workload is a bench title of our
own (kept out of this repository): an executable started by the layer's
loader like a title. Each frame it writes 8-16 MB of per-frame data from
the CPU, submits 24 or 96 command lists (one dispatch each) that read that
data and a 64-256 MB static buffer and write an output the size of a 1080p
image (1920x1080 dwords) in title memory, presents a 1080p image, and waits
for the GPU. The diagnostics build (`cargo build --release -p victory
--features diagnostics`) prints the frame line and the copy counters every
five seconds.

Test PC: i9-13900K, RTX 4090. During these runs another program (a local
LLM server) kept the 4090 about 94 % busy, so GPU waits are inflated and
vary from run to run; CPU-side figures are not affected.

## Results, default settings

| Bench shape | Before (median) | After (median) |
|---|---|---|
| 24 submissions, 64 MB static, 8 MB CPU-written | 1,312 ms (mean 6,088) | 28-38 ms |
| 24 submissions, 256 MB static, 16 MB CPU-written | over 10 s, then a 30 s GPU timeout | 29 ms |
| 96 submissions, 64 MB static, 8 MB CPU-written | 31,298 ms | 93 ms |

### Rerun with the GPU idle (03:50 UTC)

| Bench shape and settings | Median frame |
|---|---|
| 24 submissions, 64 MB static, 8 MB CPU-written: build before these changes | 390 ms |
| same, this build, default settings | 9.8 ms |
| same, `VICTORY_CARD_BUFFERS=0` | 9.0 ms |
| same, `VICTORY_INPLACE=2` (card buffers on: big buffers stay copies) | 6.5 ms |
| same, `VICTORY_INPLACE=2 VICTORY_CARD_BUFFERS=0` | 2.5 ms |
| same, synchronous (`VICTORY_LATE_WAIT=0 VICTORY_RELAY=0`) | 10.0 ms |
| 96 submissions, 256 MB static, 16 MB CPU-written: default settings | 33.0 ms |
| same, `VICTORY_INPLACE=2 VICTORY_CARD_BUFFERS=0` | 4.8 ms |

In place, nothing is copied either way and no change check runs; the
output was right in every run. Two cautions before making it the default:
`inplace.rs` says level 2 is not safe together with late wait (on by
default now), and the bench's shader does not read its inputs, so it does
not show the cost of shaders reading big buffers over the bus, which is
why big buffers are kept in card memory.

## What changed

1. **Device heaps and large title allocations are write-watched.**
   `XMemAllocDefault` blocks of 64 KiB and more, and the memory the device
   gives heaps and committed resources (`resource.rs`, `Memory::new`), were
   host heap memory, which cannot be write-watched, so every submission
   copied every range it read in full: 72 refreshes and 1,627 MB a frame on
   the bench, 174-380 ms of copying. They are now reservations of their own
   with `MEM_WRITE_WATCH`; the same frames copy 9.9 MB in 1.4 ms.
   `VICTORY_XMEM_WATCH=0` / `VICTORY_HEAP_WATCH=0` restore the old behaviour.
   Memory the title maps from physical pages (its big GPU heap in the game)
   was already covered by read-only guards and is unchanged.

2. **The card-side change check runs in parallel.** For copies in the
   card's memory (`VICTORY_CARD_BUFFERS`, on by default) a compute pass
   hashes each 64 KiB piece before and after the lists to find what shaders
   wrote. It ran one thread per piece walking 4,096 dependent loads, so each
   pass cost the same whatever its size, and the card was busy with these
   passes for about 330 ms a frame on the bench (the code's notes record 514
   such passes a frame in the game). It is now one 256-thread group per
   piece with a fixed-order fold (`CARD_COMPARE`, `card_compare_groups` in
   `host.rs`). The WARP test of the compare still passes.

3. **The shader cache survives rebuilds.** Translations were keyed by the
   hash of the whole `victory.exe`, so any rebuild made every cached
   translation stale, and the next run translated every shader again and
   had the driver compile every pipeline again in the frames that wanted
   them. The key is now a hash of the translator's sources (victory-shader,
   `shader_cache.rs`, `Cargo.lock`; made by `crates/victory-gpu/build.rs`)
   plus every `VICTORY_*` switch the translator reads and the host's wave
   width. `VICTORY_SHADER_CACHE_KEY=exe` restores the old key.

## Shader compiling during play

Translation and pipeline creation were already off the title's threads
(`pipeline/make.rs`), saved across runs (`pipeline/library.rs`,
`shader_cache.rs`) and started early on the next launch. What remains: a
frame waits up to `VICTORY_PIPELINE_WAIT_MS` (500 ms by default, shared by
all threads) for pipelines not yet made, then drops the draws that need
them. On a first run, or after the translator changes, frames in new areas
can take up to that long. A lower value trades stalls for missing draws.

## Measured but not changed

- `VICTORY_CARD_BUFFERS=0` halves the bench frame (19 vs 38 ms): the card
  copies cost a hash pass per upload and a compare per submission. They were
  turned on because the game's shaders read big buffers over the bus far
  slower than from card memory, which the bench does not model, so they stay
  on.
- `VICTORY_INPLACE=2`: see the idle-GPU table above (the busy-GPU run gave 35 ms and was not a fair test).
- Synchronous submission (`VICTORY_LATE_WAIT=0 VICTORY_RELAY=0`) costs one
  GPU round trip per submission: 34 ms each on the busy GPU (836 ms a frame
  on the bench against 29 ms with late wait, the default).

## Next

- Async compute (second host queue with GPU-side waits between queues).
- In place by default: make level 2 safe with late wait, and make the bench's shader read its inputs so the bus cost shows.
- Run the game with the diagnostics build once its files are available.
