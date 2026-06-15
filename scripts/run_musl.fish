#!/usr/bin/env fish

# Copyright (c) 2025 salus developers
#
# Licensed under the Apache License, Version 2.0
# <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
# license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
# option. All files in the project carrying such notice may not be copied,
# modified, or distributed except according to those terms.

# Builds the Linux x86_64 MUSL binaries via the blackdex/rust-musl Docker image
# and copies them to ~. This is a convenience build for local use; the release
# workflow builds its own static binaries via cross and does not rely on this.

set unstable false

for arg in $argv
    switch $arg
        case --unstable
            set unstable true
        case --help -h
            echo "Usage: run_musl.fish [OPTIONS]"
            echo ""
            echo "Builds the x86_64 MUSL binaries via Docker and copies them to ~."
            echo ""
            echo "Options:"
            echo "  --unstable     Build with --features unstable instead of the stable build"
            echo "  --help, -h     Show this help message"
            exit 0
        case '*'
            echo "Unknown argument: $arg"
            echo "Run 'run_musl.fish --help' for usage."
            exit 1
    end
end

set release_dir target/x86_64-unknown-linux-musl/release
set bins salusd salusc

function run_step
    echo ""
    echo "==> $argv"
    eval $argv
    if test $status -ne 0
        echo "FAILED: $argv"
        exit 1
    end
end

if test $unstable = true
    run_step docker run -v cargo-cache:/root/.cargo/registry -v (pwd):/home/rust/src -v ~/.gitconfig:/root/.gitconfig:ro --rm -t blackdex/rust-musl:x86_64-musl-stable cargo build --release --features unstable --bin salusd --bin salusc
else
    run_step docker run -v cargo-cache:/root/.cargo/registry -v (pwd):/home/rust/src -v ~/.gitconfig:/root/.gitconfig:ro --rm -t blackdex/rust-musl:x86_64-musl-stable cargo build --release --bin salusd --bin salusc
end

echo ""
echo "==> Fixing target directory ownership"
run_step sudo chown -R (id -un):(id -gn) target/

echo ""
echo "==> Copying binaries to ~"
for bin in $bins
    run_step cp $release_dir/$bin ~
end

echo ""
echo "All MUSL build steps completed successfully."
