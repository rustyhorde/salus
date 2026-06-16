#!/usr/bin/env fish

# Copyright (c) 2025 salus developers
#
# Licensed under the Apache License, Version 2.0
# <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
# license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
# option. All files in the project carrying such notice may not be copied,
# modified, or distributed except according to those terms.

# Installs the salus binaries to ~/.cargo/bin (where the systemd user unit's
# ExecStart=%h/.cargo/bin/salusd expects to find them).

for arg in $argv
    switch $arg
        case --help -h
            echo "Usage: run_install.fish"
            echo ""
            echo "Installs salusd, salusc and salus-agent to ~/.cargo/bin via 'cargo install'."
            exit 0
        case '*'
            echo "Unknown argument: $arg"
            echo "Run 'run_install.fish --help' for usage."
            exit 1
    end
end

function run_step
    echo ""
    echo "==> $argv"
    eval $argv
    if test $status -ne 0
        echo "FAILED: $argv"
        exit 1
    end
end

run_step cargo install --path salusd --force --locked
run_step cargo install --path salusc --force --locked
run_step cargo install --path salus-agent --force --locked

echo ""
echo "All packages installed successfully."
