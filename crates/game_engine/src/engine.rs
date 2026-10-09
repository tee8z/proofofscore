use crate::config::GameConfig;
use crate::fixed::{isqrt, Fixed};
use crate::rng::Rng;
use crate::state::{
    ActivePowerUp, Asteroid, AsteroidSize, Bullet, Enemy, EnemyBullet, EnemyType, FrameInput,
    PowerUp, PowerUpType, Ship,
};

/// The complete game state. Fully deterministic given the same seed, config, and input sequence.
pub struct GameState {
    pub ship: Ship,
    pub asteroids: Vec<Asteroid>,
    pub bullets: Vec<Bullet>,
    pub enemies: Vec<Enemy>,
    pub enemy_bullets: Vec<EnemyBullet>,
    pub power_ups: Vec<PowerUp>,
    pub active_power_up: Option<ActivePowerUp>,
    pub score: u32,
    pub level: u32,
    pub frame: u32,
    pub lives: u32,
    pub game_over: bool,
    pub wave_start_frame: u32,
    pub last_time_bonus: u32,
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

        let lives = config.lives.starting_lives;
        let mut state = GameState {
            ship,
            asteroids: Vec::new(),
            bullets: Vec::new(),
            enemies: Vec::new(),
            enemy_bullets: Vec::new(),
            power_ups: Vec::new(),
            active_power_up: None,
            score: 0,
            level: 1,
            frame: 0,
            lives,
            game_over: false,
            wave_start_frame: 0,
            last_time_bonus: 0,
            config,
            rng,
            prev_shoot: false,
        };

