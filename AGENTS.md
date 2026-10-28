# Repository Guidelines

## Project Structure

This Cargo workspace has Rust crates under `crates/`: `canape-shell` is the Slint interface, `canape-service` owns privileged operations, `canape-hw` contains hardware and simulator backends, and `canape-core` / `canape-ipc` define shared types and transport. `canape-library` handles game sources; `canape-autotdp` contains its controller; `canape-ctl` is the diagnostic client. The Xbox Game Bar widget is in `widget/CanapeWidget/`, packaging in `packaging/`, and docs and screenshots in `docs/`. Tests are near their code; library integration fixtures are in `crates/canape-library/tests/`.

## Build, Test, and Development

- `cargo fmt --all --check` checks Rust formatting.
- `cargo clippy --all-targets -- -D warnings` runs lint checks with warnings treated as errors.
- `cargo test` runs workspace tests.
- `cargo check --target x86_64-pc-windows-msvc --all-targets` checks Windows targets from Linux (install the target with `rustup target add x86_64-pc-windows-msvc` first).
- On Linux with X11 dependencies installed, run `CANAPE_MOCK_FPS=60 cargo run -p canape-service -- --console` and `CANAPE_WINDOWED=1 cargo run -p canape-shell` in separate terminals to use the simulator. `cargo run -p canape-ctl -- watch` monitors service status.

## Coding Style

Use Rust 2024 conventions and `rustfmt`. Documentation and contributor discussions are in French; code, comments, and commit messages are in English. Use `snake_case` for Rust functions/modules and `CamelCase` for types. Localize UI strings in `crates/canape-shell/lang/` and `widget/CanapeWidget/Strings/`; avoid decorative descriptions that add no useful state, value, or consequence.

## Architecture and Safety

Only `canape-service` runs elevated; keep `canape-shell` at `asInvoker`. Access hardware through `canape_hw::Device`. Verify ASUS firmware values against a reference implementation for the target model. Do not mention third-party tools or code in repository content, except in the process-conflict table in `conflicts.rs`; do not add third-party binaries or code. Keep dependencies and runtime overhead low (see `docs/architecture.md`). Never claim Windows behavior works without testing it on a ROG Ally.

## Tests and Hardware

Add focused tests alongside changed code; use `canape-library/tests/fixtures/` for external samples. Run the checks above before pushing. Simulator and cross-compilation do not replace Ally validation; state when hardware behavior remains unverified. Coordinate the Windows Ally and Linux cloud sessions through GitHub.

## Commits and Pull Requests

Follow Gitflow conventions for branch roles and integration while continuing work on the current branch; do not switch branches or rewrite history without explicit authorization. Write short, descriptive, imperative commit subjects in English. After completing and verifying each feature, commit only the changes belonging to that feature and push the current branch without asking again, if permissions allow. Never force-push. If checks fail or pushing is blocked, report the failure and do not claim the push succeeded.

Keep pull requests focused; explain user-visible changes, report validation, link issues when applicable, include UI screenshots, and flag hardware behavior still needing tests.

## Security and Configuration

Do not commit credentials, signing material, or machine-specific configuration. Treat external store data and configuration as untrusted; keep privileged operations in the service and communicate with the shell over IPC.
