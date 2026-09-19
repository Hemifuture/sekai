use thiserror::Error;

use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::EdgeId;

/// A strictly positive per-edge traversal cost, the precondition Dijkstra needs.
///
/// Wrapping the topology's own quantized lengths in a validated newtype keeps
/// the positivity check at the single point of construction instead of in every
/// shortest-path consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::generators::natural) struct PositiveEdgeMetric {
    costs: Box<[u64]>,
}

impl PositiveEdgeMetric {
    /// Adopts the topology's quantized traversal lengths after proving them positive.
    ///
    /// # Errors
    ///
    /// Returns [`EdgeMetricError::NonPositiveCost`] if any edge quantized to zero.
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

    /// Returns the cost of one edge, or `None` when the identifier is out of range.
    pub(in crate::generators::natural) fn cost(&self, edge: EdgeId) -> Option<u64> {
        self.costs.get(edge.raw() as usize).copied()
    }

    /// Returns the number of edges the metric covers.
    pub(in crate::generators::natural) fn len(&self) -> usize {
        self.costs.len()
    }
}

/// Failures that prevent building a positive edge metric.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(in crate::generators::natural) enum EdgeMetricError {
    /// The topology quantized at least one edge to a zero traversal cost.
    #[error("topology contains a non-positive traversal cost")]
    NonPositiveCost,
}