        state.spawn_asteroids();
        state.spawn_enemies();
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
        let thrust_mult = if matches!(self.active_power_up, Some(ref p) if p.power_type == PowerUpType::SpeedBoost)
        {
            Fixed::from_ratio(3, 2) // 1.5x
        } else {
            Fixed::ONE
        };
        if input.thrust {
            let cos_a = self.ship.angle.cos();
            let sin_a = self.ship.angle.sin();
            self.ship.velocity_x =
                self.ship.velocity_x + self.config.ship.thrust * thrust_mult * cos_a;
            // Y is inverted (canvas convention: Y increases downward)
            self.ship.velocity_y =
                self.ship.velocity_y - self.config.ship.thrust * thrust_mult * sin_a;
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

        // 6. Handle shooting (rising edge or rapid fire)
        let is_rapid =
            matches!(self.active_power_up, Some(ref p) if p.power_type == PowerUpType::RapidFire);
        let max_bullets = if is_rapid {
            self.config.bullets.max_count * 2
        } else {
            self.config.bullets.max_count
        };
        // Rapid fire: shoot every frame while held. Normal: rising edge only.
        let should_shoot = if is_rapid {
            input.shoot
        } else {
            input.shoot && !self.prev_shoot
        };
        if should_shoot && (self.bullets.len() as u32) < max_bullets {
            let cos_a = self.ship.angle.cos();
            let sin_a = self.ship.angle.sin();
            let is_spread = matches!(self.active_power_up, Some(ref p) if p.power_type == PowerUpType::SpreadShot);

            if is_spread {
                // Fire 3 bullets in a fan (-15°, 0°, +15° in 256-unit = -10.67, 0, +10.67)
                for offset in [-11i32, 0, 11] {
                    let a = self.ship.angle + Fixed::from(offset);
                    let ca = a.cos();
                    let sa = a.sin();
                    self.bullets.push(Bullet {
                        x: self.ship.x + self.ship.radius * ca,
                        y: self.ship.y - self.ship.radius * sa,
                        velocity_x: self.config.bullets.speed * ca,
                        velocity_y: -self.config.bullets.speed * sa,
                        radius: self.config.bullets.radius,
                        life_time: self.config.bullets.life_time,
                    });
                }
            } else {
                self.bullets.push(Bullet {
                    x: self.ship.x + self.ship.radius * cos_a,
                    y: self.ship.y - self.ship.radius * sin_a,
                    velocity_x: self.config.bullets.speed * cos_a,
                    velocity_y: -self.config.bullets.speed * sin_a,
                    radius: self.config.bullets.radius,
                    life_time: self.config.bullets.life_time,
                });
            }
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
            let mut hit = false;
            for asteroid in &self.asteroids {
                if circles_collide(
                    self.ship.x,
                    self.ship.y,
                    self.ship.radius,
                    asteroid.x,
                    asteroid.y,
                    asteroid.radius,
                ) {
                    hit = true;
                    break;
                }
            }
            if hit && self.handle_ship_hit() {
                return;
            }
        }

        // 14. Update enemies
        self.update_enemies();

        // 15. Check bullet-enemy collisions
        self.check_bullet_enemy_collisions();

        // 16. Update enemy bullets
        let cw2 = self.config.canvas_width;
        let ch2 = self.config.canvas_height;
        for eb in &mut self.enemy_bullets {
            eb.x = eb.x + eb.velocity_x;
            eb.y = eb.y + eb.velocity_y;
            wrap_position(&mut eb.x, &mut eb.y, cw2, ch2);
            eb.life_time = eb.life_time.saturating_sub(1);
        }
        self.enemy_bullets.retain(|eb| eb.life_time > 0);

        // 17. Check enemy bullet-ship collisions
        if !self.ship.invulnerable {
            let mut hit = false;
            for eb in &self.enemy_bullets {
                if circles_collide(
                    self.ship.x,
                    self.ship.y,
                    self.ship.radius,
                    eb.x,
                    eb.y,
                    eb.radius,
                ) {
                    hit = true;
                    break;
                }
            }
            if hit && self.handle_ship_hit() {
                return;
            }
        }

        // 18. Check ship-enemy collisions
        if !self.ship.invulnerable {
            let mut hit = false;
            for enemy in &self.enemies {
                if circles_collide(
                    self.ship.x,
                    self.ship.y,
                    self.ship.radius,
                    enemy.x,
                    enemy.y,
                    enemy.radius,
                ) {
                    hit = true;
                    break;
                }
            }
            if hit && self.handle_ship_hit() {
                return;
            }
        }

        // 19. Check level complete (all asteroids AND enemies destroyed)
        if self.asteroids.is_empty() && self.enemies.is_empty() {
            // Time bonus: max(0, (30s - clear_time)) * level * 5
            let clear_frames = self.frame - self.wave_start_frame;
            let clear_secs = clear_frames / 60;
            let time_bonus = if clear_secs < 30 {
                (30 - clear_secs) * self.level * 5
            } else {
                0
            };
            self.score += time_bonus;
            self.last_time_bonus = time_bonus;

            self.level += 1;
            self.wave_start_frame = self.frame;

            // Reset ship to center with invulnerability
            self.ship.x = self.config.canvas_width * Fixed::HALF;
            self.ship.y = self.config.canvas_height * Fixed::HALF;
            self.ship.velocity_x = Fixed::ZERO;
            self.ship.velocity_y = Fixed::ZERO;
            self.ship.invulnerable = true;
            self.ship.invulnerable_timer = self.config.ship.invulnerability_frames;
            self.enemy_bullets.clear();
            self.spawn_asteroids();
            self.spawn_enemies();
        }

        // 20. Update power-ups
        // Tick down lifetimes and remove expired
        for pu in &mut self.power_ups {
            pu.life_time = pu.life_time.saturating_sub(1);
        }
        self.power_ups.retain(|pu| pu.life_time > 0);

        // Check player-powerup collision
        let mut collected = None;
        for i in 0..self.power_ups.len() {
            if circles_collide(
                self.ship.x,
                self.ship.y,
                self.ship.radius,
                self.power_ups[i].x,
                self.power_ups[i].y,
                self.power_ups[i].radius,
            ) {
                collected = Some(i);
                break;
            }
        }
        if let Some(i) = collected {
            let pu = self.power_ups.remove(i);
            let duration = match pu.power_type {
                PowerUpType::RapidFire => 600,  // 10 seconds
                PowerUpType::Shield => 0,       // Until hit
                PowerUpType::SpreadShot => 480, // 8 seconds
                PowerUpType::SpeedBoost => 600, // 10 seconds
            };
            self.active_power_up = Some(ActivePowerUp {
                power_type: pu.power_type,
                remaining: duration,
            });
        }

        // Tick down active power-up timer
        if let Some(ref mut ap) = self.active_power_up {
            if ap.power_type != PowerUpType::Shield {
                if ap.remaining > 0 {
                    ap.remaining -= 1;
                }
                if ap.remaining == 0 {
                    self.active_power_up = None;
                }
            }
        }

        // 21. Increment frame counter
        self.frame += 1;
    }

