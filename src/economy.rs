#![allow(dead_code)]
//! Offline wallet rules: distance coins and the check-in table.
//! Chest and gacha types stay available for a local roll. Nothing here
//! contacts a shop server.

use crate::tuning::Rarity;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Currency {
    Coins,
    Gems,
    Scrap,
    AdventureToken,
}

impl Currency {
    pub fn id(self) -> &'static str {
        match self {
            Self::Coins => "COINS",
            Self::Gems => "GEMS",
            Self::Scrap => "SCRAP",
            Self::AdventureToken => "ADVENTURE_TOKEN",
        }
    }
    pub fn gems_to_coins_rate() -> u32 {
        500
    }
}

#[derive(Copy, Clone, Debug)]
pub struct CurrencyAmount {
    pub currency: Currency,
    pub amount: u32,
}

impl CurrencyAmount {
    pub fn coins(n: u32) -> Self {
        Self { currency: Currency::Coins, amount: n }
    }
    pub fn gems(n: u32) -> Self {
        Self { currency: Currency::Gems, amount: n }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChestKind {
    Team,
    Victory,
    Free,
    Distance,
    TeamChest,
    MegaAd,
    IAP,
}

impl ChestKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Victory => "victory",
            Self::Free => "free",
            Self::Distance => "distance",
            Self::TeamChest => "team_chest",
            Self::MegaAd => "mega_ad",
            Self::IAP => "iap",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Chest {
    pub kind: ChestKind,
    pub unlock_timer: u32,
    pub guaranteed_vehicle_at: Option<u32>,
    pub rolls_since_guarantee: u32,
}

impl Chest {
    pub fn new(kind: ChestKind, unlock_seconds: u32) -> Self {
        Self {
            kind,
            unlock_timer: unlock_seconds,
            guaranteed_vehicle_at: None,
            rolls_since_guarantee: 0,
        }
    }
    pub fn is_ready(&self) -> bool {
        self.unlock_timer == 0
    }
    pub fn tick(&mut self, dt: u32) {
        self.unlock_timer = self.unlock_timer.saturating_sub(dt);
    }
}

#[derive(Clone, Debug)]
pub struct LootEntry {
    pub item: LootItem,
    pub weight: u32,
    pub min_rarity: Rarity,
}

#[derive(Clone, Debug)]
pub enum LootItem {
    Currency(CurrencyAmount),
    TuningPart { effect_id: String, rarity_floor: Rarity },
    VehicleBlueprint { vehicle_id: String },
    VehicleUnlock { vehicle_id: String },
    AdventureTokens(u32),
    Scrap(u32),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum RewardSource {
    #[default]
    Unknown,
    Chest,
    Gacha,
    Daily,
    Checkin,
    SeasonPass,
    Event,
    Purchase,
    Team,
    Victory,
    Distance,
    Leaderboard,
    Tutorial,
}

impl RewardSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Chest => "chest",
            Self::Gacha => "gacha",
            Self::Daily => "daily",
            Self::Checkin => "checkin",
            Self::SeasonPass => "season_pass",
            Self::Event => "event",
            Self::Purchase => "purchase",
            Self::Team => "team",
            Self::Victory => "victory",
            Self::Distance => "distance",
            Self::Leaderboard => "leaderboard",
            Self::Tutorial => "tutorial",
        }
    }
}

pub fn checkin_reward(day: u32) -> LootItem {
    match day % 7 {
        0 => LootItem::Currency(CurrencyAmount::gems(20)),
        1 => LootItem::Currency(CurrencyAmount::coins(1000)),
        2 => LootItem::Currency(CurrencyAmount::gems(5)),
        3 => LootItem::Currency(CurrencyAmount::coins(2500)),
        4 => LootItem::Currency(CurrencyAmount::gems(10)),
        5 => LootItem::Currency(CurrencyAmount::coins(5000)),
        _ => LootItem::Currency(CurrencyAmount::gems(15)),
    }
}

/// Coins paid for the distance of one run.
pub fn distance_reward_coins(distance_m: f32) -> u32 {
    if distance_m < 500.0 {
        (distance_m / 5.0) as u32
    } else {
        100 + ((distance_m - 500.0) / 20.0) as u32
    }
}

pub fn gacha_roll<'a>(table: &'a [LootEntry], luck: f32) -> Option<&'a LootEntry> {
    let total: u32 = table.iter().map(|e| (e.weight as f32 * (1.0 + luck)) as u32).sum();
    if total == 0 {
        return None;
    }
    let mut r = (total as f32 * simple_rand()) as u32;
    for e in table {
        let w = (e.weight as f32 * (1.0 + luck)) as u32;
        if r < w {
            return Some(e);
        }
        r -= w;
    }
    table.last()
}

fn simple_rand() -> f32 {
    use std::cell::Cell;
    thread_local! { static S: Cell<u32> = const { Cell::new(0x12345678) }; }
    S.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        s.set(x);
        (x as f32) / (u32::MAX as f32)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_pays_more_when_farther() {
        assert!(distance_reward_coins(200.0) > distance_reward_coins(40.0));
        assert_eq!(distance_reward_coins(100.0), 20);
    }

    #[test]
    fn checkin_day_one_is_coins() {
        match checkin_reward(1) {
            LootItem::Currency(amount) => {
                assert_eq!(amount.currency, Currency::Coins);
                assert_eq!(amount.amount, 1000);
            }
            _ => panic!("expected coins"),
        }
    }
}
