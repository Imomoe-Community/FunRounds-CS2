#pragma once

#include <cstdint>

namespace funrounds::native::detail {

using PlayerCanAcquireCallback = std::int32_t (*)(std::int32_t pawnHandle,
                                                   std::int32_t controllerHandle);

bool HasSuicideSupport();
std::int32_t LookAtWeaponButtonState(std::int32_t pawnHandle);
bool HookPlayerCanAcquire(PlayerCanAcquireCallback callback);
bool UnhookPlayerCanAcquire(PlayerCanAcquireCallback callback);
bool ShutdownSuicide() noexcept;

} // namespace funrounds::native::detail