    fn check_bullet_asteroid_collisions(&mut self) {
        let mut asteroids_to_remove = Vec::new();
        let mut bullets_to_remove = Vec::new();
        let mut new_asteroids = Vec::new();

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
                    let asteroid = &self.asteroids[i];
                    let points = self.config.scoring.points_per_asteroid
                        * asteroid.size_class.points_multiplier()
                        * self.level;
                    self.score += points;

                    // Split into smaller asteroids if not already small.
                    // Medium asteroids only split into Small at level >= min_split_level.
                    if let Some(smaller_size) = asteroid.size_class.smaller() {
                        if asteroid.size_class != AsteroidSize::Medium
                            || self.level >= self.config.asteroids.min_split_level
                        {
                            let smaller_radius =
                                self.config.asteroids.size * smaller_size.radius_factor();
                            for _ in 0..2 {
                                let two = Fixed::from(2);
                                let vx = (self.rng.next_fixed() * two - Fixed::ONE)
                                    * self.config.asteroids.speed
                                    * Fixed::from(2); // Fragments are faster
                                let vy = (self.rng.next_fixed() * two - Fixed::ONE)
                                    * self.config.asteroids.speed
                                    * Fixed::from(2);
                                let angle = self.rng.next_range(Fixed::ZERO, Fixed::from(256));

                                let vertices = self.rng.next_int_range(
                                    self.config.asteroids.vertices_min as i32,
                                    self.config.asteroids.vertices_max as i32 + 1,
                                ) as u32;
                                let offset_min = Fixed::from_ratio(4, 5);
                                let offset_max = Fixed::from_ratio(6, 5);
                                let mut offsets =
                                    Vec::with_capacity(self.config.asteroids.vertices_max as usize);
                                for _ in 0..self.config.asteroids.vertices_max {
                                    offsets.push(self.rng.next_range(offset_min, offset_max));
                                }

                                new_asteroids.push(Asteroid {
                                    x: asteroid.x,
                                    y: asteroid.y,
                                    velocity_x: vx,
                                    velocity_y: vy,
                                    radius: smaller_radius,
                                    angle,
                                    vertices,
                                    offsets,
                                    size_class: smaller_size,
                                });
                            }
                        }
                    }

                    asteroids_to_remove.push(i);
                    bullets_to_remove.push(j);
                    break;
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

        // Add the split fragments
        self.asteroids.extend(new_asteroids);
    }

    /// Handle a hit on the ship. Returns true if game is over.
    fn handle_ship_hit(&mut self) -> bool {
        // Shield absorbs the hit
        if matches!(self.active_power_up, Some(ref p) if p.power_type == PowerUpType::Shield) {
            self.active_power_up = None;
            // Brief invulnerability after shield break
            self.ship.invulnerable = true;
            self.ship.invulnerable_timer = 60; // 1 second
            return false;
        }

        self.lives = self.lives.saturating_sub(1);
        if self.lives == 0 {
            self.game_over = true;
            return true;
        }
        self.ship.x = self.config.canvas_width * Fixed::HALF;
        self.ship.y = self.config.canvas_height * Fixed::HALF;
        self.ship.velocity_x = Fixed::ZERO;
        self.ship.velocity_y = Fixed::ZERO;
        self.ship.invulnerable = true;
        self.ship.invulnerable_timer = self.config.ship.invulnerability_frames;
        self.bullets.clear();
        self.enemy_bullets.clear();
        false
    }

