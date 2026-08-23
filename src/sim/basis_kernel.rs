use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::sim::tiling::BasisId;

const MAX_PERIODIC_KERNEL_AXIS: usize = 129;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BasisWeightPlane {
    pub values: Vec<f32>,
    pub mask: Option<Vec<bool>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PeriodicKernelDefinition {
    pub width: usize,
    pub height: usize,
    pub anchor_x: usize,
    pub anchor_y: usize,
    pub planes: BTreeMap<BasisId, BasisWeightPlane>,
}

impl PeriodicKernelDefinition {
    pub fn identity(basis: BasisId) -> Self {
        Self {
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            planes: BTreeMap::from([(
                basis,
                BasisWeightPlane {
                    values: vec![1.0],
                    mask: None,
                },
            )]),
        }
    }

    pub fn weight(&self, offset: [i16; 2], basis: BasisId) -> Option<f32> {
        let x = isize::try_from(self.anchor_x).ok()? + isize::from(offset[0]);
        let y = isize::try_from(self.anchor_y).ok()? + isize::from(offset[1]);
        let x = usize::try_from(x).ok()?;
        let y = usize::try_from(y).ok()?;
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = y.checked_mul(self.width)?.checked_add(x)?;
        let plane = self.planes.get(&basis)?;
        if plane.mask.as_ref().is_some_and(|mask| !mask[index]) {
            return None;
        }
        plane.values.get(index).copied()
    }

    pub fn set_weight(
        &mut self,
        offset: [i16; 2],
        basis: BasisId,
        value: f32,
    ) -> Result<(), BasisKernelError> {
        self.validate()?;
        if !value.is_finite() {
            return Err(BasisKernelError::NonFiniteWeight { basis });
        }
        let x = isize::try_from(self.anchor_x).map_err(|_| BasisKernelError::InvalidAnchor)?
            + isize::from(offset[0]);
        let y = isize::try_from(self.anchor_y).map_err(|_| BasisKernelError::InvalidAnchor)?
            + isize::from(offset[1]);
        let x = usize::try_from(x).map_err(|_| BasisKernelError::OffsetOutsideStencil)?;
        let y = usize::try_from(y).map_err(|_| BasisKernelError::OffsetOutsideStencil)?;
        if x >= self.width || y >= self.height {
            return Err(BasisKernelError::OffsetOutsideStencil);
        }
        let index = y
            .checked_mul(self.width)
            .and_then(|row| row.checked_add(x))
            .ok_or(BasisKernelError::InvalidDimensions)?;
        let plane = self
            .planes
            .get_mut(&basis)
            .ok_or(BasisKernelError::MissingBasisPlane { basis })?;
        plane.values[index] = value;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), BasisKernelError> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_PERIODIC_KERNEL_AXIS
            || self.height > MAX_PERIODIC_KERNEL_AXIS
        {
            return Err(BasisKernelError::InvalidDimensions);
        }
        if self.anchor_x >= self.width || self.anchor_y >= self.height {
            return Err(BasisKernelError::InvalidAnchor);
        }
        let expected = self
            .width
            .checked_mul(self.height)
            .ok_or(BasisKernelError::InvalidDimensions)?;
        for (basis, plane) in &self.planes {
            if plane.values.len() != expected {
                return Err(BasisKernelError::InvalidPlaneLength {
                    basis: *basis,
                    expected,
                    actual: plane.values.len(),
                });
            }
            if let Some(mask) = &plane.mask
                && mask.len() != expected
            {
                return Err(BasisKernelError::InvalidMaskLength {
                    basis: *basis,
                    expected,
                    actual: mask.len(),
                });
            }
            if plane.values.iter().any(|value| !value.is_finite()) {
                return Err(BasisKernelError::NonFiniteWeight { basis: *basis });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BasisKernelError {
    #[error("periodic kernel dimensions must be between 1 and 129 cells")]
    InvalidDimensions,
    #[error("periodic kernel anchor must lie inside the stencil")]
    InvalidAnchor,
    #[error("basis {basis:?} weight plane has {actual} values; expected {expected}")]
    InvalidPlaneLength {
        basis: BasisId,
        expected: usize,
        actual: usize,
    },
    #[error("basis {basis:?} mask has {actual} values; expected {expected}")]
    InvalidMaskLength {
        basis: BasisId,
        expected: usize,
        actual: usize,
    },
    #[error("basis {basis:?} weight plane contains a non-finite value")]
    NonFiniteWeight { basis: BasisId },
    #[error("periodic kernel has no weight plane for basis {basis:?}")]
    MissingBasisPlane { basis: BasisId },
    #[error("lattice offset lies outside the periodic kernel stencil")]
    OffsetOutsideStencil,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periodic_weight_uses_lattice_offset_and_source_basis() {
        let mut definition = PeriodicKernelDefinition::identity(BasisId(3));
        definition.set_weight([0, 0], BasisId(3), 0.25).unwrap();

        assert_eq!(definition.weight([0, 0], BasisId(3)), Some(0.25));
        assert_eq!(definition.weight([0, 0], BasisId(4)), None);
        assert_eq!(definition.weight([1, 0], BasisId(3)), None);
    }

    #[test]
    fn malformed_weight_plane_is_rejected() {
        let definition = PeriodicKernelDefinition {
            width: 3,
            height: 3,
            anchor_x: 1,
            anchor_y: 1,
            planes: [(
                BasisId(0),
                BasisWeightPlane {
                    values: vec![0.0; 8],
                    mask: None,
                },
            )]
            .into(),
        };

        assert!(matches!(
            definition.validate(),
            Err(BasisKernelError::InvalidPlaneLength { .. })
        ));
    }
}
