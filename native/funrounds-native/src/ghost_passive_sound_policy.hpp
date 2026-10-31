// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <span>
#include <vector>

#include "ghost_passive_sound_catalog.hpp"
#include "ghost_radar_policy.hpp"

namespace funrounds::native::ghost_passive_sound {

inline bool ShouldFilterStartSoundEvent(std::int32_t sourceEntityIndex,
                                        std::uint32_t soundEventHash,
                                        std::int32_t recipientSlot,
                                        std::span<const std::uint32_t> passiveSoundEventHashes,
                                        const std::vector<ghost_radar::Target>& ghostTargets,
                                        const std::vector<std::uint32_t>& hiddenFullHandles) noexcept {
    if (sourceEntityIndex < 0 || recipientSlot < 0 ||
        static_cast<std::size_t>(recipientSlot) >= ghost_radar::kMaxClientSlots ||
        !IsKnownPassiveSoundEvent(soundEventHash, passiveSoundEventHashes)) {
        return false;
    }

    const auto target = std::lower_bound(ghostTargets.begin(), ghostTargets.end(), sourceEntityIndex,
                                         [](const ghost_radar::Target& candidate, std::int32_t index) {
                                             return candidate.entityIndex < index;
                                         });
    if (target == ghostTargets.end() || target->entityIndex != sourceEntityIndex ||
        (target->enemyRecipients & (UINT64_C(1) << static_cast<unsigned int>(recipientSlot))) == 0) {
        return false;
    }

    return std::binary_search(hiddenFullHandles.begin(), hiddenFullHandles.end(), target->fullHandle);
}

} // namespace funrounds::native::ghost_passive_sound
