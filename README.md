# salus

A key/value store protected by secret shares and encryption

## Current Releases

### libsalus
[![Crates.io](https://img.shields.io/crates/v/libsalus.svg)](https://crates.io/crates/libsalus)
[![Crates.io](https://img.shields.io/crates/l/libsalus.svg)](https://crates.io/crates/libsalus)
[![Crates.io](https://img.shields.io/crates/d/libsalus.svg)](https://crates.io/crates/libsalus)

### salusd
[![Crates.io](https://img.shields.io/crates/v/salusd.svg)](https://crates.io/crates/salusd)
[![Crates.io](https://img.shields.io/crates/l/salusd.svg)](https://crates.io/crates/salusd)
[![Crates.io](https://img.shields.io/crates/d/salusd.svg)](https://crates.io/crates/salusd)

### salusc
[![Crates.io](https://img.shields.io/crates/v/salusc.svg)](https://crates.io/crates/salusc)
[![Crates.io](https://img.shields.io/crates/l/salusc.svg)](https://crates.io/crates/salusc)
[![Crates.io](https://img.shields.io/crates/d/salusc.svg)](https://crates.io/crates/salusc)

### CI/CD
[![docs.rs](https://docs.rs/libsalus/badge.svg)](https://docs.rs/libsalus)
[![codecov](https://codecov.io/gh/rustyhorde/salus/branch/master/graph/badge.svg)](https://codecov.io/gh/rustyhorde/salus)
[![CI](https://github.com/rustyhorde/salus/actions/workflows/salus.yml/badge.svg)](https://github.com/rustyhorde/salus/actions)

## Overview

Salus is a local secret store. It is a key/value store whose master encryption
key is split into [Shamir secret shares][shamir] and **never persisted to disk**.

A long-running daemon (`salusd`) owns the encrypted database and holds the
reconstructed key only in memory. A command line client (`salusc`) talks to it
over a local IPC socket; the client holds no key material and performs no crypto.

The project is three workspace crates:

- **`libsalus`** — shared library: Shamir share generation/unlocking (wraps the
  [`ssss`][ssss] crate), the wire protocol (`Action`/`Response` enums and message
  structs), and `socket_name()`, the single source of truth for the IPC socket path.
- **`salusd`** — the daemon: listens on the socket, owns the [`redb`][redb]
  database, and does all AES-256-GCM encryption. The only crate that touches
  crypto-at-rest and storage.
- **`salusc`** — the CLI client: parses subcommands, connects to the socket, sends
  `Action`s, and renders `Response`s with [`crossterm`][crossterm] styling.

Built with **edition 2024**, MSRV **1.91.1**, and dual-licensed
**MIT OR Apache-2.0**.

## Build

```bash
cargo build                  # build all three crates
cargo build --release
cargo test                   # run all tests
cargo test -p libsalus       # test a single crate
cargo clippy --all-targets   # lints (see note below)
```

> **Lints are nightly-gated.** Each crate root carries a large
> `#![cfg_attr(nightly, deny(...))]` block (`clippy::all`, `clippy::pedantic`,
> `missing_docs`, …) enabled by a `build.rs` cfg. On stable these denies are
> inert, so to actually exercise them lint on nightly:
> `cargo +nightly clippy --all-targets`.

### Run it end-to-end

1. Start the daemon in the foreground:

   ```bash
   cargo run -p salusd -- -e -v
   ```

   (`-e` enables stdout logging — for foreground/dev only, not as a service;
   `-v` raises verbosity.)

2. In another terminal, drive it with the client:

   ```bash
   salusc shares                      # first-time init; prints the shares ONCE — record them
   salusc unlock                      # prompts for `threshold` shares; reconstructs the key in memory
   salusc store -k mykey -v myvalue
   salusc read -k mykey
   salusc find '^my'
   ```

The daemon must be unlocked before `store`/`read` succeed (otherwise
`StoreNotUnlocked`). The reconstructed key auto-clears after `key_timeout`
seconds (default 20), after which you must `unlock` again.

### Local testing (debug builds)

To exercise debug builds from the project directory **without disturbing a
production install** on the same machine, keep all state under the tracked
`dev/` directory and redirect every path with CLI flags.

Two things must be isolated:

- **The socket.** On Linux the default IPC socket is an *abstract-namespace*
  name (`salus.sock`) that every install shares — a debug daemon would try to
  bind the same name as a running production daemon. Pointing the socket at a
  **file path** (any explicit `--socket-path` / `SALUS_SOCKET` value) switches to
  a filesystem socket, so the dev pair gets its own socket while production keeps
  using the abstract name. Use a path inside `dev/`.
- **The database (and config/log).** The database, config-file, and tracing
  paths are **CLI-only** (`-d` / `-c` / `-t`); they are *not* read from env or
  the TOML file. Without `-d`, a debug daemon reads and writes the **production**
  database under `~/.local/share/salusd/`. Always pass `-d` (and `-c`/`-t`) so it
  stays in `dev/`.

The repo ships base config (`dev/salusd.toml`, `dev/salusc.toml`) and a fish
helper that wires these flags up. Source it once per shell:

```bash
source scripts/dev_env.fish      # defines salusd-dev / salusc-dev

salusd-dev                       # terminal 1: foreground debug daemon
salusc-dev shares                # terminal 2: first-time init — record the shares
salusc-dev unlock                # enter `threshold` shares (default 3)
salusc-dev store -k mykey -v myvalue
salusc-dev read  -k mykey
salusc-dev find '^my'
```

The wrappers are thin — the equivalent raw commands (for non-fish shells, run
from the repo root) are:

```bash
# daemon
cargo run -p salusd -- -e \
    -c dev/salusd.toml -d dev/salusd.redb -t dev/salusd.log -s dev/salus.sock

# client (repeat per command)
cargo run -p salusc -- -c dev/salusc.toml -s dev/salus.sock shares
```

Only `dev/salusd.toml`, `dev/salusc.toml`, and `dev/.gitignore` are tracked; the
runtime artifacts (`dev/salusd.redb`, `dev/salusd.log`, `dev/salus.sock`) are
gitignored. The dev config sets a longer `key_timeout` (300s) so the in-memory
key does not clear out from under you during manual testing.

> **Stale socket.** If the daemon ever fails to start with an "address in use"
> error after a crash, remove the leftover file socket: `rm -f dev/salus.sock`.

## Usage

### `salusd` (daemon)

```text
salusd [OPTIONS]
```

| Flag | Description |
| --- | --- |
| `-v, --verbose` | Turn logging up (repeatable; conflicts with `--quiet`) |
| `-q, --quiet` | Turn logging down (repeatable; conflicts with `--verbose`) |
| `-e, --enable-std-output` | Log to stdout/stderr in addition to the trace file (foreground/dev only — **not** as a service) |
| `-c, --config-absolute-path <PATH>` | Absolute path to a non-standard config file |
| `-t, --tracing-absolute-path <PATH>` | Absolute path to a non-standard tracing output file |
| `-d, --database-absolute-path <PATH>` | Absolute path to a non-standard database file |
| `-s, --socket-path <PATH>` | Override the IPC socket path (see `SALUS_SOCKET` below) |

**Configuration** is layered, lowest precedence first: a TOML file, then
environment variables, then **explicitly-set** CLI flags (highest). A CLI flag
left at its default does not override an env/file value, so e.g. `SALUSD_VERBOSE`
is honored unless you actually pass `-v`. Any field absent from every source
falls back to its built-in default. Environment variables use the `SALUSD_`
prefix; single underscores stay within a field name (`SALUSD_KEY_TIMEOUT=30` →
`key_timeout`) and a double underscore descends into a nested table
(`SALUSD_TRACING__WITH_TARGET=true` → `[tracing] with_target`). Recognized keys:

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `key_timeout` | `u64` | `20` | Seconds before the in-memory key auto-clears. Env/TOML only — no CLI flag. |
| `socket_path` | `string` | — | IPC socket override. Also `-s` / `SALUS_SOCKET`. |
| `verbose` / `quiet` | `u8` | `0` | Also settable via CLI. |
| `enable_std_output` | `bool` | `false` | Also settable via CLI. |
| `[tracing]` | table | — | `with_target`, `with_thread_ids`, `with_thread_names`, `with_line_number`, `with_level`, `directives` (env: `SALUSD_TRACING__WITH_TARGET`, …). |

**Default paths** are per-user and cross-platform via `dirs2`: config under the
config dir, database under the data dir, and logs under the local data dir, each
in a `salusd/` subdirectory — on Linux `~/.config/salusd/`,
`~/.local/share/salusd/`; on macOS `~/Library/Application Support/salusd/`. The
IPC socket defaults to a namespaced name where the platform supports it,
otherwise a file under the runtime/temp dir. Set the **shared** `SALUS_SOCKET`
environment variable (honored by both the daemon and the client) to relocate the
socket from one place; `--socket-path` / `socket_path` override it per process.

### `salusc` (client)

```text
salusc [OPTIONS] <COMMAND>
```

Global options: `-v, --verbose`, `-q, --quiet`, `-c, --config-path <PATH>`,
`-s, --socket-path <PATH>`. Like the daemon, the client reads a TOML config file
(`<config dir>/salusc/salusc.toml` by default) and `SALUSC_` environment
variables in addition to CLI flags; it uses `SALUS_SOCKET` / `--socket-path` to
find the daemon's socket.

| Command | Description |
| --- | --- |
| `shares` | First-time init. Generates and prints the shares **once** — record them. |
| `unlock` | Prompts for `threshold` shares and reconstructs the key in the daemon's memory. |
| `store` | Store an encrypted value under a key. |
| `read` | Read and decrypt the value for a key. |
| `find` | Search keys by regular expression. |

Command options:

- `shares` — `-n, --num-shares <N>` (default `5`), `-t, --threshold <N>` (default `3`).
- `store` — `-k, --key <KEY>`, `-v, --value <VALUE>`.
- `read` — `-k, --key-opt <KEY>`.
- `find` — `<REGEX>` (positional).

## Architecture

**Wire protocol.** Client and daemon exchange `libsalus::Action` / `Response`
enums (defined in `libsalus/src/message/mod.rs`), serialized with
[`bincode-next`][bincode] (`standard()` config). Each request is a fresh socket
connection: the client writes one encoded `Action`, half-closes the send side,
and reads the `Response` to EOF. `socket_name(override)` is the single source of
truth for the socket path; it resolves an explicit per-side override, then the
shared `SALUS_SOCKET` env var, then the platform default, keeping the daemon and
client in sync.

**Daemon concurrency** (`salusd/src/runtime/mod.rs`). The daemon accepts
connections in a loop. Per connection it spawns two tasks: one decodes the
incoming `Action` and forwards it over an mpsc channel, the other (an
`ActionHandler`) consumes the channel and mutates the shared store. The store is
an `Arc<Mutex<ShareStore>>` shared across all connections; mutex poisoning is
deliberately recovered via `into_inner()` rather than panicking.

**Key/crypto flow** (`salusd/src/store/mod.rs`). A random 32-byte key is
generated at init and split into Shamir shares; the key itself is never stored.
On `unlock`, submitted shares reconstruct a candidate key, which is verified by
decrypting the sentinel `CHECK_KEY` record — only then is the key cached in
memory. Stored values are AES-256-GCM sealed with a per-write randomized nonce,
and the key name is bound as additional authenticated data (AAD).

**Storage** (`salusd/src/db/mod.rs`). A `redb` embedded database with two tables:
`salus_config` (init flag, num_shares, threshold) and `salus_store` (the sealed
values — a `SalusVal` row is the nonce plus ciphertext). Access goes through the
generic `read_value` / `write_value` helpers.

## Security

- **The master key is never persisted.** It is split into Shamir shares,
  reconstructed only in the daemon's memory, and **auto-clears after
  `key_timeout`** (default 20s).
- **Key material is zeroized.** The reconstructed key is wrapped in `Zeroizing`
  (zeroed on drop), and submitted shares are zeroized after unlock. Key-clearing
  timers are generation-guarded so a stale timer from an earlier unlock cannot
  wipe a freshly unlocked key.
- **Candidate keys are verified.** A wrong key fails to open the `CHECK_KEY`
  sentinel, so an incorrect reconstruction is rejected rather than cached. AAD
  binds every value to its key name, so a relocated/tampered ciphertext fails to
  decrypt.
- **Wire-protocol DoS hardening.** Decoding is bounded by `MAX_MESSAGE_SIZE`
  (1 MiB, in `libsalus/src/message/mod.rs`), so a forged length prefix cannot
  drive an unbounded allocation.
- **Fuzzing.** The `fuzz/` crate provides five libFuzzer targets —
  `fuzz_action_decode`, `fuzz_response_decode`, `fuzz_unlock_key`,
  `fuzz_store_roundtrip`, and `fuzz_find_regex` — each with a matching regression
  test. CI audits dependencies and runs fuzz smoke tests
  (`.github/workflows/audit.yml`).
- **The client holds no key material and performs no crypto** — all crypto and
  storage live in the daemon.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

[shamir]: https://en.wikipedia.org/wiki/Shamir%27s_secret_sharing
[ssss]: https://crates.io/crates/ssss
[redb]: https://crates.io/crates/redb
[crossterm]: https://crates.io/crates/crossterm
[bincode]: https://crates.io/crates/bincode-next