    fn update_enemies(&mut self) {
        let cw = self.config.canvas_width;
        let ch = self.config.canvas_height;
        let bullet_speed = self.config.enemies.enemy_bullet_speed;
        let bullet_lifetime = self.config.enemies.enemy_bullet_lifetime;
        let mut new_bullets = Vec::new();

        for enemy in &mut self.enemies {
            // Move
            enemy.x = enemy.x + enemy.velocity_x;
            enemy.y = enemy.y + enemy.velocity_y;
            wrap_position_padded(&mut enemy.x, &mut enemy.y, enemy.radius, cw, ch);

            // Fighter/Boss: turn toward player
            if enemy.enemy_type == EnemyType::Fighter || enemy.enemy_type == EnemyType::Boss {
                let dx = self.ship.x - enemy.x;
                let dy = self.ship.y - enemy.y;
                // atan2 approximation using the lookup table angle system
                let target_angle = Fixed::atan2(dy, dx);
                let diff = target_angle - enemy.angle;
                let turn = Fixed::from_ratio(1, 1); // 1 unit per frame (~1.4 degrees)
                if diff.0.abs() < turn.0 {
                    enemy.angle = target_angle;
                } else if diff.0 > 0 {
                    enemy.angle = enemy.angle + turn;
                } else {
                    enemy.angle = enemy.angle - turn;
                }
            }

            // Shoot
            if enemy.shoot_timer > 0 {
                enemy.shoot_timer -= 1;
            } else {
                enemy.shoot_timer = enemy.shoot_cooldown;

                // Drone: shoots in its facing direction
                // Fighter/Boss: shoots toward player
                let (bvx, bvy) = if enemy.enemy_type == EnemyType::Fighter
                    || enemy.enemy_type == EnemyType::Boss
                {
                    let dx = self.ship.x - enemy.x;
                    let dy = self.ship.y - enemy.y;
                    let dist = Fixed::from(((dx * dx + dy * dy).0 as f64).sqrt() as i32);
                    if dist.0 > 0 {
                        (bullet_speed * dx / dist, bullet_speed * dy / dist)
                    } else {
                        (bullet_speed, Fixed::ZERO)
                    }
                } else {
                    let cos_a = enemy.angle.cos();
                    let sin_a = enemy.angle.sin();
                    (bullet_speed * cos_a, -bullet_speed * sin_a)
                };

                new_bullets.push(EnemyBullet {
                    x: enemy.x,
                    y: enemy.y,
                    velocity_x: bvx,
                    velocity_y: bvy,
                    radius: Fixed::from(2),
                    life_time: bullet_lifetime,
                });
            }
        }

        self.enemy_bullets.extend(new_bullets);
    }

    fn check_bullet_enemy_collisions(&mut self) {
        let mut enemies_to_remove = Vec::new();
        let mut bullets_to_remove = Vec::new();
        let mut new_power_ups = Vec::new();

        for i in (0..self.enemies.len()).rev() {
            for j in (0..self.bullets.len()).rev() {
                if bullets_to_remove.contains(&j) {
                    continue;
                }
                if circles_collide(
                    self.enemies[i].x,
                    self.enemies[i].y,
                    self.enemies[i].radius,
                    self.bullets[j].x,
                    self.bullets[j].y,
                    self.bullets[j].radius,
                ) {
                    self.enemies[i].hp = self.enemies[i].hp.saturating_sub(1);
                    bullets_to_remove.push(j);
                    if self.enemies[i].hp == 0 {
                        self.score += self.enemies[i].points * self.level;

                        // Boss kill grants an extra life
                        if self.enemies[i].enemy_type == EnemyType::Boss
                            && self.lives < self.config.lives.max_lives
                        {
                            self.lives += 1;
                        }

                        // 20% chance to drop a power-up (bosses always drop)
                        let should_drop = self.enemies[i].enemy_type == EnemyType::Boss
                            || self.rng.next_int_range(0, 5) == 0;
                        if should_drop {
                            let power_type = match self.rng.next_int_range(0, 4) {
                                0 => PowerUpType::RapidFire,
                                1 => PowerUpType::Shield,
                                2 => PowerUpType::SpreadShot,
                                _ => PowerUpType::SpeedBoost,
                            };
                            new_power_ups.push(PowerUp {
                                x: self.enemies[i].x,
                                y: self.enemies[i].y,
                                radius: Fixed::from(8),
                                power_type,
                                life_time: 300, // 5 seconds
                            });
                        }

                        enemies_to_remove.push(i);
                    }
                    break;
                }
            }
        }

        enemies_to_remove.sort_unstable();
        enemies_to_remove.dedup();
        for &i in enemies_to_remove.iter().rev() {
            self.enemies.remove(i);
        }
        bullets_to_remove.sort_unstable();
        bullets_to_remove.dedup();
        for &j in bullets_to_remove.iter().rev() {
            self.bullets.remove(j);
        }
        self.power_ups.extend(new_power_ups);
    }

