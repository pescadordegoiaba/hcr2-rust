//! Articulated vehicle step — chassis body plus one dynamic body per wheel.
//!
//! The dumped `.rube` scenes join the wheels with joints of `type: "wheel"`:
//!
//! ```json
//! { "type": "wheel", "bodyA": 18, "bodyB": 17,
//!   "anchorA": { "x": 1.15, "y": -0.999996 },
//!   "localAxisA": { "x": 0, "y": 1 },
//!   "springFrequency": 3, "springDampingRatio": 0.22,
//!   "enableMotor": true, "maxMotorTorque": 500, "motorSpeed": 0 }
//! ```
//!
//! That is a Box2D wheel joint: the wheel slides along the chassis' local +Y
//! axis on a spring, cannot move perpendicular to it, and a motor drives its
//! spin. This module implements exactly those three things instead of the
//! earlier rigid-wheel approximation, so the body squats, dives and rebounds
//! like the original: static sag is the dumped mass over the dumped spring,
//! and the wheels travel relative to the chassis instead of being welded to it.
//!
//! Solver: sequential impulses. Each substep applies gravity once, the wheel
//! motor and the spring as explicit forces, then runs `ITER` sweeps that
//! alternate the perpendicular joint constraint with the ground contact, and
//! finally integrates. Impulses on a wheel are transmitted to the chassis in
//! the next sweep through the joint, which is what makes load transfer (and
//! therefore wheelies and landings) come out of the geometry.

use crate::{rotate, terrain_sample, World, MAX_WHEELS};

/// Sweeps of joint + contact per substep. Four substeps per 1/60 s frame means
/// 24 sweeps per frame, enough for the stiffest dumped spring (13 Hz).
const ITER: usize = 8;

/// Ground restitution: tires do not bounce in the original.
const RESTITUTION: f32 = 0.02;
/// Penetration recovery per second (Baumgarte factor / dt).
const BAUMGARTE: f32 = 0.35;
/// Cap on the recoverable penetration velocity, m/s.
const MAX_BIAS: f32 = 2.5;
/// Chassis belly contact friction against the ground.
const BODY_FRICTION: f32 = 0.4;

