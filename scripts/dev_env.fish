#!/usr/bin/env fish
# Source this to get salusd-dev / salusc-dev wrappers that drive debug builds
# against ./dev, isolated from any production install. Every path (config, db,
# log, socket) is passed as an explicit CLI flag, so nothing touches the
# per-user production locations and the dev pair uses a file socket instead of
# the shared abstract-namespace socket. Usage:
#   source scripts/dev_env.fish
#   salusd-dev            # foreground daemon (terminal 1)
#   salusc-dev shares     # client commands (terminal 2)
set -g SALUS_DEV_DIR (git rev-parse --show-toplevel)/dev

function salusd-dev --description 'Run debug salusd against ./dev'
    cargo run -p salusd -- -e \
        -c $SALUS_DEV_DIR/salusd.toml \
        -d $SALUS_DEV_DIR/salusd.redb \
        -t $SALUS_DEV_DIR/salusd.log \
        -s $SALUS_DEV_DIR/salus.sock $argv
end

function salusc-dev --description 'Run debug salusc against ./dev'
    cargo run -p salusc -- \
        -c $SALUS_DEV_DIR/salusc.toml \
        -s $SALUS_DEV_DIR/salus.sock $argv
end
