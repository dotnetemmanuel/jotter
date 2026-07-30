#!/usr/bin/env bash
# Runs a command inside a headless cage compositor, off screen, and reports the
# Wayland socket to point grim, wtype and wlpoint at.
#
# Usage: cage-run.sh ./target/release/jotter /path/to/vault
set -uo pipefail

if [ $# -eq 0 ]; then
    echo "usage: $0 <command> [args...]" >&2
    exit 2
fi

# A single-instance app hands the launch to the process already running, so the
# headless one would exit at once and take cage with it.
if pgrep -x jotter >/dev/null 2>&1; then
    echo "a jotter is already running: quit it first, or the launch goes to that one" >&2
    exit 1
fi

log="${CAGE_LOG:-${TMPDIR:-/tmp}/cage-run.log}"
sockets() { ls "$XDG_RUNTIME_DIR"/wayland-* 2>/dev/null | grep -v '\.lock$' | sort; }

before=$(sockets)
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 \
    cage -- "$@" >"$log" 2>&1 &
cage_pid=$!
echo "cage_pid=$cage_pid"
echo "cage_log=$log"

for _ in $(seq 1 40); do
    sleep 0.25
    new=$(comm -13 <(echo "$before") <(sockets))
    if [ -n "$new" ]; then
        echo "socket=$(basename "$new")"
        exit 0
    fi
    kill -0 "$cage_pid" 2>/dev/null || break
done

echo "socket=none: cage did not come up, see $log" >&2
exit 1
