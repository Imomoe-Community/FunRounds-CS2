// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <array>
#include <cstdint>
#include <memory>
#include <vector>

#include <plg/any.hpp>

#include "ghost_radar_policy.hpp"

namespace funrounds::native::detail {

struct GhostRadarSnapshot {
    std::vector<ghost_radar::Target> targets;
    std::array<std::int32_t, ghost_radar::kMaxClientSlots> recipientUserIds{};
    std::array<std::uint32_t, ghost_radar::kMaxClientSlots> recipientControllerHandles{};
    std::uint64_t connectedRecipientMask{};
};

bool InitializeGhostRadarMessages() noexcept;
bool GhostRadarMessagesReady() noexcept;
bool InitializeGhostPassiveSoundMessages() noexcept;
bool GhostPassiveSoundMessagesReady() noexcept;
bool InitializeGhostPassiveNoise() noexcept;
bool GhostPassiveNoiseReady() noexcept;
bool ShutdownGhostPassiveNoise() noexcept;
bool InitializeGhostRadarStateFilter() noexcept;
bool GhostRadarStateFilterReady() noexcept;
bool ShutdownGhostRadarStateFilter() noexcept;
void ForgetGhostRadarTargetsForMapEnd() noexcept;
std::shared_ptr<const GhostRadarSnapshot> CurrentGhostRadarSnapshot() noexcept;
bool SetGhostRadarTargets(const plg::vector<std::int32_t>& fullPawnHandles,
                          const plg::vector<std::uint64_t>& enemyRecipientMasks) noexcept;
bool ShutdownGhostRadar() noexcept;

} // namespace funrounds::native::detail
