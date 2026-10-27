# Here is the plan for the control schemes

## Shared menu and pause controls

* P opens the pause menu during gameplay and resumes from the pause menu, regardless of flight control scheme.
* Up and Down navigate menu controls.
* Left and Right adjust selected values, or navigate controls when no adjustment is available.
* Enter activates the focused control, like a mouse left click.
* Escape returns to the previous menu.
* Host name fields accept text after Enter or a click; Backspace erases text and Enter finishes editing.

## Keyboard only (realistic)
(throttle is not sticky and returns to medium input if not changed)
(roll, pitch, and yaw are also not sticky and return to 0 input if not changed)

* W is high throttle
* S is low throttle
* A is Yaw left
* D is Yaw right
* right arrow is roll right
* left arrow is roll left
* down arrow is pitch up
* up arrow is pitch down
* space is fire
* F switches between guns and missiles (guns selected on spawn)

## Keyboard only (simple)
(throttle is not sticky and returns to medium input if not changed)
(roll, pitch, and yaw are also not sticky and return to 0 input if not changed)

* W is high throttle
* S is low throttle
* A is Yaw left
* D is Yaw right
* E is roll right
* Q is roll left
* right arrow is roll right
* left arrow is roll left
* down arrow is pitch down
* up arrow is pitch up
* space is fire
* F switches between guns and missiles (guns selected on spawn)

## Keyboard and mouse (realistic)
(throttle is not sticky and returns to medium input if not changed)
(yaw is also not sticky and return to 0 input if not changed)
(the mouse is held to a radius from the screen and moving it works like setting the position of the mouse in that radius like a virtual joystic. This contol is considered sticky and does not reset during an appcense of input.)

* W is high throttle
* S is low throttle
* A is Yaw left
* D is Yaw right
* E is roll right
* Q is roll left
* Mouse x in the joystick maps to roll
* Mouse y in the joystick maps to pitch inverted
* space is fire
* F switches between guns and missiles (guns selected on spawn)

## Keyboard and mouse (simple)
(throttle is not sticky and returns to medium input if not changed)
(yaw is also not sticky and return to 0 input if not changed)
(Uses a point direct mode. So you have a pointer that marks where your plane computer seeks to aim. The plane computer chooses the quickest rotation controls to point the plane toward the aim pointer.)

* W is high throttle
* S is low throttle
* A is Yaw left (manual override of computer control)
* D is Yaw right (manual override of computer control)
* E is roll right (manual override of computer control)
* Q is roll left (manual override of computer control)
* plane computer controls Yaw, Pitch, and Roll and adapts around active manual overrides
* space is fire
* F switches between guns and missiles (guns selected on spawn)

## Weapons and radar

* Every aircraft carries both guns and missiles. The spawn menu does not choose a weapon.
* Space fires the selected weapon, subject to ammunition and its cooldown.
* A missile can always launch without a radar lock and will then fly unguided.
* Radar automatically acquires an enemy ahead, in range, and with clear line of sight.
* Current tuning: 2,800 m range, 30-degree acquisition half-angle, 45-degree tracking half-angle, and 1.25 seconds to acquire a lock.
* A lock breaks immediately when tracking angle, range, line of sight, or target validity fails.
* Missiles launched while locked home independently toward that target, even if the firing aircraft later loses its radar lock.
* Outgoing lock, incoming radar lock, and incoming missile warnings have distinct synthesized tones. Master Volume controls these cues.
* Mouse flight captures the cursor; P or Escape releases it and opens pause. Losing window focus also pauses and clears held input.
