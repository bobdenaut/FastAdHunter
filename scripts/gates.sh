#!/bin/sh
# Quality gates (root CLAUDE.md). POSIX sh — run via Git Bash, not PowerShell.
#
# Prints one line per gate. The FULL output of every gate is always written to
# target/gates.log, so more detail costs a read of that file, never a re-run of
# the build. All gates run even if an earlier one fails.
set -u

LOG=target/gates.log
mkdir -p target
: >"$LOG"
fail=0

run() {
    name=$1
    shift
    printf '\n########## %s: %s\n' "$name" "$*" >>"$LOG"
    if "$@" >>"$LOG" 2>&1; then
        echo "PASS  $name"
    else
        echo "FAIL  $name"
        fail=1
    fi
}

run fmt    cargo fmt --all --check
run clippy cargo clippy --workspace --all-targets --message-format=short -- -D warnings
run test   cargo test --workspace

if [ "$fail" -eq 0 ]; then
    awk '
        /^test result: ok/ { bins++; tests += $4 }
        END { printf "      %d tests green across %d binaries\n", tests, bins }
    ' "$LOG"
    exit 0
fi

# Failure: the lines that actually say what broke. Everything else is in $LOG,
# and the line numbers below are offsets into it.
#
# `--message-format=short` puts the path first (`src/x.rs:9:5: error: ...`), so
# the diagnostic alternative must not be anchored to the start of the line.
echo "--- what failed (full log: $LOG) ---"

# Compile and lint diagnostics: one line each is already the right density,
# since `--message-format=short` carries file:line:col and the message.
grep -nE '(^|: )(error|warning)(\[[A-Za-z0-9]+\])?:|^Diff in ' "$LOG" |
    grep -vE 'warning: build failed|generated [0-9]+ warning|error: could not compile|error: process didn.t exit' |
    head -12

# A test panic is useless without its assertion text, so these get context.
grep -nE -A5 '^thread .* panicked' "$LOG" | grep -vE '^[0-9]+-$' | head -20

# Which tests failed, by name.
sed -n '/^failures:$/,/^$/p' "$LOG" | grep -E '^    ' | head -6
exit 1
