//! Offline hill-climb for the browser page.
//!
//! The vehicle itself comes from the dumps: `vehicles::prepare` returns a body
//! built from the rube Box2D scene (mass, inertia, wheel geometry, suspension
//! frequency/damping) and the vehicle definition (torque, top speed, fuel tank,
//! air turn rate), all in the dump's own units — kg, m, N*m, Hz, m/s.
//!
//! This file adds the only thing the dumps do not have for an offline run: a
//! height-field hill. Each wheel is a spring-damper against that terrain, with
//! the traction limit `mu * Fn` and the dumped top speed cutting the drive.
//! Fixed step is 60 Hz, matching `stepsPerSecond` in every rube scene; four
//! inner substeps of 1/240 keep the stiffest suspension (snowmobile, 13 Hz)
//! inside the stability limit.
//!
//! Cipher, loader and game-server modules from the dump are not linked: they
//! describe a packed binary and an online service, not this offline run.

mod economy;
mod level_data;
mod suspension;
mod physics;
mod tuning;
mod vehicle_data;
mod vehicles;

use economy::distance_reward_coins;
use physics::FuelSpec;
use vehicle_data::MAX_WHEELS;
use vehicles::{prepare, roster_blob, upgrade_cost, vehicle_count, UpgradeLevels};

const TERRAIN_N: usize = 1400;
const COIN_N: usize = 64;
/// Fixed simulation step, from the rube `stepsPerSecond`.
const STEP: f32 = 1.0 / 60.0;
/// Inner substeps per fixed step.
const SUBSTEPS: usize = 4;
/// Chassis alignment with the terrain: stiffness (1/s^2) and damping (1/s).
/// Stands in for the suspension travel the dumped scene has and this model
/// does not; see the note next to its use in `substep`.
const ALIGN_STIFFNESS: f32 = 90.0;
const ALIGN_DAMPING: f32 = 14.0;

/// State layout handed to JavaScript:
/// 0 x, 1 y, 2 angle, 3 vx, 4 vy, 5 fuel, 6 distance, 7 speed, 8 crashed,
/// 9 finished, 10 wheel count, 11 half width, 12 half height, 13 fuel max,
/// 14 flips, 15 reward, 16 vehicle id, 17 level index, 18 track end,
/// 19.. each wheel: x, y, spin, grounded, radius, then omega and vmax.
const WHEEL_BLOCK: usize = 19;
const OMEGA_SLOT: usize = WHEEL_BLOCK + MAX_WHEELS * 5;
/// Top speed in m/s, so the HUD's rpm dial can be scaled like the original.
const VMAX_SLOT: usize = OMEGA_SLOT + 1;
const STATE_N: usize = VMAX_SLOT + 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct Coin {
    x: f32,
    y: f32,
    active: f32,
}

#[derive(Clone, Copy)]
struct Wheel {
    /// Joint anchor in chassis-local metres (the dumped `anchorA`).
    local_x: f32,
    local_y: f32,
    radius: f32,
    /// Dumped wheel mass (`massData-mass` of the wheel body).
    mass: f32,
    /// Wheel-joint spring: m_w * (2*pi*springFrequency)^2.
    spring_k: f32,
    /// Wheel-joint damper: 2 * zeta * sqrt(spring_k * m_w).
    damp_c: f32,
    // Dynamic state: the wheel is its own body, not an offset of the chassis.
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    spin_rate: f32,
    angle: f32,
    grounded: f32,
}

#[derive(Clone, Copy)]
struct Vehicle {
    mass: f32,
    inertia: f32,
    torque: f32,
    motor_cap: f32,
    vmax: f32,
    grip: f32,
    fuel_max: f32,
    fuel_burn: f32,
    idle_burn: f32,
    density: f32,
    spring: f32,
    damp: f32,
    drag: f32,
    angular_drag: f32,
    air_turn: f32,
    half_w: f32,
    half_h: f32,
    hang: f32,
    gravity: f32,
    wheel_n: usize,
    wheels: [Wheel; MAX_WHEELS],
}

