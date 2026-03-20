use crate::fixed::Fixed;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ship {
    pub x: Fixed,
    pub y: Fixed,
    /// Angle in 0-256 range (256 = full circle).
    pub angle: Fixed,
    pub velocity_x: Fixed,
    pub velocity_y: Fixed,
    pub radius: Fixed,
    pub invulnerable: bool,
    /// Frames remaining of invulnerability.
    pub invulnerable_timer: u32,
    pub thrusting: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asteroid {
    pub x: Fixed,
    pub y: Fixed,
    pub velocity_x: Fixed,
    pub velocity_y: Fixed,
    pub radius: Fixed,
    pub angle: Fixed,
    pub vertices: u32,
    /// Vertex offset multipliers for irregular shape.
    pub offsets: Vec<Fixed>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bullet {
    pub x: Fixed,
    pub y: Fixed,
    pub velocity_x: Fixed,
    pub velocity_y: Fixed,
    pub radius: Fixed,
    /// Frames remaining.
    pub life_time: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameInput {
    pub thrust: bool,
    pub rotate_left: bool,
    pub rotate_right: bool,
    pub shoot: bool,
}
