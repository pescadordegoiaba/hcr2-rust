#![allow(dead_code)]
//! Playable roster wired to the numbers recovered from the dumps.
//!
//! `vehicle_data::VEHICLE_SPECS` is generated from
//! `analysis/jsons_decrypted/json/vehicles/*.json` (rube Box2D scenes and
//! vehicle definitions) by `tools/extract_vehicle_data.py`. Everything below
//! stays in the dump's own units — kg, m, N*m, Hz, m/s — so the upgrade
//! curves can be interpolated straight from the config `from`/`to` pairs.
//!
//! Only three quantities are not in the dump and are derived here, each with
//! the reasoning next to it: fuel burn rate, quadratic drag (fitted so the
//! dumped top speed is reached with the dumped torque) and angular damping.

use crate::physics::FuelSpec;
use crate::tuning::{grip_part, UpgradeEffect, UpgradeEffectKind};
use crate::vehicle_data::{VehicleSpec, MAX_WHEELS, VEHICLE_SPECS};

pub const VEHICLE_ROSTER: &[&str] = &[
    "jeep",
    "superjeep",
    "scooter",
    "motocross",
    "dunebuggy",
    "rally",
    "tank",
    "truck",
    "racing_truck",
    "bus",
    "snowmobile",
    "super_diesel",
    "moonlander",
    "sledge",
];

/// Levels a config upgrade exposes (`max_levels` in every vehicle definition).
const MAX_LEVEL: u8 = 20;

/// Full-throttle seconds a tank lasts. The dump has the tank size
/// (`fuelTank`) but not the burn rate, which lives in the update loop of
/// libcocos2dcpp (see analysis/all_info.md §15); 45 s is the value that
/// matches the observed 40–60 s runs per tank.
const TANK_SECONDS: f32 = 45.0;

pub fn vehicle_count() -> usize {
    VEHICLE_SPECS.len()
}

pub fn spec(index: usize) -> &'static VehicleSpec {
    &VEHICLE_SPECS[index.min(VEHICLE_SPECS.len() - 1)]
}

pub fn vehicle_display_name(index: usize) -> &'static str {
    spec(index).display_name
}

/// Newline joined display names, same order as `VEHICLE_ROSTER`, taken from
/// the dumps so the page never drifts from the generated table.
pub fn roster_blob() -> &'static str {
    static BLOB: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BLOB.get_or_init(|| {
        VEHICLE_SPECS
            .iter()
            .map(|s| s.display_name)
            .collect::<Vec<_>>()
            .join("\n")
    })
}

#[derive(Clone, Copy, Debug)]
pub struct UpgradeLevels {
    pub acceleration: u8,
    pub top_speed: u8,
    pub fuel_tank: u8,
    pub suspension: u8,
    pub grip: u8,
}

impl UpgradeLevels {
    pub fn clamped(accel: i32, top: i32, fuel: i32, susp: i32, grip: i32) -> Self {
        Self {
            acceleration: clamp_level(accel),
            top_speed: clamp_level(top),
            fuel_tank: clamp_level(fuel),
            suspension: clamp_level(susp),
            grip: clamp_level(grip),
        }
    }
}

fn clamp_level(level: i32) -> u8 {
    level.clamp(1, MAX_LEVEL as i32) as u8
}

/// Position of a level on its config curve, 0.0 at level 1, 1.0 at max level.
fn curve(level: u8) -> f32 {
    (level.clamp(1, MAX_LEVEL) as f32 - 1.0) / (MAX_LEVEL as f32 - 1.0)
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t.clamp(0.0, 1.0)
}

