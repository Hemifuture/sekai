//! T1 v2 hierarchical primitive-value derivation (M2 Task 2).
//!
//! Implements the frozen contract in
//! `docs/superpowers/specs/2026-08-20-t1v2-hierarchical-derivation.md`:
//! primitives are data atoms (L0 Goldberg cells, L1 fan triangles, L2+
//! recursive four-way splits), values derive from canonical seed-sorted
//! midpoint displacement over the L0 vertex lattice, face values are the
//! mean of the three corner anchors, conditioning comes from the root L0
//! cell through the v2 remapping of the M1 C-table, and river corridors
//! suppress face values through the M1 §7 carve reused verbatim. The
//! walk shares its recursion tree — corner order, child indexing, and
//! midpoint geometry — with the display subdivision in
//! `app/amplified_mesh.rs`, so geometry and data split identically.
//!
//! Since plan Task 3 the display layer reads this engine's primitive
//! values; the M1 `TerrainAmplifier` serves on inside it as the single
//! fact source for conditioning drivers and §7 river carving.

use blake3::Hasher;

use super::hierarchical_rivers::{fresh_reach_path_caches, ReachPathCache};
use super::terrain_amplification::{
    badlands_gate, erodibility_amplitude, langbein_schumm, sediment_damping,
    surface_roughness_hurst, AmplificationFieldsView, ConditioningView, SurfaceRegime,
    TerrainAmplificationError, TerrainAmplifier, SHELF_BASE_AMPLITUDE_M, SHELF_TRANSITION_M,
};
use crate::world::natural::{
    GeologicSubstrateSnapshot, NaturalSurfaceFormationSnapshot, RiverSegment,
    SphericalTectonicSnapshot, ELEVATION_MAX_M, ELEVATION_MIN_M, FORMATION_SHELF_BREAK_DEPTH_M,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};
use crate::world::{CellId, RootSeed};

/// Domain separator for every v2 canonical seed (spec §2.1).
const DERIVATION_DOMAIN: &[u8] = b"sekai-t1v2-derivation-v1\0";
/// Hard subdivision-path depth cap (spec §5): ≈2.4 m primitives at draft
/// spacing, within the u64 ID encoding budget.
pub const HIERARCHICAL_PATH_DEPTH_MAX: usize = 16;
/// Frozen hierarchical probe count (spec §6).
pub const HIERARCHICAL_PROBE_COUNT: usize = 256;
/// Frozen probe path depths (spec §6): one probe block per depth.
const PROBE_PATH_DEPTHS: [usize; 4] = [0, 2, 5, 9];
/// Probes per depth block: 64 cells spread evenly over the surface.
const PROBE_CELL_BLOCK: usize = HIERARCHICAL_PROBE_COUNT / PROBE_PATH_DEPTHS.len();

/// C1 v3 gain from the T0 neighbour-relief range to the land A0
/// (spec §10 amendment A7): the increment ladder continues the T0
/// spectrum where the lattice ends. The range of ~6 ring samples
/// estimates σ ≈ range/2.5, and the uniform increment noise carries
/// σ = A/√3, so A0 = √3/2.5 · range ≈ 0.7 · range.
const SPECTRAL_CONTINUATION: f64 = 0.7;
/// A7 floor in metres: plains keep a whisper of fine texture (their
/// real fine relief is dissection-driven, deferred to T1 v3 drainage).
const LAND_A0_FLOOR_M: f64 = 30.0;
/// C6 badlands amplitude share on top of the continued spectrum
/// (legacy 60/320 ratio, amendment A7).
const BADLANDS_A0_SHARE: f64 = 0.2;
/// C4 Hurst channel starting value: weaker substrate is rougher at fine
/// scales (calibrated in plan Task 5).
const HURST_ERODIBILITY_DELTA: f64 = 0.1;
/// C5 Hurst channel starting value: Langbein–Schumm peaked dissection
/// (calibrated in plan Task 5).
const HURST_DISSECTION_DELTA: f64 = 0.1;
/// C10 frozen Hurst bounds (spec §2.3).
const HURST_MIN: f64 = 0.4;
const HURST_MAX: f64 = 0.85;
/// C9 coastal Hurst anchors from the fractal-coastline dimension
/// D = 2 − H (spec §9.2: D ≈ 1.2 smooth … 1.33 rugged coasts).
const HURST_COAST_SMOOTH: f64 = 0.8;
const HURST_COAST_RUGGED: f64 = 0.67;
/// C8: the measured abyssal-hill A0 envelope in metres (spec §3).
const OCEAN_HILL_AMPLITUDE_MIN_M: f64 = 50.0;
const OCEAN_HILL_AMPLITUDE_MAX_M: f64 = 300.0;

/// One hierarchical primitive value (spec §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrimitiveValue {
    /// Elevation in metres, inside the authoritative bounds.
    pub elevation_m: f32,
    /// Discrete per-primitive regime label (spec §2.2, no blending).
    pub regime: SurfaceRegime,
}

/// A validated 2-bit-per-step subdivision path (spec §1, k ≤ 16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HierarchicalPath {
    steps: [u8; HIERARCHICAL_PATH_DEPTH_MAX],
    len: u8,
}

impl HierarchicalPath {
    /// Copies a step slice into a validated path.
    ///
    /// Panics when the slice is deeper than the spec §5 cap or a step is
    /// not a 2-bit child index — both are caller programming errors.
    pub fn new(steps: &[u8]) -> Self {
        assert!(
            steps.len() <= HIERARCHICAL_PATH_DEPTH_MAX,
            "hierarchical path depth {} exceeds the spec cap {}",
            steps.len(),
            HIERARCHICAL_PATH_DEPTH_MAX
        );
        let mut packed = [0_u8; HIERARCHICAL_PATH_DEPTH_MAX];
        for (slot, &step) in packed.iter_mut().zip(steps) {
            assert!(step <= 3, "path step {step} is not a 2-bit child index");
            *slot = step;
        }
        Self {
            steps: packed,
            len: steps.len() as u8,
        }
    }

    /// Returns the path steps in root-to-leaf order.
    pub fn steps(&self) -> &[u8] {
        &self.steps[..usize::from(self.len)]
    }
}

/// One frozen hierarchical probe ID (spec §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HierarchicalProbe {
    /// The root L0 cell.
    pub cell: CellId,
    /// The fan sector inside the root cell.
    pub sector: u8,
    /// The fixed subdivision path.
    pub path: HierarchicalPath,
}

/// The primitive containing one direction at one level (spec §1 shell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocatedPrimitive {
    /// The L0 cell primitive itself.
    Cell(CellId),
    /// An L1+ fan-subdivision triangle.
    Triangle {
        /// The root L0 cell.
        cell: CellId,
        /// The fan sector inside the root cell.
        sector: u8,
        /// The subdivision path below the sector triangle.
        path: HierarchicalPath,
    },
}

/// Per-cell v2 conditioning: base increment amplitude and Hurst exponent
/// (spec §2.3 and §3). Conditions are L0 facts shared by the whole
/// primitive tree — they never derive per level.
#[derive(Debug, Clone, Copy)]
struct Conditions {
    a0_m: f64,
    hurst: f64,
}

impl Conditions {
    /// The per-level increment amplitude `A(level) = A0 · 2^(−H·level)`
    /// (spec §2.3, `p = 2^(−H)`).
    fn amplitude_m(self, level: u32) -> f64 {
        self.a0_m * (-self.hurst * f64::from(level)).exp2()
    }

