# Graphics Pipeline — Discussion Summary and Decisions

## Purpose

This document records the goals, constraints, and decisions established in discussion so far. It separates **confirmed decisions** from **ideas under consideration** and **unresolved details**. It is not a finalized rendering specification.

## Project goals and constraints

- Build a custom graphics pipeline for a low-poly jet-fighting game.
- Prioritize performance and memory use, including compatibility with older integrated GPUs.
- Use a bespoke, stylized shader/material approach rather than pursuing a complex physically based rendering (PBR) pipeline.
- Development time is limited, so avoid unnecessary up-front planning while preserving important technical decisions.
- The world terrain is arbitrary geometry, **not a heightfield**.
- The sun direction is chosen by the host and then remains static for the relevant level/session.
- Use the developer's integrated GPU as the initial medium-performance baseline.
- Initially target 2K resolution at 120 FPS. This is an ambitious working target that may be revised after measurement.
- Target a light memory footprint of approximately 512 MB, with 1 GB as the upper working allowance.
- Clouds are outside the immediate scope.

## Confirmed shadowing decisions

### Static terrain

- Do **not** use a terrain shadow map. This is a firm constraint.
- The terrain is the static shadow caster.
- Shadow-volume geometry is a construction and baking tool, not normally a per-frame stencil-rendering technique.
- Since the sun direction is fixed, terrain shadow-volume geometry is constructed once after the sun direction is chosen.
- The constructed volumes are used to bake static shadow values into terrain vertices and ground-cover data.
- Normal rendering consumes those baked values and does not render the static shadow volumes every frame.
- The volumes might later be used to determine terrain shadows on moving aircraft, but this is optional and subordinate to performance requirements.

### Aircraft and dynamic objects

- Aircraft may use **local shadow maps** for their own dynamic shadows. The prohibition on shadow maps applies to terrain shadowing, not aircraft. This is not required for the initial renderer.
- Aircraft local shadow maps are intended to support:
  - self-shadowing;
  - shadows cast onto terrain;
  - shadows cast onto other world objects.
- The number of active dynamic aircraft shadow maps should be limited. An example budget discussed was up to two active maps.
- Shadow maps should be rendered or updated selectively, such as when the aircraft or its shadow is relevant to the view, rather than being updated unconditionally every frame.
- A local shadow map must cover relevant receivers in light space; its coverage should not be restricted to a tight box around the aircraft if terrain or other objects that need its shadow lie outside that box.

## Terrain silhouette and volume considerations discussed

- The terrain's arbitrary geometry means the solution cannot assume a heightfield representation.
- One idea discussed was to model relevant horizon/silhouette angles, including terrain peaks and terrain edges, when constructing the static terrain shadow representation.
- Valleys might be excluded from the *caster-silhouette representation* where they do not affect the relevant silhouette. This was an idea, not a fully specified or validated rule.
- Do not infer from this that valleys or other terrain regions can automatically be omitted as shadow receivers.
- The correct construction method, topology, handling of open terrain boundaries, and shadow-volume robustness remain to be worked out.

## Renderer style and immediate scope

- Favor a low-poly, stylized look and a custom shader approach.
- Avoid adopting complex PBR solely because it is common in other engines.
- Performance and memory are the primary constraints; simplicity of implementation is not the overriding goal.
- The initial renderer must support moving 3D jets, static 3D terrain, and static ground cover.
- Terrain and ground cover may use specialized static-geometry optimizations.
- Bullets, rockets, explosions, and similar short-lived effects should use a custom dynamic, instance-based particle-like system.
- Clouds are on the immediate cutting-room floor and should not influence the initial architecture.

## Pipeline outline discussed — provisional, not a set of decisions

The following was suggested as a possible front-to-back structure, but has **not** been confirmed as the final pipeline:

1. **One-time level preprocessing**
   - Load geometry and build GPU buffers.
   - Construct static terrain shadow-volume geometry after the sun direction is set.
   - Use it to bake static visibility into terrain vertices and ground-cover data.
   - Discard or retain the construction geometry according to whether later dynamic-aircraft queries justify its memory cost.
   - Prepare instance data and other GPU resources.
2. **Per-frame CPU preparation**
   - Update simulation and camera.
   - Cull objects and assemble visible draw lists.
   - Select which aircraft shadow maps need updating.
3. **Dynamic aircraft shadow maps**
   - Update selected maps and reuse valid maps where possible.
4. **Main scene rendering**
   - Render terrain and other opaque geometry using the selected lighting/material approach.
   - Read baked visibility for terrain and ground-cover shadowing; do not draw the static shadow volumes per frame.
   - Render aircraft and other dynamic objects with the dynamic-shadow features that survive performance testing.
5. **Effects and presentation**
   - Render instance-based bullets, rockets, explosions, transparency, and related effects in an order appropriate to their implementation.
   - Apply optional post-processing, resolve if needed, and present.

The exact pass ordering, baked-visibility representation, receiver handling, transparency ordering, and resource/synchronization details have not been decided. The earlier outline should not be treated as a completed rendering specification.

## Known risks and unresolved questions

- **Shadow-bake construction:** establish how arbitrary terrain caster silhouettes, boundaries, and closed volumes will be generated robustly and how long the one-time bake may take.
- **Static terrain shadows on moving aircraft:** if retained, determine an efficient way to classify dynamic receivers against the static shadow representation.
- **Interaction between shadow systems:** define how the static terrain shadow-volume method and aircraft local shadow maps coexist for terrain, aircraft, and other objects.
- **Dynamic shadow-map budget:** the example limit of two active maps is a candidate budget, not necessarily a final fixed value.
- **Map update policy:** define what makes a map relevant, stale, or safe to reuse.
- **Baked visibility representation:** specify encoding, precision, interpolation, and how it is combined with direct lighting.
- **Material and lighting model:** exact shader families, lighting terms, fog, and color treatment are not finalized.
- **Transparency, effects, and post-processing:** their representations and pass ordering are not finalized.
- **Target measurement:** record the exact integrated-GPU model and confirm whether "2K" means 2560x1440 or 2048x1080 before benchmarking.
- **Frame budget:** 120 FPS provides approximately 8.33 ms for an entire frame, including simulation and rendering.
- **Memory accounting:** decide whether the 512 MB to 1 GB target measures total process memory, GPU allocations, or both.

## Explicit non-decisions

The discussion has **not** established that:

- the terrain is a heightfield;
- terrain shadow maps are acceptable;
- every terrain valley can be ignored;
- the main renderer must use a particular forward/deferred architecture;
- the proposed render-pass order is final;
- two aircraft shadow maps is a mandatory final budget;
- post-processing or a particular transparency method is required.

## Working principle

Keep the design focused on decisions that materially affect performance and memory. Prototype and measure the shadow-volume baking approach early; do not mistake the provisional pipeline outline for a completed specification. Prioritize a lean renderer for jets, static terrain, static ground cover, and instanced combat effects before optional dynamic shadows or clouds.
