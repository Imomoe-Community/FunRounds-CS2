// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <algorithm>
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <type_traits>
#include <utility>
#include <vector>

namespace funrounds::native::ghost_radar {

inline constexpr std::size_t kMaxClientSlots = 64;
inline constexpr std::int32_t kInvalidEntityHandle = -1;
inline constexpr std::int32_t kInvalidUserId = -1;

struct Target {
    std::uint32_t fullHandle{};
    std::int32_t entityIndex{-1};
    std::uint64_t enemyRecipients{};
};

struct SpottedState {
    bool spotted{};
    std::uint64_t spottedByMask{};
};

template <typename Frame, std::size_t Capacity>
class NestedInvocationStack final {
public:
    static_assert(Capacity > 0);
    static_assert(std::is_nothrow_default_constructible_v<Frame>);
    static_assert(std::is_nothrow_move_assignable_v<Frame>);

    std::optional<std::size_t> Begin() noexcept {
        if (overflowDepth_ != 0 || depth_ == Capacity) {
            ++overflowDepth_;
            return std::nullopt;
        }

        frames_[depth_] = Frame{};
        return depth_++;
    }

    Frame* Get(std::size_t index) noexcept {
        return index < depth_ ? &frames_[index] : nullptr;
    }

    bool End(Frame& frame) noexcept {
        if (overflowDepth_ != 0) {
            --overflowDepth_;
            return false;
        }
        if (depth_ == 0) {
            return false;
        }

        frame = std::move(frames_[--depth_]);
        frames_[depth_] = Frame{};
        return true;
    }

private:
    std::array<Frame, Capacity> frames_{};
    std::size_t depth_{};
    std::size_t overflowDepth_{};
};

inline bool MatchesSendInvocation(const void* expectedClient,
                                  const void* expectedMessage,
                                  const void* currentClient,
                                  const void* currentMessage) noexcept {
    return expectedClient != nullptr && expectedMessage != nullptr && expectedClient == currentClient &&
           expectedMessage == currentMessage;
}

class SendInvocationGate final {
public:
    bool TryEnterRoot() noexcept {
        auto state = state_.load(std::memory_order_acquire);
        while ((state & kClosedBit) == 0) {
            if ((state & kActiveMask) == kActiveMask) {
                return false;
            }
            if (state_.compare_exchange_weak(state,
                                             state + 1,
                                             std::memory_order_acq_rel,
                                             std::memory_order_acquire)) {
                return true;
            }
        }
        return false;
    }

    void LeaveRoot() noexcept { state_.fetch_sub(1, std::memory_order_acq_rel); }

    bool TryCloseWhenIdle() noexcept {
        std::uint64_t expected = 0;
        return state_.compare_exchange_strong(expected,
                                              kClosedBit,
                                              std::memory_order_acq_rel,
                                              std::memory_order_acquire);
    }

    bool IsClosed() const noexcept { return (state_.load(std::memory_order_acquire) & kClosedBit) != 0; }

    std::uint64_t ActiveRoots() const noexcept {
        return state_.load(std::memory_order_acquire) & kActiveMask;
    }

