// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <algorithm>
#include <bit>
#include <cstdint>
#include <vector>

namespace funrounds::native::ghost_visibility {

inline constexpr std::int32_t kInvalidEntityHandle = -1;

inline std::vector<std::uint32_t> BuildTargetSet(const std::vector<std::int32_t>& handles) {
    std::vector<std::uint32_t> result;
    result.reserve(handles.size());
    for (const auto handle : handles) {
        if (handle != kInvalidEntityHandle) {
            result.push_back(std::bit_cast<std::uint32_t>(handle));
        }
    }
    std::sort(result.begin(), result.end());
    result.erase(std::unique(result.begin(), result.end()), result.end());
    return result;
}

inline bool ContainsTarget(const std::vector<std::uint32_t>& sortedTargets,
                           std::uint32_t liveHandle) noexcept {
    return std::binary_search(sortedTargets.begin(), sortedTargets.end(), liveHandle);
}

} // namespace funrounds::native::ghost_visibility
