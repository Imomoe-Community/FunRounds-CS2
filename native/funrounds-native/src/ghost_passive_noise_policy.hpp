// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <vector>

#include "ghost_radar_policy.hpp"

namespace funrounds::native::ghost_passive_noise {

template <std::size_t Capacity>
class PlayerFootstepScope final {
    static_assert(Capacity > 0);

public:
    using EventIdentity = std::uintptr_t;

    // Call once from the manager's Pre callback. Keep only the opaque event
    // identity until its matching Post; never dereference or retain the event.
    // Nested calls inherit the outer admission; calls beyond Capacity fail open
    // until their matching Posts.
    void Begin(bool admitted, EventIdentity eventIdentity) noexcept {
        if (overflowDepth_ != 0 || depth_ == Capacity) {
            ++overflowDepth_;
            return;
        }
        active_[depth_] = admitted && eventIdentity != 0 && (depth_ == 0 || active_[depth_ - 1]);
        eventIdentities_[depth_] = eventIdentity;
        ++depth_;
    }

    // The callback pair is LIFO. Pop its top frame even if a handler changed the
    // forwarded argument between Pre and Post, so a mismatch cannot leak scope.
    bool End(EventIdentity eventIdentity) noexcept {
        if (overflowDepth_ != 0) {
            --overflowDepth_;
            return false;
        }
        if (depth_ == 0) {
            return false;
        }
        const auto index = --depth_;
        const bool matched = eventIdentity != 0 && eventIdentities_[index] == eventIdentity;
        active_[index] = false;
        eventIdentities_[index] = 0;
        return matched;
    }

    bool Active() const noexcept { return overflowDepth_ == 0 && depth_ != 0 && active_[depth_ - 1]; }
    EventIdentity ActiveEventIdentity() const noexcept {
        return Active() ? eventIdentities_[depth_ - 1] : 0;
    }
    bool Empty() const noexcept { return depth_ == 0 && overflowDepth_ == 0; }

private:
    std::array<bool, Capacity> active_{};
    std::array<EventIdentity, Capacity> eventIdentities_{};
    std::size_t depth_{};
    std::size_t overflowDepth_{};
};

inline bool ShouldSuppressPlayerFootstep(
    std::uintptr_t scopeEventIdentity,
    std::uintptr_t helperEventIdentity,
    std::uint32_t sourceFullHandle,
    std::int32_t sourceEntityIndex,
    std::uint32_t recipientControllerFullHandle,
    std::int32_t recipientSlot,
    const std::array<std::uint32_t, ghost_radar::kMaxClientSlots>& recipientControllerHandles,
    const std::array<std::int32_t, ghost_radar::kMaxClientSlots>& recipientUserIds,
    std::uint64_t connectedRecipientMask,
    const std::vector<ghost_radar::Target>& ghostTargets,
    const std::vector<std::uint32_t>& hiddenFullHandles) noexcept {
    if (scopeEventIdentity == 0 || scopeEventIdentity != helperEventIdentity ||
        sourceFullHandle == UINT32_MAX ||
        recipientControllerFullHandle == UINT32_MAX || sourceEntityIndex < 0 || recipientSlot < 0 ||
        static_cast<std::size_t>(recipientSlot) >= ghost_radar::kMaxClientSlots) {
        return false;
    }

    const auto slot = static_cast<std::size_t>(recipientSlot);
    const std::uint64_t recipientBit = UINT64_C(1) << static_cast<unsigned int>(recipientSlot);
    if (recipientControllerHandles[slot] != recipientControllerFullHandle ||
        recipientUserIds[slot] == ghost_radar::kInvalidUserId || (connectedRecipientMask & recipientBit) == 0) {
        return false;
    }

    const auto target = std::lower_bound(ghostTargets.begin(), ghostTargets.end(), sourceEntityIndex,
                                         [](const ghost_radar::Target& candidate, std::int32_t index) {
                                             return candidate.entityIndex < index;
                                         });
    if (target == ghostTargets.end() || target->entityIndex != sourceEntityIndex ||
        target->fullHandle != sourceFullHandle || (target->enemyRecipients & recipientBit) == 0) {
        return false;
    }

    return std::binary_search(hiddenFullHandles.begin(), hiddenFullHandles.end(), sourceFullHandle);
}

} // namespace funrounds::native::ghost_passive_noise
