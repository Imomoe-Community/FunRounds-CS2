// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

namespace funrounds::native::detail {

bool HasGroundReverseSupport() noexcept;
bool SetGroundReverseActive(bool active, float acceleration) noexcept;
bool ShutdownGroundReverse() noexcept;

} // namespace funrounds::native::detail
