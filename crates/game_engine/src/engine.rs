use crate::config::GameConfig;
use crate::fixed::{isqrt, Fixed};
use crate::rng::Rng;
use crate::state::{Asteroid, Bullet, FrameInput, Ship};

/// The complete game state. Fully deterministic given the same seed, config, and input sequence.
pub struct GameState {
    pub ship: Ship,
    pub asteroids: Vec<Asteroid>,
    pub bullets: Vec<Bullet>,
    pub score: u32,
    pub level: u32,
    pub frame: u32,
    pub game_over: bool,
    pub config: GameConfig,
    rng: Rng,
    prev_shoot: bool,
}

impl GameState {
    /// Create a new game with the given seed and config.
    pub fn new(seed: u64, config: GameConfig) -> Self {
        let rng = Rng::new(seed);
        let ship = Ship {
            x: config.canvas_width * Fixed::HALF,
            y: config.canvas_height * Fixed::HALF,
            angle: Fixed::ZERO,
            velocity_x: Fixed::ZERO,
            velocity_y: Fixed::ZERO,
            radius: config.ship.radius,
            invulnerable: true,
            invulnerable_timer: config.ship.invulnerability_frames,
            thrusting: false,
        };

        let mut state = GameState {
            ship,
            asteroids: Vec::new(),
            bullets: Vec::new(),
            score: 0,
            level: 1,
            frame: 0,
            game_over: false,
            config,
            rng,
            prev_shoot: false,
        };

        state.spawn_asteroids();
        state
    }

    /// Advance one frame with the given inputs.
    pub fn tick(&mut self, input: &FrameInput) {
        if self.game_over {
            return;
        }

        // 1. Update ship rotation
        if input.rotate_left {
            self.ship.angle = self.ship.angle + self.config.ship.turn_speed;
        }
        if input.rotate_right {
            self.ship.angle = self.ship.angle - self.config.ship.turn_speed;
        }

        // Normalize angle to [0, 256)
        let full_circle = Fixed::from(256);
        while self.ship.angle.0 < 0 {
            self.ship.angle = self.ship.angle + full_circle;
        }
        while self.ship.angle.0 >= full_circle.0 {
            self.ship.angle = self.ship.angle - full_circle;
        }

        // 2. Update ship thrust/friction
        self.ship.thrusting = input.thrust;
        if input.thrust {
            let cos_a = self.ship.angle.cos();
            let sin_a = self.ship.angle.sin();
            self.ship.velocity_x = self.ship.velocity_x + self.config.ship.thrust * cos_a;
            // Y is inverted (canvas convention: Y increases downward)
            self.ship.velocity_y = self.ship.velocity_y - self.config.ship.thrust * sin_a;
        } else {
            // Apply friction: velocity *= (1 - friction)
            let damping = Fixed::ONE - self.config.ship.friction;
            self.ship.velocity_x = self.ship.velocity_x * damping;
            self.ship.velocity_y = self.ship.velocity_y * damping;
        }

        // 3. Update ship position
        self.ship.x = self.ship.x + self.ship.velocity_x;
        self.ship.y = self.ship.y + self.ship.velocity_y;

        // 4. Wrap ship position
        wrap_position(
            &mut self.ship.x,
            &mut self.ship.y,
            self.config.canvas_width,
            self.config.canvas_height,
        );

        // 5. Update invulnerability timer
        if self.ship.invulnerable {
            if self.ship.invulnerable_timer > 0 {
                self.ship.invulnerable_timer -= 1;
            }
            if self.ship.invulnerable_timer == 0 {
                self.ship.invulnerable = false;
            }
        }

        // 6. Handle shooting (rising edge: false → true)
        if input.shoot && !self.prev_shoot && (self.bullets.len() as u32) < self.config.bullets.max_count {
                let cos_a = self.ship.angle.cos();
                let sin_a = self.ship.angle.sin();
                let bullet = Bullet {
                    x: self.ship.x + self.ship.radius * cos_a,
                    y: self.ship.y - self.ship.radius * sin_a,
                    velocity_x: self.config.bullets.speed * cos_a,
                    velocity_y: -self.config.bullets.speed * sin_a,
                    radius: self.config.bullets.radius,
                    life_time: self.config.bullets.life_time,
                };
                self.bullets.push(bullet);
        }
        self.prev_shoot = input.shoot;

        // 7. Update bullet positions
        for bullet in &mut self.bullets {
            bullet.x = bullet.x + bullet.velocity_x;
            bullet.y = bullet.y + bullet.velocity_y;
        }

        // 8. Wrap bullet positions
        let cw = self.config.canvas_width;
        let ch = self.config.canvas_height;
        for bullet in &mut self.bullets {
            wrap_position(&mut bullet.x, &mut bullet.y, cw, ch);
        }

        // 9. Decrement bullet lifetimes, remove expired
        for bullet in &mut self.bullets {
            bullet.life_time = bullet.life_time.saturating_sub(1);
        }
        self.bullets.retain(|b| b.life_time > 0);

        // 10. Update asteroid positions
        for asteroid in &mut self.asteroids {
            asteroid.x = asteroid.x + asteroid.velocity_x;
            asteroid.y = asteroid.y + asteroid.velocity_y;
        }

        // 11. Wrap asteroid positions (with radius padding)
        for asteroid in &mut self.asteroids {
            wrap_position_padded(&mut asteroid.x, &mut asteroid.y, asteroid.radius, cw, ch);
        }

        // 12. Check bullet-asteroid collisions
        self.check_bullet_asteroid_collisions();

        // 13. Check ship-asteroid collisions
        if !self.ship.invulnerable {
            for asteroid in &self.asteroids {
                if circles_collide(
                    self.ship.x,
                    self.ship.y,
                    self.ship.radius,
                    asteroid.x,
                    asteroid.y,
                    asteroid.radius,
                ) {
                    self.game_over = true;
                    return;
                }
            }
        }

        // 14. Check level complete
        if self.asteroids.is_empty() {
            self.level += 1;
            // Reset ship to center with invulnerability
            self.ship.x = self.config.canvas_width * Fixed::HALF;
            self.ship.y = self.config.canvas_height * Fixed::HALF;
            self.ship.velocity_x = Fixed::ZERO;
            self.ship.velocity_y = Fixed::ZERO;
            self.ship.invulnerable = true;
            self.ship.invulnerable_timer = self.config.ship.invulnerability_frames;
            self.spawn_asteroids();
        }

        // 15. Increment frame counter
        self.frame += 1;
    }

