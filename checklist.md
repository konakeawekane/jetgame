# Jet Dogfighting Game Asset Checklist

Use this document to track all art, audio, interface, design, and technical materials required for the game. Check an item only when its final file and usage rights are available.

## 1. Visual Style and Delivery Standards

- [ ] Visual style guide (realistic, stylized, low-poly, etc.)
- [ ] Color palette, including team and accessibility colors
- [ ] Reference images or mood board
- [ ] Target screen resolutions and aspect ratios
- [ ] Supported platforms and minimum hardware target
- [ ] Asset naming and folder conventions
- [ ] License and attribution record for every third-party asset
- [ ] 3D delivery standard confirmed (`.glb` or `.gltf`, scale, forward axis, up axis)
- [ ] 2D delivery standard confirmed (transparent `.png` or sprite sheets)
- [ ] Audio delivery standard confirmed (prefer `.wav`; `.ogg` acceptable for music)

## 2. Aircraft and Weapons

- [x] Player jet model or sprite
- [ ] Enemy/team jet variants or team-color materials
- [x] Jet textures and materials
- [ ] Jet collision shape or documented hitbox dimensions
- [ ] Guns and gun mounting visuals
- [ ] Rockets and rocket mounting visuals
- [ ] Bullet/tracer projectile assets
- [ ] Rocket projectile assets
- [ ] Jet contrails
- [ ] Projectile trails
- [ ] Muzzle flashes
- [ ] Bullet impact effects
- [ ] Rocket impact effects
- [ ] Explosion effects
- [ ] Smoke effects
- [ ] Damaged jet visuals
- [ ] Destroyed jet/debris visuals

## 3. Environment and Match Objects

- [ ] Sky or skybox
- [ ] Cloud assets
- [ ] Terrain, ocean, or arena background
- [ ] Ground obstacles
- [ ] Decorative environment objects
- [ ] Map boundary visuals
- [ ] Team spawn-point markers
- [ ] Objective markers and objective objects
- [ ] Map boundaries and playable-area dimensions documented
- [ ] Spawn locations documented

## 4. Branding and Shared Interface Assets

- [ ] Game title and logo
- [ ] Application/window icon
- [ ] Main-menu background
- [ ] Loading-screen background or artwork
- [ ] Font files and font-use permissions
- [ ] Mouse cursor
- [ ] Button states (normal, hover, pressed, disabled, focused)
- [ ] Panels, borders, and dialog styles
- [ ] Sliders, checkboxes, dropdowns, and text fields
- [ ] Scrollbar and list-row styles
- [ ] Selected/unselected control indicators
- [ ] Error, warning, success, and information icons

## 5. Screen-Specific Interface Content

### Splash Screen

- [x] Splash artwork/logo
- [ ] Loading/progress indicator

### Start Menu

- [ ] Play/server-browser control
- [ ] Settings control
- [ ] Quit control

### Settings Screen

- [ ] Graphics settings layout
- [ ] Audio settings layout
- [ ] Gameplay settings layout
- [ ] Control-scheme selector
- [ ] Key-binding display/editor
- [ ] Apply, reset, and back controls

### Server Browser

- [ ] Server-list layout and column labels
- [ ] Server status, ping, player-count, and lock icons
- [ ] Refresh, join, host, and back controls
- [ ] Empty-list and connection-error states

### Joining Match Loading Screen

- [ ] Joining/loading indicator
- [ ] Server and match information layout
- [ ] Cancel/failure presentation

### Hosting Match Menu

- [ ] Server name and configuration fields
- [ ] Game-mode and map selectors
- [ ] Player-limit and privacy/password controls
- [ ] Host/start and back controls

### Waiting Match Start Lobby

- [ ] Player roster layout
- [ ] Team assignment/readiness indicators
- [ ] Host controls and start-match control
- [ ] Leave/back-to-browser control

### In-Game Spawn Menu

- [ ] Team and spawn selector
- [ ] Jet selector (guns and missiles are standard equipment; no spawn weapon selection)
- [ ] Spawn control
- [ ] Spectator or unavailable-spawn states, if used

### In-Game HUD

- [ ] Crosshair
- [ ] Targeting reticle
- [ ] Aim-pointer graphic for simple mouse controls
- [ ] Virtual-joystick radius/position indicator for realistic mouse controls
- [ ] Missile-lock indicators
- [ ] Health and armor indicators, if used
- [ ] Fuel and boost indicators, if used
- [ ] Throttle indicator
- [ ] Speed and altitude indicators
- [ ] Gun and rocket selection indicators
- [ ] Ammunition indicators
- [ ] Radar/minimap graphics
- [ ] Player, teammate, enemy, and objective markers
- [ ] Kill-feed icons and layout
- [ ] Scoreboard layout
- [ ] Network latency/disconnection indicator
- [ ] Damage direction and warning indicators

### Pause Menu

