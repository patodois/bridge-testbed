# bridge-testbed

[![rewrite-bridge on crates.io](https://img.shields.io/crates/v/rewrite-bridge.svg?label=rewrite-bridge)](https://crates.io/crates/rewrite-bridge)
[![rewrite-bridge-bevy on crates.io](https://img.shields.io/crates/v/rewrite-bridge-bevy.svg?label=rewrite-bridge-bevy)](https://crates.io/crates/rewrite-bridge-bevy)

A [Bevy](https://bevy.org) 0.20 app that loads assets from every [rewrite-bridge](https://github.com/patodois/rewrite-bridge)
provider side by side, so that scale, orientation, animation, materials and physics can be checked visually across
games. It also renders fixed shots headless, for visual regression.

It spawns what the providers return, unchanged, through `rewrite-bridge-bevy` (no mesh or material conversion of its
own). Everything that is not game data is the testbed's and is labelled as such: the reference objects, the lighting,
the camera and every physics value.

It needs the owner's own game installs and a `testbed.toml` that points at them. CI builds and lints it; it has no
game data and does not run it.

**NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.** The `mc` provider reads the
block and item models of a Minecraft: Java Edition client jar that you already own, through
[mc-unpack](https://github.com/patodois/mc-unpack). This project contains no Mojang code and no Mojang assets.

## Download

Programs built for each release are on the [Releases page](https://github.com/patodois/bridge-testbed/releases). Pick the archive for your system:

| System | Archive |
|--------|---------|
| macOS, Apple silicon | `bridge-testbed-<tag>-macos-arm64.tar.gz` |
| macOS, Intel | `bridge-testbed-<tag>-macos-x86_64.tar.gz` |
| Linux, x86_64 (glibc 2.35 or newer) | `bridge-testbed-<tag>-linux-x86_64.tar.gz` |
| Windows, x86_64 | `bridge-testbed-<tag>-windows-x86_64.zip` |

Each archive holds the program, this README, both license files and an `INSTALL.txt`. `SHA256SUMS` on the release page has
the checksum of every archive (`shasum -a 256 -c SHA256SUMS` on macOS, `sha256sum -c SHA256SUMS` on Linux,
`Get-FileHash` on Windows). They are built with all five providers (Hytale, Minecraft: Java Edition, Nocturne, War Thunder, Warframe) and include `testbed.example.toml`. The author runs the testbed on macOS; the Linux and Windows builds are made by the release workflow but have not been tried by hand. It needs a GPU with Metal, Vulkan or DirectX 12 support.

**The binaries contain no game data.** They are the program only; it reads the files of copies of the games it loads that you already have
installed.

They are not code-signed, so the first run may be blocked:

- **macOS (Gatekeeper)**: a file downloaded in a browser is quarantined, and macOS refuses to open it ("Apple could not
  verify ..."). Remove the mark once, from the folder you extracted to: `xattr -d com.apple.quarantine ./bridge-testbed`. Or open
  System Settings > Privacy & Security and press "Open Anyway" after the first refusal.
- **Windows (SmartScreen)**: "Windows protected your PC" appears the first time. Choose "More info", then "Run anyway". Or
  right-click the downloaded `.zip`, open Properties and tick "Unblock" before you extract it.

To build it yourself instead, see [Try first](#try-first-no-window).

## Try first (no window)

You need Rust (the stable toolchain from [rustup.rs](https://rustup.rs)), `git`, and at least one of the games installed
(see [Config](#config) for where each keeps its files). **The first build takes about 6 minutes** on an M-series Mac
(Bevy and five game readers are compiled with optimisations) and a Bevy target folder is about 3 GB; later builds take
seconds. Start it on its own, then try the commands that open no window:

```sh
cp testbed.example.toml testbed.toml            # then set your game paths (untracked); see Config
cargo build --release                           # the ~6 minutes
cargo run --release -- list --provider hytale --filter Trork --limit 10   # ids a game offers (put them in the config)
cargo run --release -- inspect                  # the numbers of every item as text, no rendering
cargo run --release -- shots --out shots/a      # one 512 x 512 PNG per item + manifest.json; no window
```

`inspect` and `list` need no graphics at all; `shots` renders offscreen (it needs a GPU, not a screen). Only when those
look right, open the gallery.

## Run

```sh
cargo run --release                             # the gallery (bridge-testbed --config testbed.toml)
cargo run --release -- list --provider hytale --filter Trork --limit 10   # find ids for the config
cargo run --release -- inspect                  # the numbers of every item as text, no rendering
cargo run --release -- shots --out shots/a      # 512 x 512 shots + manifest.json
cargo run --release -- shots --out shots/s --no-ground --unlit   # silhouettes: RGBA, base colour only
cargo run --release -- physics --out rest.json  # every dynamic body's rest pose, as JSON
cargo run --release -- compare --base shots/a --new shots/b
```

`list` prints at most `--limit` ids (default 40; the heading says how many matched) and, when there are more, how many
it left out. Narrow it with `--filter`, which matches the id or the name.

When the output is not a terminal (a file, a pipe, CI), the logs have no ANSI colour codes; `NO_COLOR=1` turns them off in
a terminal too.

## Config

`testbed.toml` (see [`testbed.example.toml`](testbed.example.toml) for every key):

- `[providers.wt|wf|hytale|nocturne|mc]`: the game's install, as `path`. A provider is opened when its first item loads. One
  that fails to open puts its error at the slots of its items; nothing crashes. `wf` has `max_texture` to keep a scene small.

  | provider | `path` | where it is |
  |---|---|---|
  | `wt` | War Thunder's `game` folder | macOS: `/Applications/WarThunderLauncher.app/Contents/WarThunder.app/Contents/Resources/game`. Elsewhere: the `game` folder of the install, the one that holds `aces.vromfs.bin` |
  | `wf` | Warframe's `Cache.Windows` folder | the one that holds the `.toc` and `.cache` pairs (`H.Texture.toc`, ...); Warframe's Windows files, copied to your Mac if you play there (see [wf-unpack](https://github.com/patodois/wf-unpack)) |
  | `hytale` | Hytale's `Assets.zip` | **optional**: leave `path` out to use the launcher's install on macOS (`~/Library/Application Support/Hytale/install/release/package/game/latest/Assets.zip`); set it for any other location |
  | `nocturne` | the game folder that holds `smt3hd_Data` | wherever you installed it; [nocturne-unpack](https://github.com/patodois/nocturne-unpack) says how to get the Windows build onto a Mac or Linux machine |
  | `mc` | a Minecraft: Java Edition **client jar** | **optional**: leave `path` out to use the newest jar a launcher installed that mc-unpack can read (1.12.2 to 26.3). Prism Launcher, macOS: `~/Library/Application Support/PrismLauncher/libraries/com/mojang/minecraft/<v>/minecraft-<v>-client.jar`; the vanilla launcher keeps it at `versions/<v>/<v>.jar` under `~/Library/Application Support/minecraft` (macOS), `~/.minecraft` (Linux) or `%APPDATA%\.minecraft` (Windows). Set `path` to pin one version or for any other place. A modded profile's jar holds no assets and is refused |

  A leading `~` or `~/` in a `path` is your home folder (`$HOME`, on Windows `%USERPROFILE%`), so `path = "~/Games/smt3hd"`
  works; `~other` is not expanded. Other relative paths are relative to where you run the program.
- `[[items]]`: `provider`, `id`, `kind = "model" | "animated" | "scene"`, `label` (also the shot's file name), and
  optionally `dynamic = true` (a rigid body), `collide = true` (a static trimesh), `collision_id` (a scene's own
  collision), `at = [x, z]` (a fixed place).
- `[gallery]`: `columns`, `gap`, `snap_to_ground`, `scene_lights`. `[physics]`: `density`, `drop_height`, `gravity`.
  `[stack]`: the block the B key stacks.

**One game only.** Keep its `[providers.<name>]` section and its `[[items]]`, and delete the other games' sections and
items (and `[stack]`, which names a Hytale block, if you removed Hytale: the B key then does nothing). A provider with no
section is not opened: if an item still names it, that item shows `provider "wt" has no [providers.wt] section in the
config` at its slot, `inspect` prints it as an `ERROR` line and `shots` writes no PNG for it, counts it as failed and
says so in `manifest.json`; the other items are unaffected. A provider with a section whose install is not there says
`<name> failed to open: <reason>` in the same places. To build only one reader (a shorter build), use
`cargo run --release --no-default-features --features hytale`; an item or section that names a reader left out then reports
that this build has no such provider.

Ids are the providers' own. Warframe's animated ids carry a clip filter (`<model>?animations=<filter>`, `|` separates
several) and its level collision is the level path plus `?collision`; Nocturne's are the bundle name (`dvl0x01`) for a
demon and `bundle/prefab` (`f014/f014_012`) for a field prefab; see the providers' READMEs.

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
- **Static colliders use the game's own collision wherever the provider has it, and say so in the panel:**
  - Warframe: a scene item with `collision_id = "<level>?collision"` collides with the PhysX shapes of the level.
  - War Thunder: a level scene collides with its terrain model (the landmesh's collision mesh) and, at every placement,
    the object's own collision resource (`load_level_collision`: triangle meshes, convex hulls and boxes as one static
    trimesh, tree trunks as capsule colliders). A `collide = true` model item tries `load_object_collision` for its
    model name (or `collision_id`); vehicles have none, and the panel says so.
  - Everything else (Hytale, Nocturne, Minecraft, and a model or scene the provider has no collision for) gets a **static
    trimesh of the render meshes**, which is a testbed assumption and is labelled "not game collision". **Nocturne has no
    collision data in the bridge**, and **Minecraft has none in this provider** (a block's collision shape is in the
    game's code, not in the jar's models, and nothing is invented); their panels say so. A Minecraft block is therefore
    a trimesh of its model: a full cube is a 1 m box, a stair is its drawn boxes.
- Items with `dynamic = true` are **convex-hull rigid bodies** of the model at rest (assumption). Mass is **1 kg per m³
  of hull volume** by default (assumption, `[physics] density`). They wait, switched off, at their slot until dropped.
- **Space** drops the selected prop with its lowest point 3 m above the ground (`drop_height`). **B** spawns a pyramid
  of the `[stack]` block (6 Hytale blocks, 3 + 2 + 1). **R** puts every prop back and removes the stack.

### Rest poses (`physics --out FILE`)

`bridge-testbed physics --out rest.json [--seconds 6]` runs the same scenes as the Space and B keys without a window and
without a person, and writes where every body ends up, so that another implementation of the same scenes can be compared
with it. Each `dynamic = true` item that has a convex hull is dropped alone (its lowest point `drop_height` above the
ground, as Space does) and the world runs for `--seconds` (default 6); then everything is put back and the `[stack]`
pyramid is built and run for the same time. The file holds the settings (`step_s`, `gravity_m_s2`, `density_kg_m3`,
`drop_height_m`) and, per scene (`drop` or `stack`), per body: `position` (the body's own origin, metres), `rotation` (a unit
quaternion `[x, y, z, w]`, `w >= 0`), `sleeping`, `speed_m_s` (about zero at rest) and, for a drop, `start_position`. Stack
blocks are `stack block 0..N` in the order the pyramid is built (bottom layer first, left to right).

It is deterministic: every frame is one physics step of exactly 1/60 s (a fixed step, not the gallery's real-time 64 Hz),
time is stepped by hand, and the same config on the same machine gives a byte-identical file (checked by running it twice).
It needs a GPU to load the models, not a screen. Every number is a testbed assumption (see above).

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

For a reference comparison, two flags change what a shot shows (both are recorded in `manifest.json`, and `compare` says when
the two folders were taken with different flags):

- `--no-ground`: no ground plane, so the item floats on the background and nothing but the item is in the picture.
- `--unlit`: base colour only, for cutting out a silhouette. Every material is drawn unlit (its base colour times its
  texture and vertex colours; no lights, shadows, emissive or normal maps), there is no sun, no tonemapping or dithering and no
  anti-aliasing, a scene's own sky is dropped, and the background is **transparent**: the PNGs are RGBA, and every pixel's alpha
  is 0 or 255, so `alpha > 0` is the silhouette. (A translucent material keeps its own alpha.) Combine it with `--no-ground`
  for a clean cut-out; alone it keeps the ground, which then is part of the silhouette. The camera is the same as without the flags.

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
| `wt` | [wt-unpack](https://github.com/patodois/wt-unpack) `0.2.0` (`f9f0515`) | vehicles; `load_scene` for levels (a whole map is heavy, about 1.2 GB for Berlin, so none is in the default config); the game's collision of objects and levels |
| `wf` | [wf-unpack](https://github.com/patodois/wf-unpack) `0.2.0` (`5db618f`) | models, animated ids, level scenes and `?collision` scenes. A level has **no sky by default** (`scene.environment` is `None`; `inspect` prints a `sky` line for every scene). wf-unpack's `SceneOptions::lighting_environment` is opt-in and captures the level's lighting, which is not the drawn sky; the testbed leaves it off, and in any case never applies a scene's sky |
| `hytale` | [hytale-unpack](https://github.com/patodois/hytale-unpack) `0.2.0` (`889a70f`) | entity models with clips, blocks, prefabs. An entity wears its default attachments (hats, hair, eyes, clothes), merged into the body's nodes, so a character is no longer bare |
| `nocturne` | [nocturne-unpack](https://github.com/patodois/nocturne-unpack) `49df6cc` | demons with the clips of their animator, field prefabs as scenes; no collision data |
| `mc` | [mc-unpack](https://github.com/patodois/mc-unpack) `0.1.0` (crates.io) | Minecraft: Java Edition 1.12.2 to 26.3, from the client jar: blocks (`block/stone`, `block/oak_stairs?variant=facing=east,half=bottom,shape=straight`) and items (`item/diamond_sword`), 1 block = 1 m, `Prop` only; no entities, no animation, no collision data. Its textures are filtered nearest in glTF, but the bridge's `Material` has no sampler, so the testbed shows them linearly filtered (blurred pixel art) |

Each is behind a cargo feature (`wt`, `wf`, `hytale`, `nocturne`, `mc`, all on by default). A provider that is not configured,
or whose game is not installed, shows an error label at its items' slots and nothing else.

### Dependencies

Every dependency is a public crate or a public git revision; there is nothing to patch locally. `mc-unpack` is taken from crates.io (`0.1.0`). `rewrite-bridge` and
`rewrite-bridge-bevy` come from [crates.io](https://crates.io/crates/rewrite-bridge) (`0.2.2`), and all five providers ask
crates.io for `rewrite-bridge` `0.2` (`^0.2.1`) too, so Cargo builds one copy: `cargo tree -i rewrite-bridge` prints a single
`rewrite-bridge v0.2.2` whose children are the testbed, the providers and `rewrite-bridge-bevy`, and
`grep -c 'name = "rewrite-bridge"' Cargo.lock` prints 1. Move the providers' revisions
together, and keep them on the same `0.x` line of rewrite-bridge as the two lines above: a provider on another line would
be a second copy.

nocturne-unpack reads Unity 2019.4 bundles with a patched fork of rabex 0.2.1 that it takes as a git dependency itself
(see its `NOTES-upstream.md`), so nothing is repeated here. Drop it when upstream rabex opens these bundles.

## Memory and disk

Items load one at a time and models are moved into Bevy's assets, which drop their CPU copies after upload. With the
example config (a Sherman, Excalibur with six clips, the Trork with 204, a Hytale block and a small prefab, a Nocturne
demon with five clips and a field prefab) the gallery peaks at about 1.65 GB resident (2.35 GB footprint, which counts
Metal's allocations); `shots` peaks at 1.4 GB. A War Thunder level with its collision (`arcade_norway_plain`) adds about
0.5 GB resident. The `mc` provider reads the client jar's zip directory and inflates single entries on demand (the jar is never
extracted): listing 194 stairs took about 75 MB resident, and four blocks and items add almost nothing. A Bevy target directory is about 3 GB for `cargo build` and as much again for `cargo clippy`.

## Tests

`cargo test` needs no game data: config parsing, the shot flags and the rest-pose file's details, the measurements, the shot camera, the PNG diff and the loader (against a
synthetic provider: models, clips, scenes with asset references, failures).
`cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass.

## License

Licensed under either of MIT or Apache-2.0, at your option.