    /// The symmetric two-cell mean used on shared boundary chains
    /// (spec §10 amendment A1).
    fn mean(a: Self, b: Self) -> Self {
        Self {
            a0_m: 0.5 * (a.a0_m + b.a0_m),
            hurst: 0.5 * (a.hurst + b.hurst),
        }
    }
}

/// The two condition sources available inside one sector tree.
struct SectorConditions {
    /// The root cell's conditions (all interior and spoke edges).
    root: Conditions,
    /// The two-cell mean across this sector's shared L0 boundary edge.
    boundary: Conditions,
}

impl SectorConditions {
    fn pick(&self, on_boundary_chain: bool) -> Conditions {
        if on_boundary_chain {
            self.boundary
        } else {
            self.root
        }
    }
}

/// One lattice vertex of the derivation walk: geometry, derived value,
/// and canonical seed travel together (spec §2.1). Anchors are internal
/// transients of the split rule — never stored, never published.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    position: UnitVector3,
    value_m: f64,
    seed: [u8; 32],
}

/// One triangle of the walk. `boundary_edge[i]` marks the directed edge
/// (corner i → corner i+1) as lying on the root cell's shared L0
/// boundary arc, whose midpoints take the two-cell mean conditions.
#[derive(Clone, Copy)]
struct TriangleFrame {
    corners: [Anchor; 3],
    boundary_edge: [bool; 3],
}

/// The T1 v2 hierarchical evaluation engine: built once per published
/// world, then evaluated as a pure function of hierarchical IDs.
pub struct HierarchicalEvaluator {
    /// The M1 amplifier doubles as the fact source for conditioning
    /// drivers, the geodesic locator, and §7 river carving.
    amplifier: TerrainAmplifier,
    /// Per-reach memo of the A6 path tree (spec §6: caches only
    /// accelerate — every point is the pure derivation's bit-exact
    /// value, deepened lazily as queries demand).
    river_paths: Vec<ReachPathCache>,
    root_seed_raw: u64,
    // Self-contained copies of the validated surface geometry.
    ring_offsets: Vec<u32>,
    ring_vertices: Vec<u32>,
    ring_neighbors: Vec<u32>,
    cell_site: Vec<UnitVector3>,
    cell_centroid: Vec<UnitVector3>,
    cell_conditions: Vec<Conditions>,
    vertex_position: Vec<UnitVector3>,
    vertex_value_m: Vec<f64>,
}

impl HierarchicalEvaluator {
    /// Builds the engine from one validated surface and its T0 fields.
    pub fn new(
        surface: &SphericalSurfaceSnapshot,
        fields: AmplificationFieldsView<'_>,
        root_seed: RootSeed,
    ) -> Result<Self, TerrainAmplificationError> {
        let amplifier = TerrainAmplifier::new(surface, fields, root_seed)?;
        Ok(Self::from_amplifier(amplifier, surface, root_seed))
    }

    /// Assembles the engine straight from the published formation
    /// product, river network included.
    pub fn from_formation_product(
        surface: &SphericalSurfaceSnapshot,
        compatibility: &SphericalTectonicSnapshot,
        substrate: &GeologicSubstrateSnapshot,
        formation: &NaturalSurfaceFormationSnapshot,
        root_seed: RootSeed,
    ) -> Result<Self, TerrainAmplificationError> {
        let amplifier = TerrainAmplifier::from_formation_product(
            surface,
            compatibility,
            substrate,
            formation,
            root_seed,
        )?;
        Ok(Self::from_amplifier(amplifier, surface, root_seed))
    }

    /// Attaches the published river network for §4 corridor suppression.
    pub fn with_rivers(
        mut self,
        surface: &SphericalSurfaceSnapshot,
        segments: &[RiverSegment],
    ) -> Result<Self, TerrainAmplificationError> {
        self.amplifier = self.amplifier.with_rivers(surface, segments)?;
        self.river_paths = fresh_reach_path_caches(self.amplifier.river_reaches().len());
        Ok(self)
    }

    fn from_amplifier(
        amplifier: TerrainAmplifier,
        surface: &SphericalSurfaceSnapshot,
        root_seed: RootSeed,
    ) -> Self {
        let cells = surface.cells();
        let vertex_count = surface.vertices().len();

        let mut ring_offsets = Vec::with_capacity(cells.len() + 1);
        let mut ring_vertices = Vec::new();
        let mut ring_neighbors = Vec::new();
        ring_offsets.push(0_u32);
        // Boundary-vertex anchors: the arithmetic mean of every adjacent
        // cell's T0 elevation — the one declared cross-cell combination
        // in the L0→L1 anchoring rule (spec §2.1).
        let mut vertex_sum_m = vec![0.0_f64; vertex_count];
        let mut vertex_shares = vec![0_u32; vertex_count];
        let cell_conditions: Vec<Conditions> = {
            let view = amplifier.conditioning();
            for (index, cell) in cells.iter().enumerate() {
                for (side, &vertex) in cell.boundary_vertices.iter().enumerate() {
                    ring_vertices.push(vertex.raw());
                    let edge = surface
                        .edge(cell.boundary_edges[side])
                        .expect("a validated snapshot resolves every cell boundary edge");
                    let neighbor = if edge.cells[0] == cell.id {
                        edge.cells[1]
                    } else {
                        edge.cells[0]
                    };
                    ring_neighbors.push(neighbor.raw());
                    let slot = vertex.raw() as usize;
                    vertex_sum_m[slot] += f64::from(view.elevation_m[index]);
                    vertex_shares[slot] += 1;
                }
                ring_offsets.push(ring_vertices.len() as u32);
            }
            (0..cells.len())
                .map(|index| cell_conditions(&view, index))
                .collect()
        };
        let vertex_value_m: Vec<f64> = vertex_sum_m
            .iter()
            .zip(&vertex_shares)
            .map(|(&sum, &shares)| sum / f64::from(shares.max(1)))
            .collect();

        Self {
            river_paths: fresh_reach_path_caches(amplifier.river_reaches().len()),
            amplifier,
            root_seed_raw: root_seed.raw(),
            ring_offsets,
            ring_vertices,
            ring_neighbors,
            cell_site: cells.iter().map(|cell| cell.site).collect(),
            cell_centroid: cells.iter().map(|cell| cell.centroid).collect(),
            cell_conditions,
            vertex_position: surface
                .vertices()
                .iter()
                .map(|vertex| vertex.position)
                .collect(),
            vertex_value_m,
        }
    }

    /// The L0 cell primitive value: identically the T0 published
    /// elevation (spec §1 identity invariant; no carve, no noise).
    ///
    /// Panics when the cell is outside the surface.
    pub fn cell_value(&self, cell: CellId) -> PrimitiveValue {
        let elevation_m = self.amplifier.conditioning().elevation_m[cell.raw() as usize];
        PrimitiveValue {
            elevation_m,
            regime: self.regime_for(f64::from(elevation_m)),
        }
    }

    /// Evaluates one L1+ primitive: `value(cell, sector, path)` (spec §1).
    ///
    /// The primitive level is `1 + path.len()`. Panics on an invalid ID
    /// (unknown cell, sector outside the fan, step > 3, path deeper than
    /// the spec §5 cap) — IDs come from the same enumeration this engine
    /// defines, so an invalid one is a caller programming error.
    pub fn value(&self, cell: CellId, sector: u8, path: &[u8]) -> PrimitiveValue {
        let frame = self.walk_frame(cell, sector, path);
        self.face_value(cell.raw() as usize, &frame, 1 + path.len() as u8)
    }

