//! Same-authority C2 boundary fields; lakes are not connected ocean.
use super::super::global_circulation::{
    ClimateBoundaryFields, GlobalClimateForcing, GlobalClimateForcingBuilder,
};
use super::{geometry::BuiltGeometry, AtlasGenerationError};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::generators::spatial::remap_intensive_f32_cancellable;
use crate::world::natural::atlas_terrain::{bind_surface_sample, AtlasWaterSupport};
use crate::world::natural::{
    formation_runoff_fraction, ClimateSpec, ClimateWorkDomainSnapshot,
    GLOBAL_CIRCULATION_MECHANICAL_COARSE_RESOLUTION_DIVISOR,
};
use crate::world::spatial::{central_angle, SphericalSurfaceSnapshot, SurfaceRef, UnitVector3};
use civ_atlas_terrain::FineTerrain;

pub(super) fn build_forcing(
    input: (
        &FineTerrain,
        &SphericalSurfaceSnapshot,
        &BuiltGeometry,
        [u8; 32],
    ),
    domain: &ClimateWorkDomainSnapshot,
    spec: &ClimateSpec,
    cancellation: &BuildCancellation,
) -> Result<GlobalClimateForcing, AtlasGenerationError> {
    let (fine, surface, geometry, fingerprint) = input;
    domain
        .validate_binding_against(surface)
        .map_err(|e| AtlasGenerationError::Geometry(e.to_string()))?;
    let fields = &geometry.fields;
    let remap = |values: &[f32]| {
        remap_intensive_f32_cancellable(domain.source_to_climate(), values, &|| {
            cancellation.is_cancelled()
        })
        .map_err(|e| AtlasGenerationError::Geometry(e.to_string()))
    };
    let source_land_fraction = fields
        .ocean_fraction
        .iter()
        .map(|v| 1.0 - v)
        .collect::<Vec<_>>();
    let land_evapotranspiration = fields
        .dry_fraction
        .iter()
        .zip(&fields.relative_permeability)
        .map(|(&dry, &k)| (f64::from(dry) * (1.0 - formation_runoff_fraction(k))) as f32)
        .collect::<Vec<_>>();
    let depth = source_ocean_depth(fields, surface);
    let source_publishes_land = (0..fields.land_ocean().len())
        .map(|i| {
            match fields
                .land_ocean()
                .get(i)
                .expect("validated source centre category")
            {
                crate::world::natural::LandOceanKind::Land => 1.0,
                crate::world::natural::LandOceanKind::Ocean => 0.0,
            }
        })
        .collect();
    let ocean_edge_permeability = ocean_edges(
        fine,
        surface,
        &geometry.support,
        domain.climate_surface(),
        cancellation,
    )?;
    let coarse = CubedSphereGrid::new_cancellable(
        domain.face_resolution() / GLOBAL_CIRCULATION_MECHANICAL_COARSE_RESOLUTION_DIVISOR,
        surface.radius().get(),
        &|| cancellation.is_cancelled(),
    )
    .map_err(|e| AtlasGenerationError::Geometry(e.to_string()))?
    .to_surface_snapshot_cancellable(&|| cancellation.is_cancelled())
    .map_err(|e| AtlasGenerationError::Geometry(e.to_string()))?;
    let mechanical_ocean_edge_permeability = Some(ocean_edges(
        fine,
        surface,
        &geometry.support,
        &coarse,
        cancellation,
    )?);
    Ok(GlobalClimateForcingBuilder::build_from_boundary_fields(
        ClimateBoundaryFields {
            source_ref: SurfaceRef::for_spherical(surface),
            source_fingerprint: fingerprint,
            source_land_fraction: source_land_fraction.clone(),
            source_publishes_land,
            elevation_m: remap(&fields.exposed_elevation_m)?,
            land_fraction: remap(&source_land_fraction)?,
            ocean_depth_m: remap(&depth)?,
            land_evapotranspiration_fraction: remap(&land_evapotranspiration)?,
            ocean_edge_permeability,
            mechanical_ocean_edge_permeability,
            sea_level_m: 0.0,
        },
        spec,
        domain,
        cancellation,
    )?)
}

pub(super) fn source_ocean_depth(
    fields: &crate::world::natural::AtlasCellFields,
    surface: &SphericalSurfaceSnapshot,
) -> Vec<f32> {
    fields
        .ocean_volume_m3
        .iter()
        .zip(surface.cells())
        .map(|(volume, cell)| (volume / cell.area.get()) as f32)
        .collect()
}

/// Composite midpoint line quadrature at the fixed source angular pixel interval.
/// Each canonical climate edge is integrated exactly once, independent of lake refinement.
pub(super) fn ocean_edges(
    fine: &FineTerrain,
    source: &SphericalSurfaceSnapshot,
    support: &AtlasWaterSupport,
    target: &SphericalSurfaceSnapshot,
    cancellation: &BuildCancellation,
) -> Result<Vec<f32>, AtlasGenerationError> {
    let spacing = fine.canonical_angular_step_radians();
    let mut fractions = Vec::with_capacity(target.edges().len());
    for (index, edge) in target.edges().iter().enumerate() {
        if index % 64 == 0 {
            super::check_cancelled(cancellation)?;
        }
        let [a, b] = edge
            .vertices
            .map(|v| target.vertices()[v.raw() as usize].position);
        let angle = central_angle(a, b);
        let steps = (angle / spacing).ceil().max(1.0) as usize;
        let queries = (0..steps)
            .map(|i| {
                let t = (i as f64 + 0.5) / steps as f64;
                let left = ((1.0 - t) * angle).sin() / angle.sin();
                let right = (t * angle).sin() / angle.sin();
                let p: [f64; 3] =
                    std::array::from_fn(|j| left * a.components()[j] + right * b.components()[j]);
                UnitVector3::new(p[0], p[1], p[2])
                    .expect("minor edge slerp is finite")
                    .components()
            })
            .collect::<Vec<_>>();
        let samples = fine.sample_surface_batch(&queries, &|| cancellation.is_cancelled())?;
        let mut wet = 0;
        for (direction, sample) in queries.into_iter().zip(samples) {
            if sample.bed_elevation_m >= 0.0 {
                continue;
            }
            wet += usize::from(
                bind_surface_sample(fine, source, support, direction, sample.bed_elevation_m)?
                    .water
                    == crate::world::natural::SurfaceWaterKind::Ocean,
            );
        }
        fractions.push(wet as f32 / steps as f32);
    }
    Ok(fractions)
}
