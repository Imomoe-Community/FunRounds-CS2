// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstdint>
#include <string_view>

namespace funrounds::native::grenade {

enum class Kind : std::int32_t {
    Smoke = 1,
    Molotov = 2,
    Incendiary = 3,
};

enum class RequestResult : std::int32_t {
    Failed = -1,
    AlreadyStartedOrGone = 0,
    Submitted = 1,
};

enum class StateResult : std::int32_t {
    Failed = -1,
    NotStarted = 0,
    StartedOrGone = 1,
};

constexpr std::int32_t ResultValue(RequestResult value) noexcept {
    return static_cast<std::int32_t>(value);
}

constexpr std::int32_t StateValue(StateResult value) noexcept {
    return static_cast<std::int32_t>(value);
}

constexpr bool ValidKind(std::int32_t value) noexcept {
    return value == static_cast<std::int32_t>(Kind::Smoke) ||
           value == static_cast<std::int32_t>(Kind::Molotov) ||
           value == static_cast<std::int32_t>(Kind::Incendiary);
}

constexpr std::string_view ProjectileClass(Kind kind) noexcept {
    switch (kind) {
        case Kind::Smoke: return "smokegrenade_projectile";
        case Kind::Molotov:
        case Kind::Incendiary: return "molotov_projectile";
    }
    return {};
}

constexpr std::string_view VTableName(Kind kind) noexcept {
    switch (kind) {
        case Kind::Smoke: return "CSmokeGrenadeProjectile";
        case Kind::Molotov:
        case Kind::Incendiary: return "CMolotovProjectile";
    }
    return {};
}

} // namespace funrounds::native::grenade
