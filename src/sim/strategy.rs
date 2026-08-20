use crate::sim::kernel::Kernel;
use crate::sim::topology::CompiledTopology;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuExecutionStrategy {
    DirectStencil,
    SparseGather,
}

pub fn select_gpu_strategy(
    topology: Option<&CompiledTopology>,
    kernel: Option<&Kernel>,
) -> GpuExecutionStrategy {
    if topology.is_some() {
        return GpuExecutionStrategy::SparseGather;
    }
    let _small_dense_kernel =
        kernel.is_some_and(|kernel| kernel.width <= 15 && kernel.height <= 15);
    GpuExecutionStrategy::DirectStencil
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization};
    use crate::sim::topology::{
        Basis2, BoardSpec, BoundarySpec, DomainSpec, LatticeSpec, NeighborTemplate, SiteSpec,
        compile_topology,
    };

    fn kernel() -> Kernel {
        KernelDefinition {
            name: "one".to_string(),
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            mask: None,
            normalization: Normalization::None,
            parameters: Default::default(),
            values: KernelValues::Explicit(vec![1.0]),
        }
        .build()
        .unwrap()
    }

    #[test]
    fn selects_direct_stencil_for_regular_kernel_without_compiled_topology() {
        assert_eq!(
            select_gpu_strategy(None, Some(&kernel())),
            GpuExecutionStrategy::DirectStencil
        );
    }

    #[test]
    fn selects_sparse_gather_for_compiled_topology() {
        let topology = compile_topology(
            &LatticeSpec {
                basis: Basis2 {
                    first: [1.0, 0.0],
                    second: [0.0, 1.0],
                },
                sites: vec![SiteSpec {
                    name: "cell".to_string(),
                }],
                neighborhoods: vec![NeighborTemplate {
                    source_site: 0,
                    target_site: 0,
                    cell_offset: [1, 0],
                    weight: 1.0,
                }],
            },
            &BoardSpec {
                domain: DomainSpec::Rect { size: [2, 1] },
            },
            &BoundarySpec::Periodic,
        )
        .unwrap();
        assert_eq!(
            select_gpu_strategy(Some(&topology), None),
            GpuExecutionStrategy::SparseGather
        );
    }
}