    /// Descends one subdivision path to its primitive's anchor frame.
    fn walk_frame(&self, cell: CellId, sector: u8, path: &[u8]) -> TriangleFrame {
        assert!(
            path.len() <= HIERARCHICAL_PATH_DEPTH_MAX,
            "hierarchical path depth {} exceeds the spec cap {}",
            path.len(),
            HIERARCHICAL_PATH_DEPTH_MAX
        );
        let conditions = self.sector_conditions(cell.raw(), sector);
        let mut frame = self.sector_frame(cell.raw(), sector);
        for (step, &child) in path.iter().enumerate() {
            assert!(child <= 3, "path step {child} is not a 2-bit child index");
            let level = step as u32 + 1;
            let midpoints = self.split_midpoints(&frame, level, &conditions);
            frame = child_frame(frame, midpoints, child);
        }
        frame
    }

    /// The derived face elevation of the primitive containing `direction`
    /// at `level`, before river suppression and bounds clamping — the
    /// guidance field for hierarchical river rerouting (a carve input can
    /// never read carved output, so this stays cycle-free).
    pub(super) fn uncarved_sample_elevation_m(&self, direction: UnitVector3, level: u8) -> f64 {
        match self.locate(direction, level) {
            LocatedPrimitive::Cell(cell) => {
                f64::from(self.amplifier.conditioning().elevation_m[cell.raw() as usize])
            }
            LocatedPrimitive::Triangle { cell, sector, path } => {
                let frame = self.walk_frame(cell, sector, path.steps());
                let [a, b, c] = &frame.corners;
                (a.value_m + b.value_m + c.value_m) / 3.0
            }
        }
    }

    /// The M1 amplifier inside this engine — the L0 fact source the
    /// hierarchical river module shares (reaches, beds, carve laws).
    pub(super) fn amplifier(&self) -> &TerrainAmplifier {
        &self.amplifier
    }

    /// The memo slot of one reach's path tree (reach-list aligned).
    pub(super) fn river_path_slot(&self, reach: u32) -> Option<&ReachPathCache> {
        self.river_paths.get(reach as usize)
    }

    /// The number of published river reaches (segment-order aligned).
    pub fn river_reach_count(&self) -> usize {
        self.amplifier.river_reaches().len()
    }

    /// The deepest meaningful rerouting depth of one reach — where the
    /// sub-segment length falls under half the meander wavelength
    /// (spec §10 amendment A6).
    pub fn river_path_depth_cap(&self, reach: u32) -> u8 {
        super::hierarchical_rivers::path_depth_cap(self, reach)
    }

    /// Materializes one reach's rerouted polyline at `depth` (clamped to
    /// the reach's cap): `2^depth + 1` points from the upstream to the
    /// downstream cell centroid. Depth 0 is the L0 chain.
    pub fn river_path(&self, reach: u32, depth: u8) -> Vec<UnitVector3> {
        super::hierarchical_rivers::materialize_path(self, reach, depth)
    }

    /// Locates the primitive of `level` containing `direction`.
    ///
    /// Level 0 is the L0 cell; levels above `1 + k_max` clamp to the
    /// deepest primitive. Containment ties resolve deterministically to
    /// the candidate with the largest interior margin.
    pub fn locate(&self, direction: UnitVector3, level: u8) -> LocatedPrimitive {
        let cell_index = self.locate_cell_index(direction);
        if level == 0 {
            return LocatedPrimitive::Cell(CellId::from_raw(cell_index));
        }
        let depth = usize::from(level - 1).min(HIERARCHICAL_PATH_DEPTH_MAX);
        let sector = self.locate_sector(cell_index, direction);
        let (start, len) = self.ring_bounds(cell_index);
        let near = self.vertex_position[self.ring_vertices[start + sector] as usize];
        let far = self.vertex_position[self.ring_vertices[start + (sector + 1) % len] as usize];
        let mut corners = [self.cell_centroid[cell_index as usize], near, far];
        let mut steps = [0_u8; HIERARCHICAL_PATH_DEPTH_MAX];
        for slot in steps.iter_mut().take(depth) {
            let ab = geometric_midpoint(corners[0], corners[1]);
            let bc = geometric_midpoint(corners[1], corners[2]);
            let ca = geometric_midpoint(corners[2], corners[0]);
            let children = [
                [corners[0], ab, ca],
                [ab, corners[1], bc],
                [ca, bc, corners[2]],
                [ab, bc, ca],
            ];
            let mut child = 0;
            let mut best = f64::NEG_INFINITY;
            for (candidate, triangle) in children.iter().enumerate() {
                let margin = interior_margin(triangle, direction);
                if margin > best {
                    best = margin;
                    child = candidate;
                }
            }
            *slot = child as u8;
            corners = children[child];
        }
        LocatedPrimitive::Triangle {
            cell: CellId::from_raw(cell_index),
            sector: sector as u8,
            path: HierarchicalPath::new(&steps[..depth]),
        }
    }

    /// The thin shell (spec §1): locate the level primitive containing
    /// `direction` and return its value — a naturally stepped field.
    pub fn sample(&self, direction: UnitVector3, level: u8) -> PrimitiveValue {
        match self.locate(direction, level) {
            LocatedPrimitive::Cell(cell) => self.cell_value(cell),
            LocatedPrimitive::Triangle { cell, sector, path } => {
                self.value(cell, sector, path.steps())
            }
        }
    }

    /// The frozen hierarchical probe set (spec §6): 64 cells spread
    /// evenly over the surface (`cell = ⌊j·N/64⌋`), sector `j mod
    /// ring_len`, path steps `(j + t) mod 4`, repeated per depth block
    /// {0, 2, 5, 9}. The formula is the definition — no stored IDs.
    pub fn probe_ids(&self) -> Vec<HierarchicalProbe> {
        let cell_count = self.cell_conditions.len();
        (0..HIERARCHICAL_PROBE_COUNT)
            .map(|index| {
                let depth = PROBE_PATH_DEPTHS[index / PROBE_CELL_BLOCK];
                let j = index % PROBE_CELL_BLOCK;
                let cell_index = (j * cell_count / PROBE_CELL_BLOCK) as u32;
                let (_, len) = self.ring_bounds(cell_index);
                let steps: Vec<u8> = (0..depth).map(|t| ((j + t) % 4) as u8).collect();
                HierarchicalProbe {
                    cell: CellId::from_raw(cell_index),
                    sector: (j % len) as u8,
                    path: HierarchicalPath::new(&steps),
                }
            })
            .collect()
    }

    /// Blake3 over the little-endian f32 elevations of the frozen probe
    /// set, in probe order (spec §6).
    pub fn probe_fingerprint(&self) -> [u8; 32] {
        let mut hasher = Hasher::new();
        for probe in self.probe_ids() {
            let value = self.value(probe.cell, probe.sector, probe.path.steps());
            hasher.update(&value.elevation_m.to_le_bytes());
        }
        *hasher.finalize().as_bytes()
    }

    fn ring_bounds(&self, cell_index: u32) -> (usize, usize) {
        let start = self.ring_offsets[cell_index as usize] as usize;
        let end = self.ring_offsets[cell_index as usize + 1] as usize;
        (start, end - start)
    }

