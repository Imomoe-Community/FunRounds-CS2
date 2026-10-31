// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include <cstring>

#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"

namespace
{
constexpr char kSignature[] = "FunRounds::SelectSpawnSpot";
constexpr char kSpawnClass[] = "info_deathmatch_spawn";
using SelectSpawnSpot = bool ( * )( void *, const char *, void ** );

SelectSpawnSpot Selector()
{
	if ( !funrounds::native::detail::EnsureEngine() || !__s2sdk_GetEntityClassname || !__s2sdk_IsValidEntHandle )
		return nullptr;
	return reinterpret_cast< SelectSpawnSpot >(
		s2sdk::GetGameConfigSignature( funrounds::native::detail::GameConfig(), kSignature ) );
}

bool IsRandomSpawnPoint( std::int32_t handle )
{
	return handle != -1 && s2sdk::IsValidEntHandle( handle ) &&
		std::strcmp( s2sdk::GetEntityClassname( handle ).c_str(), kSpawnClass ) == 0;
}
}

extern "C" PLUGIN_API bool HasPlayerRandomSpawnPointSupport()
{
	try
	{
		return Selector() != nullptr;
	}
	catch ( ... )
	{
		return false;
	}
}

extern "C" PLUGIN_API std::int32_t SelectPlayerRandomSpawnPoint( std::int32_t pawnHandle, std::int32_t startPointHandle )
{
	try
	{
		auto select = Selector();
		if ( !select || !funrounds::native::detail::IsPlayerPawn( pawnHandle ) || !IsRandomSpawnPoint( startPointHandle ) )
			return -1;
		auto *pawn = funrounds::native::detail::EntityFromHandle( pawnHandle );
		auto *point = funrounds::native::detail::EntityFromHandle( startPointHandle );
		if ( !pawn || !point || funrounds::native::detail::HandleFromEntity( pawn ) != pawnHandle )
			return -1;

		// Use the engine's point validation and optional LOS/distance rules without
		// changing the global game type or patching its Deathmatch-only outer gate.
		if ( !select( pawn, kSpawnClass, &point ) || !point )
			return -1;
		const auto selected = funrounds::native::detail::HandleFromEntity( point );
		// The engine helper can fall back to a normal team start. That is not a
		// successful random-spawn selection for this API.
		return IsRandomSpawnPoint( selected ) && funrounds::native::detail::EntityFromHandle( selected ) == point ? selected : -1;
	}
	catch ( ... )
	{
		return -1;
	}
}
