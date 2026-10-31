// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use rand::{Rng, RngExt};
use serde::Deserialize;
use std::fmt;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttributeRange {
    pub min: u16,
    pub max: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum AttributeValue {
    Fixed(u16),
    Range(AttributeRange),
}

impl AttributeValue {
    pub fn validate(self, minimum: u16, maximum: u16) -> bool {
        let (min, max) = match self {
            Self::Fixed(value) => (value, value),
            Self::Range(range) => (range.min, range.max),
        };
        minimum <= min && min <= max && max <= maximum
    }

    pub fn sample(self, rng: &mut impl Rng) -> u16 {
        match self {
            Self::Fixed(value) => value,
            Self::Range(range) => rng.random_range(range.min..=range.max),
        }
    }
}

impl fmt::Display for AttributeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(value) => value.fmt(f),
            Self::Range(range) => write!(f, "{}..={}", range.min, range.max),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RandomHelmet {
    Random,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum HelmetValue {
    Fixed(bool),
    Random(RandomHelmet),
    #[serde(skip)]
    FromArmor,
}

impl fmt::Display for HelmetValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(value) => value.fmt(f),
            Self::Random(_) => f.write_str("random"),
            Self::FromArmor => f.write_str("from_armor"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifeAttributes {
    pub health: u16,
    pub armor: u16,
    pub helmet: bool,
}

impl LifeAttributes {
    pub fn sample(health: AttributeValue, armor: AttributeValue, helmet: HelmetValue, rng: &mut impl Rng) -> Self {
        let health = health.sample(rng);
        let armor = armor.sample(rng);
        let helmet = match helmet {
            HelmetValue::Fixed(value) => value,
            HelmetValue::Random(RandomHelmet::Random) => rng.random_bool(0.5),
            HelmetValue::FromArmor => armor > 0,
        };
        Self { health, armor, helmet }
    }
}

impl crate::Mode {
    pub fn sample_attributes(&self, rng: &mut impl Rng) -> LifeAttributes {
        LifeAttributes::sample(self.health, self.armor, self.helmet, rng)
    }
}
