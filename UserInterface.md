# Here is the plan for the user interface

## All Screens
* splash screen
* start menu
* settings screen
* server browser
* joining match loading screen
* hosting match menu
* in game spawn menu
* in game (just the literal gameplay)
* in game pause menu
* end of game report screen
* waiting match start lobby screen

## All flow/navigation in Screens
* splash screen -> start menu
* start menu -> settings screen
* start menu -> server browser
* settings screen -> start menu
* server browser -> start menu
* server browser -> joining match loading screen
* server browser -> hosting match menu
* hosting match menu -> server browser (cancel)
* hosting match menu -> waiting match start lobby screen (create preview lobby)
* joining match loading screen -> in game spawn menu
* joining match loading screen -> waiting match start lobby screen
* ingame spawn menu -> in game
* in game -> in game spawn menu
* in game -> in game pause menu
* in game -> end of game report screen
* in game pause menu -> in game
* end of game report screen -> hosting match menu
* end of game report screen -> waiting match start lobby screen
* waiting match start lobby screen -> in game spawn menu
* waiting match start lobby screen -> server browser

## Global connection-loss rule

* If the connection to the game server is lost at any time during a game, return to the server browser regardless of the active in-game screen or overlay.

## Implemented input and preview behavior

* All listed screens are available through the lightweight UI controller.
* Arrow keys navigate menus and adjust values; Enter activates the focused control like a left click.
* P opens the in-game pause menu and resumes from it. Escape navigates back.
* Settings opened from the pause menu return to that pause menu.
* The server list, joining sequence, lobby roster, results, and simulation-dependent HUD values use explicit offline placeholders.
* A mock joining sequence can lead to the waiting lobby or directly to spawn for an active match.
* Pause-menu preview actions exercise respawning, match completion, and connection loss without a network backend.

## Local gameplay integration

* Spawn chooses team/aircraft only. Every plane carries guns and missiles; guns are selected on spawn and F switches weapons during flight.
* Gameplay now runs the local aircraft simulation and combat world, with live HUD values and match results.
* P/Escape pauses the local game; losing focus also pauses and clears controls. A future remote server would maintain its own simulation.
* The server browser and network-session flow remain offline placeholders. Simulate Link Loss still returns globally to the server browser.
