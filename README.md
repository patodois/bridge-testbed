# bridge-testbed

A [Bevy](https://bevy.org) 0.20 app that loads assets from every [rewrite-bridge](https://github.com/patodois/rewrite-bridge)
provider side by side, so that scale, orientation, animation, materials and physics can be checked visually across
games. It also renders fixed shots headless, for visual regression.

It spawns what the providers return, unchanged, through `rewrite-bridge-bevy` (no mesh or material conversion of its
own). Everything that is not game data is the testbed's and is labelled as such: the reference objects, the lighting,
the camera and every physics value.

It needs the owner's own game installs and a `testbed.toml` that points at them. CI builds and lints it; it has no
game data and does not run it.

## Run

```sh
cp testbed.example.toml testbed.toml            # then set your game paths (untracked)
cp .cargo/config.toml.example .cargo/config.toml  # then set the absolute paths (untracked), see "One rewrite-bridge"
cargo run --release                             # the gallery (bridge-testbed --config testbed.toml)
cargo run --release -- list --provider hytale --filter Trork     # find ids for the config
cargo run --release -- inspect                  # the numbers of every item as text, no rendering
cargo run --release -- shots --out shots/a      # 512 x 512 shots + manifest.json
cargo run --release -- compare --base shots/a --new shots/b
```

## Config

`testbed.toml` (see [`testbed.example.toml`](testbed.example.toml) for every key):

- `[providers.wt|wf|hytale]`: the game's install (`path`). A provider is opened when its first item loads. One that fails
  to open puts its error at the slots of its items; nothing crashes. `wf` has `max_texture` to keep a scene small.
- `[[items]]`: `provider`, `id`, `kind = "model" | "animated" | "scene"`, `label` (also the shot's file name), and
  optionally `dynamic = true` (a rigid body), `collide = true` (a static trimesh), `collision_id` (a scene's own
  collision), `at = [x, z]` (a fixed place).
- `[gallery]`: `columns`, `gap`, `snap_to_ground`, `scene_lights`. `[physics]`: `density`, `drop_height`, `gravity`.
  `[stack]`: the block the B key stacks.

Ids are the providers' own. Warframe's animated ids carry a clip filter (`<model>?animations=<filter>`, `|` separates
several) and its level collision is the level path plus `?collision`; see wf-unpack's README.

## The gallery

Items load one at a time, on a thread, and are laid out in rows on a ground plane (items face +Z, the camera starts on
that side). Each item has:

- its label and its world AABB in metres, as text and as a box (yellow for the selected item);
- an arrow on the ground, just past the front of its AABB, pointing +Z: the bridge's front. A model that visibly faces
  another way is inconsistent with the contract;
- small axes at its own origin. The testbed does not move an item to the ground (unless `snap_to_ground` is on), so a
  model whose origin is not at its feet shows it.

Reference objects (testbed, not game data): a **1 m cube**, a vertical **2 m ruler with 10 cm ticks** (long ticks every
50 cm and 1 m), and the **axes at the origin** (X red, Y green, Z blue).

**Lighting** is the same for every item (`src/stage.rs`): one directional sun of 10,000 lux from (0.45, 0.8, 0.55) with
shadows, a white ambient of 400 cd/m², Bevy's default camera exposure and tone mapping, a grey ground (sRGB 0.42) and a
dark grey background. A provider scene's own lights are dropped (`scene_lights = false`) and its sky is not applied, so
two games are compared under the same light.

Keys: left drag orbits, right or middle drag pans, wheel zooms, WASD and Q/E move the target; Tab / Shift+Tab selects an
item, F frames it, Home frames everything; V toggles the AABBs, N the grid, L the labels, H the help.

The panel shows, for the selected item, its provider, kind and id, size, node, mesh, material, vertex and triangle
counts, image megabytes, the mean linear albedo of its base colours, its material factors, how many triangles wind
against their stored normals, its collision, and warnings (very dark or bright colours, flipped triangles). These are
hints for a person looking at the picture, not verdicts.

## Animation

An animated item plays its first clip, looping. **C** / **Shift+C** go to the next or previous clip, **P** pauses,
**Z** plays each clip once and moves on. The panel shows the clip's name, its duration, the time and the keyframe
(the number of distinct keyframe times at or before now, of all of them) of the playing clip.

Clips are played as the provider gave them. Warframe's carry **root motion** (the root bone's translation channel):
`inspect` prints how far each clip moves the skeleton's root, and a sprint clip walks Excalibur out of his AABB.

## Physics

Avian 3D 0.8 (the version that supports Bevy 0.20). **The bridge carries no physics data yet, so every physics value
here is a testbed assumption**; the panel says so on screen.

- Gravity 9.81 m/s² (assumption). Friction and restitution are Avian's defaults (assumption). The ground collider is a
  testbed plane at y = 0.
- Scenes get a **static trimesh**: of the provider's own collision shapes when the item has a `collision_id` (real game
  collision, and the panel says "provider's own collision shapes"), otherwise of the render meshes (an assumption).
  Models get one with `collide = true`.
