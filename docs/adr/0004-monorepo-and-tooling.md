# ADR-0004: Monorepo layout and tooling

- Status: **accepted**
- Date: 2026-10-17

## Decision

- **One repository, two workspaces.**
  - **Cargo** at the root covers `engine/*`, `apps/desktop/src-tauri` and `tools/*`.
  - **pnpm** covers `apps/*` and `packages/*`.
  - **Turborepo** runs JS tasks.
  - The layout follows master prompt §4.1, except that the Rust crates live under `engine/` and the Tauri crate is a member of the root Cargo workspace.
- **Toolchains are pinned in the repo:**
  - `rust-toolchain.toml`: Rust 1.99.1
  - `package.json#packageManager`: pnpm 11.28.6 (corepack fetches it automatically)
  - `engines.node`: ≥ 24.14
  - CI uses Node 24.22.0 LTS.
- **Dependency versions:**
  - exact pins in the pnpm `catalog:` and in `[workspace.dependencies]`, with native crates `=`-pinned;
  - every pin is recorded with its source in `docs/versions.md`.
- **pnpm 11** over 12. Its defaults give supply-chain protection: a 24-hour `minimumReleaseAge`, and dependency build scripts denied unless allowed in `allowBuilds`. pnpm 12's notes mention a Rust CLI we haven't evaluated.
- **TypeScript 6.0** over 7.0. TS 7 has no programmatic API yet, so tooling built on it breaks.
- **jsdom 29.1.1** over 30.x. jsdom 30 requires Node ≥ 24.15, and the reference dev box runs 24.14. We'll revisit when we move to Node 26 at M1.
- **The product name** is a single constant, `os_engine::PRODUCT_NAME`, plus the `productName` in the Tauri config. It may change after a trademark search.

## Windows developer setup (findings from M0)

- **libclang:** `llama-cpp-sys-2` runs bindgen, so it needs libclang. We use LLVM's portable archive (`clang+llvm-23.1.3-x86_64-pc-windows-msvc.tar.xz`) under `%LOCALAPPDATA%\Programs\llvm`, with `LIBCLANG_PATH` set. No admin installer is needed. The `.tar.zst` variant can't be unpacked by Windows' built-in `tar` because its zstd window is too large.
- **CMake:** the copy bundled with VS 2022 Build Tools works (3.31.6). `scripts/dev-env.sh` puts it, Rust and libclang on PATH.
- **Debuginfo:** full debuginfo on optimised dependencies crashed rustc (`0xc0000005`) on the `windows` crate, so `[profile.dev.package."*"] debug = false`.
