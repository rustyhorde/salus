# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Salus is a local secret store: a key/value store where the master encryption key is split into Shamir secret shares and never persisted. A long-running daemon (`salusd`) owns the encrypted database and holds the reconstructed key only in memory; a CLI client (`salus`) talks to it over a local IPC socket. Three workspace crates:

- **`libsalus`** — shared library. Shamir share generation/unlocking (wraps the `ssss` crate), the wire protocol (`Action`/`Response` enums plus message structs), and `socket_name()`, the single source of truth for the IPC socket path. Both binaries depend on this so the protocol stays in sync.
- **`salusd`** — the daemon. Listens on the socket, owns the `redb` database, does all AES-256-GCM encryption, and is the only crate that touches crypto-at-rest and storage.
- **`salusc`** — the CLI client. Parses subcommands, connects to the socket, sends `Action`s, and renders `Response`s with `crossterm` styling. Holds no key material and does no crypto.

## Commands

```bash
cargo build                      # build all three crates
cargo build --release
cargo test                       # run all tests (unit tests live in libsalus, e.g. key/mod.rs)
cargo test -p libsalus           # test a single crate
cargo test gen_key_works         # run a single test by name
cargo clippy --all-targets       # lints — see lint note below
cargo run -p salusd              # run the daemon (foreground)
cargo run -p salusc -- shares    # run the client; args after `--`
cargo run -p xtask -- dist salusd   # completions/man page/systemd unit -> dist/salusd
cargo run -p xtask -- dist salusc   # completions/man page -> dist/salusc
```

**Final verification.** Always run `scripts/run_all.fish --no-fuzz --no-musl --no-install` as the final verification step before considering a change complete. (`run_all.fish` defaults to also running `run_install.fish` and a Docker-based MUSL build via `run_musl.fish`; `--no-musl --no-install` keeps the standard code check fast and Docker-free. Drop those flags — or run `scripts/run_musl.fish` directly — to build the static MUSL binaries locally.)

### Running the system end-to-end

1. Start the daemon: `cargo run -p salusd -- -e -v` (`-e` enables stdout logging — only for foreground/dev, not as a service; `-v` raises verbosity).
2. In another terminal, drive it with the client:
   - `salusc shares` — first-time init; generates and prints the shares **once** (record them).
   - `salusc unlock` — prompts for `threshold` shares (default 3) and reconstructs the key in the daemon's memory. The key auto-clears after `key_timeout` seconds (default 20), after which you must unlock again.
   - `salusc store -k <key> -v <value>` / `salusc read -k <key>` / `salusc find <regex>`.

The daemon must be unlocked before `store`/`read` succeed (otherwise `StoreNotUnlocked`).

**Dev daemon vs. installed service share the same defaults.** A dev `cargo run -p salusd` and the packaged `salusd.service` both default to the *same* per-user database (`~/.local/share/salusd/salusd.redb`) and IPC socket, because the paths derive from the `salusd` crate name. They cannot run at once — redb takes an exclusive file lock and the second daemon exits with `Error::DatabaseLocked` ("Another salusd may already be running…"). When developing alongside an installed service, either `systemctl --user stop salusd` first, or point the dev daemon at a separate DB and socket: `cargo run -p salusd -- -e -v -d /tmp/salus-dev.redb -s /tmp/salus-dev.sock`.

## Architecture details worth knowing

**Wire protocol.** Client and daemon exchange `libsalus::Action` / `Response` enums serialized with `bincode-next` (`standard()` config). Each request is a fresh socket connection: the client writes one encoded `Action`, half-closes the send side, and reads the `Response` to EOF (`read_to_end`). Adding an operation means: add an `Action` (and usually a `Response`) variant in `libsalus/src/message/mod.rs`, a client method in `salusc/src/inter/mod.rs`, a CLI subcommand in `salusc/src/runtime/cli.rs`, and a handler arm in `salusd`'s `ActionHandler::action_handler` that calls into `ShareStore`.

**Daemon concurrency.** `salusd/src/runtime/mod.rs` accepts connections in a loop. Per connection it spawns two tasks: one decodes the incoming `Action` and forwards it over an mpsc channel, the other (an `ActionHandler`) consumes the channel and mutates the shared `ShareStore`. The store is an `Arc<Mutex<ShareStore>>` shared across all connections. Mutex poisoning is deliberately recovered via `into_inner()` (see `unlock_store` / `unlock_redb`) rather than panicking.

