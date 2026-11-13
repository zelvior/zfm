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
xdotool key --window "$window" ctrl+t ctrl+t ctrl+Tab ctrl+Page_Up ctrl+1 ctrl+w ctrl+w F5 ctrl+h ctrl+h ctrl+l Escape ctrl+f Escape
sleep 1
if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited while handling navigation keys:' >&2; cat "$work/stderr" >&2; exit 1; fi
# Verify that Ctrl+N opens a modal prompt and Enter actually creates a file.
xdotool windowfocus --sync "$window"
xdotool key --window "$window" ctrl+n
dialog=''
for _ in $(seq 1 100); do
    if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited while opening the name dialog:' >&2; cat "$work/stderr" >&2; exit 1; fi
    dialog=$(xdotool search --onlyvisible --pid "$pid" --name '^New file$' 2>/dev/null | head -n 1 || true)
    [ -z "$dialog" ] || break
    sleep 0.1
done
if [ -z "$dialog" ]; then echo 'New file dialog did not appear:' >&2; cat "$work/stderr" >&2; exit 1; fi
xdotool windowfocus --sync "$dialog"
xdotool type --clearmodifiers 'created.txt'
xdotool key Return
for _ in $(seq 1 100); do
    [ ! -f "$work/created.txt" ] || break
    if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited while creating a file:' >&2; cat "$work/stderr" >&2; exit 1; fi
    sleep 0.1
done
if [ ! -f "$work/created.txt" ]; then echo 'Enter did not create the new file:' >&2; cat "$work/stderr" >&2; exit 1; fi
echo "GTK navigation and name dialog smoke tests passed (window $window)."
