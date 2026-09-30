use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::{LayeredClimateState, LayeredTendencySystem, LayeredTendencyWorkspace};
use sekai::world::natural::{ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing};

struct CountingAllocator;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
// This private standalone, single-threaded probe forwards every allocation to
// System unchanged; the counters observe requested allocations, not live RSS.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let profile = if args.iter().any(|arg| arg == "--c1") {
        ClimateModelProfile::C1SingleLayerV1
    } else { ClimateModelProfile::C2LayeredV1 };
    let land = if args.iter().any(|arg| arg == "--dry") { 1.0 } else { 0.25 };
    let grid = CubedSphereGrid::new(24, 6_371_000.0).unwrap();
    let count = grid.cell_count();
    let forcing = PlanetForcing::new(
        *grid.fingerprint(), vec![0.0;count], vec![land;count],
        vec![0.3;count], vec![1.0-land;count], vec![[240.0;12];count],
        vec![[15.0;12];count], vec![[15.0;12];count], vec![[0.006;12];count],
    ).unwrap();
    let layout = ClimateLayerLayout::for_profile(profile);
    let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
    for role in state.active_roles().to_vec() {
        for (cell, velocity) in state.velocity_m_s_mut(role).unwrap().iter_mut().enumerate() {
            let radial = grid.cells()[cell].center_unit();
            let scale = if role == ClimateLayerRole::LowerAtmosphere { 3.0 } else { 0.7 };
            *velocity = [(-scale*radial[1]) as f32,(scale*radial[0]) as f32,0.0];
        }
    }
    let permeability = vec![0.75;grid.edges().len()];
    let system = LayeredTendencySystem::new(&grid);
    let cancellation = BuildCancellation::new();
    let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
    let expected = system.evaluate_fast_with_workspace(&state,&forcing,&permeability,0,&cancellation,&mut workspace).unwrap();
    let digest = blake3::hash(format!("{expected:?}").as_bytes());
    println!("PROBE cells={count} profile={profile:?} state=nonzero_zonal_velocity land_fraction={land} output_debug_hash={digest}");
    if args.iter().any(|arg| arg == "--verify-only") { return; }
    for sample in 1..=7 {
        ALLOCATIONS.store(0,Ordering::Relaxed);
        BYTES.store(0,Ordering::Relaxed);
        let started = Instant::now();
        for _ in 0..30 {
            let result = system.evaluate_fast_with_workspace(&state,&forcing,&permeability,0,&cancellation,&mut workspace).unwrap();
            assert_eq!(result, expected);
            std::hint::black_box(result);
        }
        let elapsed = started.elapsed().as_secs_f64();
        let allocations = ALLOCATIONS.load(Ordering::Relaxed);
        let bytes = BYTES.load(Ordering::Relaxed);
        println!("PROBE_SAMPLE sample={sample} evaluations=30 elapsed_s={elapsed:.9} allocations={allocations} requested_bytes={bytes}");
    }
}
