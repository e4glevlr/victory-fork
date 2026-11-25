//! Runs the indexed texture pass with synthetic, fully mapped RGBA8 data.

use super::{HEAP_IIDS, HeapDesc, Placement, SHADER_VISIBLE, VIEWS_KIND, create_heap, create_placed_srv, create_srv, heap};
use crate::draw::{HEAP_TEXTURE_BYTES, HEAP_TEXTURES_REUPLOADED_FULL, State};
use crate::object::{self, S_OK, This};
use crate::resource::{self, Memory, PAGEABLE_IIDS};
use std::ptr::null;
use std::sync::atomic::Ordering::Relaxed;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory4};

const MIB: u64 = 1 << 20;

// 2D, linear, single-mip RGBA8. Include the D3D12 description's padding.
#[repr(align(8))]
struct TextureDesc([u32; 14]);

impl TextureDesc {
    fn rgba8(height: u32) -> Self {
        Self([3, 0, 0, 0, 1024, 0, height, 1 | (1 << 16), 28, 1, 0, 1, 0, 0])
    }

    fn resource(&self, memory: &Memory) -> This {
        let mut resource = null();
        assert_eq!(
            resource::create_at_address(
                null(),
                memory.address(),
                self.0.as_ptr().cast(),
                0,
                null(),
                &PAGEABLE_IIDS[0],
                &mut resource,
            ),
            S_OK
        );
        resource
    }
}

fn release(resource: This) {
    let release = object::slot::<extern "system" fn(This) -> u32>(resource, 0x10);
    assert_eq!(release(resource), 0);
}

fn live_bytes() -> u64 {
    resource::HOST_TEXTURES[1].load(Relaxed)
}