struct World {
    vehicle_id: i32,
    level: usize,
    gravity: f32,
    track_end: f32,
    x: f32,
    y: f32,
    angle: f32,
    vx: f32,
    vy: f32,
    omega: f32,
    fuel: f32,
    crashed: f32,
    finished: f32,
    crash_time: f32,
    gas: f32,
    brake_in: f32,
    flips: u32,
    spin_accum: f32,
    car: Vehicle,
    coins: [Coin; COIN_N],
}

const EMPTY_WHEEL: Wheel = Wheel {
    local_x: 0.0,
    local_y: 0.0,
    radius: 0.2,
    mass: 0.25,
    spring_k: 80.0,
    damp_c: 2.0,
    x: 0.0,
    y: 0.0,
    vx: 0.0,
    vy: 0.0,
    spin_rate: 0.0,
    angle: 0.0,
    grounded: 0.0,
};

static mut WORLD: World = World {
    vehicle_id: 0,
    level: 0,
    gravity: 10.0,
    track_end: 2200.0,
    x: 0.0,
    y: 4.0,
    angle: 0.0,
    vx: 0.0,
    vy: 0.0,
    omega: 0.0,
    fuel: 1.0,
    crashed: 0.0,
    finished: 0.0,
    crash_time: 0.0,
    gas: 0.0,
    brake_in: 0.0,
    flips: 0,
    spin_accum: 0.0,
    car: Vehicle {
        mass: 2.8,
        inertia: 1.4,
        torque: 11.0,
        motor_cap: 500.0,
        vmax: 31.0,
        grip: 0.7,
        fuel_max: 40.0,
        fuel_burn: 0.9,
        idle_burn: 0.27,
        density: 1.0,
        spring: 1000.0,
        damp: 23.0,
        drag: 0.009,
        angular_drag: 0.25,
        air_turn: 0.7,
        half_w: 1.2,
        half_h: 0.5,
        hang: 1.4,
        gravity: 10.0,
        wheel_n: 2,
        wheels: [EMPTY_WHEEL; MAX_WHEELS],
    },
    coins: [Coin { x: 0.0, y: 0.0, active: 0.0 }; COIN_N],
};

static mut TERRAIN: [f32; TERRAIN_N * 2] = [0.0; TERRAIN_N * 2];
static mut STATE: [f32; STATE_N] = [0.0; STATE_N];
static mut STEP_ACC: f32 = 0.0;
static mut UPGRADES: UpgradeLevels = UpgradeLevels {
    acceleration: 1,
    top_speed: 1,
    fuel_tank: 1,
    suspension: 1,
    grip: 1,
};
static mut GRIP_PART: bool = false;

fn car_from(prepared: vehicles::Prepared) -> Vehicle {
    let mut wheels = [EMPTY_WHEEL; MAX_WHEELS];
    for i in 0..prepared.wheel_n.min(MAX_WHEELS) {
        let m = prepared.wheel_mass[i];
        wheels[i] = Wheel {
            local_x: prepared.wheel_x[i],
            local_y: prepared.wheel_y[i],
            radius: prepared.wheel_r[i],
            mass: m,
            // Wheel-joint spring and damper, straight from the dumped
            // springFrequency / springDampingRatio of the `wheel` joint.
            spring_k: m * prepared.omega * prepared.omega,
            damp_c: 2.0 * prepared.zeta * (m * prepared.omega * prepared.omega * m).sqrt(),
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            spin_rate: 0.0,
            angle: 0.0,
            grounded: 0.0,
        };
    }
    Vehicle {
        mass: prepared.mass,
        inertia: prepared.inertia,
        torque: prepared.torque,
        motor_cap: prepared.motor_cap,
        vmax: prepared.vmax,
        grip: prepared.grip,
        fuel_max: prepared.fuel_max,
        fuel_burn: prepared.fuel_burn,
        idle_burn: prepared.idle_burn,
        density: prepared.density,
        spring: prepared.spring,
        damp: prepared.damp,
        drag: prepared.drag,
        angular_drag: prepared.angular_drag,
        air_turn: prepared.air_turn,
        half_w: prepared.half_w,
        half_h: prepared.half_h,
        hang: prepared.hang,
        gravity: prepared.gravity,
        wheel_n: prepared.wheel_n.min(MAX_WHEELS).max(1),
        wheels,
    }
}

