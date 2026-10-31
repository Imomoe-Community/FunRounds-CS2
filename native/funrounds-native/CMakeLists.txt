# SPDX-License-Identifier: AGPL-3.0-or-later
cmake_minimum_required(VERSION 3.24)

if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
    project(funrounds_native LANGUAGES CXX)
endif()
if(WIN32)
    if(NOT MSVC)
        message(FATAL_ERROR "The reviewed Windows player Ignite ABI requires the MSVC MASM thunk")
    endif()
    enable_language(ASM_MASM)
endif()

if(NOT DEFINED FUNROUNDS_PLUGIFY_INCLUDE_DIR)
    message(FATAL_ERROR "FUNROUNDS_PLUGIFY_INCLUDE_DIR must point to the SDK's generated Plugify headers")
endif()
if(NOT DEFINED FUNROUNDS_S2SDK_SOURCE_DIR)
    message(FATAL_ERROR "FUNROUNDS_S2SDK_SOURCE_DIR must point to the pinned S2SDK source tree")
endif()
if(NOT DEFINED FUNROUNDS_S2SDK_GENERATED_INCLUDE_DIR)
    message(FATAL_ERROR "FUNROUNDS_S2SDK_GENERATED_INCLUDE_DIR must contain generated s2sdk C++ imports")
endif()
# An explicit path is an input contract: reject it rather than silently selecting
# a different protobuf build.
if(DEFINED FUNROUNDS_PROTOBUF_LIBRARY)
    if(NOT EXISTS "${FUNROUNDS_PROTOBUF_LIBRARY}" OR IS_DIRECTORY "${FUNROUNDS_PROTOBUF_LIBRARY}")
        message(FATAL_ERROR "FUNROUNDS_PROTOBUF_LIBRARY must name an existing library file paired with the selected SourceSDK: ${FUNROUNDS_PROTOBUF_LIBRARY}")
    endif()
    set(FUNROUNDS_PROTOBUF_TARGET "${FUNROUNDS_PROTOBUF_LIBRARY}")
elseif(TARGET libprotobuf)
    set(FUNROUNDS_PROTOBUF_TARGET libprotobuf)
endif()

# Reuse the exact SourceSDK header and compile-definition contract selected by S2SDK.
if(NOT TARGET sourcesdk)
    set(SOURCESDK_GAME_TARGET "cs2" CACHE STRING "SourceSDK header target" FORCE)
    if(DEFINED FUNROUNDS_PROTOBUF_LIBRARY OR TARGET libprotobuf)
        set(SOURCESDK_COMPILE_PROTOBUF OFF CACHE BOOL "Reuse the selected protobuf library" FORCE)
    else()
        set(SOURCESDK_COMPILE_PROTOBUF ON CACHE BOOL "Build protobuf for the standalone native plugin" FORCE)
        set(protobuf_WITH_ZLIB OFF CACHE BOOL "Match the SDK protobuf build without zlib" FORCE)
    endif()
    # CCSBot's public SourceSDK declaration includes CSGOUserCmd, whose generated
    # header imports usercmd.pb.h. Generate both headers when the paired
    # protobuf library is reused rather than rebuilt.
    # When SourceSDK is compiling its full proto set, it will generate these
    # imported headers itself. Otherwise, include its normal minimal set plus
    # the two CCSBot header dependencies.
    if(NOT (SOURCESDK_COMPILE_PROTOBUF AND DEFINED SOURCESDK_USE_MINIMAL_PROTOS AND
            NOT SOURCESDK_USE_MINIMAL_PROTOS AND NOT SOURCESDK_PROTOS))
        if(NOT SOURCESDK_PROTOS)
            set(SOURCESDK_PROTOS
                netmessages
                network_connection
                networkbasetypes
                networksystem_protomessages
                gameevents
                usermessages
                source2_steam_stats
                valveextensions
                usercmd
                cs_usercmd
                CACHE STRING "SourceSDK protos required by the native plugin" FORCE
            )
        else()
            if(NOT "usercmd" IN_LIST SOURCESDK_PROTOS)
                list(APPEND SOURCESDK_PROTOS usercmd)
            endif()
            if(NOT "cs_usercmd" IN_LIST SOURCESDK_PROTOS)
                list(APPEND SOURCESDK_PROTOS cs_usercmd)
            endif()
            set(SOURCESDK_PROTOS "${SOURCESDK_PROTOS}" CACHE STRING "SourceSDK protos required by the native plugin" FORCE)
        endif()
    endif()
    set(SOURCESDK_ENABLE_TESTS OFF CACHE BOOL "Do not build SourceSDK tests for the native plugin" FORCE)
    set(SOURCESDK_CONFIGURE_EXPORT_MAP OFF CACHE BOOL "Keep Plugify entrypoints visible in the native plugin" FORCE)
    add_subdirectory("${FUNROUNDS_S2SDK_SOURCE_DIR}/external/sourcesdk" "${CMAKE_BINARY_DIR}/sourcesdk")
endif()

