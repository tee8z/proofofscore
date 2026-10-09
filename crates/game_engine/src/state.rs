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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsteroidSize {
    Large,
    Medium,
    Small,
}

impl AsteroidSize {
    pub fn radius_factor(self) -> Fixed {
        match self {
            AsteroidSize::Large => Fixed::ONE,
            AsteroidSize::Medium => Fixed::HALF,
            AsteroidSize::Small => Fixed::from_ratio(1, 4),
        }
    }

    pub fn points_multiplier(self) -> u32 {
        match self {
            AsteroidSize::Small => 1,
            AsteroidSize::Medium => 2,
            AsteroidSize::Large => 3,
        }
    }

    pub fn smaller(self) -> Option<AsteroidSize> {
        match self {
            AsteroidSize::Large => Some(AsteroidSize::Medium),
            AsteroidSize::Medium => Some(AsteroidSize::Small),
            AsteroidSize::Small => None,
        }
    }
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
    #[serde(default = "default_asteroid_size")]
    pub size_class: AsteroidSize,
}

fn default_asteroid_size() -> AsteroidSize {
    AsteroidSize::Large
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnemyType {
    Drone,   // Drifts, shoots on fixed interval
    Fighter, // Turns toward player, shoots at player
    Bomber,  // Slow, tanky, drops mines on death
    Boss,    // Large, high HP, multiple attacks, grants extra life
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Enemy {
    pub x: Fixed,
    pub y: Fixed,
    pub velocity_x: Fixed,
    pub velocity_y: Fixed,
    pub angle: Fixed,
    pub radius: Fixed,
    pub hp: u32,
    pub enemy_type: EnemyType,
    pub shoot_cooldown: u32,
    pub shoot_timer: u32,
    pub points: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnemyBullet {
    pub x: Fixed,
    pub y: Fixed,
    pub velocity_x: Fixed,
    pub velocity_y: Fixed,
    pub radius: Fixed,
    pub life_time: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerUpType {
    RapidFire,  // 2x fire rate, double max bullets
    Shield,     // Absorbs one hit
    SpreadShot, // Fires 3 bullets in a fan
    SpeedBoost, // 1.5x thrust
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PowerUp {
    pub x: Fixed,
    pub y: Fixed,
    pub radius: Fixed,
    pub power_type: PowerUpType,
    pub life_time: u32, // Frames before despawn
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivePowerUp {
    pub power_type: PowerUpType,
    pub remaining: u32, // Frames remaining (0 for Shield = until hit)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameInput {
    pub thrust: bool,
    pub rotate_left: bool,
    pub rotate_right: bool,
    pub shoot: bool,
}

/// Pack a slice of FrameInputs into bytes (4 bits per frame, 2 frames per byte).
/// Bit layout per frame: bit0=thrust, bit1=rotate_left, bit2=rotate_right, bit3=shoot.
/// Low nibble = even frame, high nibble = odd frame.
pub fn encode_inputs(inputs: &[FrameInput]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(inputs.len().div_ceil(2));
    for chunk in inputs.chunks(2) {
        let lo = input_to_nibble(&chunk[0]);
        let hi = if chunk.len() > 1 {
            input_to_nibble(&chunk[1])
        } else {
            0
        };
        bytes.push(lo | (hi << 4));
    }
    bytes
}

/// Unpack bytes back into FrameInputs.
pub fn decode_inputs(data: &[u8], frame_count: u32) -> Vec<FrameInput> {
    input_frames(data, frame_count).collect()
}

/// Decode only supplied frames, without reserving memory from a claimed count.
/// Callers verifying a complete replay must first validate its encoded length.
pub fn input_frames(data: &[u8], frame_count: u32) -> impl Iterator<Item = FrameInput> + '_ {
    data.iter()
        .flat_map(|byte| [nibble_to_input(byte & 0x0F), nibble_to_input(byte >> 4)])
        .take(frame_count as usize)
}

fn input_to_nibble(input: &FrameInput) -> u8 {
    (input.thrust as u8)
        | ((input.rotate_left as u8) << 1)
        | ((input.rotate_right as u8) << 2)
        | ((input.shoot as u8) << 3)
}

fn nibble_to_input(nibble: u8) -> FrameInput {
    FrameInput {
        thrust: nibble & 1 != 0,
        rotate_left: nibble & 2 != 0,
        rotate_right: nibble & 4 != 0,
        shoot: nibble & 8 != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claimed_frames_do_not_determine_allocation_size() {
        let decoded = decode_inputs(&[0], u32::MAX);
        assert_eq!(decoded.len(), 2);
        assert!(decoded.capacity() < 16);
        assert_eq!(input_frames(&[], u32::MAX).count(), 0);
    }

    #[test]
    fn test_encode_decode_round_trip() {
        let inputs = vec![
            FrameInput {
                thrust: true,
                rotate_left: false,
                rotate_right: false,
                shoot: false,
            },
            FrameInput {
                thrust: false,
                rotate_left: true,
                rotate_right: false,
                shoot: true,
            },
            FrameInput {
                thrust: true,
                rotate_left: true,
                rotate_right: true,
                shoot: true,
            },
        ];
        let encoded = encode_inputs(&inputs);
        let decoded = decode_inputs(&encoded, 3);
        assert_eq!(inputs.len(), decoded.len());
        for (a, b) in inputs.iter().zip(decoded.iter()) {
            assert_eq!(a.thrust, b.thrust);
            assert_eq!(a.rotate_left, b.rotate_left);
            assert_eq!(a.rotate_right, b.rotate_right);
            assert_eq!(a.shoot, b.shoot);
        }
    }

    #[test]
    fn test_encode_empty() {
        let encoded = encode_inputs(&[]);
        assert!(encoded.is_empty());
        let decoded = decode_inputs(&encoded, 0);
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_encode_single_frame() {
        let inputs = vec![FrameInput {
            thrust: true,
            rotate_left: false,
            rotate_right: true,
            shoot: false,
        }];
        let encoded = encode_inputs(&inputs);
        assert_eq!(encoded.len(), 1);
        let decoded = decode_inputs(&encoded, 1);
        assert!(decoded[0].thrust);
        assert!(!decoded[0].rotate_left);
        assert!(decoded[0].rotate_right);
        assert!(!decoded[0].shoot);
    }
}