    fn sector_conditions(&self, cell_index: u32, sector: u8) -> SectorConditions {
        let (start, len) = self.ring_bounds(cell_index);
        assert!(
            usize::from(sector) < len,
            "sector {sector} is outside the {len}-sector fan of cell {cell_index}"
        );
        let root = self.cell_conditions[cell_index as usize];
        let neighbor = self.ring_neighbors[start + usize::from(sector)] as usize;
        SectorConditions {
            root,
            boundary: Conditions::mean(root, self.cell_conditions[neighbor]),
        }
    }

    /// The L1 sector triangle: centroid anchor plus the adjacent boundary
    /// vertex pair, with the shared cell-boundary edge flagged.
    fn sector_frame(&self, cell_index: u32, sector: u8) -> TriangleFrame {
        let (start, len) = self.ring_bounds(cell_index);
        assert!(
            usize::from(sector) < len,
            "sector {sector} is outside the {len}-sector fan of cell {cell_index}"
        );
        let near = self.ring_vertices[start + usize::from(sector)];
        let far = self.ring_vertices[start + (usize::from(sector) + 1) % len];
        TriangleFrame {
            corners: [
                Anchor {
                    position: self.cell_centroid[cell_index as usize],
                    value_m: f64::from(
                        self.amplifier.conditioning().elevation_m[cell_index as usize],
                    ),
                    seed: self.anchor_seed(b"c", cell_index),
                },
                self.vertex_anchor(near),
                self.vertex_anchor(far),
            ],
            boundary_edge: [false, true, false],
        }
    }

    fn vertex_anchor(&self, vertex_index: u32) -> Anchor {
        Anchor {
            position: self.vertex_position[vertex_index as usize],
            value_m: self.vertex_value_m[vertex_index as usize],
            seed: self.anchor_seed(b"v", vertex_index),
        }
    }

    /// The three midpoint anchors of one split, in (AB, BC, CA) order.
    fn split_midpoints(
        &self,
        frame: &TriangleFrame,
        level: u32,
        conditions: &SectorConditions,
    ) -> [Anchor; 3] {
        let edge = |a: usize, b: usize, index: usize| {
            self.midpoint_anchor(
                &frame.corners[a],
                &frame.corners[b],
                conditions.pick(frame.boundary_edge[index]),
                level,
            )
        };
        [edge(0, 1, 0), edge(1, 2, 1), edge(2, 0, 2)]
    }

    /// Derives one midpoint anchor (spec §2.1): geometric midpoint,
    /// endpoint mean plus the conditioned increment, and the canonical
    /// sorted-seed hash. Every part is commutative in (a, b), so both
    /// sides of a shared edge derive bit-identical anchors.
    fn midpoint_anchor(
        &self,
        a: &Anchor,
        b: &Anchor,
        conditions: Conditions,
        level: u32,
    ) -> Anchor {
        let seed = self.midpoint_seed(&a.seed, &b.seed);
        Anchor {
            position: geometric_midpoint(a.position, b.position),
            value_m: 0.5 * (a.value_m + b.value_m)
                + conditions.amplitude_m(level) * signed_unit_noise(&seed),
            seed,
        }
    }

    /// The published face value: the corner mean, river-suppressed at the
    /// face centroid along the level's rerouted channel path (spec §4 and
    /// §10 amendment A6; L1+ only — the L0 identity has priority),
    /// clamped into the authoritative elevation bounds.
    fn face_value(
        &self,
        cell_index: usize,
        frame: &TriangleFrame,
        leaf_level: u8,
    ) -> PrimitiveValue {
        let [a, b, c] = &frame.corners;
        let mut elevation = (a.value_m + b.value_m + c.value_m) / 3.0;
        if self.amplifier.has_rivers() {
            let [ax, ay, az] = a.position.components();
            let [bx, by, bz] = b.position.components();
            let [cx, cy, cz] = c.position.components();
            let centroid = UnitVector3::new(ax + bx + cx, ay + by + cy, az + bz + cz)
                .expect("a fan triangle centroid stays strictly inside one hemisphere");
            let relief = f64::from(self.amplifier.conditioning().local_relief_norm[cell_index]);
            if let Some(carve) =
                super::hierarchical_rivers::carve_elevation_m(self, centroid, leaf_level, relief)
            {
                elevation = elevation.min(carve.max(f64::from(ELEVATION_MIN_M)));
            }
        }
        let elevation =
            elevation.clamp(f64::from(ELEVATION_MIN_M), f64::from(ELEVATION_MAX_M)) as f32;
        PrimitiveValue {
            elevation_m: elevation,
            regime: self.regime_for(f64::from(elevation)),
        }
    }

    fn regime_for(&self, elevation_m: f64) -> SurfaceRegime {
        regime_for_depth(elevation_m - self.amplifier.conditioning().sea_level_m)
    }

    pub(super) fn seed_hasher(&self) -> Hasher {
        let mut hasher = Hasher::new();
        hasher.update(DERIVATION_DOMAIN);
        hasher.update(&self.root_seed_raw.to_le_bytes());
        hasher
    }

