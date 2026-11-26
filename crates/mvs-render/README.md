# mvs-render

The renderer on Direct3D 11. The build's cooked vertex and pixel shaders are created unchanged from the store and fed
the build's own inputs: the View, GPUScene primitive and lightmap rows (`view`), the vertex factories and the material
uniform buffers (`material`). `gpu` holds the device and its caches, `assets` the store's meshes, textures and
materials on the GPU, `draw` the draw list and its executor, `targets` the deferred path's scene targets,
`renderer` the frame. The stage (`scene`), deferred lighting (`lighting`), translucency (`translucency`), post
(`post`), fighters (`fighters`), particles (`particles`) and the effects the sim's events start (`fx`) build on them.

`src/bin/render-frame.rs` draws one held frame offscreen (a trace match at a tick, or a pose tape's frame), writes
its PNG and times it.

`src/bin/preview.rs` draws a look (`src/look.rs`: a `look.json` and a file of pixel shaders, with lights, meshes,
particle systems and camera changes) into a recorded match on any stage, as a still or a short clip: a visual idea
seen in the real renderer before it is built as a feature. Nothing draws a look in a match.
