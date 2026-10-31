// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include <cstdint>

namespace funrounds::native::detail {
bool HasThirdPersonEyeSupport() noexcept;
bool RegisterThirdPersonEyeTarget(std::int32_t pawn, std::int32_t camera) noexcept;
bool UnregisterThirdPersonEyeTarget(std::int32_t pawn, std::int32_t camera) noexcept;
// Map transition only: forget old identities without resolving reused handles.
bool ResetThirdPersonEyeTargets() noexcept;
bool ShutdownThirdPersonEye() noexcept;
}
