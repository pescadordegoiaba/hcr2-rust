#![allow(dead_code)]
//! Tuning parts and garage upgrades. Effects mutate chassis, wheels and
//! the engine before a run starts.

use crate::physics::{ChassisSpec, EngineSpec, WheelSpec};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Rarity {
    Common,
    Rare,
    Epic,
    Premium,
    Mythic,
}

impl Rarity {
    pub fn name(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Rare => "rare",
            Self::Epic => "epic",
            Self::Premium => "premium",
            Self::Mythic => "mythic",
        }
    }
    pub fn weight(self) -> u32 {
        match self {
            Self::Common => 100,
            Self::Rare => 30,
            Self::Epic => 10,
            Self::Premium => 5,
            Self::Mythic => 1,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TuningPartEffectKind {
    Acceleration,
    AccelerationOffset,
    AirControl,
    AngularAcceleration,
    AutoAirControl,
    BrakeTorque,
    CenterOfMass,
    CollidableMass,
    Downforce,
    EngineParams,
    Force,
    FuelConsumption,
    Glide,
    Impulse,
    JumpImpulse,
    MaxAngularVel,
    PartMass,
    Refuel,
    Thruster,
    WheelFriction,
    DisableDeathFromHeadCollision,
}

impl TuningPartEffectKind {
    pub fn name(self) -> &'static str {
        use TuningPartEffectKind::*;
        match self {
            Acceleration => "acceleration",
            AccelerationOffset => "acceleration_offset",
            AirControl => "air_control",
            AngularAcceleration => "angular_acceleration",
            AutoAirControl => "auto_air_control",
            BrakeTorque => "brake_torque",
            CenterOfMass => "center_of_mass",
            CollidableMass => "collidable_mass",
            Downforce => "downforce",
            EngineParams => "engine_params",
            Force => "force",
            FuelConsumption => "fuel_consumption",
            Glide => "glide",
            Impulse => "impulse",
            JumpImpulse => "jump_impulse",
            MaxAngularVel => "max_angular_vel",
            PartMass => "part_mass",
            Refuel => "refuel",
            Thruster => "thruster",
            WheelFriction => "wheel_friction",
            DisableDeathFromHeadCollision => "disable_death_head_collision",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TuningPartEffect {
    pub kind: TuningPartEffectKind,
    pub magnitude: f32,
    pub offset: Option<f32>,
}

impl TuningPartEffect {
    pub fn apply_chassis(&self, ch: &mut ChassisSpec) {
        match self.kind {
            TuningPartEffectKind::CenterOfMass => {
                ch.center_of_mass.x += self.magnitude;
                if let Some(y) = self.offset {
                    ch.center_of_mass.y += y;
                }
            }
            TuningPartEffectKind::CollidableMass => ch.collidable_mass += self.magnitude,
            TuningPartEffectKind::PartMass => ch.mass += self.magnitude,
            TuningPartEffectKind::Downforce => ch.drag_coeff += self.magnitude,
            _ => {}
        }
    }
    pub fn apply_wheel(&self, w: &mut WheelSpec) {
        match self.kind {
            TuningPartEffectKind::WheelFriction => w.friction *= 1.0 + self.magnitude,
            _ => {}
        }
    }
    pub fn apply_engine(&self, e: &mut EngineSpec) {
        match self.kind {
            TuningPartEffectKind::EngineParams => e.max_torque *= 1.0 + self.magnitude,
            TuningPartEffectKind::Thruster => e.thrust += self.magnitude,
            _ => {}
        }
    }
    /// Scalar form of `apply_wheel`, for callers that work on plain numbers.
    pub fn grip_multiplier(&self) -> f32 {
        match self.kind {
            TuningPartEffectKind::WheelFriction => 1.0 + self.magnitude,
            _ => 1.0,
        }
    }
    /// Scalar form of `apply_engine`'s torque branch.
    pub fn torque_multiplier(&self) -> f32 {
        match self.kind {
            TuningPartEffectKind::EngineParams => 1.0 + self.magnitude,
            _ => 1.0,
        }
    }
    pub fn apply_fuel(&self, rate: &mut f32) {
        if self.kind == TuningPartEffectKind::FuelConsumption {
            *rate *= (1.0 + self.magnitude).max(0.05);
        }
    }
}

#[derive(Clone, Debug)]
pub struct TuningPart {
    pub part_id: String,
    pub effect: TuningPartEffect,
    pub rarity: Rarity,
    pub equipped_on: Option<String>,
    pub upgrade_level: u8,
}

impl TuningPart {
    pub fn new(part_id: &str, effect: TuningPartEffect, rarity: Rarity) -> Self {
        Self {
            part_id: part_id.to_string(),
            effect,
            rarity,
            equipped_on: None,
            upgrade_level: 0,
        }
    }
}

/// Grip part offered in the garage. Magnitude matches a common friction part.
pub fn grip_part() -> TuningPart {
    TuningPart::new(
        "tire_soft",
        TuningPartEffect {
            kind: TuningPartEffectKind::WheelFriction,
            magnitude: 0.18,
            offset: None,
        },
        Rarity::Common,
    )
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UpgradeEffectKind {
    Acceleration,
    AirBrake,
    AirControl,
    AirDrag,
    Angle,
    BodyMass,
    BoostParams,
    CenterOfMass,
    CollidableMass,
    Damage,
    Downforce,
    Durability,
    EngineParams,
    Friction,
    FuelTank,
    Generic,
    GrapplingHook,
    GripFactor,
    Hovering,
    Hydraulics,
    Iq,
    MaxAngularVelocity,
    Nitro,
    PowerDistribution,
    Rollcage,
    SpringDamping,
    SpringFrequency,
    Stability,
    Thrust,
    TopSpeed,
    Wing,
}

impl UpgradeEffectKind {
    pub fn name(self) -> &'static str {
        use UpgradeEffectKind::*;
        match self {
            Acceleration => "acceleration",
            AirBrake => "air_brake",
            AirControl => "air_control",
            AirDrag => "air_drag",
            Angle => "angle",
            BodyMass => "body_mass",
            BoostParams => "boost_params",
            CenterOfMass => "center_of_mass",
            CollidableMass => "collidable_mass",
            Damage => "damage",
            Downforce => "downforce",
            Durability => "durability",
            EngineParams => "engine_params",
            Friction => "friction",
            FuelTank => "fuel_tank",
            Generic => "generic",
            GrapplingHook => "grappling_hook",
            GripFactor => "grip_factor",
            Hovering => "hovering",
            Hydraulics => "hydraulics",
            Iq => "iq",
            MaxAngularVelocity => "max_angular_velocity",
            Nitro => "nitro",
            PowerDistribution => "power_distribution",
            Rollcage => "rollcage",
            SpringDamping => "spring_damping",
            SpringFrequency => "spring_frequency",
            Stability => "stability",
            Thrust => "thrust",
            TopSpeed => "top_speed",
            Wing => "wing",
        }
    }
}

#[derive(Clone, Debug)]
pub struct UpgradeEffect {
    pub kind: UpgradeEffectKind,
    pub level: u8,
    pub per_level: f32,
}

impl UpgradeEffect {
    pub fn total(&self) -> f32 {
        let level = self.level.max(1) as f32;
        match self.kind {
            UpgradeEffectKind::TopSpeed | UpgradeEffectKind::Acceleration => {
                self.per_level * level.powf(0.9)
            }
            _ => self.per_level * level,
        }
    }
    /// Coins to buy the next level. Level is the level the player has now.
    pub fn next_cost_coins(&self) -> u32 {
        let base = 500u32;
        let exp = self.level.saturating_sub(1) as u32;
        base.saturating_mul(2u32.saturating_pow(exp.min(12)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rarity_weights_descending() {
        assert!(Rarity::Common.weight() > Rarity::Rare.weight());
        assert!(Rarity::Rare.weight() > Rarity::Mythic.weight());
    }

    #[test]
    fn tuning_effect_modifies_friction() {
        let mut w = WheelSpec::default();
        let e = TuningPartEffect {
            kind: TuningPartEffectKind::WheelFriction,
            magnitude: 0.5,
            offset: None,
        };
        e.apply_wheel(&mut w);
        assert!(w.friction > 1.0);
    }

    #[test]
    fn upgrade_cost_grows() {
        let u = UpgradeEffect {
            kind: UpgradeEffectKind::Acceleration,
            level: 5,
            per_level: 0.1,
        };
        let next = UpgradeEffect {
            kind: UpgradeEffectKind::Acceleration,
            level: 6,
            per_level: 0.1,
        };
        assert!(next.next_cost_coins() > u.next_cost_coins());
        assert!(next.total() > u.total());
    }
}
