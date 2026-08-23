use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
pub struct TileId(pub u32);

/// Stable semantic identifier for one independent polygonal site inside a
/// periodic unit cell.  This is an item alias rather than a newtype so legacy
/// `TileId` RON and protocol payloads remain byte-for-byte compatible.
pub use TileId as BasisId;

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
pub struct PrototypeId(pub u32);

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    pub fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
}

impl std::ops::Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl std::ops::Mul<f64> for Vec2 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct RigidTransform {
    pub translation: Vec2,
    pub rotation: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum PrototypeShape {
    RegularPolygon { sides: u16, side_length: f64 },
    SimplePolygon { vertices: Vec<Vec2> },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TilePrototype {
    pub id: PrototypeId,
    pub name: String,
    pub shape: PrototypeShape,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TileInstance {
    pub id: TileId,
    pub prototype: PrototypeId,
    pub transform: RigidTransform,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum TilingMode {
    #[default]
    Topological,
    Geometric,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PeriodicTilingDraft {
    pub translation_a: Vec2,
    pub translation_b: Vec2,
    pub prototypes: Vec<TilePrototype>,
    pub instances: Vec<TileInstance>,
    pub mode: TilingMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeometryIssue {
    pub code: &'static str,
    pub message: String,
    pub vertex: Option<usize>,
}
