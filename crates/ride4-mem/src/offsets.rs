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

// Lap timing (Time Attack), found 2026-10-22: scanned for clocks advancing at wall-clock rate and
// for the lap time the game displayed (2:18.278 -> 138278 ms), then followed outers/references.
/// `UWorld::GameState` (`TimeAttackGameStateBP_C` in Time Attack).
pub const WORLD_GAME_STATE: u64 = 0x130;
/// `HudLogicComponent` on the game state.
pub const GAME_STATE_HUD_LOGIC: u64 = 0x358;
/// f32 seconds: best lap of this session.
pub const HUD_BEST_LAP: u64 = 0xe8;
/// `TimedResultsManagerComponent` (on the competition actor).
pub const HUD_RESULTS: u64 = 0xf8;
/// f32 seconds: race clock. It stops while paused, unlike wall time.
pub const HUD_RACE_CLOCK: u64 = 0x174;
/// `TArray<TimedResultItem*>`, one per participant; the player is first (ghosts stay empty).
pub const RESULTS_ITEMS: u64 = 0xd0;
/// i32: completed laps (the count of the lap-time array at +0x98).
pub const ITEM_LAPS: u64 = 0xa0;
/// i32 milliseconds: the last completed lap, exactly as the game shows it.
pub const ITEM_LAST_LAP_MS: u64 = 0xa8;

// Current lap timer, found 2026-10-23 with `examples/lapclock.rs` (clocks inside the objects above
// and the ones they point to). It ran to 139.29 s, dropped to 0 as the game logged a 139.322 lap,
// and also resets when the lap is restarted without finishing it.
/// `TArray` of per-rider lap timers on `HudLogicComponent`, [`LAP_TIMER_SIZE`] bytes each.
pub const HUD_LAP_TIMERS: u64 = 0x110;
pub const LAP_TIMER_SIZE: u64 = 0x30;
/// f32 seconds into the current lap.
pub const LAP_TIMER_TIME: u64 = 0x08;
/// The rider's pawn: the player's entry points at the player's bike (ghosts have their own).
pub const LAP_TIMER_PAWN: u64 = 0x20;