/// The level the run is on. Terrain is the game's own profile: every level
/// definition carries an `algorithm.expr` that Hill Climb Racing 2 evaluates per
/// x, and `tools/extract_levels.py` bakes those samples into `level_data`.
pub fn level_count() -> usize {
    level_data::LEVELS.len()
}

pub fn level_spec(index: usize) -> &'static level_data::LevelSpec {
    &level_data::LEVELS[index.min(level_data::LEVELS.len() - 1)]
}

/// Vehicle info for the UI: one `real_id|display_name|half_w` line per vehicle,
/// in roster order. `real_id` is the folder name the APK stores the art under.
pub fn vehicle_info_blob() -> &'static str {
    static BLOB: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BLOB.get_or_init(|| {
        vehicle_data::VEHICLE_SPECS
            .iter()
            .map(|s| format!("{}|{}|{:.4}", s.real_id, s.display_name, s.half_w))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// Level list for the UI: one `id|family|distance|gravity` line per level.
pub fn levels_blob() -> &'static str {
    static BLOB: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BLOB.get_or_init(|| {
        level_data::LEVELS
            .iter()
            .map(|l| format!("{}|{}|{:.0}|{:.0}", l.id, l.family, l.distance, l.gravity))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// Ground height and slope at `x`, interpolated from the baked profile.
fn terrain_sample(level: usize, x: f32) -> (f32, f32) {
    let track = level_spec(level).track;
    let n = track.len() / 2;
    if n == 0 {
        return (0.0, 0.0);
    }
    if x <= track[0] {
        return (track[1], 0.0);
    }
    let last = n - 1;
    if x >= track[last * 2] {
        return (track[last * 2 + 1], 0.0);
    }
    // Binary search for the segment containing x.
    let (mut lo, mut hi) = (0usize, last);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if track[mid * 2] <= x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let x0 = track[lo * 2];
    let y0 = track[lo * 2 + 1];
    let x1 = track[hi * 2];
    let y1 = track[hi * 2 + 1];
    let dx = (x1 - x0).max(1e-6);
    let t = (x - x0) / dx;
    (y0 + (y1 - y0) * t, (y1 - y0) / dx)
}

fn rotate(angle: f32, x: f32, y: f32) -> (f32, f32) {
    let (s, c) = angle.sin_cos();
    (x * c - y * s, x * s + y * c)
}

fn write_terrain(world: &World) {
    let step = (world.track_end + 80.0) / (TERRAIN_N as f32 - 1.0);
    unsafe {
        for i in 0..TERRAIN_N {
            let x = -20.0 + i as f32 * step;
            let (y, _) = terrain_sample(world.level, x);
            TERRAIN[i * 2] = x;
            TERRAIN[i * 2 + 1] = y;
        }
    }
}

fn place_coins(world: &mut World) {
    let mut x = 55.0;
    let lift = world.car.hang + world.car.wheels[0].radius * 0.5;
    for coin in &mut world.coins {
        if x >= world.track_end - 30.0 {
            *coin = Coin { x: 0.0, y: 0.0, active: 0.0 };
            continue;
        }
        let (ground, _) = terrain_sample(world.level, x);
        *coin = Coin { x, y: ground + lift, active: 1.0 };
        // Roughly one pickup per ten seconds of driving at cruising speed.
        x += 110.0;
    }
}

fn sync_state(world: &World) {
    let mut speed = (world.vx * world.vx + world.vy * world.vy).sqrt();
    if !speed.is_finite() {
        speed = 0.0;
    }
    unsafe {
        STATE[0] = world.x;
        STATE[1] = world.y;
        STATE[2] = world.angle;
        STATE[3] = world.vx;
        STATE[4] = world.vy;
        STATE[5] = world.fuel;
        STATE[6] = world.x.max(0.0);
        STATE[7] = speed;
        STATE[8] = world.crashed;
        STATE[9] = world.finished;
        STATE[10] = world.car.wheel_n as f32;
        STATE[11] = world.car.half_w;
        STATE[12] = world.car.half_h;
        STATE[13] = world.car.fuel_max;
        STATE[14] = world.flips as f32;
        STATE[15] = distance_reward_coins(world.x.max(0.0)) as f32;
        STATE[16] = world.vehicle_id as f32;
        STATE[17] = world.level as f32;
        STATE[18] = world.track_end;
        for i in 0..MAX_WHEELS {
            let wheel = world.car.wheels[i];
            let base = WHEEL_BLOCK + i * 5;
            // Wheel bodies carry their own world position now; the renderer
            // draws them where the joint actually put them.
            STATE[base] = wheel.x;
            STATE[base + 1] = wheel.y;
            STATE[base + 2] = wheel.angle;
            STATE[base + 3] = wheel.grounded;
            STATE[base + 4] = wheel.radius;
        }
        STATE[OMEGA_SLOT] = world.omega;
        STATE[VMAX_SLOT] = world.car.vmax;
    }
}

#[no_mangle]
pub extern "C" fn set_upgrades(accel: i32, top_speed: i32, fuel: i32, suspension: i32, grip: i32) {
    unsafe {
        UPGRADES = UpgradeLevels::clamped(accel, top_speed, fuel, suspension, grip);
    }
}

#[no_mangle]
pub extern "C" fn set_grip_part(enabled: i32) {
    unsafe { GRIP_PART = enabled != 0; }
}

#[no_mangle]
pub extern "C" fn upgrade_cost_at(level: i32) -> u32 {
    upgrade_cost(level)
}

#[no_mangle]
pub extern "C" fn reward_for_distance(metres: f32) -> u32 {
    if metres.is_finite() { distance_reward_coins(metres.max(0.0)) } else { 0 }
}

#[no_mangle]
pub extern "C" fn roster_len() -> usize {
    vehicle_count()
}

#[no_mangle]
pub extern "C" fn roster_ptr() -> *const u8 {
    roster_blob().as_ptr()
}

#[no_mangle]
pub extern "C" fn roster_bytes() -> usize {
    roster_blob().len()
}

#[no_mangle]
pub extern "C" fn vehicle_info_ptr() -> *const u8 {
    vehicle_info_blob().as_ptr()
}

#[no_mangle]
pub extern "C" fn vehicle_info_bytes() -> usize {
    vehicle_info_blob().len()
}

#[no_mangle]
pub extern "C" fn levels_len() -> usize {
    level_count()
}

#[no_mangle]
pub extern "C" fn levels_ptr() -> *const u8 {
    levels_blob().as_ptr()
}

#[no_mangle]
pub extern "C" fn levels_bytes() -> usize {
    levels_blob().len()
}

#[no_mangle]
pub extern "C" fn start(vehicle_id: i32, level_id: i32) {
    let vehicle_id = vehicle_id.clamp(0, vehicle_count() as i32 - 1);
    let level_id = (level_id.max(0) as usize).min(level_count() - 1);
    let spec = level_spec(level_id);
    let (levels, grip) = unsafe { (UPGRADES, GRIP_PART) };
    let prepared = prepare(vehicle_id as usize, levels, grip);
    let car = car_from(prepared);
    // The level's own gravity (moon runs at 6, underwater at 8 m/s^2).
    let gravity = spec.gravity;
    let (ground, _) = terrain_sample(level_id, 2.0);
    let mut world = World {
        vehicle_id,
        level: level_id,
        gravity,
        track_end: spec.track_length,
        x: 2.0,
        // Chassis origin sits one wheel-hang above the surface.
        y: ground + car.hang + 0.05,
        angle: 0.0,
        vx: 0.0,
        vy: 0.0,
        omega: 0.0,
        fuel: car.fuel_max,
        crashed: 0.0,
        finished: 0.0,
        crash_time: 0.0,
        gas: 0.0,
        brake_in: 0.0,
        flips: 0,
        spin_accum: 0.0,
        car,
        coins: [Coin { x: 0.0, y: 0.0, active: 0.0 }; COIN_N],
    };
    // Wheels start on their joint anchors (zero spring displacement). Gravity
    // then sags the chassis down onto them, exactly as the dumped scene does.
    for i in 0..world.car.wheel_n.min(MAX_WHEELS) {
        let w = world.car.wheels[i];
        let (rx, ry) = rotate(world.angle, w.local_x, w.local_y);
        world.car.wheels[i].x = world.x + rx;
        world.car.wheels[i].y = world.y + ry;
    }
    place_coins(&mut world);
    write_terrain(&world);
    sync_state(&world);
    unsafe {
        WORLD = world;
        STEP_ACC = 0.0;
    }
}

#[no_mangle]
pub extern "C" fn set_input(gas: f32, brake: f32) {
    unsafe {
        WORLD.gas = gas.clamp(0.0, 1.0);
        WORLD.brake_in = brake.clamp(0.0, 1.0);
    }
}

fn substep(world: &mut World, dt: f32) {
    if world.crashed > 0.5 || world.finished > 0.5 {
        return;
    }
    let tanked = world.car.fuel_max > 0.0;
    let gas = if !tanked || world.fuel > 0.2 { world.gas } else { 0.0 };
    if tanked {
        let fuel_spec = FuelSpec {
            capacity: world.car.fuel_max,
            consumption_rate: world.car.fuel_burn,
            density: world.car.density,
            idle_consumption: world.car.idle_burn,
        };
        fuel_spec.consume(&mut world.fuel, gas, dt);
    }

    // Chassis body plus one dynamic body per wheel, joined by the dumped
    // `wheel` joints (spring along the chassis' local +Y, motor on the spin).
    suspension::step_vehicle(world, dt, gas);

    let grounded_any = (0..world.car.wheel_n.min(MAX_WHEELS))
        .any(|i| world.car.wheels[i].grounded > 0.5);

    for coin in &mut world.coins {
        if coin.active < 0.5 {
            continue;
        }
        let dx = world.x - coin.x;
        let dy = world.y - coin.y;
        if dx * dx + dy * dy < 1.6 * 1.6 {
            coin.active = 0.0;
            // A pickup is worth a few seconds of throttle, not a full tank:
            // 12% of the dumped capacity against the 45 s burn rate.
            if world.car.fuel_max > 0.0 {
                world.fuel = (world.fuel + world.car.fuel_max * 0.12).min(world.car.fuel_max);
            }
        }
    }

    // Upside down on the ground for long enough ends the run.
    let flipped = world.angle.abs() > 2.4;
    if flipped && grounded_any {
        world.crash_time += dt;
        if world.crash_time > 0.35 {
            world.crashed = 1.0;
        }
    } else {
        world.crash_time = 0.0;
    }
    if world.x >= world.track_end {
        world.finished = 1.0;
        world.vx = 0.0;
    }
    if !world.x.is_finite() || !world.y.is_finite() || world.y < -30.0 {
        world.crashed = 1.0;
        world.vx = 0.0;
        world.vy = 0.0;
    }
}



#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn tick(dt: f32) {
    let frame = if dt.is_finite() { dt.clamp(0.0, 0.25) } else { 0.0 };
    unsafe {
        // Fixed 60 Hz steps, at most four per frame to survive a stall.
        STEP_ACC = (STEP_ACC + frame).min(STEP * 4.0);
        while STEP_ACC >= STEP {
            STEP_ACC -= STEP;
            for _ in 0..SUBSTEPS {
                substep(&mut WORLD, STEP / SUBSTEPS as f32);
            }
        }
        sync_state(&WORLD);
    }
}

#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn state_ptr() -> *const f32 {
    unsafe { STATE.as_ptr() }
}

#[no_mangle]
pub extern "C" fn state_len() -> usize {
    STATE_N
}

#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn terrain_ptr() -> *const f32 {
    unsafe { TERRAIN.as_ptr() }
}

#[no_mangle]
pub extern "C" fn terrain_len() -> usize {
    TERRAIN_N
}

#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn coins_ptr() -> *const f32 {
    unsafe { WORLD.coins.as_ptr() as *const f32 }
}