    fn check_bullet_asteroid_collisions(&mut self) {
        let mut asteroids_to_remove = Vec::new();
        let mut bullets_to_remove = Vec::new();

        // Iterate backwards through asteroids like the JS does
        for i in (0..self.asteroids.len()).rev() {
            for j in (0..self.bullets.len()).rev() {
                if bullets_to_remove.contains(&j) {
                    continue;
                }
                if circles_collide(
                    self.asteroids[i].x,
                    self.asteroids[i].y,
                    self.asteroids[i].radius,
                    self.bullets[j].x,
                    self.bullets[j].y,
                    self.bullets[j].radius,
                ) {
                    asteroids_to_remove.push(i);
                    bullets_to_remove.push(j);
                    self.score += self.config.scoring.points_per_asteroid * self.level;
                    break; // Same as JS: break inner loop on collision
                }
            }
        }

        // Remove in reverse order to preserve indices
        asteroids_to_remove.sort_unstable();
        asteroids_to_remove.dedup();
        for &i in asteroids_to_remove.iter().rev() {
            self.asteroids.remove(i);
        }

        bullets_to_remove.sort_unstable();
        bullets_to_remove.dedup();
        for &j in bullets_to_remove.iter().rev() {
            self.bullets.remove(j);
        }
    }

    fn spawn_asteroids(&mut self) {
        let count = self.config.asteroids.initial_count * isqrt(self.level);
        let level_speed_factor =
            Fixed::ONE + Fixed::from_ratio(1, 10) * Fixed::from(self.level as i32 - 1);
        let min_dist = Fixed::from(100);
        let min_dist_sq = min_dist * min_dist;

        for _ in 0..count {
            // Generate position avoiding ship (min 100px distance)
            let (x, y) = loop {
                let x = self.rng.next_range(Fixed::ZERO, self.config.canvas_width);
                let y = self.rng.next_range(Fixed::ZERO, self.config.canvas_height);
                let dx = self.ship.x - x;
                let dy = self.ship.y - y;
                let dist_sq = dx * dx + dy * dy;
                if dist_sq.0 >= min_dist_sq.0 {
                    break (x, y);
                }
            };

            // Velocity: (random * 2 - 1) * speed * level_factor
            let two = Fixed::from(2);
            let vx = (self.rng.next_fixed() * two - Fixed::ONE)
                * self.config.asteroids.speed
                * level_speed_factor;
            let vy = (self.rng.next_fixed() * two - Fixed::ONE)
                * self.config.asteroids.speed
                * level_speed_factor;

            // Angle: random 0-256
            let angle = self.rng.next_range(Fixed::ZERO, Fixed::from(256));

            // Vertices: random in [vertices_min, vertices_max]
            let vertices = self.rng.next_int_range(
                self.config.asteroids.vertices_min as i32,
                self.config.asteroids.vertices_max as i32 + 1,
            ) as u32;

            // Offsets: random in [0.8, 1.2] for each vertex up to vertices_max
            let offset_min = Fixed::from_ratio(4, 5); // 0.8
            let offset_max = Fixed::from_ratio(6, 5); // 1.2
            let mut offsets = Vec::with_capacity(self.config.asteroids.vertices_max as usize);
            for _ in 0..self.config.asteroids.vertices_max {
                offsets.push(self.rng.next_range(offset_min, offset_max));
            }

            self.asteroids.push(Asteroid {
                x,
                y,
                velocity_x: vx,
                velocity_y: vy,
                radius: self.config.asteroids.size,
                angle,
                vertices,
                offsets,
            });
        }
    }
}