    /// Canonical seed of an L0 lattice anchor: `blake3(domain ∥ tag ∥ id)`
    /// with tag `"c"` for centroids and `"v"` for boundary vertices.
    fn anchor_seed(&self, tag: &[u8; 1], id: u32) -> [u8; 32] {
        let mut hasher = self.seed_hasher();
        hasher.update(tag);
        hasher.update(&id.to_le_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Canonical midpoint seed: `blake3(domain ∥ "m" ∥ sort(seed_a,
    /// seed_b))` — byte-sorting makes both derivation sides identical by
    /// construction (spec §2.1).
    fn midpoint_seed(&self, a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        let mut hasher = self.seed_hasher();
        hasher.update(b"m");
        hasher.update(low);
        hasher.update(high);
        *hasher.finalize().as_bytes()
    }

    /// The authoritative containing cell: nearest site, found by a greedy
    /// walk over the Voronoi adjacency (the site Delaunay graph, on which
    /// nearest-site greedy always terminates at the true cell) from the
    /// locator's dual-triangle corners.
    fn locate_cell_index(&self, direction: UnitVector3) -> u32 {
        let corners = self.amplifier.locate_corner_cells(direction);
        let score = |cell: u32| self.cell_site[cell as usize].dot(direction);
        let mut best = corners[0];
        let mut best_score = score(best);
        for &candidate in &corners[1..] {
            let candidate_score = score(candidate);
            if candidate_score > best_score {
                best = candidate;
                best_score = candidate_score;
            }
        }
        loop {
            let (start, len) = self.ring_bounds(best);
            let mut improved = false;
            for &neighbor in &self.ring_neighbors[start..start + len] {
                let candidate_score = score(neighbor);
                if candidate_score > best_score {
                    best = neighbor;
                    best_score = candidate_score;
                    improved = true;
                }
            }
            if !improved {
                return best;
            }
        }
    }

    /// The fan sector containing `direction`, by largest interior margin.
    fn locate_sector(&self, cell_index: u32, direction: UnitVector3) -> usize {
        let (start, len) = self.ring_bounds(cell_index);
        let centroid = self.cell_centroid[cell_index as usize];
        let mut best = 0;
        let mut best_margin = f64::NEG_INFINITY;
        for sector in 0..len {
            let near = self.vertex_position[self.ring_vertices[start + sector] as usize];
            let far = self.vertex_position[self.ring_vertices[start + (sector + 1) % len] as usize];
            let margin = interior_margin(&[centroid, near, far], direction);
            if margin > best_margin {
                best_margin = margin;
                best = sector;
            }
        }
        best
    }

    /// The number of L0 cells in the evaluated surface.
    pub fn cell_count(&self) -> usize {
        self.cell_conditions.len()
    }

    /// The number of fan sectors of one cell (6, or 5 on pentagons).
    pub fn sector_count(&self, cell: CellId) -> usize {
        self.ring_bounds(cell.raw()).1
    }

    /// The mean cell spacing in metres — the L0 primitive scale that the
    /// spec §5 ladder halves per level.
    pub fn cell_spacing_m(&self) -> f64 {
        self.amplifier.base_wavelength_m() * 0.5
    }

    /// The corner directions of one L1 sector triangle, in the frozen
    /// (centroid, near vertex, far vertex) walk order.
    pub fn sector_corners(&self, cell: CellId, sector: u8) -> [UnitVector3; 3] {
        let (start, len) = self.ring_bounds(cell.raw());
        assert!(
            usize::from(sector) < len,
            "sector {sector} is outside the {len}-sector fan of cell {}",
            cell.raw()
        );
        [
            self.cell_centroid[cell.raw() as usize],
            self.vertex_position[self.ring_vertices[start + usize::from(sector)] as usize],
            self.vertex_position
                [self.ring_vertices[start + (usize::from(sector) + 1) % len] as usize],
        ]
    }

    /// Streams the face values of every leaf `extra_levels` below the
    /// primitive `(cell, sector, prefix)` in depth-first child order
    /// 0, 1, 2, 3 — the same traversal order as the display subdivision,
    /// so callers pair leaves with geometry by index. Each leaf value is
    /// bit-identical to `value()` of its full path; the shared walk just
    /// amortizes the upper anchors instead of re-deriving them per leaf.
    pub fn for_each_leaf_value(
        &self,
        cell: CellId,
        sector: u8,
        prefix: &[u8],
        extra_levels: u8,
        sink: &mut dyn FnMut(PrimitiveValue),
    ) {
        assert!(
            prefix.len() + usize::from(extra_levels) <= HIERARCHICAL_PATH_DEPTH_MAX,
            "leaf depth {} exceeds the spec cap {}",
            prefix.len() + usize::from(extra_levels),
            HIERARCHICAL_PATH_DEPTH_MAX
        );
        let conditions = self.sector_conditions(cell.raw(), sector);
        let mut frame = self.sector_frame(cell.raw(), sector);
        for (step, &child) in prefix.iter().enumerate() {
            assert!(child <= 3, "path step {child} is not a 2-bit child index");
            let level = step as u32 + 1;
            let midpoints = self.split_midpoints(&frame, level, &conditions);
            frame = child_frame(frame, midpoints, child);
        }
        self.emit_leaf_values(
            cell.raw() as usize,
            frame,
            prefix.len() as u32,
            extra_levels,
            &conditions,
            sink,
        );
    }

    fn emit_leaf_values(
        &self,
        cell_index: usize,
        frame: TriangleFrame,
        depth: u32,
        remaining: u8,
        conditions: &SectorConditions,
        sink: &mut dyn FnMut(PrimitiveValue),
    ) {
        if remaining == 0 {
            sink(self.face_value(cell_index, &frame, 1 + depth as u8));
            return;
        }
        let midpoints = self.split_midpoints(&frame, depth + 1, conditions);
        for child in 0..4u8 {
            self.emit_leaf_values(
                cell_index,
                child_frame(frame, midpoints, child),
                depth + 1,
                remaining - 1,
                conditions,
                sink,
            );
        }
    }
}

/// The v2 remapping of the M1 C-table onto per-cell (A0, H) parameters
/// (spec §3). Effects reuse the amplifier's normalized drivers and the
/// M1 C-table laws as the single fact sources; the cell's own T0 regime
/// selects the amplitude family.
fn cell_conditions(view: &ConditioningView<'_>, index: usize) -> Conditions {
    let relief = f64::from(view.local_relief_norm[index]);
    let relief_m = f64::from(view.local_relief_m[index]);
    let orogeny = f64::from(view.orogeny_factor[index]);
    let erodibility = f64::from(view.erodibility_norm[index]);
    let damping = sediment_damping(f64::from(view.sediment_norm[index]));
    let dissection = langbein_schumm(f64::from(view.precipitation_mm[index]));
    let regime = regime_for_depth(f64::from(view.elevation_m[index]) - view.sea_level_m);
    // C10 baseline: the M1 roughness-blended Hurst exponent.
    let baseline_hurst = surface_roughness_hurst(relief.max(orogeny));
    let (a0_m, hurst) = match regime {
        SurfaceRegime::LandInterior | SurfaceRegime::CoastalBand => {
            // C1 v3 spectral continuation (amendment A7): A0 in metres
            // from the measured T0 neighbour relief, so a 3 km orogen
            // and a 300 m hill land scale apart instead of saturating
            // one normalized knob; orogeny no longer multiplies the
            // amplitude (the relief already carries it) and stays a
            // Hurst driver only. C4 amplitude channel, C6 joint
            // badlands peak, and C7 sediment damping modify around it.
            let a0 = (SPECTRAL_CONTINUATION * relief_m).max(LAND_A0_FLOOR_M)
                * erodibility_amplitude(erodibility)
                * damping
                * (1.0 + BADLANDS_A0_SHARE * badlands_gate(erodibility, dissection));
            let hurst = if matches!(regime, SurfaceRegime::CoastalBand) {
                // C9: the fractal-coastline anchor D = 2 − H
                // (spec §9.2), rugged coasts rough and low-H.
                HURST_COAST_SMOOTH + (HURST_COAST_RUGGED - HURST_COAST_SMOOTH) * relief
            } else {
                // C4 and C5 Hurst channels on the land interior.
                baseline_hurst
                    - HURST_ERODIBILITY_DELTA * erodibility
                    - HURST_DISSECTION_DELTA * dissection
            };
            (a0, hurst)
        }
        // C7 keeps shelf plateaus and buried plains smooth.
        SurfaceRegime::ContinentalShelf => (SHELF_BASE_AMPLITUDE_M * damping, baseline_hurst),
        // C8: the measured abyssal-hill envelope scaled by the
        // spreading-rate proxy, damped by C7.
        SurfaceRegime::OceanFloor => (
            (OCEAN_HILL_AMPLITUDE_MIN_M
                + (OCEAN_HILL_AMPLITUDE_MAX_M - OCEAN_HILL_AMPLITUDE_MIN_M)
                    * f64::from(view.age_gradient_norm[index]))
                * damping,
            baseline_hurst,
        ),
    };
    Conditions {
        a0_m,
        hurst: hurst.clamp(HURST_MIN, HURST_MAX),
    }
}

/// The discrete v2 regime classification (spec §2.2): the M1 §4 boundary
/// constants applied to elevation against sea level, without blending.
fn regime_for_depth(depth_m: f64) -> SurfaceRegime {
    if depth_m.abs() <= SHELF_TRANSITION_M {
        SurfaceRegime::CoastalBand
    } else if depth_m > 0.0 {
        SurfaceRegime::LandInterior
    } else if depth_m >= -FORMATION_SHELF_BREAK_DEPTH_M {
        SurfaceRegime::ContinentalShelf
    } else {
        SurfaceRegime::OceanFloor
    }
}

/// The child frame of one four-way split, in the frozen display order:
/// 0 = (A, AB, CA), 1 = (AB, B, BC), 2 = (CA, BC, C), 3 = (AB, BC, CA).
/// Boundary flags follow the surviving halves of the parent edges.
fn child_frame(frame: TriangleFrame, midpoints: [Anchor; 3], child: u8) -> TriangleFrame {
    let [a, b, c] = frame.corners;
    let [ab, bc, ca] = midpoints;
    let edges = frame.boundary_edge;
    match child {
        0 => TriangleFrame {
            corners: [a, ab, ca],
            boundary_edge: [edges[0], false, edges[2]],
        },
        1 => TriangleFrame {
            corners: [ab, b, bc],
            boundary_edge: [edges[0], edges[1], false],
        },
        2 => TriangleFrame {
            corners: [ca, bc, c],
            boundary_edge: [false, edges[1], edges[2]],
        },
        _ => TriangleFrame {
            corners: [ab, bc, ca],
            boundary_edge: [false; 3],
        },
    }
}

/// The renormalized-sum midpoint — the same commutative expression as the
/// display subdivision, so geometry and values walk one tree.
fn geometric_midpoint(a: UnitVector3, b: UnitVector3) -> UnitVector3 {
    let [ax, ay, az] = a.components();
    let [bx, by, bz] = b.components();
    UnitVector3::new(ax + bx, ay + by, az + bz)
        .expect("fan subdivision midpoints stay strictly inside one hemisphere")
}

/// N(seed) ∈ [−1, 1] (spec §2.1): the first eight little-endian seed
/// bytes as u64, mapped through `u64 / 2^63 − 1`.
pub(super) fn signed_unit_noise(seed: &[u8; 32]) -> f64 {
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&seed[..8]);
    u64::from_le_bytes(bytes) as f64 / (1_u64 << 63) as f64 - 1.0
}

/// The smallest signed side margin of `direction` against the CCW
/// spherical triangle: positive strictly inside, negative outside.
fn interior_margin(corners: &[UnitVector3; 3], direction: UnitVector3) -> f64 {
    side(corners[0], corners[1], direction)
        .min(side(corners[1], corners[2], direction))
        .min(side(corners[2], corners[0], direction))
}

/// The signed volume `p · (a × b)`: positive when `p` is on the interior
/// side of the directed great-circle edge a → b of a CCW triangle.
fn side(a: UnitVector3, b: UnitVector3, p: UnitVector3) -> f64 {
    let [ax, ay, az] = a.components();
    let [bx, by, bz] = b.components();
    let [px, py, pz] = p.components();
    px * (ay * bz - az * by) + py * (az * bx - ax * bz) + pz * (ax * by - ay * bx)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::generators::natural::fibonacci_probe;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{RiverSegmentKind, SphericalOrogenyKind};
    use crate::world::{Meters, RiverSegmentId, SphericalSpaceSpec};

    fn test_surface() -> SphericalSurfaceSnapshot {
        surface_with(162)
    }

    fn surface_with(target_cell_count: u32) -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build_cancellable(
            &SphericalSpaceSpec {
                radius: Meters::new(6_371_000.0).unwrap(),
                target_cell_count,
            },
            || false,
        )
        .unwrap()
    }

