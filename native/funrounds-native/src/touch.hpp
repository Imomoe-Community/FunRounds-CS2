// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstdint>

namespace funrounds::native::detail {

using EntityTouchPreCallback = std::int32_t (*)(std::int32_t entityHandle, std::int32_t otherHandle);

bool IsTouchAvailable();
bool HookTouch(std::int32_t entityHandle, EntityTouchPreCallback callback);
// Returns false for null callbacks or when one of the callback's retained
// Touch hooks cannot be detached. A callback that owns no hooks succeeds.
bool UnhookTouch(EntityTouchPreCallback callback);
bool ShutdownTouch() noexcept;
#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
bool ProbeTouchUnhookContract(std::int32_t entityHandle) noexcept;
#endif

} // namespace funrounds::native::detail
