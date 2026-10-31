// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include <cstddef>
#include <cstdint>
#include <cstring>

#include <const.h>

#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"

namespace {

template <typename T>
void WriteField(void* destination, std::int32_t offset, const T& value) noexcept {
    std::memcpy(static_cast<std::byte*>(destination) + offset, &value, sizeof(value));
}

} // namespace

extern "C" PLUGIN_API void SetEntityDefaultCollisionGroup(std::int32_t entityHandle) {
    try {
        using namespace funrounds::native::detail;
        if (entityHandle == -1 || !EnsureEngine()) {
            return;
        }
        void* entity = EntityFromHandle(entityHandle);
        if (!entity || HandleFromEntity(entity) != entityHandle) {
            return;
        }

        SchemaField collisionPointer{};
        SchemaField collisionAttribute{};
        SchemaField attributeGroup{};
        SchemaField collisionGroup{};
        if (!ResolveSchemaField("CBaseEntity", "m_pCollision", sizeof(void*), collisionPointer) ||
            !ResolveSchemaField("CCollisionProperty", "m_collisionAttribute", 0, collisionAttribute) ||
            !ResolveSchemaField("VPhysicsCollisionAttribute_t", "m_nCollisionGroup", sizeof(std::uint8_t), attributeGroup) ||
            !ResolveSchemaField("CCollisionProperty", "m_CollisionGroup", sizeof(std::uint8_t), collisionGroup)) {
            return;
        }

        const auto methodOffset = s2sdk::GetGameConfigOffset(GameConfig(), "CBaseEntity::CollisionRulesChanged");
        if (methodOffset < 0) {
            return;
        }
        auto** vtable = *reinterpret_cast<void***>(entity);
        if (!vtable || !vtable[methodOffset]) {
            return;
        }

        void* property = nullptr;
        std::memcpy(&property, static_cast<std::byte*>(entity) + collisionPointer.offset, sizeof(property));
        if (!property) {
            return;
        }

        constexpr auto defaultGroup = static_cast<std::uint8_t>(COLLISION_GROUP_DEFAULT);
        WriteField(static_cast<std::byte*>(property) + collisionAttribute.offset,
                   attributeGroup.offset,
                   defaultGroup);
        WriteField(property, collisionGroup.offset, defaultGroup);
        reinterpret_cast<void (*)(void*)>(vtable[methodOffset])(entity);
    } catch (...) {
    }
}