    /// Northern sloped land, flat southern abyssal plain, and an
    /// east/west erodibility split so adjacent cells carry different
    /// conditions (the cross-boundary invariant must not pass trivially).
    struct SyntheticFields {
        elevation: Vec<f32>,
        sediment: Vec<f32>,
        erodibility: Vec<f32>,
        precipitation: Vec<f32>,
        crust_age: Vec<f32>,
        lineation_east: Vec<f32>,
        lineation_north: Vec<f32>,
        orogeny_kind: Vec<SphericalOrogenyKind>,
        orogeny_age: Vec<f32>,
    }

    impl SyntheticFields {
        fn new(surface: &SphericalSurfaceSnapshot) -> Self {
            let cells = surface.cells();
            let mut fields = Self {
                elevation: Vec::new(),
                sediment: Vec::new(),
                erodibility: Vec::new(),
                precipitation: Vec::new(),
                crust_age: Vec::new(),
                lineation_east: Vec::new(),
                lineation_north: Vec::new(),
                orogeny_kind: Vec::new(),
                orogeny_age: Vec::new(),
            };
            for cell in cells {
                let [x, y, z] = cell.centroid.components();
                fields.elevation.push(if z >= 0.0 {
                    (2_500.0 * z - 500.0) as f32
                } else {
                    -1_800.0
                });
                fields.sediment.push(0.0);
                fields.erodibility.push(if x > 0.0 { 0.9 } else { 0.2 });
                fields.precipitation.push(800.0);
                fields.crust_age.push((y.abs() * 100.0) as f32);
                fields.lineation_east.push(1.0);
                fields.lineation_north.push(0.0);
                fields.orogeny_kind.push(SphericalOrogenyKind::None);
                fields.orogeny_age.push(0.0);
            }
            fields
        }