#[no_mangle]
pub extern "C" fn coins_len() -> usize {
    COIN_N
}

#[cfg(test)]
mod tests {
    use super::*;

    // The simulation lives in a global (the wasm ABI has no self pointer), so
    // the tests must not run concurrently or they stomp each other's world.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn run(vehicle: i32, level: i32, seconds: f32) -> (f32, f32, f32) {
        start(vehicle, level);
        set_input(1.0, 0.0);
        let frames = (seconds * 60.0) as usize;
        for _ in 0..frames {
            tick(STEP);
        }
        unsafe { (STATE[0], STATE[7], STATE[1]) }
    }

    #[test]
    fn jeep_accelerates_like_the_dumped_spec() {
        let _guard = serial();
        let (x, speed, y) = run(0, 0, 4.0);
        assert!(x > 25.0, "expected forward progress, x={x}");
        assert!(speed > 10.0, "jeep should build speed, got {speed}");
        // Dumped top speed for the jeep is 31 m/s; traction and drag keep the
        // car under it rather than a hard clamp doing the work.
        assert!(speed <= 31.5, "speed must respect the dumped vmax, got {speed}");
        assert!(y.is_finite(), "state exploded");
    }

    #[test]
    fn every_vehicle_moves_and_stays_finite() {
        let _guard = serial();
        for id in 0..vehicle_count() as i32 {
            let (x, speed, y) = run(id, 0, 4.0);
            assert!(x.is_finite() && y.is_finite() && speed.is_finite(), "vehicle {id} exploded");
            // The sled has no engine, so it may stay put; the rest must move.
            if id != 13 {
                assert!(x > 5.0, "vehicle {id} did not move, x={x}");
            }
        }
    }

    #[test]
    fn tank_drives_on_all_six_dumped_wheels() {
        let _guard = serial();
        start(6, 0);
        unsafe { assert!((STATE[10] - 6.0).abs() < 1e-6, "wheel count {}", STATE[10]) };
        set_input(1.0, 0.0);
        for _ in 0..120 {
            tick(STEP);
        }
        unsafe {
            assert!(STATE[0] > 3.0, "tank did not move, x={}", STATE[0]);
        }
    }

    #[test]
    fn moon_level_uses_its_own_gravity_and_burns_fuel() {
        let _guard = serial();
        let moon = level_data::LEVELS
            .iter()
            .position(|l| l.family == "moon")
            .expect("moon level baked");
        // The level definition carries gravity [0, -6]; the solver must honour it.
        assert_eq!(level_spec(moon).gravity, -6.0);
        start(1, moon as i32);
        unsafe { assert!(STATE[13] > 0.0, "tank size missing") };
        set_input(1.0, 0.0);
        tick(STEP);
        unsafe {
            assert!(STATE[5] < STATE[13], "fuel should be spent under throttle");
            assert!((STATE[17] - moon as f32).abs() < 1e-6);
        }
    }

    #[test]
    fn terrain_matches_the_baked_level_profile() {
        let _guard = serial();
        // The baked polyline is the game's own expression, so sampling it at a
        // vertex must return that vertex exactly.
        let spec = level_spec(0);
        assert!(spec.track.len() >= 8, "track too short");
        let (x, y) = (spec.track[10], spec.track[11]);
        let (got, _) = terrain_sample(0, x);
        assert!((got - y).abs() < 1e-3, "x={} expected {} got {}", x, y, got);
    }

    #[test]
    fn suspension_comes_from_the_dumped_frequency() {
        let _guard = serial();
        let car = prepare(0, UpgradeLevels::clamped(1, 1, 1, 1, 1), false);
        let expected = (car.mass / car.wheel_n as f32)
            * (std::f32::consts::TAU * vehicle_data::VEHICLE_SPECS[0].spring_frequency).powi(2);
        assert!((car.spring - expected).abs() / expected < 0.01, "spring={}", car.spring);
    }

    #[test]
    fn distance_reward_is_exported_math() {
        assert_eq!(reward_for_distance(100.0), 20);
        assert_eq!(roster_len(), 14);
    }
}
