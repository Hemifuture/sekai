//! Verify connections between observed wet openings inside one convex coastal compartment.

use civ_atlas_terrain::FineTerrain;

use super::super::{check_cancelled, AtlasGenerationError};
use super::BedArc;
use crate::engine::BuildCancellation;
use crate::world::spatial::{add, UnitVector3};

/// Test whether all previously sampled wet `points` share a submerged route inside their
/// compartment. Every candidate arc includes its midpoint and has sample gaps at most `spacing`
/// radians. An unproved connection returns false so the caller can refine the compartment.
/// Cancellation and terrain evaluation errors propagate without a partial result.
pub(super) fn connected(
    fine: &FineTerrain,
    points: &[[f64; 3]],
    spacing: f64,
    cancel: &BuildCancellation,
) -> Result<bool, AtlasGenerationError> {
    sampled_connection(points, spacing, cancel, |directions| {
        Ok(fine
            .sample_surface_batch(directions, &|| cancel.is_cancelled())?
            .into_iter()
            .map(|p| p.bed_elevation_m))
    })
}

/// Append interior stations of one minor arc and return their count in the shared batch.
pub(super) fn add_path(ends: [[f64; 3]; 2], spacing: f64, directions: &mut Vec<[f64; 3]>) -> usize {
    let arc = BedArc::new(ends);
    if arc.angle == 0.0 {
        return 0;
    }
    let intervals = arc.intervals(spacing);
    directions.extend((1..intervals).map(|i| arc.direction(i as f64 / intervals as f64)));
    intervals - 1
}

/// Consume this whole arc's samples, even when a dry station interrupts the route.
pub(super) fn submerged_path(beds: &mut impl Iterator<Item = f64>, count: usize) -> bool {
    beds.by_ref().take(count).filter(|&bed| bed < 0.0).count() == count
}

fn unit([x, y, z]: [f64; 3]) -> Result<[f64; 3], AtlasGenerationError> {
    UnitVector3::new(x, y, z)
        .map(|point| point.components())
        .map_err(|e| AtlasGenerationError::Geometry(e.to_string()))
}

