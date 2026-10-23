#include "engine.hpp"

#include <cstring>

namespace funrounds::native::detail {
namespace {

constexpr char kSchemaInterface[] = "SchemaSystem_001";
constexpr char kServerModule[] = S2SDK_LIBRARY_PREFIX "server" S2SDK_LIBRARY_SUFFIX;

s2sdk::ConfigId g_gameConfig = 0;
ISchemaSystem* g_schemaSystem = nullptr;
ISchemaSystemTypeScope* g_serverSchema = nullptr;

bool ImportsAvailable() noexcept {
    return __s2sdk_QueryInterface != nullptr &&
           __s2sdk_LoadGameConfigFile != nullptr &&
           __s2sdk_CloseGameConfigFile != nullptr &&
           __s2sdk_GetGameConfigOffset != nullptr &&
           __s2sdk_GetGameConfigSignature != nullptr &&
           __s2sdk_GetGameConfigVTable != nullptr &&
           __s2sdk_EntHandleToEntPointer != nullptr &&
           __s2sdk_EntPointerToEntHandle != nullptr &&
           __s2sdk_IsEntityPlayerPawn != nullptr &&
           __s2sdk_IsEntityPlayerController != nullptr;
}

} // namespace

bool EnsureEngine() {
    if (!ImportsAvailable()) {
        return false;
    }

    if (g_gameConfig == 0) {
        plg::string configPath = plg::GetPlugin()->GetLocation();
        configPath += "/gamedata/funrounds-native.jsonc";
        g_gameConfig = s2sdk::LoadGameConfigFile({configPath});
        if (g_gameConfig == 0) {
            return false;
        }
    }

    if (!g_schemaSystem) {
        g_schemaSystem = static_cast<ISchemaSystem*>(s2sdk::QueryInterface("schemasystem", kSchemaInterface));
    }
    if (!g_schemaSystem || !g_schemaSystem->SchemaSystemIsReady()) {
        return false;
    }

    if (!g_serverSchema) {
        g_serverSchema = g_schemaSystem->FindTypeScopeForModule(kServerModule);
    }
    return g_serverSchema != nullptr;
}

void ShutdownEngine() noexcept {
    if (g_gameConfig != 0 && __s2sdk_CloseGameConfigFile) {
        s2sdk::CloseGameConfigFile(g_gameConfig);
    }
    g_gameConfig = 0;
    g_serverSchema = nullptr;
    g_schemaSystem = nullptr;
}

s2sdk::ConfigId GameConfig() noexcept {
    return g_gameConfig;
}

ISchemaSystemTypeScope* ServerSchema() noexcept {
    return g_serverSchema;
}

bool ResolveSchemaField(const char* className,
                        const char* fieldName,
                        std::size_t expectedSize,
                        SchemaField& result) noexcept {
    if (!g_serverSchema || !className || !fieldName) {
        return false;
    }

    auto* classInfo = g_serverSchema->FindDeclaredClass(className).Get();
    if (!classInfo || classInfo->m_nSize < 0 || (classInfo->m_nFieldCount != 0 && !classInfo->m_pFields)) {
        return false;
    }

    for (std::uint16_t index = 0; index < classInfo->m_nFieldCount; ++index) {
        const auto& candidate = classInfo->m_pFields[index];
        if (!candidate.m_pszName || std::strcmp(candidate.m_pszName, fieldName) != 0 || !candidate.m_pType) {
            continue;
        }

        int size = 0;
        std::uint8_t alignment = 0;
        if (!candidate.m_pType->GetSizeAndAlignment(size, alignment) || size < 0 ||
            (expectedSize != 0 && static_cast<std::size_t>(size) != expectedSize) ||
            candidate.m_nSingleInheritanceOffset < 0) {
            return false;
        }

        const auto offset = static_cast<std::size_t>(candidate.m_nSingleInheritanceOffset);
        const auto objectSize = static_cast<std::size_t>(classInfo->m_nSize);
        if (offset > objectSize || expectedSize > objectSize - offset) {
            return false;
        }
        result = {candidate.m_nSingleInheritanceOffset, static_cast<std::size_t>(size)};
        return true;
    }
    return false;
}

void* EntityFromHandle(std::int32_t handle) noexcept {
    if (handle == -1 || !__s2sdk_EntHandleToEntPointer) {
        return nullptr;
    }
    return s2sdk::EntHandleToEntPointer(handle);
}

std::int32_t HandleFromEntity(void* entity) noexcept {
    if (!entity || !__s2sdk_EntPointerToEntHandle) {
        return -1;
    }
    return s2sdk::EntPointerToEntHandle(entity);
}

bool IsPlayerPawn(std::int32_t handle) noexcept {
    return handle != -1 && __s2sdk_IsEntityPlayerPawn && s2sdk::IsEntityPlayerPawn(handle);
}

bool IsPlayerController(std::int32_t handle) noexcept {
    return handle != -1 && __s2sdk_IsEntityPlayerController && s2sdk::IsEntityPlayerController(handle);
}

} // namespace funrounds::native::detail