/// Body the hill solver integrates. Every field is in SI units.
#[derive(Clone, Copy, Debug)]
pub struct Prepared {
    pub mass: f32,
    pub inertia: f32,
    pub torque: f32,
    pub motor_cap: f32,
    pub vmax: f32,
    pub grip: f32,
    pub fuel_max: f32,
    pub fuel_burn: f32,
    pub idle_burn: f32,
    pub density: f32,
    /// Chassis-relative spring constant (kept for the upgrade curves' tests).
    pub spring: f32,
    pub damp: f32,
    /// Dumped wheel body masses, one per wheel, and the joint constants:
    /// 2*pi*springFrequency (rad/s) and the dumped damping ratio.
    pub wheel_mass: [f32; MAX_WHEELS],
    pub omega: f32,
    pub zeta: f32,
    pub drag: f32,
    pub angular_drag: f32,
    pub air_turn: f32,
    pub gravity: f32,
    pub half_w: f32,
    pub half_h: f32,
    /// Rear axle contact point height below the chassis origin, used to place
    /// the car on the ground when a run starts.
    pub hang: f32,
    pub wheel_n: usize,
    pub wheel_x: [f32; MAX_WHEELS],
    pub wheel_y: [f32; MAX_WHEELS],
    pub wheel_r: [f32; MAX_WHEELS],
}

pub fn upgrade_cost(level: i32) -> u32 {
    UpgradeEffect {
        kind: UpgradeEffectKind::Acceleration,
        level: clamp_level(level),
        per_level: 0.04,
    }
    .next_cost_coins()
}

/// Apply garage levels and the optional grip part to the dumped spec.
pub fn prepare(index: usize, levels: UpgradeLevels, grip_part_on: bool) -> Prepared {
    let s = spec(index);

    // Total mass and inertia about the chassis origin (parallel axis).
    let mut mass = s.chassis_mass;
    let mut inertia = s.inertia;
    for i in 0..s.wheel_n {
        let m = s.wheel_mass[i];
        mass += m;
        inertia += m * (s.wheel_x[i] * s.wheel_x[i] + s.wheel_y[i] * s.wheel_y[i]);
    }
    mass = mass.max(0.05);
    inertia = inertia.max(0.01);

    // Engine: torque and top speed interpolate across the dumped curve.
    let accel = curve(levels.acceleration);
    let mut torque = lerp(s.torque_base, s.torque_max, accel);
    let vmax = lerp(s.speed_base, s.speed_max, curve(levels.top_speed));

    // Suspension: frequency is dumped, damping is a ratio in the dump. Values
    // above 1 (rally 3.6, racing truck 1.68) are outside the Box2D ratio
    // domain, so they saturate instead of making the spring rigid.
    let mut zeta = lerp(s.damping_base, s.damping_max, curve(levels.suspension));
    zeta = zeta.clamp(0.05, 1.2);
    let omega = std::f32::consts::TAU * s.spring_frequency;
    let wheel_share = mass / s.wheel_n.max(1) as f32;
    let spring = wheel_share * omega * omega;
    let damp = 2.0 * zeta * (spring * wheel_share).sqrt();

    // Grip: the dump's body_friction curve, Box2D default when absent (sled).
    let mut grip = lerp(s.grip_base, s.grip_max, curve(levels.grip));
    if grip <= 0.0 {
        grip = 0.2;
    }

    // Fuel: dumped tank size, burn rate derived from TANK_SECONDS.
    let mut fuel_capacity = lerp(s.fuel_base, s.fuel_max, curve(levels.fuel_tank));
    if fuel_capacity <= 0.0 {
        fuel_capacity = 0.0; // sled: no engine, no tank
    }
    let fuel_rate = if fuel_capacity > 0.0 {
        fuel_capacity / TANK_SECONDS
    } else {
        0.0
    };

    if grip_part_on {
        let part = grip_part();
        let mut boost = part.effect.grip_multiplier();
        if !boost.is_finite() || boost <= 0.0 {
            boost = 1.0;
        }
        grip *= boost;
        torque *= part.effect.torque_multiplier();
    }

    // Quadratic drag fitted so the dumped top speed is the terminal speed for
    // the dumped torque: k = (torque / radius) / vmax^2.
    let radius = s.wheel_r[0].max(0.05);
    let drag = if vmax > 0.1 {
        (torque.max(0.0) / radius) / (vmax * vmax)
    } else {
        0.0
    };

    let mut wheel_mass = [0.25f32; MAX_WHEELS];
    for i in 0..s.wheel_n.min(MAX_WHEELS) {
        wheel_mass[i] = s.wheel_mass[i].max(1e-3);
    }

    Prepared {
        mass,
        inertia,
        wheel_mass,
        omega,
        zeta,
        torque,
        motor_cap: s.motor_max_torque,
        vmax,
        grip,
        fuel_max: fuel_capacity,
        fuel_burn: fuel_rate,
        idle_burn: fuel_rate * 0.3,
        density: 1.0,
        spring,
        damp,
        drag,
        // Nothing in the dump fixes angular damping; a small value keeps the
        // body from tumbling forever once it leaves the ground.
        angular_drag: 0.25,
        air_turn: s.air_turn_rate.to_radians(),
        gravity: s.gravity,
        half_w: s.half_w,
        half_h: s.half_h,
        // Clearance of the lowest tire below the chassis origin: the tank and
        // the snowmobile hang their wheels at different heights.
        hang: (0..s.wheel_n)
            .map(|i| s.wheel_r[i] - s.wheel_y[i])
            .fold(f32::MIN, f32::max),
        wheel_n: s.wheel_n,
        wheel_x: s.wheel_x,
        wheel_y: s.wheel_y,
        wheel_r: s.wheel_r,
    }
}