/// Wrap position to canvas bounds (ship/bullet style: teleport at edge).
fn wrap_position(x: &mut Fixed, y: &mut Fixed, width: Fixed, height: Fixed) {
    if x.0 < 0 {
        *x = *x + width;
    } else if x.0 > width.0 {
        *x = *x - width;
    }
    if y.0 < 0 {
        *y = *y + height;
    } else if y.0 > height.0 {
        *y = *y - height;
    }
}

/// Wrap position with radius padding (asteroid style: fully disappear before reappearing).
fn wrap_position_padded(x: &mut Fixed, y: &mut Fixed, radius: Fixed, width: Fixed, height: Fixed) {
    let neg_r = -radius;
    let w_plus_r = width + radius;
    let h_plus_r = height + radius;

    if x.0 < neg_r.0 {
        *x = w_plus_r;
    } else if x.0 > w_plus_r.0 {
        *x = neg_r;
    }
    if y.0 < neg_r.0 {
        *y = h_plus_r;
    } else if y.0 > h_plus_r.0 {
        *y = neg_r;
    }
}

/// Check if two circles collide: sqrt(dx*dx + dy*dy) < r1 + r2
/// Optimized: compare squared distances to avoid sqrt.
fn circles_collide(x1: Fixed, y1: Fixed, r1: Fixed, x2: Fixed, y2: Fixed, r2: Fixed) -> bool {
    let dx = x1 - x2;
    let dy = y1 - y2;
    let dist_sq = dx * dx + dy * dy;
    let radii_sum = r1 + r2;
    let radii_sq = radii_sum * radii_sum;
    dist_sq.0 < radii_sq.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GameConfig;
    use crate::state::FrameInput;

    fn no_input() -> FrameInput {
        FrameInput {
            thrust: false,
            rotate_left: false,
            rotate_right: false,
            shoot: false,
        }
    }

    #[test]
    fn test_new_game_state() {
        let config = GameConfig::default_config();
        let state = GameState::new(12345, config.clone());
        assert_eq!(state.score, 0);
        assert_eq!(state.level, 1);
        assert_eq!(state.frame, 0);
        assert!(!state.game_over);
        assert!(state.ship.invulnerable);
        // Should have initial_count * isqrt(1) = 5 * 1 = 5 asteroids
        assert_eq!(state.asteroids.len(), 5);
        // Ship at center
        assert_eq!(state.ship.x, config.canvas_width * Fixed::HALF);
        assert_eq!(state.ship.y, config.canvas_height * Fixed::HALF);
    }

    #[test]
    fn test_determinism() {
        let config = GameConfig::default_config();
        let inputs: Vec<FrameInput> = (0..300)
            .map(|i| FrameInput {
                thrust: i % 5 == 0,
                rotate_left: i % 7 == 0,
                rotate_right: i % 11 == 0,
                shoot: i % 13 == 0,
            })
            .collect();

        // Run game 1
        let mut state1 = GameState::new(42, config.clone());
        for input in &inputs {
            state1.tick(input);
        }

        // Run game 2 with same seed and inputs
        let mut state2 = GameState::new(42, config);
        for input in &inputs {
            state2.tick(input);
        }

        assert_eq!(state1.score, state2.score);
        assert_eq!(state1.level, state2.level);
        assert_eq!(state1.frame, state2.frame);
        assert_eq!(state1.game_over, state2.game_over);
        assert_eq!(state1.ship.x, state2.ship.x);
        assert_eq!(state1.ship.y, state2.ship.y);
        assert_eq!(state1.asteroids.len(), state2.asteroids.len());
        assert_eq!(state1.bullets.len(), state2.bullets.len());
    }

    #[test]
    fn test_different_seeds_differ() {
        let config = GameConfig::default_config();
        let state1 = GameState::new(100, config.clone());
        let state2 = GameState::new(200, config);
        // Asteroids should be in different positions
        if !state1.asteroids.is_empty() && !state2.asteroids.is_empty() {
            let a1 = &state1.asteroids[0];
            let a2 = &state2.asteroids[0];
            assert!(a1.x != a2.x || a1.y != a2.y);
        }
    }

    #[test]
    fn test_ship_rotation() {
        let config = GameConfig::default_config();
        let mut state = GameState::new(1, config);
        let initial_angle = state.ship.angle;

        state.tick(&FrameInput {
            thrust: false,
            rotate_left: true,
            rotate_right: false,
            shoot: false,
        });

        assert!(state.ship.angle.0 > initial_angle.0);
    }

    #[test]
    fn test_ship_thrust() {
        let config = GameConfig::default_config();
        let mut state = GameState::new(1, config);

        // Thrust for several frames
        for _ in 0..10 {
            state.tick(&FrameInput {
                thrust: true,
                rotate_left: false,
                rotate_right: false,
                shoot: false,
            });
        }

        // Ship should have moved from center
        let center_x = state.config.canvas_width * Fixed::HALF;
        assert!(state.ship.x != center_x || state.ship.velocity_x.0 != 0);
    }

    #[test]
    fn test_shooting() {
        let config = GameConfig::default_config();
        let mut state = GameState::new(1, config);

        // No shoot
        state.tick(&no_input());
        assert_eq!(state.bullets.len(), 0);

        // Shoot (rising edge)
        state.tick(&FrameInput {
            thrust: false,
            rotate_left: false,
            rotate_right: false,
            shoot: true,
        });
        assert_eq!(state.bullets.len(), 1);

        // Hold shoot - should NOT fire again
        state.tick(&FrameInput {
            thrust: false,
            rotate_left: false,
            rotate_right: false,
            shoot: true,
        });
        assert_eq!(state.bullets.len(), 1);

        // Release and press again
        state.tick(&no_input());
        state.tick(&FrameInput {
            thrust: false,
            rotate_left: false,
            rotate_right: false,
            shoot: true,
        });
        assert_eq!(state.bullets.len(), 2);
    }

    #[test]
    fn test_bullet_lifetime() {
        let mut config = GameConfig::default_config();
        config.bullets.life_time = 5;
        let mut state = GameState::new(1, config);

        // Shoot
        state.tick(&FrameInput {
            thrust: false,
            rotate_left: false,
            rotate_right: false,
            shoot: true,
        });
        assert_eq!(state.bullets.len(), 1);

        // Tick 5 more times (bullet should expire)
        for _ in 0..5 {
            state.tick(&no_input());
        }
        assert_eq!(state.bullets.len(), 0);
    }

    #[test]
    fn test_invulnerability_expires() {
        let mut config = GameConfig::default_config();
        config.ship.invulnerability_frames = 10;
        let mut state = GameState::new(1, config);

        assert!(state.ship.invulnerable);

        for _ in 0..10 {
            state.tick(&no_input());
        }

        assert!(!state.ship.invulnerable);
    }

    #[test]
    fn test_screen_wrapping() {
        let config = GameConfig::default_config();
        let mut state = GameState::new(1, config.clone());

        // Place ship near right edge and give it rightward velocity
        state.ship.x = config.canvas_width - Fixed::ONE;
        state.ship.velocity_x = Fixed::from(5);
        state.ship.velocity_y = Fixed::ZERO;

        state.tick(&no_input());

        // Ship should have wrapped
        assert!(state.ship.x.0 < config.canvas_width.0);
    }

    #[test]
    fn test_collision_detection() {
        assert!(circles_collide(
            Fixed::ZERO,
            Fixed::ZERO,
            Fixed::from(10),
            Fixed::from(5),
            Fixed::ZERO,
            Fixed::from(10),
        ));

        assert!(!circles_collide(
            Fixed::ZERO,
            Fixed::ZERO,
            Fixed::from(5),
            Fixed::from(100),
            Fixed::ZERO,
            Fixed::from(5),
        ));
    }

    #[test]
    fn test_scoring() {
        let mut config = GameConfig::default_config();
        config.asteroids.initial_count = 1;
        config.scoring.points_per_asteroid = 10;
        let state = GameState::new(1, config);

        // Level 1, 1 asteroid. Score for destroying it = 10 * 1 = 10
        let initial_asteroids = state.asteroids.len();
        assert!(initial_asteroids > 0);
    }

    #[test]
    fn test_level_progression() {
        let mut config = GameConfig::default_config();
        config.asteroids.initial_count = 1;
        config.asteroids.speed = Fixed::ZERO; // stationary asteroids
        let mut state = GameState::new(1, config);

        assert_eq!(state.level, 1);
        let asteroid_count = state.asteroids.len();
        assert_eq!(asteroid_count, 1); // 1 * isqrt(1) = 1

        // Move a bullet directly at the asteroid to test level up
        // We'll just manually clear asteroids to test the mechanism
        state.asteroids.clear();
        state.tick(&no_input());

        // Should have leveled up
        assert_eq!(state.level, 2);
        assert!(!state.asteroids.is_empty());
        assert!(state.ship.invulnerable);
    }
}
