# mvs-view

What the renderer draws a tick with, without the GPU: the frame input (`frame`: the camera, each drawn mesh's pose
and materials, `FrameInput`), the views' posing from the `World` (`pose`, `anim`, `skins`: the character
views' posing, the animation nodes and the pose blends), the match camera (`camera`) and the reader of the pose
tape (`tape`).

Posing is render code, not sim: it may compute in `f32`, and is checked against the pose tape within 0.001 cm.
