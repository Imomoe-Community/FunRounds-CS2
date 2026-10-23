#pragma once

#include <cstdint>

namespace funrounds::native::detail {

using EntityTouchPreCallback = std::int32_t (*)(std::int32_t entityHandle, std::int32_t otherHandle);

bool IsTouchAvailable();
bool HookTouch(std::int32_t entityHandle, EntityTouchPreCallback callback);
bool UnhookTouch(EntityTouchPreCallback callback);
bool ShutdownTouch() noexcept;

} // namespace funrounds::native::detail
