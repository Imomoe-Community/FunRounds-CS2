// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstdint>

namespace funrounds::native::detail {

bool HasGrenadeDetonateSupport(std::int32_t kind) noexcept;
std::int32_t GetGrenadeDetonationState(std::int32_t projectileHandle, std::int32_t kind) noexcept;
std::int32_t DetonateGrenade(std::int32_t projectileHandle, std::int32_t kind) noexcept;
bool ShutdownStandardGrenades() noexcept;

} // namespace funrounds::native::detail
