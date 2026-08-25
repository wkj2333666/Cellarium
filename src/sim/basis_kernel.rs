use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::sim::tiling::BasisId;

const MAX_PERIODIC_KERNEL_AXIS: usize = 129;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BasisWeightPlane {
    pub values: Vec<f32>,
    pub mask: Option<Vec<bool>>,
}

impl<'de> Deserialize<'de> for BasisWeightPlane {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            values: Vec<f32>,
            mask: Option<Vec<bool>>,
        }

        let Wire { mut values, mask } = Wire::deserialize(deserializer)?;
        if let Some(active) = &mask {
            for (value, active) in values.iter_mut().zip(active) {
                if !active {
                    *value = 0.0;
                }
            }
        }
        Ok(Self { values, mask })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PeriodicKernelDefinition {
    pub width: usize,
    pub height: usize,
    pub anchor_x: usize,
    pub anchor_y: usize,
    pub planes: BTreeMap<BasisId, BasisWeightPlane>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KernelEntry {
    pub offset: [i16; 2],
    pub basis: BasisId,
    pub weight: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResizeReport {
    pub discarded_active_nonzero: Vec<KernelEntry>,
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
        let index = self.index_for_offset(offset)?;
        let plane = self.planes.get(&basis)?;
        if plane.mask.as_ref().is_some_and(|mask| !mask[index]) {
            return None;
        }
        plane.values.get(index).copied()
    }

    pub fn raw_weight(&self, offset: [i16; 2], basis: BasisId) -> Option<f32> {
        let index = self.index_for_offset(offset)?;
        self.planes.get(&basis)?.values.get(index).copied()
    }

    pub fn is_active(&self, offset: [i16; 2], basis: BasisId) -> Option<bool> {
        let index = self.index_for_offset(offset)?;
        let plane = self.planes.get(&basis)?;
        Some(plane.mask.as_ref().is_none_or(|mask| mask[index]))
    }

    pub fn canonicalize(&mut self) -> Result<(), BasisKernelError> {
        self.validate()?;
        for plane in self.planes.values_mut() {
            if let Some(mask) = &plane.mask {
                for (value, active) in plane.values.iter_mut().zip(mask) {
                    if !active {
                        *value = 0.0;
                    }
                }
            }
        }
        Ok(())
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
        let index = self
            .index_for_offset(offset)
            .ok_or(BasisKernelError::OffsetOutsideStencil)?;
        let plane = self
            .planes
            .get_mut(&basis)
            .ok_or(BasisKernelError::MissingBasisPlane { basis })?;
        if plane.mask.as_ref().is_some_and(|mask| !mask[index]) {
            return Err(BasisKernelError::InactiveEntry { basis, offset });
        }
        plane.values[index] = value;
        Ok(())
    }

    pub fn set_active(
        &mut self,
        offset: [i16; 2],
        basis: BasisId,
        active: bool,
    ) -> Result<(), BasisKernelError> {
        self.validate()?;
        let cell_count = self.width * self.height;
        let index = self
            .index_for_offset(offset)
            .ok_or(BasisKernelError::OffsetOutsideStencil)?;
        let plane = self
            .planes
            .get_mut(&basis)
            .ok_or(BasisKernelError::MissingBasisPlane { basis })?;
        if active {
            if let Some(mask) = &mut plane.mask
                && !mask[index]
            {
                plane.values[index] = 0.0;
                mask[index] = true;
            }
        } else {
            let mask = plane.mask.get_or_insert_with(|| vec![true; cell_count]);
            mask[index] = false;
            plane.values[index] = 0.0;
        }
        Ok(())
    }

    pub fn resize(
        &mut self,
        width: usize,
        height: usize,
        anchor_x: usize,
        anchor_y: usize,
    ) -> Result<ResizeReport, BasisKernelError> {
        self.validate()?;
        validate_geometry(width, height, anchor_x, anchor_y)?;

        let mut report = ResizeReport::default();
        let mut planes = BTreeMap::new();
        for (basis, old_plane) in &self.planes {
            let mut values = vec![0.0; width * height];
            let mut mask = vec![false; width * height];

            for old_y in 0..self.height {
                for old_x in 0..self.width {
                    let old_index = old_y * self.width + old_x;
                    let active = old_plane
                        .mask
                        .as_ref()
                        .is_none_or(|old_mask| old_mask[old_index]);
                    let offset = [
                        old_x as i16 - self.anchor_x as i16,
                        old_y as i16 - self.anchor_y as i16,
                    ];
                    let new_x = anchor_x as isize + isize::from(offset[0]);
                    let new_y = anchor_y as isize + isize::from(offset[1]);
                    if new_x >= 0
                        && new_y >= 0
                        && (new_x as usize) < width
                        && (new_y as usize) < height
                    {
                        let new_index = new_y as usize * width + new_x as usize;
                        mask[new_index] = active;
                        values[new_index] = if active {
                            old_plane.values[old_index]
                        } else {
                            0.0
                        };
                    } else if active && old_plane.values[old_index] != 0.0 {
                        report.discarded_active_nonzero.push(KernelEntry {
                            offset,
                            basis: *basis,
                            weight: old_plane.values[old_index],
                        });
                    }
                }
            }
            planes.insert(
                *basis,
                BasisWeightPlane {
                    values,
                    mask: Some(mask),
                },
            );
        }

        self.width = width;
        self.height = height;
        self.anchor_x = anchor_x;
        self.anchor_y = anchor_y;
        self.planes = planes;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), BasisKernelError> {
        validate_geometry(self.width, self.height, self.anchor_x, self.anchor_y)?;
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

    fn index_for_offset(&self, offset: [i16; 2]) -> Option<usize> {
        let x = isize::try_from(self.anchor_x).ok()? + isize::from(offset[0]);
        let y = isize::try_from(self.anchor_y).ok()? + isize::from(offset[1]);
        let x = usize::try_from(x).ok()?;
        let y = usize::try_from(y).ok()?;
        if x >= self.width || y >= self.height {
            return None;
        }
        y.checked_mul(self.width)?.checked_add(x)
    }
}

fn validate_geometry(
    width: usize,
    height: usize,
    anchor_x: usize,
    anchor_y: usize,
) -> Result<(), BasisKernelError> {
    if width == 0
        || height == 0
        || width > MAX_PERIODIC_KERNEL_AXIS
        || height > MAX_PERIODIC_KERNEL_AXIS
    {
        return Err(BasisKernelError::InvalidDimensions);
    }
    if anchor_x >= width || anchor_y >= height {
        return Err(BasisKernelError::InvalidAnchor);
    }
    Ok(())
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
    #[error("basis {basis:?} entry at offset {offset:?} is inactive")]
    InactiveEntry { basis: BasisId, offset: [i16; 2] },
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

    #[test]
    fn inactive_periodic_weight_is_zero_and_cannot_be_written() {
        let mut definition = PeriodicKernelDefinition {
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            planes: [(
                BasisId(0),
                BasisWeightPlane {
                    values: vec![0.75],
                    mask: Some(vec![false]),
                },
            )]
            .into(),
        };

        definition.canonicalize().unwrap();

        assert_eq!(definition.raw_weight([0, 0], BasisId(0)), Some(0.0));
        assert_eq!(definition.weight([0, 0], BasisId(0)), None);
        assert_eq!(
            definition.set_weight([0, 0], BasisId(0), 0.5),
            Err(BasisKernelError::InactiveEntry {
                basis: BasisId(0),
                offset: [0, 0],
            })
        );
    }

    #[test]
    fn deserialization_clears_inactive_periodic_weights() {
        let plane: BasisWeightPlane =
            ron::from_str("(values:[0.75,-0.25],mask:Some([false,true]))").unwrap();

        assert_eq!(plane.values, vec![0.0, -0.25]);
        assert_eq!(plane.mask, Some(vec![false, true]));
    }

    #[test]
    fn support_activation_starts_at_zero_and_deactivation_clears_weight() {
        let mut definition = PeriodicKernelDefinition {
            width: 1,
            height: 1,
            anchor_x: 0,
            anchor_y: 0,
            planes: [(
                BasisId(0),
                BasisWeightPlane {
                    values: vec![0.0],
                    mask: Some(vec![false]),
                },
            )]
            .into(),
        };

        definition.set_active([0, 0], BasisId(0), true).unwrap();
        assert_eq!(definition.weight([0, 0], BasisId(0)), Some(0.0));
        definition.set_weight([0, 0], BasisId(0), 0.4).unwrap();
        definition.set_active([0, 0], BasisId(0), false).unwrap();

        assert_eq!(definition.weight([0, 0], BasisId(0)), None);
        assert_eq!(definition.raw_weight([0, 0], BasisId(0)), Some(0.0));
    }

    #[test]
    fn resize_preserves_weights_by_lattice_offset() {
        let mut definition = PeriodicKernelDefinition {
            width: 3,
            height: 3,
            anchor_x: 1,
            anchor_y: 1,
            planes: [(
                BasisId(0),
                BasisWeightPlane {
                    values: vec![0.0; 9],
                    mask: Some(vec![true; 9]),
                },
            )]
            .into(),
        };
        definition.set_weight([1, -1], BasisId(0), 0.4).unwrap();

        let report = definition.resize(5, 3, 2, 1).unwrap();

        assert!(report.discarded_active_nonzero.is_empty());
        assert_eq!(definition.weight([1, -1], BasisId(0)), Some(0.4));
        assert_eq!(definition.is_active([-2, 0], BasisId(0)), Some(false));
        assert_eq!(definition.raw_weight([-2, 0], BasisId(0)), Some(0.0));
    }

    #[test]
    fn shrinking_reports_discarded_active_nonzero_entries() {
        let mut definition = PeriodicKernelDefinition {
            width: 3,
            height: 1,
            anchor_x: 1,
            anchor_y: 0,
            planes: [(
                BasisId(7),
                BasisWeightPlane {
                    values: vec![0.25, 0.0, -0.5],
                    mask: Some(vec![true; 3]),
                },
            )]
            .into(),
        };

        let report = definition.resize(1, 1, 0, 0).unwrap();

        assert_eq!(
            report.discarded_active_nonzero,
            vec![
                KernelEntry {
                    offset: [-1, 0],
                    basis: BasisId(7),
                    weight: 0.25,
                },
                KernelEntry {
                    offset: [1, 0],
                    basis: BasisId(7),
                    weight: -0.5,
                },
            ]
        );
    }
}
