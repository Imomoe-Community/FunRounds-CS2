// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

namespace funrounds::native::detail {

constexpr bool ShouldBlockChickenFollow(bool active, bool exactChicken) noexcept {
    return active && exactChicken;
}

} // namespace funrounds::native::detail
