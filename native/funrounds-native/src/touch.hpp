// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstdint>

namespace funrounds::native::detail {

using EntityTouchPreCallback = std::int32_t (*)(std::int32_t entityHandle, std::int32_t otherHandle);

bool IsTouchAvailable();
bool HookTouch(std::int32_t entityHandle, EntityTouchPreCallback callback);
// Removes only this callback's subscriptions. Other callbacks keep their shared
// detours alive; false means one of this callback's final-hook detachments must
// be retried.
bool UnhookTouch(EntityTouchPreCallback callback);
bool ShutdownTouch() noexcept;
#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
bool ProbeTouchUnhookContract(std::int32_t entityHandle) noexcept;
#endif

} // namespace funrounds::native::detail
