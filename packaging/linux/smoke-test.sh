#!/usr/bin/env bash
# Launches a built GradeBook binary under a virtual X display and checks that its window
# appears and stays up. Real window system, real winit backend, real software renderer:
# the one thing the headless Rust tests can't cover.
#
# Usage: packaging/linux/smoke-test.sh path/to/gradebook [screenshot.png]
# Needs: xvfb-run and xdotool (plus ImageMagick's `import` for the optional screenshot).
set -euo pipefail

binary=${1:?usage: smoke-test.sh path/to/gradebook [screenshot.png]}
screenshot=${2:-}

data_dir=$(mktemp -d)
trap 'rm -rf "$data_dir"' EXIT

export GRADEBOOK_DATA_DIR=$data_dir
export SMOKE_BINARY
SMOKE_BINARY=$(realpath "$binary")
export SMOKE_SCREENSHOT=$screenshot

# The single quotes are deliberate: the variables inside belong to the inner shell.
# shellcheck disable=SC2016
xvfb-run -a -s "-screen 0 1280x900x24" bash -c '
    log="$GRADEBOOK_DATA_DIR/stderr.log"
    "$SMOKE_BINARY" 2> "$log" &
    pid=$!

    fail() {
        echo "FAIL: $1"
        echo "--- stderr ---"
        cat "$log"
        kill "$pid" 2> /dev/null || true
        exit 1
    }

    # Up to 5 seconds for the window to show up.
    found=no
    for _ in $(seq 1 250); do
        if xdotool search --name "^GradeBook$" > /dev/null 2>&1; then found=yes; break; fi
        kill -0 "$pid" 2> /dev/null || fail "GradeBook exited while starting"
        sleep 0.02
    done
    [ "$found" = yes ] || fail "no GradeBook window appeared within 5 seconds"

    sleep 2
    kill -0 "$pid" 2> /dev/null || fail "GradeBook exited right after opening its window"

    if [ -n "$SMOKE_SCREENSHOT" ]; then
        import -window root "$SMOKE_SCREENSHOT"
    fi

    kill "$pid"
    wait "$pid" 2> /dev/null || true
    echo "ok: the window appeared and stayed up"
'
