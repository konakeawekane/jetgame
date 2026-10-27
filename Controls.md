# Here is the plan for the control schemes

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
* F is swap from guns to rockets

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
* F is swap from guns to rockets

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
* F is swap from guns to rockets

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
* F is swap from guns to rockets
