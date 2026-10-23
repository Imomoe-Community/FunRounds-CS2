#pragma once

#include <cstdint>

namespace funrounds::native::detail {

using DamagePreCallback = std::int32_t (*)(std::int32_t victimHandle,
                                           std::int32_t attackerHandle,
                                           std::int32_t inflictorHandle,
                                           std::int32_t weaponKind,
                                           std::int32_t hitGroup,
                                           bool tracedHead,
                                           float damage,
                                           std::uint32_t damageType,
                                           float& multiplier,
                                           bool& markHeadshot,
                                           std::int32_t weaponHandle,
                                           bool skipPluginRules);

bool EnsureDamageAvailable();
bool HookDamage(DamagePreCallback callback);
bool UnhookDamage(DamagePreCallback callback);
bool ShutdownDamage() noexcept;
std::uint64_t CurrentDamageFrameToken() noexcept;
std::int32_t ApplyDamage(std::int32_t victimHandle,
                         std::int32_t inflictorHandle,
                         std::int32_t attackerHandle,
                         const float force[3],
                         const float position[3],
                         float damage,
                         std::int32_t damageType,
                         std::int32_t weaponHandle,
                         bool skipPluginRules);

} // namespace funrounds::native::detail
