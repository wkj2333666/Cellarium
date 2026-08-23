use super::{
    PeriodicTilingDraft, PrototypeId, PrototypeShape, RigidTransform, TileId, TileInstance,
    TilePrototype, TilingMode, Vec2,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TilingPreset {
    Square,
    EquilateralTriangles,
    RegularHexagon,
    OctagonSquare,
}

impl TilingPreset {
    pub const ALL: [Self; 4] = [
        Self::Square,
        Self::EquilateralTriangles,
        Self::RegularHexagon,
        Self::OctagonSquare,
    ];
}

pub fn build_preset(preset: TilingPreset, scale: f64) -> PeriodicTilingDraft {
    let s = scale.max(1e-12);
    match preset {
        TilingPreset::Square => PeriodicTilingDraft {
            translation_a: Vec2::new(s, 0.0),
            translation_b: Vec2::new(0.0, s),
            prototypes: vec![TilePrototype {
                id: PrototypeId(0),
                name: "square".into(),
                shape: PrototypeShape::SimplePolygon {
                    vertices: vec![
                        Vec2::ZERO,
                        Vec2::new(s, 0.0),
                        Vec2::new(s, s),
                        Vec2::new(0.0, s),
                    ],
                },
            }],
            instances: vec![TileInstance {
                id: TileId(0),
                prototype: PrototypeId(0),
                transform: RigidTransform::default(),
            }],
            mode: TilingMode::Topological,
        },
        TilingPreset::EquilateralTriangles => {
            let height = s * 3.0_f64.sqrt() / 2.0;
            let a = Vec2::new(s, 0.0);
            let b = Vec2::new(s / 2.0, height);
            PeriodicTilingDraft {
                translation_a: a,
                translation_b: b,
                prototypes: vec![
                    TilePrototype {
                        id: PrototypeId(0),
                        name: "up-triangle".into(),
                        shape: PrototypeShape::SimplePolygon {
                            vertices: vec![Vec2::ZERO, a, a + b],
                        },
                    },
                    TilePrototype {
                        id: PrototypeId(1),
                        name: "down-triangle".into(),
                        shape: PrototypeShape::SimplePolygon {
                            vertices: vec![Vec2::ZERO, a + b, b],
                        },
                    },
                ],
                instances: vec![
                    TileInstance {
                        id: TileId(0),
                        prototype: PrototypeId(0),
                        transform: RigidTransform::default(),
                    },
                    TileInstance {
                        id: TileId(1),
                        prototype: PrototypeId(1),
                        transform: RigidTransform::default(),
                    },
                ],
                mode: TilingMode::Topological,
            }
        }
        TilingPreset::RegularHexagon => PeriodicTilingDraft {
            translation_a: Vec2::new(1.5 * s, 3.0_f64.sqrt() * s / 2.0),
            translation_b: Vec2::new(0.0, 3.0_f64.sqrt() * s),
            prototypes: vec![TilePrototype {
                id: PrototypeId(0),
                name: "hexagon".into(),
                shape: PrototypeShape::RegularPolygon {
                    sides: 6,
                    side_length: s,
                },
            }],
            instances: vec![TileInstance {
                id: TileId(0),
                prototype: PrototypeId(0),
                transform: RigidTransform::default(),
            }],
            mode: TilingMode::Topological,
        },
        TilingPreset::OctagonSquare => {
            // One octagon and the diamond-oriented square at a lattice corner
            // are representatives of the periodic 4.8.8 quotient.
            let period = s * (1.0 + 2.0_f64.sqrt());
            PeriodicTilingDraft {
                translation_a: Vec2::new(period, 0.0),
                translation_b: Vec2::new(0.0, period),
                prototypes: vec![
                    TilePrototype {
                        id: PrototypeId(0),
                        name: "octagon".into(),
                        shape: PrototypeShape::RegularPolygon {
                            sides: 8,
                            side_length: s,
                        },
                    },
                    TilePrototype {
                        id: PrototypeId(1),
                        name: "square".into(),
                        shape: PrototypeShape::SimplePolygon {
                            vertices: vec![
                                Vec2::new(-s / 2.0, -s / 2.0),
                                Vec2::new(s / 2.0, -s / 2.0),
                                Vec2::new(s / 2.0, s / 2.0),
                                Vec2::new(-s / 2.0, s / 2.0),
                            ],
                        },
                    },
                ],
                instances: vec![
                    TileInstance {
                        id: TileId(0),
                        prototype: PrototypeId(0),
                        transform: RigidTransform::default(),
                    },
                    TileInstance {
                        id: TileId(1),
                        prototype: PrototypeId(1),
                        transform: RigidTransform {
                            translation: Vec2::new(period / 2.0, period / 2.0),
                            rotation: std::f64::consts::FRAC_PI_4,
                        },
                    },
                ],
                mode: TilingMode::Topological,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{canonical_half_edges, validate_coverage};
    #[test]
    fn square_preset_validates() {
        let draft = build_preset(TilingPreset::Square, 1.0);
        assert!(validate_coverage(&draft).is_ok());
        assert!(canonical_half_edges(&draft, 1e-9).is_ok());
    }
    #[test]
    fn octagon_square_has_two_editable_representatives() {
        let draft = build_preset(TilingPreset::OctagonSquare, 1.0);
        assert_eq!(draft.instances.len(), 2);
        assert_eq!(
            draft.prototypes[0].shape,
            PrototypeShape::RegularPolygon {
                sides: 8,
                side_length: 1.0
            }
        );
        assert!(validate_coverage(&draft).is_ok());
        assert!(canonical_half_edges(&draft, 1e-9).is_ok());
    }

    #[test]
    fn every_preset_is_an_exact_once_periodic_tiling() {
        for preset in TilingPreset::ALL {
            let report = validate_coverage(&build_preset(preset, 1.0)).unwrap();
            assert_eq!(report.coverage_multiplicity, 1, "{preset:?}");
            assert_eq!(report.euler_characteristic, 0, "{preset:?}");
        }
    }
}
