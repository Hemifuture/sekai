use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    ProvenanceSedimentRouter, SedimentGenerationError, SedimentInputs,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, ProfileSurfaceBuilder};
use sekai::world::natural::{
    NaturalQualityProfile, SedimentSourceKind, SedimentSourceKindField, SurfaceWaterField,
    SurfaceWaterKind, SEDIMENT_PROVENANCE_SOURCE_COUNT,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

fn surface(radius_m: f64, target_cell_count: u32) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn simple_path(surface: &SphericalSurfaceSnapshot, length: usize) -> Vec<CellId> {
    fn visit(
        surface: &SphericalSurfaceSnapshot,
        cell: CellId,
        length: usize,
        path: &mut Vec<CellId>,
    ) -> bool {
        path.push(cell);
        if path.len() == length {
            return true;
        }
        let mut neighbors = surface
            .cell_edges(cell)
            .unwrap()
            .iter()
            .filter_map(|&edge| surface.opposite_cell(cell, edge))
            .collect::<Vec<_>>();
        neighbors.sort();
        for neighbor in neighbors {
            if !path.contains(&neighbor) && visit(surface, neighbor, length, path) {
                return true;
            }
        }
        path.pop();
        false
    }
    let mut path = Vec::new();
    assert!(visit(surface, CellId::from_raw(0), length, &mut path));
    path
}

struct Fields {
    elevation_m: Vec<f32>,
    water: SurfaceWaterField,
    receiver: Vec<Option<CellId>>,
    drainage_surface_m: Vec<f32>,
    lake_depth_m: Vec<f32>,
    discharge_m3_s: Vec<f32>,
    fluvial_erosion_m: Vec<f32>,
    hillslope_removed_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    hillslope_deposited_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    coastal_removed_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    coastal_ocean_injection_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    marine_exposure: Vec<f32>,
    density_kg_m3: Vec<f32>,
    sources: SedimentSourceKindField,
    previous_thickness_m: Vec<f32>,
    previous_provenance: Vec<[f32; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
}

impl Fields {
    fn inputs(&self) -> SedimentInputs<'_> {
        SedimentInputs {
            elevation_m: &self.elevation_m,
            sea_level_m: 0.0,
            surface_water: &self.water,
            flow_receiver: &self.receiver,
            drainage_surface_elevation_m: &self.drainage_surface_m,
            lake_depth_m: &self.lake_depth_m,
            mean_annual_discharge_m3_s: &self.discharge_m3_s,
            fluvial_erosion_m: &self.fluvial_erosion_m,
            hillslope_removed_by_source_kg: &self.hillslope_removed_by_source_kg,
            hillslope_deposited_by_source_kg: &self.hillslope_deposited_by_source_kg,
            coastal_removed_by_source_kg: &self.coastal_removed_by_source_kg,
            coastal_ocean_injection_by_source_kg: &self.coastal_ocean_injection_by_source_kg,
            marine_exposure: &self.marine_exposure,
            substrate_density_kg_m3: &self.density_kg_m3,
            sediment_sources: &self.sources,
            previous_sediment_thickness_m: &self.previous_thickness_m,
            previous_provenance_fraction: &self.previous_provenance,
        }
    }
}

fn zero_fields(surface: &SphericalSurfaceSnapshot) -> Fields {
    let count = surface.cells().len();
    Fields {
        elevation_m: vec![100.0; count],
        water: SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::DryLand; count]),
        receiver: vec![None; count],
        drainage_surface_m: vec![100.0; count],
        lake_depth_m: vec![0.0; count],
        discharge_m3_s: vec![0.0; count],
        fluvial_erosion_m: vec![0.0; count],
        hillslope_removed_by_source_kg: vec![[0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count],
        hillslope_deposited_by_source_kg: vec![[0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count],
        coastal_removed_by_source_kg: vec![[0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count],
        coastal_ocean_injection_by_source_kg: vec![[0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count],
        marine_exposure: vec![0.0; count],
        density_kg_m3: vec![2_700.0; count],
        sources: SedimentSourceKindField::from_kinds(vec![SedimentSourceKind::Felsic; count]),
        previous_thickness_m: vec![0.0; count],
        previous_provenance: vec![[0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count],
    }
}

#[test]
fn five_sources_and_paired_hillslope_close_without_rerouting_or_source_free_deposition() {
    let surface = surface(10_000.0, 42);
    let mut fields = zero_fields(&surface);
    let source_kinds = [
        SedimentSourceKind::Felsic,
        SedimentSourceKind::Mafic,
        SedimentSourceKind::Volcaniclastic,
        SedimentSourceKind::Sedimentary,
        SedimentSourceKind::Metamorphic,
    ];
    let mut kinds = vec![SedimentSourceKind::Felsic; surface.cells().len()];
    for (index, &kind) in source_kinds.iter().enumerate() {
        kinds[index] = kind;
        if index < 4 {
            fields.fluvial_erosion_m[index] = 0.01;
        }
    }
    fields.sources = SedimentSourceKindField::from_kinds(kinds);
    let paired_mass_kg = 12_345.0;
    fields.hillslope_removed_by_source_kg[4][4] = paired_mass_kg;
    fields.hillslope_deposited_by_source_kg[5][4] = paired_mass_kg;

    let result =
        ProvenanceSedimentRouter::route(&surface, fields.inputs(), 1.0, &BuildCancellation::new())
            .unwrap();
    assert!(result.budget_report().global_relative_error() <= 1.0e-8);
    assert!(result
        .budget_report()
        .provenance_relative_errors()
        .iter()
        .all(|&error| error <= 1.0e-7));
    assert!(result
        .budget_report()
        .produced_by_source_kg()
        .iter()
        .all(|&mass| mass > 0.0));
    assert_eq!(
        result.fields().dominant_source(5),
        Some(SedimentSourceKind::Metamorphic)
    );
    assert_eq!(result.fields().sediment_throughput_kg()[5], 0.0);
    assert!(result.fields().endorheic_storage_kg()[0] > 0.0);

    let zero = zero_fields(&surface);
    let zero =
        ProvenanceSedimentRouter::route(&surface, zero.inputs(), 1.0, &BuildCancellation::new())
            .unwrap();
    assert_eq!(zero.budget_report().produced_mass_kg(), 0.0);
    assert!(zero
        .fields()
        .sediment_thickness_m()
        .iter()
        .all(|&value| value == 0.0));
    assert!(zero
        .routed_sediment_deposition_m()
        .iter()
        .all(|&value| value == 0.0));
    assert!(zero
        .coastal_deposition_m()
        .iter()
        .all(|&value| value == 0.0));
}

fn routed_chain(surface: &SphericalSurfaceSnapshot, discharge_m3_s: f32) -> (Vec<CellId>, Fields) {
    let path = simple_path(surface, 4);
    let mut fields = zero_fields(surface);
    fields.water =
        SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::Ocean; surface.cells().len()]);
    for (position, &cell) in path.iter().enumerate() {
        let index = cell.raw() as usize;
        fields.elevation_m[index] = [300.0, 200.0, 100.0, -50.0][position];
        fields.drainage_surface_m[index] = fields.elevation_m[index];
        fields.discharge_m3_s[index] = discharge_m3_s;
        if position < 3 {
            let mut kinds = fields.water.raw_values().to_vec();
            kinds[index] = SurfaceWaterKind::DryLand.raw();
            fields.water = SurfaceWaterField::from_raw(kinds).unwrap();
            fields.receiver[index] = Some(path[position + 1]);
        }
    }
    fields.fluvial_erosion_m[path[0].raw() as usize] = 0.5;
    (path, fields)
}

#[test]
fn transport_capacity_orders_floodplain_deposition_and_downstream_throughput() {
    let surface = surface(10_000.0, 42);
    let (path, low) = routed_chain(&surface, 1.0);
    let (_, high) = routed_chain(&surface, 1_000_000.0);
    let low_result =
        ProvenanceSedimentRouter::route(&surface, low.inputs(), 1.0, &BuildCancellation::new())
            .unwrap();
    let high_result =
        ProvenanceSedimentRouter::route(&surface, high.inputs(), 1.0, &BuildCancellation::new())
            .unwrap();
    let head = path[0].raw() as usize;
    let ocean = path[3].raw() as usize;
    assert!(
        low_result.routed_sediment_deposition_m()[head]
            > high_result.routed_sediment_deposition_m()[head]
    );
    assert!(
        high_result.fields().sediment_throughput_kg()[ocean]
            > low_result.fields().sediment_throughput_kg()[ocean]
    );
    assert!(high_result.fields().shelf_delivery_kg()[ocean] > 0.0);
    assert!(high_result.budget_report().global_relative_error() <= 1.0e-8);
}

#[test]
fn lake_and_closed_basin_store_mass_while_exposed_shelf_exports_more_to_deep_ocean() {
    let surface = surface(10_000.0, 42);
    let mut inland = zero_fields(&surface);
    inland.fluvial_erosion_m[0] = 0.02;
    inland.fluvial_erosion_m[1] = 0.02;
    let mut water = vec![SurfaceWaterKind::DryLand; surface.cells().len()];
    water[0] = SurfaceWaterKind::Lake;
    inland.water = SurfaceWaterField::from_kinds(water);
    inland.lake_depth_m[0] = 10.0;
    let inland_result =
        ProvenanceSedimentRouter::route(&surface, inland.inputs(), 1.0, &BuildCancellation::new())
            .unwrap();
    assert!(inland_result.fields().endorheic_storage_kg()[0] > 0.0);
    assert!(inland_result.fields().endorheic_storage_kg()[1] > 0.0);
    assert!(inland_result
        .fields()
        .deep_ocean_delivery_kg()
        .iter()
        .all(|&mass| mass == 0.0));

    let coast_edge = &surface.edges()[0];
    let land = coast_edge.cells[0].raw() as usize;
    let ocean = coast_edge.cells[1].raw() as usize;
    let marine = |exposure: f32| {
        let mut fields = zero_fields(&surface);
        let mut water = vec![SurfaceWaterKind::Ocean; surface.cells().len()];
        water[land] = SurfaceWaterKind::DryLand;
        fields.water = SurfaceWaterField::from_kinds(water);
        fields.elevation_m[land] = 100.0;
        fields.elevation_m[ocean] = -100.0;
        fields.drainage_surface_m = fields.elevation_m.clone();
        fields.discharge_m3_s[ocean] = 1_000.0;
        fields.marine_exposure[ocean] = exposure;
        let mass = 2.0e9;
        fields.coastal_removed_by_source_kg[land][0] = mass;
        fields.coastal_ocean_injection_by_source_kg[ocean][0] = mass;
        ProvenanceSedimentRouter::route(&surface, fields.inputs(), 1.0, &BuildCancellation::new())
            .unwrap()
    };
    let calm = marine(0.0);
    let exposed = marine(0.9);
    assert!(calm.fields().shelf_delivery_kg()[ocean] > exposed.fields().shelf_delivery_kg()[ocean]);
    assert!(
        exposed.fields().deep_ocean_delivery_kg()[ocean]
            > calm.fields().deep_ocean_delivery_kg()[ocean]
    );
    assert!(calm.fields().delta_potential()[ocean] > exposed.fields().delta_potential()[ocean]);
    assert!(calm.coastal_deposition_m()[ocean] > 0.0);
    assert!(calm.budget_report().global_relative_error() <= 1.0e-8);
}

#[test]
fn malformed_ledgers_cycles_and_active_cancellation_are_rejected() {
    let surface = surface(10_000.0, 42);
    let mut unmatched = zero_fields(&surface);
    unmatched.coastal_removed_by_source_kg[0][0] = 1.0;
    assert!(matches!(
        ProvenanceSedimentRouter::route(
            &surface,
            unmatched.inputs(),
            1.0,
            &BuildCancellation::new(),
        ),
        Err(SedimentGenerationError::SourceLedgerMismatch {
            process: "coastal",
            ..
        })
    ));

    let path = simple_path(&surface, 2);
    let mut cyclic = zero_fields(&surface);
    cyclic.receiver[path[0].raw() as usize] = Some(path[1]);
    cyclic.receiver[path[1].raw() as usize] = Some(path[0]);
    assert!(matches!(
        ProvenanceSedimentRouter::route(&surface, cyclic.inputs(), 1.0, &BuildCancellation::new(),),
        Err(SedimentGenerationError::ReceiverCycle)
    ));

    let mut overflow = zero_fields(&surface);
    overflow.coastal_removed_by_source_kg[0][0] = f64::MAX;
    overflow.coastal_removed_by_source_kg[1][0] = f64::MAX;
    overflow.coastal_ocean_injection_by_source_kg[0][0] = f64::MAX;
    overflow.coastal_ocean_injection_by_source_kg[1][0] = f64::MAX;
    assert!(matches!(
        ProvenanceSedimentRouter::route(
            &surface,
            overflow.inputs(),
            1.0,
            &BuildCancellation::new(),
        ),
        Err(SedimentGenerationError::NumericalOverflow)
    ));

    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface().clone();
    let fields = zero_fields(&surface);
    let signal = BuildCancellation::new();
    let worker_signal = signal.clone();
    let worker = std::thread::spawn(move || {
        ProvenanceSedimentRouter::route(&surface, fields.inputs(), 1.0, &worker_signal)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while signal.observation_count() < 16 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(signal.observation_count() >= 16);
    signal.cancel();
    assert!(matches!(
        worker.join().unwrap(),
        Err(SedimentGenerationError::Cancelled)
    ));
}