        fn view(&self) -> AmplificationFieldsView<'_> {
            AmplificationFieldsView {
                final_elevation_m: &self.elevation,
                sea_level_m: 0.0,
                sediment_thickness_m: &self.sediment,
                erodibility: &self.erodibility,
                annual_precipitation_mm: &self.precipitation,
                crust_age_myr: &self.crust_age,
                lineation_east: &self.lineation_east,
                lineation_north: &self.lineation_north,
                orogeny_kind: &self.orogeny_kind,
                orogeny_age_myr: &self.orogeny_age,
            }
        }
    }

    fn evaluator() -> (HierarchicalEvaluator, SphericalSurfaceSnapshot) {
        let surface = test_surface();
        let fields = SyntheticFields::new(&surface);
        let evaluator =
            HierarchicalEvaluator::new(&surface, fields.view(), RootSeed::new(42)).unwrap();
        (evaluator, surface)
    }

    /// Spec §7 invariant 4: every L0 cell face value is bit-identical to
    /// the published T0 elevation, labelled by the discrete regime table.
    #[test]
    fn cell_values_reproduce_t0_bit_exactly() {
        let (evaluator, surface) = evaluator();
        let fields = SyntheticFields::new(&surface);
        for (index, &expected) in fields.elevation.iter().enumerate() {
            let value = evaluator.cell_value(CellId::from_raw(index as u32));
            assert_eq!(value.elevation_m.to_bits(), expected.to_bits());
            let depth = f64::from(expected);
            let regime = value.regime;
            if depth.abs() <= SHELF_TRANSITION_M {
                assert_eq!(regime, SurfaceRegime::CoastalBand);
            } else if depth > 0.0 {
                assert_eq!(regime, SurfaceRegime::LandInterior);
            } else if depth >= -FORMATION_SHELF_BREAK_DEPTH_M {
                assert_eq!(regime, SurfaceRegime::ContinentalShelf);
            } else {
                assert_eq!(regime, SurfaceRegime::OceanFloor);
            }
        }
    }

    /// Spec §7 invariant 1: identical IDs evaluate bit-identically across
    /// independent builds and across threads.
    #[test]
    fn evaluation_is_deterministic_across_builds_and_threads() {
        let (first, _surface) = evaluator();
        let (second, _surface_two) = evaluator();
        assert_eq!(first.probe_fingerprint(), second.probe_fingerprint());

        let sequential: Vec<u32> = (0..24)
            .map(|index| {
                first
                    .sample(fibonacci_probe(index, 24), 6)
                    .elevation_m
                    .to_bits()
            })
            .collect();
        let threaded: Vec<u32> = std::thread::scope(|scope| {
            let low = scope.spawn(|| {
                (0..12)
                    .map(|index| {
                        first
                            .sample(fibonacci_probe(index, 24), 6)
                            .elevation_m
                            .to_bits()
                    })
                    .collect::<Vec<_>>()
            });
            let high = scope.spawn(|| {
                (12..24)
                    .map(|index| {
                        first
                            .sample(fibonacci_probe(index, 24), 6)
                            .elevation_m
                            .to_bits()
                    })
                    .collect::<Vec<_>>()
            });
            let mut all = low.join().unwrap();
            all.extend(high.join().unwrap());
            all
        });
        assert_eq!(sequential, threaded);
    }

    /// Spec §7 invariant 2: both cells sharing an L0 boundary edge derive
    /// the shared-edge midpoints — seed, value, and position — byte for
    /// byte identically, two levels deep, and adjacent sectors of one
    /// cell agree on their shared spoke midpoint.
    #[test]
    fn shared_edge_midpoints_agree_across_cells_and_sectors() {
        let (evaluator, surface) = evaluator();
        // Pick an edge whose two cells carry different conditions so the
        // two-cell mean rule is actually load-bearing here.
        let edge = surface
            .edges()
            .iter()
            .find(|edge| {
                let a = evaluator.cell_conditions[edge.cells[0].raw() as usize];
                let b = evaluator.cell_conditions[edge.cells[1].raw() as usize];
                (a.a0_m - b.a0_m).abs() > 1.0
            })
            .expect("the erodibility split yields condition-contrasting edges");
        let [p, q] = edge.cells;
        let sector_of = |cell: CellId| -> u8 {
            surface
                .cell(cell)
                .unwrap()
                .boundary_edges
                .iter()
                .position(|&candidate| candidate == edge.id)
                .unwrap() as u8
        };
        let (sector_p, sector_q) = (sector_of(p), sector_of(q));
        // The two rings traverse the shared edge in opposite directions.
        let frame_p = evaluator.sector_frame(p.raw(), sector_p);
        let frame_q = evaluator.sector_frame(q.raw(), sector_q);
        assert_eq!(frame_p.corners[1].seed, frame_q.corners[2].seed);
        assert_eq!(frame_p.corners[2].seed, frame_q.corners[1].seed);

        let conditions_p = evaluator.sector_conditions(p.raw(), sector_p);
        let conditions_q = evaluator.sector_conditions(q.raw(), sector_q);
        let mids_p = evaluator.split_midpoints(&frame_p, 1, &conditions_p);
        let mids_q = evaluator.split_midpoints(&frame_q, 1, &conditions_q);
        let assert_anchor_eq = |a: &Anchor, b: &Anchor| {
            assert_eq!(a.seed, b.seed);
            assert_eq!(a.value_m.to_bits(), b.value_m.to_bits());
            assert_eq!(
                a.position.components().map(f64::to_bits),
                b.position.components().map(f64::to_bits)
            );
        };
        assert_anchor_eq(&mids_p[1], &mids_q[1]);

        // One level deeper along the shared chain: P's child 1 keeps the
        // (near-vertex, midpoint) half, Q reaches the same half via
        // child 2 from its reversed corner order.
        let child_p = child_frame(frame_p, mids_p, 1);
        let child_q = child_frame(frame_q, mids_q, 2);
        let deeper_p = evaluator.split_midpoints(&child_p, 2, &conditions_p);
        let deeper_q = evaluator.split_midpoints(&child_q, 2, &conditions_q);
        assert_anchor_eq(&deeper_p[1], &deeper_q[1]);

        // Adjacent sectors of one cell derive the shared spoke midpoint
        // identically (frame edge CA of sector s is edge AB of s + 1).
        let (_, len) = evaluator.ring_bounds(p.raw());
        let next_sector = (usize::from(sector_p) + 1) % len;
        let frame_next = evaluator.sector_frame(p.raw(), next_sector as u8);
        let conditions_next = evaluator.sector_conditions(p.raw(), next_sector as u8);
        let mids_next = evaluator.split_midpoints(&frame_next, 1, &conditions_next);
        assert_anchor_eq(&mids_p[2], &mids_next[0]);
    }

    /// Spec §7 invariant 3: every midpoint offset stays inside the
    /// conditioned amplitude bound, and the bound itself decays strictly
    /// with level (`p = 2^(−H) < 1`).
    #[test]
    fn midpoint_offsets_respect_the_decaying_amplitude_bound() {
        let (evaluator, _surface) = evaluator();
        let paths: [[u8; 6]; 4] = [
            [0, 1, 2, 3, 0, 1],
            [3, 3, 3, 3, 3, 3],
            [1, 0, 2, 1, 0, 2],
            [2, 2, 1, 1, 0, 0],
        ];
        let cell_count = evaluator.cell_conditions.len();
        for cell_index in (0..cell_count).step_by(17) {
            let (_, len) = evaluator.ring_bounds(cell_index as u32);
            for sector in 0..len as u8 {
                for path in &paths {
                    let conditions = evaluator.sector_conditions(cell_index as u32, sector);
                    let mut frame = evaluator.sector_frame(cell_index as u32, sector);
                    for (step, &child) in path.iter().enumerate() {
                        let level = step as u32 + 1;
                        let midpoints = evaluator.split_midpoints(&frame, level, &conditions);
                        for (index, (a, b)) in [(0, 1), (1, 2), (2, 0)].into_iter().enumerate() {
                            let bound = conditions
                                .pick(frame.boundary_edge[index])
                                .amplitude_m(level);
                            let offset = midpoints[index].value_m
                                - 0.5 * (frame.corners[a].value_m + frame.corners[b].value_m);
                            assert!(
                                offset.abs() <= bound + 1.0e-9,
                                "offset {offset} exceeds bound {bound} at level {level}"
                            );
                        }
                        frame = child_frame(frame, midpoints, child);
                    }
                }
            }
        }
        for conditions in &evaluator.cell_conditions {
            if conditions.a0_m > 0.0 {
                for level in 1..12 {
                    assert!(conditions.amplitude_m(level + 1) < conditions.amplitude_m(level));
                }
            }
        }
    }

    /// Spec §7 invariant 5 (the M1 criteria at primitive level): carving
    /// only ever lowers face values, and the carve surface itself
    /// descends monotonically along a river chain.
    #[test]
    fn river_carving_only_lowers_and_stays_monotone() {
        let surface = test_surface();
        let fields = SyntheticFields::new(&surface);
        let plain = HierarchicalEvaluator::new(&surface, fields.view(), RootSeed::new(9)).unwrap();
        let edge = &surface.edges()[0];
        let (a, b) = (edge.cells[0], edge.cells[1]);
        let next = surface
            .edges()
            .iter()
            .find(|candidate| candidate.cells.contains(&b) && !candidate.cells.contains(&a))
            .unwrap();
        let c = if next.cells[0] == b {
            next.cells[1]
        } else {
            next.cells[0]
        };
        let segments = vec![
            RiverSegment::new(
                RiverSegmentId::from_raw(0),
                a,
                b,
                RiverSegmentKind::Channel,
                1,
                120.0,
            )
            .unwrap(),
            RiverSegment::new(
                RiverSegmentId::from_raw(1),
                b,
                c,
                RiverSegmentKind::Channel,
                2,
                260.0,
            )
            .unwrap(),
        ];
        let carved = HierarchicalEvaluator::new(&surface, fields.view(), RootSeed::new(9))
            .unwrap()
            .with_rivers(&surface, &segments)
            .unwrap();

        // The carve surface descends along the chain (M1 module criteria
        // reused through the shared carve implementation).
        let chain = [a, b, c].map(|cell| surface.cells()[cell.raw() as usize].centroid);
        let mut previous = f64::INFINITY;
        for leg in 0..2 {
            let from = chain[leg].components();
            let to = chain[leg + 1].components();
            for step in 0..=24 {
                let t = f64::from(step) / 24.0;
                let direction = UnitVector3::new(
                    from[0] + t * (to[0] - from[0]),
                    from[1] + t * (to[1] - from[1]),
                    from[2] + t * (to[2] - from[2]),
                )
                .unwrap();
                let carve = carved.amplifier.river_carve_m(direction, 0.0).unwrap();
                assert!(carve <= previous + 1e-6, "carve rose: {carve} > {previous}");
                previous = carve;
            }
        }

        // Suppression is min-only at every primitive: the carved world is
        // nowhere above the plain one (identical seeds, so any difference
        // is exactly the carve).
        for index in 0..512 {
            let probe = fibonacci_probe(index, 512);
            for level in [1_u8, 3, 5] {
                let with_rivers = carved.sample(probe, level).elevation_m;
                let without = plain.sample(probe, level).elevation_m;
                assert!(with_rivers <= without + 1e-3);
            }
        }
    }

    /// Spec §7 invariant 6: the deep-level land fraction stays within one
    /// percentage point of the L0 land fraction over a large sample.
    ///
    /// Runs on a finer fixture than the other tests: at 162 cells the L0
    /// stair coastline itself is quantized by whole percentage points, so
    /// the statistic would measure fixture coarseness, not derivation
    /// drift. The real product tier is gated in the integration suite.
    #[test]
    fn deep_land_fraction_matches_l0() {
        let surface = surface_with(642);
        let fields = SyntheticFields::new(&surface);
        let evaluator =
            HierarchicalEvaluator::new(&surface, fields.view(), RootSeed::new(42)).unwrap();
        let total = 16_384_usize;
        let mut l0_land = 0_u32;
        let mut deep_land = 0_u32;
        for index in 0..total {
            let probe = fibonacci_probe(index, total);
            if evaluator.sample(probe, 0).elevation_m >= 0.0 {
                l0_land += 1;
            }
            if evaluator.sample(probe, 6).elevation_m >= 0.0 {
                deep_land += 1;
            }
        }
        let drift = (f64::from(l0_land) - f64::from(deep_land)).abs() / total as f64;
        assert!(drift <= 0.01, "land fraction drift {drift}");
    }

    /// Spec §7 invariant 7: the thin shell returns exactly the value of
    /// the primitive it locates, the located cell is the authoritative
    /// nearest site, and located triangles geometrically contain the
    /// direction.
    #[test]
    fn sample_returns_the_located_primitive_value() {
        let (evaluator, surface) = evaluator();
        for index in 0..512 {
            let direction = fibonacci_probe(index, 512);
            let brute = surface
                .cells()
                .iter()
                .max_by(|a, b| a.site.dot(direction).total_cmp(&b.site.dot(direction)))
                .unwrap()
                .id;
            let LocatedPrimitive::Cell(located_cell) = evaluator.locate(direction, 0) else {
                panic!("level 0 must locate a cell primitive");
            };
            assert_eq!(located_cell, brute);

            for level in [0_u8, 1, 2, 4, 7] {
                let located = evaluator.locate(direction, level);
                let shell = evaluator.sample(direction, level);
                let direct = match located {
                    LocatedPrimitive::Cell(cell) => evaluator.cell_value(cell),
                    LocatedPrimitive::Triangle { cell, sector, path } => {
                        assert_eq!(cell, brute);
                        evaluator.value(cell, sector, path.steps())
                    }
                };
                assert_eq!(shell, direct);
            }

            let LocatedPrimitive::Triangle { cell, sector, .. } = evaluator.locate(direction, 1)
            else {
                panic!("level 1 must locate a triangle primitive");
            };
            let (start, len) = evaluator.ring_bounds(cell.raw());
            let near = evaluator.vertex_position
                [evaluator.ring_vertices[start + usize::from(sector)] as usize];
            let far = evaluator.vertex_position
                [evaluator.ring_vertices[start + (usize::from(sector) + 1) % len] as usize];
            let corners = [evaluator.cell_centroid[cell.raw() as usize], near, far];
            assert!(
                interior_margin(&corners, direction) >= -1.0e-9,
                "located sector does not contain the direction"
            );
        }
    }

    /// The streaming leaf walk yields exactly `value()` of every leaf in
    /// depth-first child order 0..4 — the display pairing contract.
    #[test]
    fn leaf_stream_matches_per_leaf_values_in_dfs_order() {
        let (evaluator, _surface) = evaluator();
        let prefix = [1_u8, 3];
        let extra = 3_u8;
        let mut streamed = Vec::new();
        evaluator.for_each_leaf_value(CellId::from_raw(7), 2, &prefix, extra, &mut |value| {
            streamed.push(value)
        });
        assert_eq!(streamed.len(), 4_usize.pow(u32::from(extra)));

        let mut suffixes = Vec::new();
        let mut suffix = Vec::new();
        enumerate_dfs(extra, &mut suffix, &mut suffixes);
        assert_eq!(suffixes.len(), streamed.len());
        for (index, tail) in suffixes.iter().enumerate() {
            let mut path = prefix.to_vec();
            path.extend_from_slice(tail);
            let direct = evaluator.value(CellId::from_raw(7), 2, &path);
            assert_eq!(
                streamed[index].elevation_m.to_bits(),
                direct.elevation_m.to_bits()
            );
            assert_eq!(streamed[index].regime, direct.regime);
        }
    }

    fn enumerate_dfs(remaining: u8, current: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        if remaining == 0 {
            out.push(current.clone());
            return;
        }
        for child in 0..4u8 {
            current.push(child);
            enumerate_dfs(remaining - 1, current, out);
            current.pop();
        }
    }

    /// Spec §6 probe-set structure: 256 IDs, the frozen depth blocks,
    /// 64 evenly spread cells, and full sector coverage on hexagons.
    #[test]
    fn probe_set_covers_cells_sectors_and_depths() {
        let (evaluator, _surface) = evaluator();
        let probes = evaluator.probe_ids();
        assert_eq!(probes.len(), HIERARCHICAL_PROBE_COUNT);
        let mut cells = HashSet::new();
        let mut hexagon_sectors = HashSet::new();
        for (index, probe) in probes.iter().enumerate() {
            let expected_depth = PROBE_PATH_DEPTHS[index / PROBE_CELL_BLOCK];
            assert_eq!(probe.path.steps().len(), expected_depth);
            assert!(probe.path.steps().iter().all(|&step| step <= 3));
            let (_, len) = evaluator.ring_bounds(probe.cell.raw());
            assert!(usize::from(probe.sector) < len);
            cells.insert(probe.cell);
            if len == 6 {
                hexagon_sectors.insert(probe.sector);
            }
        }
        assert_eq!(cells.len(), PROBE_CELL_BLOCK);
        assert_eq!(hexagon_sectors.len(), 6);
    }
}