#[test]
#[ignore = "requires fresh diagnostics process with D3D12 WARP, a 10 MiB texture budget and rechecks disabled"]
fn indexed_texture_budget() {
    for (var, expected) in [
        ("VICTORY_D3D12_WARP", "1"),
        ("VICTORY_TEXTURE_BUDGET_MB", "10"),
        ("VICTORY_HEAP_RECHECK", "0"),
        ("VICTORY_PLACED_FILLER", "0"),
    ] {
        assert_eq!(std::env::var(var).as_deref(), Ok(expected));
    }
    let reupload_when_full = std::env::var_os("VICTORY_REUPLOAD_WHEN_FULL").is_none_or(|v| v != "0");
    let device = crate::host::host().expect("D3D12 WARP");
    // SAFETY: queries on the factory, adapter and live device.
    let adapter = unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory1().unwrap();
        let warp: IDXGIAdapter1 = factory.EnumWarpAdapter().unwrap();
        let desc = warp.GetDesc1().unwrap();
        let luid = device.device.GetAdapterLuid();
        assert_eq!((luid.LowPart, luid.HighPart), (desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart));
        desc
    };
    println!("WARP adapter {:04x}:{:04x}", adapter.VendorId, adapter.DeviceId);

    #[repr(C)]
    struct Extended(HeapDesc, Placement);
    let desc = Extended(
        HeapDesc { kind: VIEWS_KIND, count: 2, flags: SHADER_VISIBLE, nodes: 0 },
        Placement { address: 0, size: 0 },
    );
    let mut heap_object = null();
    assert_eq!(create_heap(null(), &desc.0, &HEAP_IIDS[0], &mut heap_object), S_OK);
    let heap = heap(heap_object);
    let host = heap.host().unwrap();
    let state = State::default();
    let pass = || state.heap_textures(&device.new_list().unwrap(), heap, host);

    let backing = Memory::zeroed(8 * MIB, 64).unwrap();
    let baseline = TextureDesc::rgba8(2048).resource(&backing);
    assert!(resource::host_resource(baseline).is_some());
    assert_eq!(live_bytes(), 8 * MIB);

    let desc = TextureDesc::rgba8(512);
    let memory_a = Memory::zeroed(2 * MIB, 64).unwrap();
    let memory_b = Memory::zeroed(2 * MIB, 64).unwrap();
    let slot_a = heap.handle(0).unwrap();
    let slot_b = heap.handle(1).unwrap();

    // An already mirrored descriptor can reupload at the limit, according
    // to the configured policy.
    let existing = desc.resource(&memory_a);
    assert!(resource::host_resource(existing).is_some());
    assert_eq!(live_bytes(), 10 * MIB);
    assert!(!resource::room());
    create_srv(null(), existing, null(), slot_a);
    assert!(host.texture_slot_made_from(0, slot_a));
    let frame = crate::frame::current();
    let (control_due, _) = host.texture_slots_due(frame, 2);
    assert_eq!(control_due.len(), 1);
    assert_eq!(control_due[0].0, 0);
    let staged_before = HEAP_TEXTURE_BYTES.load(Relaxed);
    let reuploads_before = HEAP_TEXTURES_REUPLOADED_FULL.load(Relaxed);
    pass();
    let control_staged = HEAP_TEXTURE_BYTES.load(Relaxed) - staged_before;
    let control_reuploads = HEAP_TEXTURES_REUPLOADED_FULL.load(Relaxed) - reuploads_before;
    let control_live = live_bytes();
    let (control_pending, _) = host.texture_slots_due(frame, 2);
    println!(
        "reupload_when_full={reupload_when_full}: existing staged={} MiB, live={} MiB, reuploads={control_reuploads}, due={control_pending:?}",
        control_staged / MIB, control_live / MIB,
    );
    assert_eq!(control_live, 10 * MIB);
    if reupload_when_full {
        assert_eq!(control_staged, 2 * MIB);
        assert_eq!(control_reuploads, 1);
        assert!(control_pending.is_empty());
    } else {
        assert_eq!(control_staged, 0);
        assert_eq!(control_reuploads, 0);
        assert_eq!(control_pending, control_due);
    }
    assert!(host.texture_slot_gone(0));
    release(existing);
    assert_eq!(live_bytes(), 8 * MIB);

    // The production queue serves the newest descriptor first. Write B before A.
    let address_b = memory_b.address();
    let mut view_b = [0; 16];
    view_b[..6].copy_from_slice(&[28, 4, 5768, 0, 0, 1]); // RGBA8 Texture2D SRV
    view_b[10] = address_b as u32;
    view_b[11] = (address_b >> 32) as u32;
    create_placed_srv(null(), desc.0.as_ptr().cast(), &view_b, slot_b);
    let a = desc.resource(&memory_a);
    create_srv(null(), a, null(), slot_a);
    let frame = crate::frame::current();
    let (due, _) = host.texture_slots_due(frame, 2);
    assert_eq!(due.iter().map(|&(slot, _)| slot).collect::<Vec<_>>(), [0, 1]);

    pass();
    let first_live = live_bytes();
    let (pending, _) = host.texture_slots_due(frame + 1, 2);
    assert!(host.texture_slot_gone(0));
    release(a);
    let released_live = live_bytes();
    let mut token = 0;
    assert_eq!(crate::frame::wait(null(), 0, 1000, null(), 0, &mut token), S_OK);
    pass();
    let retry_live = live_bytes();
    let (remaining, _) = host.texture_slots_due(crate::frame::current(), 2);

    println!(
        "first live={} MiB, due={pending:?}, B generation={}; after release={} MiB, retry={} MiB, due={remaining:?}",
        first_live / MIB, due[1].1, released_live / MIB, retry_live / MIB,
    );

    // A new descriptor is not eligible for reupload until it is mirrored.
    // Its padding differs, but it must reuse B's copy once it can proceed.
    assert!(!resource::room());
    let placed_count = resource::placed_textures();
    let mut alias = desc.0;
    (alias[1], alias[13]) = (0x1234, 0x5678);
    create_placed_srv(null(), &alias, &view_b, slot_b);
    assert!(!host.texture_slot_made_from(1, slot_b));
    let frame = crate::frame::current();
    let (alias_due, _) = host.texture_slots_due(frame, 2);
    assert_eq!(alias_due.len(), 1);
    assert_eq!(alias_due[0].0, 1);
    let staged_before = HEAP_TEXTURE_BYTES.load(Relaxed);
    pass();
    assert_eq!(HEAP_TEXTURE_BYTES.load(Relaxed), staged_before);
    assert_eq!(live_bytes(), 10 * MIB);
    assert_eq!(host.texture_slots_due(frame, 2).0, alias_due);

    release(baseline);
    pass();
    let alias_staged = HEAP_TEXTURE_BYTES.load(Relaxed) - staged_before;
    assert_eq!(alias_staged, 2 * MIB);
    assert_eq!(live_bytes(), 2 * MIB);
    assert_eq!(resource::placed_textures(), placed_count);
    assert!(host.texture_slots_due(frame, 2).0.is_empty());
    println!("unmirrored alias waited at capacity, then staged 2 MiB with no new placed resource");

    assert_eq!(first_live, 10 * MIB);
    assert_eq!(pending, [due[1]]);
    assert_eq!(released_live, 8 * MIB);
    assert_eq!(retry_live, 10 * MIB);
    assert!(remaining.is_empty());

    release(heap_object);
}
