//! Addresses and field offsets for the Steam build of Ride 4 (app 1259980).
//!
//! Found on 2026-10-22 with cue-driven memory scans (rider holds inputs on request, the scanner
//! keeps addresses that follow), then mapped onto Unreal objects by walking GUObjectArray and
//! naming each object through the FNamePool. A game update may move any of these.

/// The exe always loads here (no ASLR).
pub const IMAGE_BASE: u64 = 0x1_4000_0000;
/// `.data` section, used to rescan for GWorld if the fixed address stops resolving.
pub const DATA: (u64, u64) = (0x1_4570_6000, 0x1_45ce_8000);
/// `UWorld*` globals. The first is the live one; the second read null after a relaunch.
pub const GWORLD: [u64; 2] = [0x1_45c8_0d80, 0x1_45c7_d670];

// Pointer chain: GWorld -> UWorld -> GameInstance -> LocalPlayers[0] -> PlayerController -> Pawn
pub const WORLD_GAME_INSTANCE: u64 = 0x188;
/// `TArray<ULocalPlayer*>`: data pointer, then count.
pub const GI_LOCAL_PLAYERS: u64 = 0x38;
pub const LP_PLAYER_CONTROLLER: u64 = 0x30;
/// `AController::Pawn`: the player's `BaseMotorBike_C` (ghosts are separate actors).
pub const PC_PAWN: u64 = 0x250;

// BaseMotorBike_C components
/// RootComponent, an `IgnitionSkeletalMeshComponent`.
pub const BIKE_MESH: u64 = 0x130;
/// `IgnitionVehicleMovementComponent`.
pub const BIKE_MOVEMENT: u64 = 0x290;
/// `IgnitionBikeComponent`.
pub const BIKE_IGNITION: u64 = 0x8f0;
/// `BikeAudioComponent_BP_C` (a blueprint variable, so the likeliest to move).
pub const BIKE_AUDIO: u64 = 0x1070;

// Fields
/// `ComponentToWorld`: quaternion x, y, z, w (f32).
pub const MESH_ROTATION: u64 = 0x1c0;
/// `ComponentToWorld`: translation in cm (f32 x3).
pub const MESH_TRANSLATION: u64 = 0x1d0;
/// Raw throttle input, 0..1. Brake (0..1) and rear brake (0/1) follow at +4 and +8.
pub const MOVE_THROTTLE: u64 = 0x1dc;
/// Engaged gear, 0 = neutral (a second gear field elsewhere reads 1 in neutral).
pub const IGNITION_GEAR: u64 = 0x264;
/// Engine speed as a fraction, idle (0) to limiter (1). Not absolute rpm.
pub const AUDIO_RPM: u64 = 0x238;

// UObject layout (UE 4.25)
pub const UOBJECT_CLASS: u64 = 0x10;
pub const UOBJECT_NAME: u64 = 0x18;