/// Fuel helper shared with `physics::FuelSpec`.
pub fn fuel_spec(prepared: &Prepared) -> FuelSpec {
    FuelSpec {
        capacity: prepared.fuel_max,
        consumption_rate: prepared.fuel_burn,
        density: prepared.density,
        idle_consumption: prepared.idle_burn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_names_match_blob() {
        let names: Vec<_> = roster_blob().split('\n').collect();
        assert_eq!(names.len(), VEHICLE_ROSTER.len());
        for (i, name) in names.iter().enumerate() {
            assert_eq!(*name, vehicle_display_name(i));
            // Clone ids are not always the dump ids (clone "tank" is the
            // dump's "minitank"), so only require the roster to stay in step.
            assert!(!spec(i).real_id.is_empty());
        }
        assert!(VEHICLE_ROSTER.contains(&"moonlander"));
    }

    #[test]
    fn specs_come_from_the_dump() {
        let jeep = spec(0);
        assert_eq!(jeep.real_id, "jeep");
        // rube file: chassis massData-mass and wheel circle radius.
        assert!((jeep.chassis_mass - 2.273764).abs() < 1e-3);
        assert!((jeep.wheel_r[0] - 0.4).abs() < 1e-3);
        // definition: engine torque and top speed curves.
        assert!((jeep.torque_base - 11.0).abs() < 1e-3);
        assert!((jeep.speed_base - 31.0).abs() < 1e-3);
        assert!((jeep.fuel_tank - 40.0).abs() < 1e-3);
    }

    #[test]
    fn acceleration_upgrade_follows_the_dumped_curve() {
        // The jeep's torque is a flat 11 N*m in the dump (its engine upgrade
        // only moves top speed), the rally's is a 10 -> 25 curve.
        let jeep_stock = prepare(0, UpgradeLevels::clamped(1, 1, 1, 1, 1), false);
        let jeep_max = prepare(0, UpgradeLevels::clamped(20, 1, 1, 1, 1), false);
        assert!((jeep_stock.torque - jeep_max.torque).abs() < 1e-3);

        let rally_stock = prepare(5, UpgradeLevels::clamped(1, 1, 1, 1, 1), false);
        let rally_max = prepare(5, UpgradeLevels::clamped(20, 1, 1, 1, 1), false);
        assert!((rally_stock.torque - 10.0).abs() < 0.01, "{}", rally_stock.torque);
        assert!((rally_max.torque - 25.0).abs() < 0.01, "{}", rally_max.torque);
    }

    #[test]
    fn grip_part_raises_friction() {
        let plain = prepare(0, UpgradeLevels::clamped(1, 1, 1, 1, 1), false);
        let part = prepare(0, UpgradeLevels::clamped(1, 1, 1, 1, 1), true);
        assert!(part.grip >= plain.grip);
    }

    #[test]
    fn suspension_is_frequency_based() {
        let car = prepare(0, UpgradeLevels::clamped(1, 1, 1, 1, 1), false);
        // k = m_share * (2*pi*3Hz)^2
        let expected = (car.mass / 2.0) * (std::f32::consts::TAU * 3.0).powi(2);
        assert!((car.spring - expected).abs() / expected < 0.02, "spring={}", car.spring);
        assert!(car.damp > 0.0);
    }
}
