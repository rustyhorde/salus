#!/usr/bin/env fish
# Source this to get salusd-dev / salusc-dev / salus-agent-dev wrappers that
# drive debug builds against ./dev, isolated from any production install. Every
# path (config, db, log, socket) is passed as an explicit CLI flag, so nothing
# touches the per-user production locations and the dev trio uses file sockets
# instead of the shared abstract-namespace sockets.
#
# NOTE: the agent stores shares in the OS keyring (Secret Service / Keychain),
# which is per-user and NOT isolated by ./dev. Use a distinct set name (e.g.
# `devtest`) and clean up with `salusc-dev forget --name devtest` when done.
#
# Usage:
#   source scripts/dev_env.fish
#   salusd-dev               # foreground daemon       (terminal 1)
#   salus-agent-dev          # foreground login agent  (terminal 2)
#   salusc-dev shares        # client commands         (terminal 3)
set -g SALUS_DEV_DIR (git rev-parse --show-toplevel)/dev

function salusd-dev --description 'Run debug salusd against ./dev'
    cargo run -p salusd -- -e \
        -c $SALUS_DEV_DIR/salusd.toml \
        -d $SALUS_DEV_DIR/salusd.redb \
        -t $SALUS_DEV_DIR/salusd.log \
        -s $SALUS_DEV_DIR/salus.sock $argv
end

function salus-agent-dev --description 'Run debug salus-agent against ./dev'
    cargo run -p salus-agent -- -e \
        -c $SALUS_DEV_DIR/salus-agent.toml \
        -t $SALUS_DEV_DIR/salus-agent.log \
        -s $SALUS_DEV_DIR/salus-agent.sock $argv
end

function salusc-dev --description 'Run debug salusc against ./dev'
    cargo run -p salusc -- \
        -c $SALUS_DEV_DIR/salusc.toml \
        -s $SALUS_DEV_DIR/salus.sock \
        -a $SALUS_DEV_DIR/salus-agent.sock $argv
end
