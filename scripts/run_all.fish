#!/usr/bin/env fish

# Copyright (c) 2025 salus developers
#
# Licensed under the Apache License, Version 2.0
# <LICENSE-APACHE or https://www.apache.org/licenses/LICENSE-2.0> or the MIT
# license <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
# option. All files in the project carrying such notice may not be copied,
# modified, or distributed except according to those terms.

set run_tests true
set run_coverage true
set run_docs true
set run_fuzz true
set run_clean false

for arg in $argv
    switch $arg
        case --help -h
            echo "Usage: run_all.fish [OPTIONS]"
            echo ""
            echo "Runs the core salus CI pipeline locally."
            echo ""
            echo "Options:"
            echo "  --no-test      Skip nextest and all coverage steps"
            echo "  --no-coverage  Skip coverage steps only (lcov + html reports)"
            echo "  --no-docs      Skip the documentation step"
            echo "  --no-fuzz      Skip the cargo fuzz steps"
            echo "  --clean        Run cargo clean after all steps complete"
            echo "  --help, -h     Show this help message"
            echo ""
            echo "Steps (in order):"
            echo "  1.  cargo fmt"
            echo "  2.  cargo fmt --all -- --check"
            echo "  3.  cargo matrix clippy --all-targets -- -Dwarnings"
            echo "  4.  cargo matrix build"
            echo "  5.  cargo matrix nextest run                        (skipped with --no-test)"
            echo "  6.  cargo test -p libsalus --doc                    (skipped with --no-test)"
            echo "  7.  cargo test --manifest-path fuzz/Cargo.toml      (skipped with --no-test)"
            echo "  8.  cargo doc -p libsalus                           (skipped with --no-docs)"
            echo "  9.  cargo matrix -F unstable llvm-cov nextest ...   (skipped with --no-test or --no-coverage)"
            echo "  10. cargo llvm-cov report --lcov ...                (skipped with --no-test or --no-coverage)"
            echo "  11. cargo llvm-cov report --html                    (skipped with --no-test or --no-coverage)"
            echo "  12. cargo fuzz run (30s each target)               (skipped with --no-fuzz)"
            echo "  13. cargo clean                                     (only with --clean)"
            exit 0
        case --no-test
            set run_tests false
            set run_coverage false
        case --no-coverage
            set run_coverage false
        case --no-docs
            set run_docs false
        case --no-fuzz
            set run_fuzz false
        case --clean
            set run_clean true
        case '*'
            echo "Unknown argument: $arg"
            echo "Run 'run_all.fish --help' for usage."
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

run_step cargo fmt
run_step cargo fmt --all -- --check
run_step cargo matrix clippy --all-targets -- -Dwarnings
run_step cargo matrix build

if test $run_tests = true
    run_step cargo matrix nextest run
    run_step cargo test -p libsalus --doc
    run_step cargo test --manifest-path fuzz/Cargo.toml
end

if test $run_docs = true
    run_step cargo doc -p libsalus
end

if test $run_coverage = true
    run_step cargo matrix -F unstable llvm-cov nextest --no-report
    run_step cargo llvm-cov report --lcov --output-path lcov.info
    run_step cargo llvm-cov report --html
end

if test $run_fuzz = true
    run_step cargo +nightly fuzz run fuzz_action_decode -- -max_total_time=30
    run_step cargo +nightly fuzz run fuzz_response_decode -- -max_total_time=30
    run_step cargo +nightly fuzz run fuzz_unlock_key -- -max_total_time=30
    run_step cargo +nightly fuzz run fuzz_store_roundtrip -- -max_total_time=30
    run_step cargo +nightly fuzz run fuzz_find_regex -- -max_total_time=30
end

if test $run_clean = true
    run_step cargo clean
end

echo ""
echo "All steps completed successfully."
