// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstdint>

namespace funrounds::native::detail {

using UseTargetCallback = void (*)(std::int32_t pawnHandle, std::int32_t targetHandle, bool inUse);
using HealthshotCanUseCallback = std::int32_t (*)(std::int32_t pawnHandle, std::int32_t weaponHandle);
using HealthshotEffectCallback = void (*)(std::int32_t pawnHandle, std::int32_t weaponHandle);

bool HasUseTargetSupport() noexcept;
bool HookUseTarget(UseTargetCallback callback) noexcept;
bool UnhookUseTarget(UseTargetCallback callback) noexcept;

bool HasHealthshotSupport() noexcept;
bool HookHealthshot(HealthshotCanUseCallback canUse, HealthshotEffectCallback effect) noexcept;
bool UnhookHealthshot(HealthshotCanUseCallback canUse, HealthshotEffectCallback effect) noexcept;

bool HasChickenFollowBlockSupport() noexcept;
bool SetChickenFollowBlocking(bool active) noexcept;

bool ShutdownInteractions() noexcept;

} // namespace funrounds::native::detail