**Key/crypto flow (`salusd/src/store/mod.rs`).** A random 32-byte key is generated at init and split into shares; the key is never stored. On `unlock`, submitted shares reconstruct a candidate key, which is verified by decrypting the sentinel `CHECK_KEY` record — only then is the key cached in memory. Stored values are AES-256-GCM sealed with a per-write randomized nonce; both nonce and ciphertext live in the `SalusVal` row. `unlock` collects shares across multiple `Action::Share` messages, then `Action::Unlock` triggers reconstruction.

**Storage.** `redb` embedded DB with two tables: `salus_config` (init flag, num_shares, threshold — `ConfigVal`) and `salus_store` (the sealed values — `SalusVal`). Access goes through the generic `read_value`/`write_value` helpers in `salusd/src/db/mod.rs`.

**Config & paths.** Both binaries layer config through the `config` crate, lowest precedence first: a TOML file (optional), then environment variables, then **explicitly-set** CLI flags (highest). Each `Cli`'s `Source::collect` only emits a flag the user actually set — `Count`/`bool` flags at their default are omitted — so a CLI default never clobbers an env/file value. `ConfigSalusd`/`ConfigSalusc` use `#[serde(default)]`, so any field absent from all sources falls back to `Default` (the built-in default layer; e.g. `key_timeout` = 20). Env vars use the `SALUSD_`/`SALUSC_` prefix with `prefix_separator("_")` and `separator("__")`: single underscores stay inside a field name (`SALUSD_KEY_TIMEOUT` → `key_timeout`) and a double underscore descends into a nested struct (`SALUSD_TRACING__WITH_TARGET` → `tracing.with_target`).

Default locations are **per-user via `dirs2`** (cross-platform): config under `config_dir()`, database under `data_dir()`, logs under `data_local_dir()`, each in a `<app>/` subdir (Linux `~/.config`, `~/.local/share`; macOS `~/Library/Application Support`). The slimmed `PathDefaults` trait (implemented on the daemon's `Cli`) supplies the app name, env prefix, and the explicit `*_absolute_path` overrides. The IPC socket path is configurable: `socket_name(override)` in libsalus resolves an explicit per-side override (the `--socket-path` flag / `socket_path` config) → the shared `SALUS_SOCKET` env var → a platform default (namespaced name where supported, else a file under `runtime_dir()`/temp). `SALUS_SOCKET` is resolved inside libsalus so the daemon and client stay in sync from a single setting.

## Conventions

- **Edition 2024**, workspace resolver 3. Shared deps are pinned in the root `Cargo.toml` `[workspace.dependencies]` — add or bump versions there, not in member crates.
- **Lints are nightly-gated.** Every crate root (`lib.rs`/`main.rs`) carries a large `#![cfg_attr(nightly, deny(...))]` block (including `clippy::all`, `clippy::pedantic`, and `missing_docs`). The `nightly` cfg is set by each crate's `build.rs` via `rustversion`. To actually exercise these denies, lint on nightly: `cargo +nightly clippy --all-targets`. On stable they're inert, so a stable build passing does not mean CI will.
- Every source file starts with the MIT/Apache dual-license header comment — preserve it on new files.
- Builders use the `bon` crate (`Builder` derive, `Type::builder()...build()`); accessors use `getset`.
- **No panics. Handle every error.** `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`, panicking indexing (`x[i]`), and overflow-prone arithmetic are forbidden in **all** code, including tests. Return `Result` and use `?`; wrap with `anyhow::Context` / `.with_context(|| Error::Variant)`. Tests return `Result<()>` and use `?`; assert wrong-variant cases with `anyhow::bail!`, not `panic!`. Prefer `.get()`, `split_first_chunk`, `checked_*`/`saturating_*` over indexing/raw arithmetic. Enforced (nightly) by the `clippy::unwrap_used`, `expect_used`, `panic`, `unreachable`, `todo`, `unimplemented`, `indexing_slicing`, `arithmetic_side_effects`, `get_unwrap`, and `unwrap_in_result` denies in each crate root. The only sanctioned escape is a **scoped** `#[allow(..., reason = "...")]` with a written justification (e.g. fuzz targets, where panic is the bug signal, or a proven-infallible invariant). Check with `cargo +nightly clippy --all-targets`.