# A parent may add SourceSDK without compiling protobuf; in that case it must
# provide the paired library explicitly rather than causing a duplicate SDK build.
if(NOT FUNROUNDS_PROTOBUF_TARGET AND TARGET libprotobuf)
    set(FUNROUNDS_PROTOBUF_TARGET libprotobuf)
endif()
if(NOT FUNROUNDS_PROTOBUF_TARGET)
    message(FATAL_ERROR "No paired libprotobuf is available. Set FUNROUNDS_PROTOBUF_LIBRARY or configure the parent SourceSDK with SOURCESDK_COMPILE_PROTOBUF=ON.")
endif()

get_target_property(FUNROUNDS_S2SDK_INCLUDE_DIRS sourcesdk INTERFACE_INCLUDE_DIRECTORIES)
get_target_property(FUNROUNDS_S2SDK_COMPILE_DEFINITIONS sourcesdk INTERFACE_COMPILE_DEFINITIONS)
set(FUNROUNDS_S2SDK_INCLUDE_DIRS
    ${FUNROUNDS_S2SDK_INCLUDE_DIRS}
    "${FUNROUNDS_S2SDK_SOURCE_DIR}/external/plugify/include"
    "${FUNROUNDS_S2SDK_GENERATED_INCLUDE_DIR}"
)

add_library(funrounds-native SHARED
    src/plugin.cpp
    src/engine.cpp
    src/burn.cpp
    src/damage.cpp
    src/touch.cpp
    src/suicide.cpp
    src/collision.cpp
    src/ground_reverse.cpp
    src/third_person.cpp
    src/ghost_visibility.cpp
    src/ghost_radar.cpp
    src/ghost_passive_noise.cpp
    src/ghost_radar_state.cpp
    src/interactions.cpp
    src/random_spawn.cpp
    "${FUNROUNDS_PLUGIFY_INCLUDE_DIR}/polyhook/polyhook.cpp"
    "${FUNROUNDS_S2SDK_GENERATED_INCLUDE_DIR}/s2sdk/s2sdk.cpp"
)
if(WIN32)
    add_library(funrounds-native-ignite-win OBJECT src/burn_ignite_win.asm)
    target_sources(funrounds-native PRIVATE $<TARGET_OBJECTS:funrounds-native-ignite-win>)
endif()

set_target_properties(funrounds-native PROPERTIES
    CXX_STANDARD 23
    CXX_STANDARD_REQUIRED ON
    CXX_EXTENSIONS OFF
    PREFIX "${CMAKE_SHARED_LIBRARY_PREFIX}"
    OUTPUT_NAME "funrounds-native"
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
)

target_include_directories(funrounds-native PRIVATE
    "${FUNROUNDS_PLUGIFY_INCLUDE_DIR}"
    "${CMAKE_CURRENT_SOURCE_DIR}/src"
    "${CMAKE_CURRENT_BINARY_DIR}/exports"
)
target_include_directories(funrounds-native SYSTEM PRIVATE ${FUNROUNDS_S2SDK_INCLUDE_DIRS})
target_link_libraries(funrounds-native PRIVATE sourcesdk "${FUNROUNDS_PROTOBUF_TARGET}")
target_compile_definitions(funrounds-native PRIVATE
    PLUGIFY_IS_DEBUG=$<CONFIG:Debug>
    PLUGIFY_IS_RELEASE=$<CONFIG:Release>
    PLUGIFY_ARCH_BITS=$<IF:$<EQUAL:${CMAKE_SIZEOF_VOID_P},8>,64,32>
    PLUGIFY_MODULE="funrounds_native"
    S2SDK_LIBRARY_PREFIX="${CMAKE_SHARED_LIBRARY_PREFIX}"
    S2SDK_LIBRARY_SUFFIX="${CMAKE_SHARED_LIBRARY_SUFFIX}"
    S2SDK_GAME_NAME="csgo"
)
target_compile_definitions(funrounds-native PRIVATE ${FUNROUNDS_S2SDK_COMPILE_DEFINITIONS})

file(MAKE_DIRECTORY "${CMAKE_BINARY_DIR}/gamedata")
configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/gamedata/funrounds-native.jsonc"
    "${CMAKE_BINARY_DIR}/gamedata/funrounds-native.jsonc"
    COPYONLY
)
configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/funrounds-native.pplugin"
    "${CMAKE_BINARY_DIR}/funrounds-native.pplugin"
    COPYONLY
)

include(GenerateExportHeader)
file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/exports")
generate_export_header(funrounds-native
    EXPORT_MACRO_NAME PLUGIN_API
    EXPORT_FILE_NAME "${CMAKE_CURRENT_BINARY_DIR}/exports/plugin_export.h"
)

if(MSVC)
    target_compile_options(funrounds-native PRIVATE /W4 /WX /Zc:preprocessor)
    set_target_properties(funrounds-native PROPERTIES MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
else()
    target_compile_options(funrounds-native PRIVATE -Wall -Wextra -Wshadow -Wconversion -Werror)
endif()
