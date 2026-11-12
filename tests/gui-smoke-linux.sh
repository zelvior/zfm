#!/usr/bin/env bash
# Run under xvfb-run with GTK and xdotool installed, using the released binary.
set -euo pipefail
if [ "$#" -ne 1 ]; then echo 'Usage: gui-smoke-linux.sh /path/to/zfm' >&2; exit 2; fi
binary=$1
work=$(mktemp -d)
pid=''
cleanup() {
    if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
    rm -rf "$work"
}
trap cleanup EXIT
printf 'GUI smoke fixture\n' > "$work/example.txt"
GDK_BACKEND=x11 G_DEBUG=fatal-criticals "$binary" "$work" >"$work/stdout" 2>"$work/stderr" &
pid=$!
window=''
for _ in $(seq 1 100); do
    if ! kill -0 "$pid" 2>/dev/null; then
        echo 'ZFM exited before showing a window:' >&2; cat "$work/stderr" >&2; exit 1
    fi
    window=$(xdotool search --onlyvisible --pid "$pid" --name '^ZFM' 2>/dev/null | head -n 1 || true)
    [ -z "$window" ] || break
    sleep 0.1
done
if [ -z "$window" ]; then echo 'ZFM did not show a window within 10 seconds:' >&2; cat "$work/stderr" >&2; exit 1; fi
# Exercise the GTK event loop without requiring a window manager.
xdotool windowfocus --sync "$window"
xdotool key --window "$window" ctrl+t ctrl+w F5
sleep 1
if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited while handling navigation keys:' >&2; cat "$work/stderr" >&2; exit 1; fi
echo "GTK window opened and responded to navigation keys (window $window)."
