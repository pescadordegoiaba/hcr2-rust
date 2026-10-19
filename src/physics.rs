#![allow(dead_code)]
//! 2D vehicle body used by the offline hill. Field names follow the dump
//! (`maxTorque`, `dragCoeff`, `centerOfMass`, suspension frequency). The
//! step below is the free-body integrator from that dump. The hill contact
//! itself lives in `lib.rs` and reads these same specs.

/// A 2D vector (x horizontal, y up).
#[derive(Clone, Copy, Debug, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn add(self, o: Self) -> Self {
        Self { x: self.x + o.x, y: self.y + o.y }
    }
    pub fn scale(self, s: f32) -> Self {
        Self { x: self.x * s, y: self.y * s }
    }
}

/// Chassis rigid-body parameters.
#[derive(Clone, Debug)]
pub struct ChassisSpec {
    pub mass: f32,
    pub collidable_mass: f32,
    pub center_of_mass: Vec2,
    pub drag_coeff: f32,
    pub angular_drag: f32,
    pub front_bar_spring: f32,
    pub rear_bar_spring: f32,
}

impl Default for ChassisSpec {
    fn default() -> Self {
        Self {
            mass: 800.0,
            collidable_mass: 800.0,
            center_of_mass: Vec2::new(0.0, 0.2),
            drag_coeff: 0.02,
            angular_drag: 0.5,
            front_bar_spring: 5000.0,
            rear_bar_spring: 5000.0,
        }
    }
}

/// Wheel plus suspension joint.
#[derive(Clone, Debug)]
pub struct WheelSpec {
    pub radius: f32,
    pub friction: f32,
    pub suspension: SuspensionSpec,
    pub driven: bool,
    pub brakes: bool,
}

impl Default for WheelSpec {
    fn default() -> Self {
        Self {
            radius: 0.35,
            friction: 1.0,
            suspension: SuspensionSpec::default(),
            driven: true,
            brakes: true,
        }
    }
}

/// Frequency-damped spring, Box2D style.
#[derive(Clone, Debug)]
pub struct SuspensionSpec {
    pub frequency: f32,
    pub damping: f32,
    pub min_travel: f32,
    pub max_travel: f32,
    pub joint: Vec2,
}

impl Default for SuspensionSpec {
    fn default() -> Self {
        Self {
            frequency: 8.0,
            damping: 0.7,
            min_travel: -0.15,
            max_travel: 0.15,
            joint: Vec2::new(0.0, -0.4),
        }
    }
}

/// Engine torque curve and boost.
#[derive(Clone, Debug)]
pub struct EngineSpec {
    pub max_torque: f32,
    pub max_rpm: f32,
    pub power_distribution: f32,
    pub nitro_impulse: f32,
    pub nitro_duration: f32,
    pub thrust: f32,
}

impl Default for EngineSpec {
    fn default() -> Self {
        Self {
            max_torque: 800.0,
            max_rpm: 60.0,
            power_distribution: 1.0,
            nitro_impulse: 0.0,
            nitro_duration: 0.0,
            thrust: 0.0,
        }
    }
}

/// Fuel tank. `consume` is the per-tick rule from the dump.
#[derive(Clone, Debug)]
pub struct FuelSpec {
    pub capacity: f32,
    pub consumption_rate: f32,
    pub density: f32,
    pub idle_consumption: f32,
}

impl Default for FuelSpec {
    fn default() -> Self {
        Self {
            capacity: 100.0,
            consumption_rate: 5.0,
            density: 1.0,
            idle_consumption: 0.5,
        }
    }
}

impl FuelSpec {
    pub fn consume(&self, fuel: &mut f32, throttle: f32, dt: f32) {
        let rate = self.idle_consumption
            + (self.consumption_rate - self.idle_consumption) * throttle.max(0.0);
        let density = if self.density.abs() < 0.001 { 1.0 } else { self.density };
        *fuel = (*fuel - rate * dt / density).max(0.0);
    }
    pub fn is_empty(fuel: f32) -> bool {
        fuel <= 0.0
    }
}

/// Live body. `step` is the dump integrator (no ground). The hill solver
/// keeps its own pose and uses `step` only as the air-drag and gravity term
/// when a wheel is off the ground is not required — drag is applied in
/// `lib.rs` with `drag_coeff`. This method stays so the dump tests hold.
#[derive(Clone, Debug, Default)]
pub struct PhysicsState {
    pub position: Vec2,
    pub velocity: Vec2,
    pub angle: f32,
    pub angular_vel: f32,
    pub fuel: f32,
    pub wheel_speed: [f32; 2],
    pub on_ground: bool,
    pub airborne_time: f32,
    pub wheelie_time: f32,
    pub flip_count: u32,
}

impl PhysicsState {
    pub fn step(
        &mut self,
        chassis: &ChassisSpec,
        engine: &EngineSpec,
        throttle: f32,
        brake: f32,
        dt: f32,
    ) {
        self.velocity.y -= 9.81 * dt;
        let v = self.velocity;
        let speed = (v.x * v.x + v.y * v.y).sqrt();
        if speed > 0.01 && chassis.mass > 0.0 {
            let drag = chassis.drag_coeff * speed * speed;
            self.velocity = self.velocity.add(v.scale(-drag / chassis.mass * dt));
        }
        if throttle > 0.0 && chassis.mass > 0.0 {
            let torque = engine.max_torque * throttle;
            let force = torque / 0.35;
            self.velocity.x += force / chassis.mass * dt;
        }
        if brake > 0.0 && self.on_ground {
            self.velocity.x *= 1.0 - (2.0 * brake * dt).min(1.0);
        }
        self.angular_vel *= 1.0 - chassis.angular_drag * dt;
        self.position = self.position.add(self.velocity.scale(dt));
        self.angle += self.angular_vel * dt;
        if !self.on_ground {
            self.airborne_time += dt;
        } else {
            self.airborne_time = 0.0;
        }
    }

    pub fn register_rotation(&mut self, prev_angle: f32) -> bool {
        let delta = self.angle - prev_angle;
        if delta.abs() > std::f32::consts::PI {
            self.flip_count += 1;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuel_consumes() {
        let f = FuelSpec::default();
        let mut fuel = 100.0;
        f.consume(&mut fuel, 1.0, 1.0);
        assert!(fuel < 100.0);
        assert!(!FuelSpec::is_empty(fuel));
    }

    #[test]
    fn physics_step_applies_gravity() {
        let mut s = PhysicsState { on_ground: false, ..Default::default() };
        s.step(&ChassisSpec::default(), &EngineSpec::default(), 0.0, 0.0, 0.1);
        assert!(s.velocity.y < 0.0);
    }

    #[test]
    fn flip_detection() {
        let mut s = PhysicsState::default();
        let prev = s.angle;
        s.angle = std::f32::consts::TAU;
        assert!(s.register_rotation(prev));
        assert_eq!(s.flip_count, 1);
    }
}
