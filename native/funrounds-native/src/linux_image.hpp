// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstddef>
#include <cstdint>
#include <limits>

namespace funrounds::native::detail {

inline constexpr std::uint32_t kPtLoad = 1;

template <typename ProgramHeader>
bool ComputeMappedImageEnd(const ProgramHeader* headers,
                           std::size_t count,
                           std::uint64_t& imageEnd) noexcept {
    imageEnd = 0;
    for (std::size_t index = 0; index < count; ++index) {
        const ProgramHeader& header = headers[index];
        if (header.p_type != kPtLoad) {
            continue;
        }
        if (header.p_memsz < header.p_filesz ||
            header.p_vaddr > std::numeric_limits<std::uint64_t>::max() - header.p_memsz) {
            return false;
        }
        const std::uint64_t end = header.p_vaddr + header.p_memsz;
        if (end > imageEnd) {
            imageEnd = end;
        }
    }
    return imageEnd != 0;
}

} // namespace funrounds::native::detail
