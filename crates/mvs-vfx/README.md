# mvs-vfx

The Cascade particle player on the CPU: systems read from the store's particles
kind (`data`, `template`), spawned and ticked (`player`, `system`, `emitter`, `distribution`, `math`), and each
emitter handed to the renderer as an `EmitterDraw` in the build's particle vertex layout (`draw`). No GPU code.

The player's random draws come from one shared stream, so recorded pixels cannot match: the player is
checked by a recorded particle tape of its draws.