    fn spawn_enemies(&mut self) {
        let level = self.level;
        let cfg = &self.config.enemies;

        let mut types_to_spawn: Vec<EnemyType> = Vec::new();

        if level >= cfg.drone_start_level {
            // Spawn 1-2 drones per level past the threshold
            let count = 1 + (level - cfg.drone_start_level) / 2;
            for _ in 0..count.min(4) {
                types_to_spawn.push(EnemyType::Drone);
            }
        }
        if level >= cfg.fighter_start_level {
            let count = 1 + (level - cfg.fighter_start_level) / 3;
            for _ in 0..count.min(3) {
                types_to_spawn.push(EnemyType::Fighter);
            }
        }
        if level >= cfg.bomber_start_level {
            let count = (level - cfg.bomber_start_level) / 3;
            for _ in 0..count.min(2) {
                types_to_spawn.push(EnemyType::Bomber);
            }
        }

        // Boss every 5 levels
        if level.is_multiple_of(5) {
            types_to_spawn.push(EnemyType::Boss);
        }

        let min_dist = Fixed::from(100);
        let min_dist_sq = min_dist * min_dist;

        for enemy_type in types_to_spawn {
            // Spawn at screen edge
            let (x, y) = loop {
                let edge = self.rng.next_int_range(0, 4);
                let x = match edge {
                    0 => Fixed::ZERO,
                    1 => self.config.canvas_width,
                    _ => self.rng.next_range(Fixed::ZERO, self.config.canvas_width),
                };
                let y = match edge {
                    2 => Fixed::ZERO,
                    3 => self.config.canvas_height,
                    _ => self.rng.next_range(Fixed::ZERO, self.config.canvas_height),
                };
                let dx = self.ship.x - x;
                let dy = self.ship.y - y;
                if (dx * dx + dy * dy).0 >= min_dist_sq.0 {
                    break (x, y);
                }
            };

            let two = Fixed::from(2);
            let speed = match enemy_type {
                EnemyType::Drone => Fixed::from_ratio(3, 4),  // 0.75
                EnemyType::Fighter => Fixed::ONE,             // 1.0
                EnemyType::Bomber => Fixed::from_ratio(1, 2), // 0.5
                EnemyType::Boss => Fixed::from_ratio(1, 3),   // 0.33 - slow but menacing
            };
            let vx = (self.rng.next_fixed() * two - Fixed::ONE) * speed;
            let vy = (self.rng.next_fixed() * two - Fixed::ONE) * speed;
            let angle = self.rng.next_range(Fixed::ZERO, Fixed::from(256));

            let cycle = (level / 10) + 1; // HP scales each cycle
            let (hp, radius, points, shoot_cooldown) = match enemy_type {
                EnemyType::Drone => (1, Fixed::from(12), 25, cfg.drone_shoot_cooldown),
                EnemyType::Fighter => (1, Fixed::from(14), 50, cfg.fighter_shoot_cooldown),
                EnemyType::Bomber => (3, Fixed::from(18), 100, 0), // Bombers don't shoot
                EnemyType::Boss => (
                    10 * cycle,
                    Fixed::from(30),
                    500,
                    cfg.fighter_shoot_cooldown / 2,
                ),
            };

            self.enemies.push(Enemy {
                x,
                y,
                velocity_x: vx,
                velocity_y: vy,
                angle,
                radius,
                hp,
                enemy_type,
                shoot_cooldown,
                shoot_timer: shoot_cooldown / 2, // Stagger first shot
                points,
            });
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
                size_class: AsteroidSize::Large,
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

/// Replay a game given seed, config, and recorded inputs.
/// Returns (score, level, frame_count, game_over).
pub fn replay(seed: u64, config: GameConfig, inputs: &[FrameInput]) -> (u32, u32, u32, bool) {
    replay_iter(seed, config, inputs.iter().cloned())
}

/// Replay decoded inputs as they arrive, without retaining a frame vector.
pub fn replay_iter(
    seed: u64,
    config: GameConfig,
    inputs: impl IntoIterator<Item = FrameInput>,
) -> (u32, u32, u32, bool) {
    let mut state = GameState::new(seed, config);
    for input in inputs {
        if state.game_over {
            break;
        }
        state.tick(&input);
    }
    (state.score, state.level, state.frame, state.game_over)
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
