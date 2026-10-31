// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstdint>
#include <memory>
#include <vector>

#include <plg/any.hpp>

namespace funrounds::native::detail {

// Installs the resident bot-vision hook when its public gamedata signature is available.
bool InitializeGhostVisibility() noexcept;
bool GhostVisibilityReady() noexcept;

// Read the immutable exact-handle set from non-game-thread policy callbacks.
std::shared_ptr<const std::vector<std::uint32_t>> CurrentGhostVisibilityTargets() noexcept;

// Map transition only: discard old-world handles without resolving them.
void ForgetGhostVisibilityTargetsForMapEnd() noexcept;

// Publishes the complete set of exact pawn handles. The empty set disables ghost filtering.
bool SetGhostVisibilityTargets(const plg::vector<std::int32_t>& pawnHandles) noexcept;

bool ShutdownGhostVisibility() noexcept;

} // namespace funrounds::native::detail