    bool Open() noexcept {
        auto state = state_.load(std::memory_order_acquire);
        if (state == 0) {
            return true;
        }
        if (state != kClosedBit) {
            return false;
        }
        return state_.compare_exchange_strong(state,
                                              0,
                                              std::memory_order_acq_rel,
                                              std::memory_order_acquire);
    }

private:
    static constexpr std::uint64_t kClosedBit = UINT64_C(1) << 63;
    static constexpr std::uint64_t kActiveMask = ~kClosedBit;
    std::atomic<std::uint64_t> state_{};
};

inline SpottedState FilterSpottedState(bool originalSpotted,
                                       std::uint64_t originalSpottedByMask,
                                       std::uint64_t enemyRecipients,
                                       std::uint64_t connectedRecipients) noexcept {
    std::uint64_t filteredMask = originalSpottedByMask & ~enemyRecipients;
    if (originalSpotted) {
        filteredMask |= connectedRecipients & ~enemyRecipients;
    }
    return {false, filteredMask};
}

inline bool BuildTargetSet(const std::vector<Target>& input, std::vector<Target>& output) {
    std::vector<Target> sorted = input;
    for (const auto& target : sorted) {
        if (target.fullHandle == UINT32_MAX || target.entityIndex < 0) {
            return false;
        }
    }

    std::sort(sorted.begin(), sorted.end(), [](const Target& left, const Target& right) {
        return left.entityIndex < right.entityIndex;
    });

    std::vector<Target> unique;
    unique.reserve(sorted.size());
    for (const auto& target : sorted) {
        if (!unique.empty() && unique.back().entityIndex == target.entityIndex) {
            if (unique.back().fullHandle != target.fullHandle) {
                return false;
            }
            unique.back().enemyRecipients |= target.enemyRecipients;
            continue;
        }
        unique.push_back(target);
    }

    output = std::move(unique);
    return true;
}

inline bool IsCurrentRecipient(std::int32_t slot,
                               std::int32_t liveUserId,
                               const std::array<std::int32_t, kMaxClientSlots>& userIds) noexcept {
    return slot >= 0 && static_cast<std::size_t>(slot) < userIds.size() && liveUserId != kInvalidUserId &&
           userIds[static_cast<std::size_t>(slot)] == liveUserId;
}

inline bool HasTargetForRecipient(std::int32_t recipientSlot,
                                  const std::vector<Target>& sortedTargets) noexcept {
    if (recipientSlot < 0 || static_cast<std::size_t>(recipientSlot) >= kMaxClientSlots) {
        return false;
    }
    const std::uint64_t recipientBit = UINT64_C(1) << static_cast<unsigned int>(recipientSlot);
    return std::any_of(sortedTargets.begin(), sortedTargets.end(), [recipientBit](const Target& target) {
        return (target.enemyRecipients & recipientBit) != 0;
    });
}

inline bool ShouldFilterEntityUpdate(std::int32_t entityIndex,
                                     std::int32_t recipientSlot,
                                     const std::vector<Target>& sortedTargets) noexcept {
    if (entityIndex < 0 || recipientSlot < 0 || static_cast<std::size_t>(recipientSlot) >= kMaxClientSlots) {
        return false;
    }

    const auto target = std::lower_bound(sortedTargets.begin(), sortedTargets.end(), entityIndex,
                                         [](const Target& candidate, std::int32_t index) {
                                             return candidate.entityIndex < index;
                                         });
    if (target == sortedTargets.end() || target->entityIndex != entityIndex) {
        return false;
    }
    return (target->enemyRecipients & (UINT64_C(1) << static_cast<unsigned int>(recipientSlot))) != 0;
}

template <typename RepeatedUpdates>
std::size_t FilterEntityUpdates(RepeatedUpdates& updates,
                                std::int32_t recipientSlot,
                                const std::vector<Target>& sortedTargets) {
    std::size_t removed = 0;
    for (int index = updates.size(); index > 0;) {
        --index;
        if (ShouldFilterEntityUpdate(updates.Get(index).entity_idx(), recipientSlot, sortedTargets)) {
            updates.DeleteSubrange(index, 1);
            ++removed;
        }
    }
    return removed;
}

template <typename RepeatedUpdates>
bool HasEntityUpdateToFilter(const RepeatedUpdates& updates,
                             std::int32_t recipientSlot,
                             const std::vector<Target>& sortedTargets) noexcept {
    for (int index = 0; index < updates.size(); ++index) {
        if (ShouldFilterEntityUpdate(updates.Get(index).entity_idx(), recipientSlot, sortedTargets)) {
            return true;
        }
    }
    return false;
}

} // namespace funrounds::native::ghost_radar
