#![cfg_attr(not(test), allow(dead_code))]

use thiserror::Error;

use super::field::QuantizedScalarField;
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::EdgeId;

const MULTIPLIER_SCALE: i64 = 1_000_000;
const RESISTANCE_COEFFICIENT: i64 = 450_000;
const FABRIC_COEFFICIENT: i64 = 1_000_000;
const MINIMUM_MULTIPLIER: i64 = 450_000;
const MAXIMUM_MULTIPLIER: i64 = 2_200_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::generators::natural) struct PositiveEdgeMetric {
    costs: Box<[u64]>,
}

impl PositiveEdgeMetric {
    pub(in crate::generators::natural) fn from_topology_lengths(
        topology: &NaturalTopologyIndex,
    ) -> Result<Self, EdgeMetricError> {
        if topology.edge_traversal_costs().contains(&0) {
            return Err(EdgeMetricError::NonPositiveCost);
        }
        Ok(Self {
            costs: topology.edge_traversal_costs().into(),
        })
    }

    pub(in crate::generators::natural) fn cost(&self, edge: EdgeId) -> Option<u64> {
        self.costs.get(edge.raw() as usize).copied()
    }

    pub(in crate::generators::natural) fn costs(&self) -> &[u64] {
        &self.costs
    }

    pub(in crate::generators::natural) fn len(&self) -> usize {
        self.costs.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(in crate::generators::natural) enum EdgeMetricError {
    #[error("field {field} has {found} cells, expected {expected}")]
    FieldCardinality {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("closed spherical plate metric edge {edge:?} has no two owners")]
    MissingOwners { edge: EdgeId },
    #[error("topology contains a non-positive traversal cost")]
    NonPositiveCost,
    #[error("edge {edge:?} weighted traversal cost overflowed u64")]
    CostOverflow { edge: EdgeId },
}

pub(in crate::generators::natural) fn build_plate_metric(
    topology: &NaturalTopologyIndex,
    resistance: &QuantizedScalarField,
    fabric: &QuantizedScalarField,
) -> Result<PositiveEdgeMetric, EdgeMetricError> {
    validate_field_cardinality(topology, resistance, "resistance")?;
    validate_field_cardinality(topology, fabric, "fabric")?;

    let mut slopes = Vec::with_capacity(topology.edge_count());
    let mut weighted_square_sum = 0.0;
    let mut total_length = 0.0;
    for (edge_index, owners) in topology.edge_owners().iter().copied().enumerate() {
        let [Some(first), Some(second)] = owners else {
            return Err(EdgeMetricError::MissingOwners {
                edge: EdgeId::from_raw(edge_index as u32),
            });
        };
        let base = topology.edge_traversal_costs()[edge_index];
        if base == 0 {
            return Err(EdgeMetricError::NonPositiveCost);
        }
        let difference = i32::from(fabric.get(first).unwrap())
            .abs_diff(i32::from(fabric.get(second).unwrap())) as f64;
        let slope = difference / base as f64;
        slopes.push(slope);
        weighted_square_sum += slope * slope * base as f64;
        total_length += base as f64;
    }
    let rms = (weighted_square_sum / total_length).sqrt();

    let mut costs = Vec::with_capacity(topology.edge_count());
    for (edge_index, owners) in topology.edge_owners().iter().copied().enumerate() {
        let [Some(first), Some(second)] = owners else {
            unreachable!("owner cardinality was checked in the slope pass")
        };
        let resistance_sum =
            i64::from(resistance.get(first).unwrap()) + i64::from(resistance.get(second).unwrap());
        let resistance_scaled = resistance_sum * RESISTANCE_COEFFICIENT / (2 * i64::from(i16::MAX));
        let fabric_normalized = if rms > f64::EPSILON {
            (slopes[edge_index] / (2.0 * rms)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let fabric_scaled = (fabric_normalized * FABRIC_COEFFICIENT as f64).round() as i64;
        let multiplier = (MULTIPLIER_SCALE + resistance_scaled + fabric_scaled)
            .clamp(MINIMUM_MULTIPLIER, MAXIMUM_MULTIPLIER) as u64;
        let base = topology.edge_traversal_costs()[edge_index];
        let weighted =
            u128::from(base) * u128::from(multiplier) + u128::from(MULTIPLIER_SCALE as u64 / 2);
        let cost = weighted / u128::from(MULTIPLIER_SCALE as u64);
        let cost = u64::try_from(cost)
            .map_err(|_| EdgeMetricError::CostOverflow {
                edge: EdgeId::from_raw(edge_index as u32),
            })?
            .max(1);
        costs.push(cost);
    }
    Ok(PositiveEdgeMetric {
        costs: costs.into_boxed_slice(),
    })
}

fn validate_field_cardinality(
    topology: &NaturalTopologyIndex,
    field: &QuantizedScalarField,
    name: &'static str,
) -> Result<(), EdgeMetricError> {
    if field.len() != topology.cell_count() {
        return Err(EdgeMetricError::FieldCardinality {
            field: name,
            expected: topology.cell_count(),
            found: field.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_plate_metric, PositiveEdgeMetric};
    use crate::generators::natural::morphology::field::QuantizedScalarField;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::spatial::SphericalNaturalSurface;
    use crate::world::{CellId, Meters, SphericalSpaceSpec};

    fn fixture() -> (
        crate::world::spatial::SphericalSurfaceSnapshot,
        NaturalTopologyIndex,
    ) {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        })
        .unwrap();
        let view = SphericalNaturalSurface::new(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        (surface, topology)
    }

    fn field(values: impl IntoIterator<Item = i16>) -> QuantizedScalarField {
        QuantizedScalarField::from_test_values(values.into_iter().collect())
    }

    #[test]
    fn zero_field_metric_is_the_exact_legacy_metric() {
        let (surface, topology) = fixture();
        let zero = field((0..surface.cells().len()).map(|_| 0));
        let metric = build_plate_metric(&topology, &zero, &zero).unwrap();

        assert_eq!(metric.costs(), topology.edge_traversal_costs());
        assert_eq!(
            metric,
            PositiveEdgeMetric::from_topology_lengths(&topology).unwrap()
        );
    }

    #[test]
    fn fabric_and_resistance_change_costs_but_keep_positive_symmetric_edges() {
        let (surface, topology) = fixture();
        let resistance =
            field((0..surface.cells().len()).map(
                |index| {
                    if index % 5 < 2 {
                        i16::MAX
                    } else {
                        -i16::MAX
                    }
                },
            ));
        let fabric = field((0..surface.cells().len()).map(|index| {
            let centered = (index as i32 % 17) - 8;
            (centered * 3_500).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        }));
        let zero = field((0..surface.cells().len()).map(|_| 0));
        let metric = build_plate_metric(&topology, &resistance, &fabric).unwrap();
        let resistance_only = build_plate_metric(&topology, &resistance, &zero).unwrap();
        let fabric_only = build_plate_metric(&topology, &zero, &fabric).unwrap();

        assert!(metric.costs().iter().all(|&cost| cost > 0));
        assert_ne!(metric.costs(), topology.edge_traversal_costs());
        assert_ne!(metric.costs(), resistance_only.costs());
        assert_ne!(fabric_only.costs(), topology.edge_traversal_costs());
        for (cell_index, arcs) in topology.arcs().iter().enumerate() {
            let cell = CellId::from_raw(cell_index as u32);
            for arc in arcs {
                assert_eq!(
                    metric.cost(arc.edge),
                    Some(metric.costs()[arc.edge.raw() as usize])
                );
                assert!(topology.arcs()[arc.neighbor.raw() as usize]
                    .iter()
                    .any(|reverse| reverse.neighbor == cell
                        && reverse.edge == arc.edge
                        && metric.cost(reverse.edge) == metric.cost(arc.edge)));
            }
        }
    }

    #[test]
    fn metric_rejects_a_field_from_another_cardinality() {
        let (_, topology) = fixture();
        let short = field([0]);
        let full = field((0..topology.cell_count()).map(|_| 0));

        assert!(build_plate_metric(&topology, &short, &full).is_err());
        assert!(build_plate_metric(&topology, &full, &short).is_err());
    }
}