pub(crate) fn step_vehicle(world: &mut World, dt: f32, gas: f32) {
    let car = world.car;
    let n = car.wheel_n.min(MAX_WHEELS);
    let mass = car.mass.max(1e-3);
    let inv_m = 1.0 / mass;
    let inv_i = 1.0 / car.inertia.max(1e-4);
    let g = world.gravity;

    // ---- gravity (semi-implicit: velocity first, position later) ----------
    world.vy += g * dt;
    for i in 0..n {
        world.car.wheels[i].vy += g * dt;
    }

    // ---- per-wheel constant geometry, refreshed once per substep ----------
    // anchor/axis are recomputed after integration too, but the solver needs
    // them stable while it sweeps.
    let mut anchor = [(0.0f32, 0.0f32); MAX_WHEELS];
    let mut ra = [(0.0f32, 0.0f32); MAX_WHEELS];
    let mut axis = [(0.0f32, 0.0f32); MAX_WHEELS];
    let mut perp = [(0.0f32, 0.0f32); MAX_WHEELS];
    for i in 0..n {
        let w = world.car.wheels[i];
        let (rax, ray) = rotate(world.angle, w.local_x, w.local_y);
        ra[i] = (rax, ray);
        anchor[i] = (world.x + rax, world.y + ray);
        axis[i] = rotate(world.angle, 0.0, 1.0);
        perp[i] = rotate(world.angle, 1.0, 0.0);
    }

    // ---- motor and suspension spring -------------------------------------
    for i in 0..n {
        let w = world.car.wheels[i];
        let mw = w.mass.max(1e-3);
        let inertia_w = 0.5 * mw * w.radius * w.radius;
        let (ux, uy) = axis[i];
        let (ax, ay) = anchor[i];
        let (rax, ray) = ra[i];
        let cvx = world.vx - world.omega * ray;
        let cvy = world.vy + world.omega * rax;

        // Spring: displacement along the axis, positive when the wheel has
        // been pushed up towards the chassis (compression).
        let s = (w.x - ax) * ux + (w.y - ay) * uy;
        let vs = (w.vx - cvx) * ux + (w.vy - cvy) * uy;
        let static_load = mw * -g;
        let force = (-w.spring_k * s - w.damp_c * vs)
            .clamp(-static_load * 25.0 - 1.0, static_load * 25.0 + 1.0);

        // Apply to the wheel, and the reaction to the chassis at the anchor.
        world.car.wheels[i].vx += force * inv_of(mw) * dt * ux;
        world.car.wheels[i].vy += force * inv_of(mw) * dt * uy;
        world.vx -= force * inv_m * dt * ux;
        world.vy -= force * inv_m * dt * uy;
        world.omega -= (rax * (force * uy) - ray * (force * ux)) * inv_i * dt;

        // Motor: the engine torque from `engine_params` drives the wheel spin
        // towards the speed implied by the top speed. The joint's
        // `maxMotorTorque` (500 N*m on the jeep) is only a ceiling in the
        // original and never binds, so it caps rather than sets the torque.
        // Forward travel is clockwise rotation, i.e. negative spin.
        let target = -gas * car.vmax / w.radius.max(0.05);
        let want = target - w.spin_rate;
        // The dumped torque is the engine's total, shared across the driven
        // wheels (both on the jeep, all six on the minitank).
        let driven = n.max(1) as f32;
        let torque = (car.torque.min(car.motor_cap).max(0.0)) / driven;
        let max_dw = torque * dt / inertia_w;
        let applied = want.clamp(-max_dw, max_dw);
        world.car.wheels[i].spin_rate += applied;
        // Reaction torque on the chassis (a pure couple): this is what makes
        // the nose lift under throttle and dive under braking.
        world.omega -= applied * inertia_w * inv_i;
    }

    // ---- iterative solve: perpendicular joint constraint + contact --------
    for _ in 0..ITER {
        for i in 0..n {
            // Geometry is recomputed inside: the positional correction moves the
            // chassis, so a value cached before the sweep would be stale.
            solve_joint(world, i, inv_m, inv_i);
        }
        for i in 0..n {
            solve_contact(world, i, dt);
        }
    }

    // ---- chassis belly contact, so a flipped car does not sink ------------
    // Only once the body is on its side or roof: the dumped `half_h` is the
    // bounding box of the whole sprite, so as a collision box it reaches below
    // the wheels (the bus is 1.5 m below its origin with wheels hanging 1.24 m)
    // and would hold the car up before the tires ever touched the ground.
    if world.angle.abs() > 1.0 {
        solve_body_contact(world, dt, inv_m, inv_i);
    }

    // ---- drag, air control -----------------------------------------------
    let speed = (world.vx * world.vx + world.vy * world.vy).sqrt();
    if speed > 0.05 {
        // F = k v^2 along the velocity vector; k is fitted in `prepare` so the
        // dumped top speed is the terminal speed.
        let decel = (car.drag * speed * speed * inv_m * dt).min(speed);
        world.vx -= world.vx / speed * decel;
        world.vy -= world.vy / speed * decel;
    }

    let grounded = (0..n).any(|i| world.car.wheels[i].grounded > 0.5);
    if grounded {
        // Weight transfer is now real: nothing to fake here.
    } else {
        let dir = gas - world.brake_in;
        world.omega += dir * car.air_turn * dt;
        world.omega = world.omega.clamp(-car.air_turn * 1.6, car.air_turn * 1.6);
    }

    // ---- integrate -------------------------------------------------------
    world.x += world.vx * dt;
    world.y += world.vy * dt;
    let prev = world.angle;
    world.angle += world.omega * dt;
    world.spin_accum += world.angle - prev;
    if world.angle > std::f32::consts::PI {
        world.angle -= std::f32::consts::TAU;
    } else if world.angle < -std::f32::consts::PI {
        world.angle += std::f32::consts::TAU;
    }
    if world.spin_accum.abs() >= std::f32::consts::TAU {
        world.spin_accum -= world.spin_accum.signum() * std::f32::consts::TAU;
        world.flips += 1;
    }

    for i in 0..n {
        let w = &mut world.car.wheels[i];
        w.x += w.vx * dt;
        w.y += w.vy * dt;
        w.angle += w.spin_rate * dt;
    }

    if world.x < 0.5 && world.vx < 0.0 {
        world.x = 0.5;
        world.vx = 0.0;
    }
}

