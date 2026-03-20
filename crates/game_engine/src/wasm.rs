#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::config::GameConfig;
use crate::engine::GameState;
use crate::fixed::Fixed;
use crate::state::FrameInput;
use serde::Serialize;

/// Serializable state for JSON output to JS renderer.
/// All angles converted from 256-unit to radians, all Fixed converted to f32.
#[derive(Serialize)]
struct RenderState {
    ship: RenderShip,
    asteroids: Vec<RenderAsteroid>,
    bullets: Vec<RenderBullet>,
    score: u32,
    level: u32,
    frame: u32,
    game_over: bool,
}

#[derive(Serialize)]
struct RenderShip {
    x: f32,
    y: f32,
    angle: f32,
    radius: f32,
    invulnerable: bool,
    thrusting: bool,
}

#[derive(Serialize)]
struct RenderAsteroid {
    x: f32,
    y: f32,
    radius: f32,
    angle: f32,
    vertices: u32,
    offsets: Vec<f32>,
}

#[derive(Serialize)]
struct RenderBullet {
    x: f32,
    y: f32,
    radius: f32,
}

/// Convert a 256-unit angle (Fixed) to radians (f32).
fn angle_to_radians(angle: Fixed) -> f32 {
    // radians = angle * 2 * PI / 256
    angle.to_f32() * core::f32::consts::TAU / 256.0
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct GameEngine {
    state: GameState,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl GameEngine {
    /// Create a new game engine. Seed is split into two u32 halves since JS
    /// cannot natively pass u64 to WASM. Combine as: (seed_high << 32) | seed_low.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(seed_high: u32, seed_low: u32, config_json: &str) -> Result<GameEngine, String> {
        let seed = ((seed_high as u64) << 32) | (seed_low as u64);
        let config: GameConfig =
            serde_json::from_str(config_json).map_err(|e| format!("Invalid config: {}", e))?;
        Ok(GameEngine {
            state: GameState::new(seed, config),
        })
    }

    pub fn tick(&mut self, thrust: bool, rotate_left: bool, rotate_right: bool, shoot: bool) {
        self.state.tick(&FrameInput {
            thrust,
            rotate_left,
            rotate_right,
            shoot,
        });
    }

    pub fn get_state_json(&self) -> String {
        let render = RenderState {
            ship: RenderShip {
                x: self.state.ship.x.to_f32(),
                y: self.state.ship.y.to_f32(),
                angle: angle_to_radians(self.state.ship.angle),
                radius: self.state.ship.radius.to_f32(),
                invulnerable: self.state.ship.invulnerable,
                thrusting: self.state.ship.thrusting,
            },
            asteroids: self
                .state
                .asteroids
                .iter()
                .map(|a| RenderAsteroid {
                    x: a.x.to_f32(),
                    y: a.y.to_f32(),
                    radius: a.radius.to_f32(),
                    angle: angle_to_radians(a.angle),
                    vertices: a.vertices,
                    offsets: a.offsets.iter().map(|o| o.to_f32()).collect(),
                })
                .collect(),
            bullets: self
                .state
                .bullets
                .iter()
                .map(|b| RenderBullet {
                    x: b.x.to_f32(),
                    y: b.y.to_f32(),
                    radius: b.radius.to_f32(),
                })
                .collect(),
            score: self.state.score,
            level: self.state.level,
            frame: self.state.frame,
            game_over: self.state.game_over,
        };

        serde_json::to_string(&render).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn is_game_over(&self) -> bool {
        self.state.game_over
    }

    pub fn score(&self) -> u32 {
        self.state.score
    }

    pub fn level(&self) -> u32 {
        self.state.level
    }

    pub fn frame(&self) -> u32 {
        self.state.frame
    }
}