- [ ] Resume control
- [ ] Settings control
- [ ] Leave-match control and confirmation dialog

### End-of-Game Report

- [ ] Victory and defeat presentation
- [ ] Score/statistics layout
- [ ] Continue/rematch flow controls

## 6. Audio

- [ ] Jet engine idle/low-speed loop
- [ ] Jet engine high-speed loop
- [ ] Afterburner/boost sound
- [ ] Gunfire sound
- [ ] Rocket launch sound
- [ ] Rocket flight sound
- [ ] Bullet impact sound
- [ ] Rocket impact sound
- [ ] Explosion sound variations
- [ ] Aircraft damage sounds
- [ ] Warning alarms
- [ ] Missile-lock acquisition and locked tones
- [ ] UI hover sound
- [ ] UI click sound
- [ ] UI error sound
- [ ] UI notification sound
- [ ] Match countdown sound
- [ ] Victory sound
- [ ] Defeat sound
- [ ] Menu music
- [ ] Gameplay music
- [ ] Radio or announcer voice lines, if used

## 7. Controls to Implement and Verify

- [ ] Keyboard-only realistic scheme
- [ ] Keyboard-only simple scheme
- [ ] Keyboard-and-mouse realistic scheme
- [ ] Keyboard-and-mouse simple scheme
- [ ] Throttle returns to medium without input
- [ ] Direct roll, pitch, and yaw return to zero without input
- [ ] Realistic mouse uses a sticky, radius-limited virtual joystick
- [ ] Realistic mouse X maps to roll
- [ ] Realistic mouse Y maps to inverted pitch
- [ ] Simple mouse aim uses War Thunder-style automatic yaw, pitch, and roll to seek the pointer
- [ ] Manual yaw/roll inputs override the relevant automatic steering inputs
- [ ] Automatic steering adapts around active manual overrides
- [ ] Space fires the selected weapon
- [ ] F swaps between guns and rockets
- [ ] Control remapping requirements decided
- [ ] Controller/gamepad support decision recorded

### Confirmed Control Requirements

- [x] `A = yaw left` and `D = yaw right`
- [x] Keyboard-realistic uses joystick-style inverted pitch: `Down = pitch up`, `Up = pitch down`
- [x] Keyboard-simple uses non-inverted pitch: `Down = pitch down`, `Up = pitch up`
- [x] Keyboard-simple intentionally supports both `E/Q` and arrow keys for roll
- [x] Realistic mouse intentionally supports both `E/Q` roll and mouse-X roll
- [x] Manual controls override automatic steering while the computer adapts on the fly
- [ ] Define handling when opposing or multiple manual override inputs occur simultaneously
- [ ] Define mouse virtual-joystick radius, sensitivity, centering, and cursor capture

## 8. Game and Networking Design Information

- [ ] Jet movement and durability statistics
- [ ] Gun statistics (damage, fire rate, spread, range, ammunition)
- [ ] Rocket statistics (damage, speed, tracking, lock time, ammunition)
- [ ] Available loadouts documented
- [ ] Game modes documented
- [ ] Scoring rules documented
- [ ] Spawn and respawn rules documented
- [ ] Match-start and victory conditions documented
- [ ] Maximum player count and team structure documented
- [ ] Dedicated-server versus player-hosted model decided
- [ ] Lobby and server-browser behavior documented
- [ ] Server region requirements documented
- [ ] Server tick rate selected
- [ ] Latency tolerance and interpolation targets selected
- [ ] Client/server authority and anti-cheat expectations documented
- [ ] Disconnect, reconnect, timeout, and host-loss behavior documented
- [ ] Chat requirements documented
- [ ] Account/authentication requirements documented
- [ ] Persistent statistics or progression requirements documented

## 9. Screen Navigation Verification

- [ ] Splash screen to start menu
- [ ] Start menu to settings screen
- [ ] Start menu to server browser
- [ ] Settings screen to start menu
- [ ] Server browser to start menu
- [ ] Server browser to joining-match loading screen
- [ ] Server browser to hosting-match menu
- [ ] Joining-match loading screen to spawn menu
- [ ] Joining-match loading screen to waiting lobby
- [ ] Waiting lobby to spawn menu
- [ ] Waiting lobby to server browser
- [ ] Spawn menu to gameplay
- [ ] Gameplay to spawn menu
- [ ] Gameplay to pause menu
- [ ] Pause menu to gameplay
- [ ] Gameplay to end-of-game report
- [ ] End-of-game report to hosting-match menu
- [ ] End-of-game report to waiting lobby

## 10. Optional Polish Assets

- [ ] Multiple jet skins
- [ ] Pilot portraits or player avatars
- [ ] Additional explosion and impact variations
- [ ] Cinematic camera effects
- [ ] Environmental weather variations
- [ ] Tutorial illustrations or control diagrams
- [ ] Accessibility icons and high-contrast HUD variant
- [ ] Promotional screenshots, banner, and store artwork