fn inv_of(m: f32) -> f32 {
    1.0 / m.max(1e-4)
}

/// Perpendicular part of the wheel joint: the wheel centre must stay on the
/// line through the chassis anchor along the suspension axis.
fn solve_joint(world: &mut World, i: usize, inv_m: f32, inv_i: f32) {
    let w = world.car.wheels[i];
    let mw = w.mass.max(1e-3);
    let inv_mw = inv_of(mw);
    // Anchor and axis in world space, from the chassis' current transform.
    let (rax, ray) = rotate(world.angle, w.local_x, w.local_y);
    let anchor = (world.x + rax, world.y + ray);
    let (px, py) = rotate(world.angle, 1.0, 0.0);

    let cvx = world.vx - world.omega * ray;
    let cvy = world.vy + world.omega * rax;
    let rvx = w.vx - cvx;
    let rvy = w.vy - cvy;
    let vp = rvx * px + rvy * py;

    // Effective mass along the perpendicular direction, including the chassis
    // rotation arm (cross(r, p)).
    let rn = rax * py - ray * px;
    let denom = inv_mw + inv_m + rn * rn * inv_i;
    let j = -vp / denom;

    world.car.wheels[i].vx += j * px * inv_mw;
    world.car.wheels[i].vy += j * py * inv_mw;
    world.vx -= j * px * inv_m;
    world.vy -= j * py * inv_m;
    world.omega -= (rax * (j * py) - ray * (j * px)) * inv_i;

    // Positional drift: without this the wheel slowly slides sideways off the
    // axis (velocity-only constraints do not correct accumulated error) and the
    // spring then pushes at a bogus angle, which tips the body over.
    let w2 = world.car.wheels[i];
    let (ax, ay) = anchor;
    let cs = (w2.x - ax) * px + (w2.y - ay) * py;
    if cs.abs() > 1e-5 {
        let total = inv_mw + inv_m;
        let wheel_share = inv_mw / total;
        // 40% relaxation keeps the correction from fighting the contact solve.
        let corr = cs * 0.25;
        world.car.wheels[i].x -= corr * px * wheel_share;
        world.car.wheels[i].y -= corr * py * wheel_share;
        world.x += corr * px * (1.0 - wheel_share);
        world.y += corr * py * (1.0 - wheel_share);
    }
}

