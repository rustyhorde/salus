# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Salus is a local secret store: a key/value store where the master encryption key is split into Shamir secret shares and never persisted. A long-running daemon (`salusd`) owns the encrypted database and holds the reconstructed key only in memory; a CLI client (`salus`) talks to it over a local IPC socket. Three workspace crates:

- **`libsalus`** — shared library. Shamir share generation/unlocking (wraps the `ssss` crate), the wire protocol (`Action`/`Response` enums plus message structs), and `socket_name()`, the single source of truth for the IPC socket path. Both binaries depend on this so the protocol stays in sync.
- **`salusd`** — the daemon. Listens on the socket, owns the `redb` database, does all AES-256-GCM encryption, and is the only crate that touches crypto-at-rest and storage.
- **`salus`** — the CLI client. Parses subcommands, connects to the socket, sends `Action`s, and renders `Response`s with `crossterm` styling. Holds no key material and does no crypto.

## Commands

```bash
cargo build                      # build all three crates
cargo build --release
cargo test                       # run all tests (unit tests live in libsalus, e.g. key/mod.rs)
cargo test -p libsalus           # test a single crate
cargo test gen_key_works         # run a single test by name
cargo clippy --all-targets       # lints — see lint note below
cargo run -p salusd              # run the daemon (foreground)
cargo run -p salus -- shares     # run the client; args after `--`
```

### Running the system end-to-end

1. Start the daemon: `cargo run -p salusd -- -e -v` (`-e` enables stdout logging — only for foreground/dev, not as a service; `-v` raises verbosity).
2. In another terminal, drive it with the client:
   - `salus shares` — first-time init; generates and prints the shares **once** (record them).
   - `salus unlock` — prompts for `threshold` shares (default 3) and reconstructs the key in the daemon's memory. The key auto-clears after `key_timeout` seconds (default 20), after which you must unlock again.
   - `salus store -k <key> -v <value>` / `salus read -k <key>` / `salus find <regex>`.

The daemon must be unlocked before `store`/`read` succeed (otherwise `StoreNotUnlocked`).

## Architecture details worth knowing

**Wire protocol.** Client and daemon exchange `libsalus::Action` / `Response` enums serialized with `bincode-next` (`standard()` config). Each request is a fresh socket connection: the client writes one encoded `Action`, half-closes the send side, and reads the `Response` to EOF (`read_to_end`). Adding an operation means: add an `Action` (and usually a `Response`) variant in `libsalus/src/message/mod.rs`, a client method in `salus/src/inter/mod.rs`, a CLI subcommand in `salus/src/runtime/cli.rs`, and a handler arm in `salusd`'s `ActionHandler::action_handler` that calls into `ShareStore`.

**Daemon concurrency.** `salusd/src/runtime/mod.rs` accepts connections in a loop. Per connection it spawns two tasks: one decodes the incoming `Action` and forwards it over an mpsc channel, the other (an `ActionHandler`) consumes the channel and mutates the shared `ShareStore`. The store is an `Arc<Mutex<ShareStore>>` shared across all connections. Mutex poisoning is deliberately recovered via `into_inner()` (see `unlock_store` / `unlock_redb`) rather than panicking.

**Key/crypto flow (`salusd/src/store/mod.rs`).** A random 32-byte key is generated at init and split into shares; the key is never stored. On `unlock`, submitted shares reconstruct a candidate key, which is verified by decrypting the sentinel `CHECK_KEY` record — only then is the key cached in memory. Stored values are AES-256-GCM sealed with a per-write randomized nonce; both nonce and ciphertext live in the `SalusVal` row. `unlock` collects shares across multiple `Action::Share` messages, then `Action::Unlock` triggers reconstruction.

**Storage.** `redb` embedded DB with two tables: `salus_config` (init flag, num_shares, threshold — `ConfigVal`) and `salus_store` (the sealed values — `SalusVal`). Access goes through the generic `read_value`/`write_value` helpers in `salusd/src/db/mod.rs`.

**Config & paths.** `salusd` layers config from env vars (prefix `SALUSD_`), CLI flags, then a TOML file. The `PathDefaults` trait (implemented on the daemon's `Cli`) centralizes default locations: config/db under `/var/lib/salus`, logs under `/var/log/salus`. The socket is `/var/run/salus.sock` (namespaced where supported, else a `/tmp` file).

## Conventions

- **Edition 2024**, workspace resolver 3. Shared deps are pinned in the root `Cargo.toml` `[workspace.dependencies]` — add or bump versions there, not in member crates.
- **Lints are nightly-gated.** Every crate root (`lib.rs`/`main.rs`) carries a large `#![cfg_attr(nightly, deny(...))]` block (including `clippy::all`, `clippy::pedantic`, and `missing_docs`). The `nightly` cfg is set by each crate's `build.rs` via `rustversion`. To actually exercise these denies, lint on nightly: `cargo +nightly clippy --all-targets`. On stable they're inert, so a stable build passing does not mean CI will.
- Every source file starts with the MIT/Apache dual-license header comment — preserve it on new files.
- Builders use the `bon` crate (`Builder` derive, `Type::builder()...build()`); accessors use `getset`.