- Items with `dynamic = true` are **convex-hull rigid bodies** of the model at rest (assumption). Mass is **1 kg per m³
  of hull volume** by default (assumption, `[physics] density`). They wait, switched off, at their slot until dropped.
- **Space** drops the selected prop with its lowest point 3 m above the ground (`drop_height`). **B** spawns a pyramid
  of the `[stack]` block (6 Hytale blocks, 3 + 2 + 1). **R** puts every prop back and removes the stack.

## Shots and compare

`bridge-testbed shots --out DIR` renders each item alone with no window (Bevy's offscreen path: no primary window and no
winit, so it also works with the screen locked, and on macOS the result is Metal's), 512 x 512, under the fixed lighting,
from a fixed camera: 35° to the right of the item's +Z front, 22° above the horizon, at the distance where the AABB's
bounding sphere fits a 45° field of view. An animated item is frozen at the start of its first clip. It writes
`DIR/<label>.png` and `DIR/manifest.json` (item, AABB, vertex and triangle counts, placements, clip count, mean albedo,
warnings). Time is stepped by hand, so the same config on the same machine gives byte-identical files.

`bridge-testbed compare --base DIR1 --new DIR2 [--threshold 0.01] [--tolerance 4]` diffs the PNGs per pixel (a pixel
differs when a channel differs by more than `--tolerance` of 255; an item regresses when more than `--threshold` of its
pixels differ), compares the manifests' numbers (AABB, vertices, triangles, clips), prints a table, writes diff images to
`NEW/diff/` and exits nonzero on regressions (also for an item that went missing or began to fail).

Shots are game content: they are never committed (`.gitignore` covers them).

## Scripted captures

`bridge-testbed --script "..." --out DIR` runs the gallery without a window under a script and writes PNGs, so it can
be checked without a person at the keyboard. `key SPACE` presses a key for a frame so that the real key handlers run:

```sh
bridge-testbed --script "frame all; wait 1; shot overview; key tab; key tab; frame Trork; key c; wait 1; shot clip; \
  frame Block 3.5; key space; wait 0.1; shot dropped; wait 2.5; shot landed; key r; key b; wait 4; shot stack" --out captures
```

The commands are listed in `src/capture.rs`.

## Providers

| name | crate | notes |
|---|---|---|
| `wt` | [wt-unpack](https://github.com/patodois/wt-unpack) | vehicles; `load_scene` for levels (a whole map is heavy, about 1.2 GB for Berlin, so none is in the default config) |
| `wf` | [wf-unpack](https://github.com/patodois/wf-unpack) | models, animated ids, level scenes and `?collision` scenes (branch `bridge2`) |
| `hytale` | [hytale-unpack](https://github.com/patodois/hytale-unpack) | entity models with clips, blocks, prefabs |
| `nocturne` | nocturne-unpack | **a slot only**; the crate is still being written |

Each is behind a cargo feature (`wt`, `wf`, `hytale`, all on by default). To wire nocturne: add `nocturne-unpack` with
its `bridge` feature to `Cargo.toml` as an optional dependency, make the `nocturne` feature enable it, and construct its
`Provider` in the `"nocturne"` arm of `open` in `src/providers.rs`. A `[providers.nocturne]` section and items with
`provider = "nocturne"` then work like the others.

### One rewrite-bridge

The providers pin rewrite-bridge at `1b58201`, while `rewrite-bridge-bevy` lives at `4340109`. Cargo would build two
incompatible copies of the crate (`Model` from one would not be `Model` of the other). The library source of the two
revisions is identical, so `.cargo/config.toml` (untracked, absolute paths; template in
[`.cargo/config.toml.example`](.cargo/config.toml.example)) patches `rewrite-bridge` and `rewrite-bridge-bevy` onto one
checkout at `4340109`. `Cargo.lock` then holds exactly one `rewrite-bridge` (`grep -c 'name = "rewrite-bridge"' Cargo.lock`
prints 1). Delete the patch once the providers pin a revision that has `rewrite-bridge-bevy`. CI clones the checkout and
writes the same config.

The testbed uses the animated ids and scenes of wf-unpack's `bridge2` branch, which is not on GitHub yet: `Cargo.toml`
pins wf-unpack's `main` so that the dependency resolves and the config patches in the local branch. CI therefore builds
without the `wf` feature until the branch is pushed; then pin its commit and drop the patch.

## Memory and disk

Items load one at a time and models are moved into Bevy's assets, which drop their CPU copies after upload. With the
example config (a Sherman, Excalibur with six clips, the Trork with 204, a Hytale block and a small prefab) the gallery
peaks at about 1.4 GB resident (2.6 GB footprint, which counts Metal's allocations). A Bevy target directory is about
2.5 GB for `cargo build` and as much again for `cargo clippy`.

## Tests

`cargo test` needs no game data: config parsing, the measurements, the shot camera, the PNG diff and the loader (against a
synthetic provider: models, clips, scenes with asset references, failures).
`cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass.

## License

Licensed under either of MIT or Apache-2.0, at your option.