/// Ground contact for one wheel: normal impulse plus Coulomb friction, which
/// also spins the wheel up and therefore produces rolling.
fn solve_contact(world: &mut World, i: usize, dt: f32) {
    let car = world.car;
    let w = world.car.wheels[i];
    let mw = w.mass.max(1e-3);
    let inv_mw = inv_of(mw);
    let inertia = 0.5 * mw * w.radius * w.radius;
    let inv_iw = 1.0 / inertia.max(1e-6);

    let (ground, slope) = terrain_sample(world.level, w.x);
    let inv_len = (slope * slope + 1.0).sqrt();
    let nx = -slope / inv_len;
    let ny = 1.0 / inv_len;

    let dist = (w.y - ground) * ny;
    let penetration = w.radius - dist;
    if penetration < 0.0 {
        world.car.wheels[i].grounded = 0.0;
        return;
    }
    world.car.wheels[i].grounded = 1.0;

    // Normal: remove the closing velocity and push out of the surface.
    let vn = w.vx * nx + w.vy * ny;
    let bias = (penetration * BAUMGARTE / dt).min(MAX_BIAS);
    let jn = -(vn * (1.0 + RESTITUTION) - bias) * mw;
    if jn > 0.0 {
        world.car.wheels[i].vx += jn * inv_mw * nx;
        world.car.wheels[i].vy += jn * inv_mw * ny;
        // The chassis feels it through the joint on the next sweep. Injecting
        // an extra share here double-counted the impulse and pitched the body.
    }

    // Friction at the contact patch. Its velocity includes the wheel's spin,
    // so a spinning wheel drives the car and a rolling wheel keeps it going.
    let tx = ny;
    let ty = -nx;
    let w2 = world.car.wheels[i];
    let contact_v = (w2.vx + w2.spin_rate * w2.radius * ny) * tx
        + (w2.vy - w2.spin_rate * w2.radius * nx) * ty;
    // Effective mass for tangential motion: 1/m + r^2/I, with I = m r^2 / 2.
    let denom = inv_mw + w2.radius * w2.radius * inv_iw;
    let mut jt = -contact_v / denom;
    let limit = car.grip * jn.max(0.0);
    jt = jt.clamp(-limit, limit);

    world.car.wheels[i].vx += jt * tx * inv_mw;
    world.car.wheels[i].vy += jt * ty * inv_mw;
    // Torque from the friction force acting at the rim.
    world.car.wheels[i].spin_rate += jt * w2.radius * inv_iw;

    // Brake: torque opposing spin, capped by the tire's grip.
    if world.brake_in > 0.01 && w2.spin_rate.abs() > 0.01 {
        let brake_torque = world.brake_in * car.motor_cap * 0.8;
        let dw = -w2.spin_rate.signum() * (brake_torque / inertia) * dt;
        let applied = if dw.abs() > w2.spin_rate.abs() { -w2.spin_rate } else { dw };
        world.car.wheels[i].spin_rate += applied;
        world.omega -= applied * inertia / car.inertia.max(1e-4);
    }
}

/// Box contact for the chassis itself, sampled at its four corners: without it
/// a car that lands on its roof sinks through the ground.
fn solve_body_contact(world: &mut World, dt: f32, inv_m: f32, inv_i: f32) {
    let car = world.car;
    let hw = car.half_w.max(0.05);
    let hh = car.half_h.max(0.05);
    let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
    let mut worst: Option<(f32, f32, f32, f32, f32)> = None;

    for (lx, ly) in corners {
        let (rx, ry) = rotate(world.angle, lx, ly);
        let px = world.x + rx;
        let py = world.y + ry;
        let (ground, slope) = terrain_sample(world.level, px);
        let inv_len = (slope * slope + 1.0).sqrt();
        let nx = -slope / inv_len;
        let ny = 1.0 / inv_len;
        let pen = (ground - py) * ny;
        if pen > 0.0 {
            let score = pen;
            if worst.map(|w| score > w.0).unwrap_or(true) {
                worst = Some((score, px, py, rx, ry));
            }
        }
    }

    let Some((pen, _px, _py, rx, ry)) = worst else { return };
    let (ground, slope) = terrain_sample(world.level, world.x);
    let inv_len = (slope * slope + 1.0).sqrt();
    let nx = -slope / inv_len;
    let ny = 1.0 / inv_len;

    let vx = world.vx - world.omega * ry;
    let vy = world.vy + world.omega * rx;
    let vn = vx * nx + vy * ny;
    let rn = rx * ny - ry * nx;
    let denom = inv_m + rn * rn * inv_i;
    let bias = (pen * BAUMGARTE / dt).min(MAX_BIAS);
    let jn = (-(vn * (1.0 + RESTITUTION) - bias) / denom).max(0.0);

    world.vx += jn * nx * inv_m;
    world.vy += jn * ny * inv_m;
    world.omega += (rx * (jn * ny) - ry * (jn * nx)) * inv_i;

    // Belly friction, so a crashed car slides to a stop instead of skating.
    let tx = ny;
    let ty = -nx;
    let vt = vx * tx + vy * ty;
    let rt = rx * ty - ry * tx;
    let denom_t = inv_m + rt * rt * inv_i;
    let jt = (-vt / denom_t).clamp(-BODY_FRICTION * jn, BODY_FRICTION * jn);
    world.vx += jt * tx * inv_m;
    world.vy += jt * ty * inv_m;
    world.omega += (rx * (jt * ty) - ry * (jt * tx)) * inv_i;
    let _ = ground;
}
