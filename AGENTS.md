# Agent guide

bridge-testbed compares rewrite-bridge provider assets side by side for developers checking scale, orientation, animation, materials and physics.
It is a Bevy 0.20 application using rewrite-bridge-bevy, with a gallery and headless captures.

## Toolchain, build and CI

- CI uses stable Rust on Linux and macOS; no minimum Rust version is declared.
- The default features enable all five providers: `wt`, `wf`, `hytale`, `nocturne`, `mc`.
- Linux CI installs `libwayland-dev libxkbcommon-dev libx11-dev` for windowing.
- Run the exact CI commands from the root:

```sh
cargo fmt --check
cargo build --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

CI has no game data. To run the gallery, configure local paths in `testbed.toml` using `testbed.example.toml`.
The README runs the application with `cargo run --release`.
Release CI builds with `cargo build --release --locked --target <target> -p bridge-testbed`.
Its targets are `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`.

## Code map

- `src/main.rs`, `config.rs`, `providers.rs`, `loader.rs`: commands, config and provider loading.
- `src/gallery.rs`, `hud.rs`, `stage.rs`, `refs.rs`: gallery and labelled reference objects.
- `src/camera.rs`, `offscreen.rs`, `shots.rs`, `capture.rs`: cameras and captures.
- `src/analyze.rs`, `inspect.rs`, `compare.rs`: measurements and image comparisons.
- `src/physics.rs`, `restpose.rs`, `wt_collision.rs`: physics and rest-pose output.

## Project rules

- Spawn provider output unchanged through rewrite-bridge-bevy; do not add another conversion layer.
- Keep reference objects, lighting, camera and physics values distinguishable from game data.
- Move provider revisions together and keep them on the same rewrite-bridge 0.x line.
- Unit tests use a synthetic provider and need no installed games.
- Keep local `testbed.toml`, shots, captures and image diffs out of Git.
- Load items one at a time; Bevy drops CPU model copies after uploading them.
- Large level scenes can need gigabytes; the example config avoids a whole War Thunder map.

## Public files

- Never commit game files, extracted assets, renders, decompiled code or secrets.
- The public-tree gate needs Python 3.9+ and Git; run it from the repository root:
  `python3 tools/check_public_tree.py` (checks Git's index, including staged new files).
- CI checks committed changes with `python3 tools/check_public_tree.py --range BASE..HEAD`.

## Contributing, license and AI

- Open an issue to discuss a problem or send a pull request with the change.
- Before a pull request, run the applicable CI commands above and the public-tree gate.
- License: MIT OR Apache-2.0, at your option; see `LICENSE-MIT` and `LICENSE-APACHE`.

The code was written with AI coding agents (Anthropic's Claude and OpenAI's GPT models) under the maintainer's direction.
AI-assisted contributions are welcome when their use is disclosed and the changes are reviewed.
