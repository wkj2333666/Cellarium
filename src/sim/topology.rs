use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basis2 {
    pub first: [f32; 2],
    pub second: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteSpec {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NeighborTemplate {
    pub source_site: usize,
    pub target_site: usize,
    pub cell_offset: [i32; 2],
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LatticeSpec {
    pub basis: Basis2,
    pub sites: Vec<SiteSpec>,
    pub neighborhoods: Vec<NeighborTemplate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainSpec {
    Rect { size: [u32; 2] },
    Mask { size: [u32; 2], active: Vec<bool> },
    Sparse { cells: Vec<[i32; 2]> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardSpec {
    pub domain: DomainSpec,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BoundarySpec {
    Open,
    Constant(f32),
    Periodic,
    Clamp,
    Reflect,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TopologyError {
    #[error("lattice must define at least one site")]
    EmptySites,
    #[error("lattice site name `{0}` is duplicated or empty")]
    InvalidSiteName(String),
    #[error("lattice basis contains a non-finite value")]
    NonFiniteBasis,
    #[error("neighbor template refers to an invalid site")]
    InvalidSiteReference,
    #[error("neighbor template weight must be finite")]
    NonFiniteWeight,
    #[error("rectangular board dimensions must be positive")]
    InvalidBoardSize,
    #[error("board mask length must equal width * height")]
    InvalidMaskLength,
    #[error("sparse board contains duplicate cells")]
    DuplicateSparseCell,
    #[error("boundary constant must be finite")]
    NonFiniteBoundaryConstant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledTopology {
    pub basis: Basis2,
    pub site_names: Vec<String>,
    pub site_cells: Vec<[i32; 2]>,
    pub offsets: Vec<u32>,
    pub neighbors: Vec<u32>,
    pub weights: Vec<f32>,
    pub boundary_constant: Option<f32>,
}

impl CompiledTopology {
    pub fn site_count(&self) -> usize {
        self.site_cells.len()
    }

    pub fn neighbors_of(&self, site_id: u32) -> &[u32] {
        let start = self.offsets[site_id as usize] as usize;
        let end = self.offsets[site_id as usize + 1] as usize;
        &self.neighbors[start..end]
    }

    pub fn weights_of(&self, site_id: u32) -> &[f32] {
        let start = self.offsets[site_id as usize] as usize;
        let end = self.offsets[site_id as usize + 1] as usize;
        &self.weights[start..end]
    }
}

pub fn compile_topology(
    lattice: &LatticeSpec,
    board: &BoardSpec,
    boundary: &BoundarySpec,
) -> Result<CompiledTopology, TopologyError> {
    validate_lattice(lattice)?;
    let domain = Domain::from_spec(&board.domain)?;
    let boundary_constant = match boundary {
        BoundarySpec::Constant(value) if !value.is_finite() => {
            return Err(TopologyError::NonFiniteBoundaryConstant);
        }
        BoundarySpec::Constant(value) => Some(*value),
        _ => None,
    };

    let mut site_cells = Vec::new();
    let mut ids = HashMap::new();
    for cell in &domain.cells {
        for site in 0..lattice.sites.len() {
            let id = site_cells.len() as u32;
            ids.insert((*cell, site), id);
            site_cells.push(*cell);
        }
    }

    let mut adjacency = vec![Vec::<(u32, f32)>::new(); site_cells.len()];
    for (source_id, cell) in site_cells.iter().enumerate() {
        let source_site = source_id % lattice.sites.len();
        for template in lattice
            .neighborhoods
            .iter()
            .filter(|template| template.source_site == source_site)
        {
            let requested = [
                cell[0] + template.cell_offset[0],
                cell[1] + template.cell_offset[1],
            ];
            if let Some(target_cell) = domain.map_boundary(requested, boundary)
                && let Some(target_id) = ids.get(&(target_cell, template.target_site))
            {
                adjacency[source_id].push((*target_id, template.weight));
            }
        }
    }

    let mut offsets = Vec::with_capacity(adjacency.len() + 1);
    let mut neighbors = Vec::new();
    let mut weights = Vec::new();
    offsets.push(0);
    for entries in adjacency {
        neighbors.extend(entries.iter().map(|(neighbor, _)| *neighbor));
        weights.extend(entries.iter().map(|(_, weight)| *weight));
        offsets.push(neighbors.len() as u32);
    }

    Ok(CompiledTopology {
        basis: lattice.basis,
        site_names: lattice.sites.iter().map(|site| site.name.clone()).collect(),
        site_cells,
        offsets,
        neighbors,
        weights,
        boundary_constant,
    })
}

fn validate_lattice(lattice: &LatticeSpec) -> Result<(), TopologyError> {
    if lattice.sites.is_empty() {
        return Err(TopologyError::EmptySites);
    }
    if lattice
        .basis
        .first
        .iter()
        .chain(lattice.basis.second.iter())
        .any(|value| !value.is_finite())
    {
        return Err(TopologyError::NonFiniteBasis);
    }
    let mut names = HashMap::new();
    for site in &lattice.sites {
        if site.name.is_empty() || names.insert(&site.name, ()).is_some() {
            return Err(TopologyError::InvalidSiteName(site.name.clone()));
        }
    }
    for template in &lattice.neighborhoods {
        if template.source_site >= lattice.sites.len()
            || template.target_site >= lattice.sites.len()
        {
            return Err(TopologyError::InvalidSiteReference);
        }
        if !template.weight.is_finite() {
            return Err(TopologyError::NonFiniteWeight);
        }
    }
    Ok(())
}

struct Domain {
    cells: Vec<[i32; 2]>,
    bounds: Option<([i32; 2], [i32; 2])>,
}

impl Domain {
    fn from_spec(spec: &DomainSpec) -> Result<Self, TopologyError> {
        match spec {
            DomainSpec::Rect { size } => {
                validate_size(*size)?;
                let cells = (0..size[1] as i32)
                    .flat_map(|y| (0..size[0] as i32).map(move |x| [x, y]))
                    .collect::<Vec<_>>();
                Ok(Self {
                    cells,
                    bounds: Some(([0, 0], [size[0] as i32 - 1, size[1] as i32 - 1])),
                })
            }
            DomainSpec::Mask { size, active } => {
                validate_size(*size)?;
                if active.len() != size[0] as usize * size[1] as usize {
                    return Err(TopologyError::InvalidMaskLength);
                }
                let cells = (0..size[1] as i32)
                    .flat_map(|y| {
                        (0..size[0] as i32).filter_map(move |x| {
                            active[y as usize * size[0] as usize + x as usize].then_some([x, y])
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(Self {
                    cells,
                    bounds: Some(([0, 0], [size[0] as i32 - 1, size[1] as i32 - 1])),
                })
            }
            DomainSpec::Sparse { cells } => {
                let mut unique = HashMap::new();
                for cell in cells {
                    if unique.insert(*cell, ()).is_some() {
                        return Err(TopologyError::DuplicateSparseCell);
                    }
                }
                let mut cells = cells.clone();
                cells.sort_unstable();
                let bounds: Option<([i32; 2], [i32; 2])> =
                    cells.iter().copied().fold(None, |bounds, cell| {
                        Some(match bounds {
                            None => (cell, cell),
                            Some((minimum, maximum)) => (
                                [minimum[0].min(cell[0]), minimum[1].min(cell[1])],
                                [maximum[0].max(cell[0]), maximum[1].max(cell[1])],
                            ),
                        })
                    });
                Ok(Self {
                    cells: cells.clone(),
                    bounds,
                })
            }
        }
    }

    fn map_boundary(&self, requested: [i32; 2], boundary: &BoundarySpec) -> Option<[i32; 2]> {
        if self.cells.contains(&requested) {
            return Some(requested);
        }
        let (minimum, maximum) = self.bounds?;
        let mapped = match boundary {
            BoundarySpec::Open | BoundarySpec::Constant(_) => return None,
            BoundarySpec::Periodic => [
                wrap(requested[0], minimum[0], maximum[0]),
                wrap(requested[1], minimum[1], maximum[1]),
            ],
            BoundarySpec::Clamp => [
                requested[0].clamp(minimum[0], maximum[0]),
                requested[1].clamp(minimum[1], maximum[1]),
            ],
            BoundarySpec::Reflect => [
                reflect(requested[0], minimum[0], maximum[0]),
                reflect(requested[1], minimum[1], maximum[1]),
            ],
        };
        self.cells.contains(&mapped).then_some(mapped)
    }
}

fn validate_size(size: [u32; 2]) -> Result<(), TopologyError> {
    if size[0] == 0 || size[1] == 0 {
        Err(TopologyError::InvalidBoardSize)
    } else {
        Ok(())
    }
}

fn wrap(value: i32, minimum: i32, maximum: i32) -> i32 {
    let size = maximum - minimum + 1;
    minimum + ((value - minimum) % size + size) % size
}

fn reflect(value: i32, minimum: i32, maximum: i32) -> i32 {
    let span = maximum - minimum;
    if span == 0 {
        return minimum;
    }
    let period = span * 2;
    let folded = ((value - minimum) % period + period) % period;
    minimum
        + if folded <= span {
            folded
        } else {
            period - folded
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(neighborhoods: Vec<NeighborTemplate>) -> LatticeSpec {
        LatticeSpec {
            basis: Basis2 {
                first: [1.0, 0.0],
                second: [0.0, 1.0],
            },
            sites: vec![SiteSpec {
                name: "cell".to_string(),
            }],
            neighborhoods,
        }
    }

    #[test]
    fn periodic_rectangular_topology_has_deterministic_dense_ids() {
        let lattice = square(vec![NeighborTemplate {
            source_site: 0,
            target_site: 0,
            cell_offset: [1, 0],
            weight: 2.0,
        }]);
        let topology = compile_topology(
            &lattice,
            &BoardSpec {
                domain: DomainSpec::Rect { size: [2, 1] },
            },
            &BoundarySpec::Periodic,
        )
        .unwrap();

        assert_eq!(topology.site_cells, vec![[0, 0], [1, 0]]);
        assert_eq!(topology.offsets, vec![0, 1, 2]);
        assert_eq!(topology.neighbors, vec![1, 0]);
        assert_eq!(topology.weights, vec![2.0, 2.0]);
    }

    #[test]
    fn masked_open_topology_omits_inactive_and_out_of_bounds_neighbors() {
        let lattice = square(vec![NeighborTemplate {
            source_site: 0,
            target_site: 0,
            cell_offset: [1, 0],
            weight: 1.0,
        }]);
        let topology = compile_topology(
            &lattice,
            &BoardSpec {
                domain: DomainSpec::Mask {
                    size: [3, 1],
                    active: vec![true, false, true],
                },
            },
            &BoundarySpec::Open,
        )
        .unwrap();

        assert_eq!(topology.site_cells, vec![[0, 0], [2, 0]]);
        assert_eq!(topology.offsets, vec![0, 0, 0]);
    }

    #[test]
    fn clamp_and_reflect_resolve_edges_without_changing_site_ids() {
        let lattice = square(vec![NeighborTemplate {
            source_site: 0,
            target_site: 0,
            cell_offset: [-1, 0],
            weight: 1.0,
        }]);
        let board = BoardSpec {
            domain: DomainSpec::Rect { size: [3, 1] },
        };
        let clamped = compile_topology(&lattice, &board, &BoundarySpec::Clamp).unwrap();
        let reflected = compile_topology(&lattice, &board, &BoundarySpec::Reflect).unwrap();

        assert_eq!(clamped.neighbors_of(0), &[0]);
        assert_eq!(reflected.neighbors_of(0), &[1]);
        assert_eq!(clamped.site_cells, reflected.site_cells);
    }

    #[test]
    fn sparse_multisite_topology_sorts_cells_and_resolves_site_targets() {
        let lattice = LatticeSpec {
            basis: Basis2 {
                first: [1.0, 0.0],
                second: [0.5, 0.866_025_4],
            },
            sites: vec![
                SiteSpec {
                    name: "A".to_string(),
                },
                SiteSpec {
                    name: "B".to_string(),
                },
            ],
            neighborhoods: vec![NeighborTemplate {
                source_site: 0,
                target_site: 1,
                cell_offset: [1, 0],
                weight: 0.75,
            }],
        };
        let topology = compile_topology(
            &lattice,
            &BoardSpec {
                domain: DomainSpec::Sparse {
                    cells: vec![[1, 0], [0, 0]],
                },
            },
            &BoundarySpec::Open,
        )
        .unwrap();

        assert_eq!(topology.site_cells, vec![[0, 0], [0, 0], [1, 0], [1, 0]]);
        assert_eq!(topology.neighbors_of(0), &[3]);
        assert_eq!(topology.weights_of(0), &[0.75]);
        assert_eq!(topology.site_names, vec!["A", "B"]);
    }

    #[test]
    fn validates_lattice_and_domain_inputs() {
        let mut lattice = square(Vec::new());
        lattice.sites.push(SiteSpec {
            name: "cell".to_string(),
        });
        assert_eq!(
            compile_topology(
                &lattice,
                &BoardSpec {
                    domain: DomainSpec::Rect { size: [1, 1] },
                },
                &BoundarySpec::Open,
            )
            .unwrap_err(),
            TopologyError::InvalidSiteName("cell".to_string())
        );
    }
}