fn sampled_connection<Beds>(
    points: &[[f64; 3]],
    spacing: f64,
    cancel: &BuildCancellation,
    sample: impl Fn(&[[f64; 3]]) -> Result<Beds, AtlasGenerationError>,
) -> Result<bool, AtlasGenerationError>
where
    Beds: IntoIterator<Item = f64>,
{
    check_cancelled(cancel)?;
    if !spacing.is_finite() || spacing <= 0.0 {
        return Err(AtlasGenerationError::Geometry(
            "coastal path sampling requires a positive finite spacing".into(),
        ));
    }
    let mut ports = points
        .iter()
        .copied()
        .map(unit)
        .collect::<Result<Vec<_>, _>>()?;
    ports.sort_unstable_by(|a, b| a.partial_cmp(b).expect("normalized ports are finite"));
    ports.dedup();
    if ports.len() < 2 {
        return Ok(true);
    }

    // The first star usually proves the entire compartment in one bed batch. Later stars
    // only inspect ports outside the reached component, so no direct pair is tested twice.
    let mut directions = Vec::new();
    let mut counts = Vec::with_capacity(ports.len() - 1);
    let mut star = |origin, targets: &[[f64; 3]]| -> Result<Vec<bool>, AtlasGenerationError> {
        directions.clear();
        counts.clear();
        for &target in targets {
            check_cancelled(cancel)?;
            counts.push(add_path([origin, target], spacing, &mut directions));
        }
        let mut beds = sample(&directions)?.into_iter();
        Ok(counts
            .iter()
            .map(|&n| submerged_path(&mut beds, n))
            .collect())
    };
    let first_star = star(ports[0], &ports[1..])?;
    if !first_star.contains(&false) {
        return Ok(true);
    }
    if ports.len() == 2 {
        return Ok(false);
    }
    let mut reached = Vec::with_capacity(ports.len() + 1);
    reached.push(ports[0]);
    let mut pending = Vec::new();
    for (&port, wet) in ports[1..].iter().zip(first_star) {
        if wet {
            reached.push(port);
        } else {
            pending.push(port);
        }
    }
    let mut from = 1;
    let mut try_centre = true;
    while !pending.is_empty() {
        if from == reached.len() {
            if !try_centre {
                return Ok(false);
            }
            try_centre = false;
            // The normalized mean stays in the same convex spherical compartment. It can
            // provide a bend between entrances only if the bed and an access arc are wet.
            let centre = unit(ports.iter().copied().fold([0.0; 3], add))?;
            if ports.contains(&centre) {
                return Ok(false);
            }
            let wet = sample(&[centre])?
                .into_iter()
                .next()
                .is_some_and(|z| z < 0.0);
            if !wet || !star(centre, &reached)?.contains(&true) {
                return Ok(false);
            }
            reached.push(centre);
        }
        let mut paths = star(reached[from], &pending)?.into_iter();
        from += 1;
        pending.retain(|port| {
            let wet = paths.next().expect("each pending port has one tested path");
            if wet {
                reached.push(*port);
            }
            !wet
        });
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrances_between_dry_edge_stations_require_a_submerged_transect() {
        // In this gnomonic chart P lies a quarter of the way along AB, Q along BC.
        // No coarse corner, edge midpoint or cell centre lies within their narrow channel.
        let ports = [[1., -0.5, 0.], [1., 0.75, 0.5]];
        let stencil = [
            [-1., 0.],
            [0., 0.],
            [1., 0.],
            [0.5, 1.],
            [0., 2.],
            [-0.5, 1.],
            [0., 2. / 3.],
        ];
        for (ridge_m, connected) in [(2.0, false), (0.0, false), (-1.0, true)] {
            let bed = |[radial, east, north]: [f64; 3]| {
                let (x, y) = (east / radial, north / radial);
                if (y - 0.4 * (x + 0.5)).abs() > 0.03 || !(-0.55..=0.8).contains(&x) {
                    2.0
                } else if (-0.2..=0.2).contains(&x) {
                    ridge_m
                } else {
                    -1.0
                }
            };
            assert!(stencil.iter().all(|&[x, y]| bed([1., x, y]) > 0.0));
            assert!(ports.iter().copied().all(|p| bed(p) < 0.0));
            for spacing in [0.05, std::f64::consts::PI] {
                // Even an arc shorter than one prescribed interval must check its interior.
                let found = sampled_connection(&ports, spacing, &BuildCancellation::new(), |p| {
                    Ok(p.iter().copied().map(bed).collect::<Vec<_>>())
                })
                .unwrap();
                assert_eq!(found, connected);
            }
        }
    }

    #[test]
    fn a_wet_interior_junction_can_join_three_ports_with_dry_direct_routes() {
        // Unit-length gnomonic offsets are equally spaced around the junction at (0, 0).
        let y = 3.0_f64.sqrt() / 2.0;
        let ports = [[1., 1., 0.], [1., -0.5, y], [1., -0.5, -y]];
        for (junction_m, connected) in [(1., false), (-1., true)] {
            let bed = |[radial, east, north]: [f64; 3]| {
                let (x, y) = (east / radial, north / radial);
                if x.hypot(y) < 0.12 {
                    return junction_m;
                }
                let on_arm = ports
                    .iter()
                    .any(|&[_, dx, dy]| x * dx + y * dy > 0.0 && (x * dy - y * dx).abs() < 0.05);
                if on_arm {
                    -1.0
                } else {
                    1.0
                }
            };
            let mut directions = [ports[2], ports[0], ports[1], ports[0].map(|x| 2.0 * x)];
            for _ in 0..2 {
                let found = sampled_connection(&directions, 0.04, &BuildCancellation::new(), |p| {
                    Ok(p.iter().copied().map(bed).collect::<Vec<_>>())
                })
                .unwrap();
                assert_eq!(found, connected);
                directions.reverse();
            }
        }
    }
}
