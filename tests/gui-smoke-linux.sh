#!/usr/bin/env bash
# Modified: isolate persistent settings and cover the compact layout, tabs and text preview.
# Run under xvfb-run with GTK and xdotool installed, using the released binary.
set -euo pipefail
if [ "$#" -ne 1 ]; then echo 'Usage: gui-smoke-linux.sh /path/to/zfm' >&2; exit 2; fi
binary=$1
work=$(mktemp -d)
config=$(mktemp -d)
pid=''
cleanup() {
    if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
    rm -rf "$work" "$config"
}
trap cleanup EXIT
printf 'GUI smoke fixture\n' > "$work/example.txt"
mkdir -p "$config/data"
XDG_DATA_HOME="$config/data" XDG_CONFIG_HOME="$config" GDK_BACKEND=x11 G_DEBUG=fatal-criticals "$binary" "$work" >"$work/stdout" 2>"$work/stderr" &
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
# Ctrl+B bookmarks the current folder without modifying settings.
xdotool key --window "$window" ctrl+b
for _ in $(seq 1 100); do
    [ ! -f "$config/zfm/bookmarks.conf" ] || break
    if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited while bookmarking:' >&2; cat "$work/stderr" >&2; exit 1; fi
    sleep 0.1
done
python3 - "$config/zfm/bookmarks.conf" "$work" <<'PYBOOKMARK'
import pathlib, sys
lines = pathlib.Path(sys.argv[1]).read_text().splitlines()
assert lines == ['version=1', sys.argv[2].encode().hex()], lines
PYBOOKMARK
xdotool key --window "$window" ctrl+b
# The settings dialog opens and can be cancelled without changing the list.
xdotool key --window "$window" ctrl+comma
settings=''
for _ in $(seq 1 100); do
    settings=$(xdotool search --onlyvisible --pid "$pid" --name '^ZFM Settings$' 2>/dev/null | head -n 1 || true)
    [ -z "$settings" ] || break
    if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited while opening settings:' >&2; cat "$work/stderr" >&2; exit 1; fi
    sleep 0.1
done
if [ -z "$settings" ]; then echo 'Settings dialog did not appear:' >&2; cat "$work/stderr" >&2; exit 1; fi
xdotool windowfocus --sync "$settings"
xdotool key --window "$settings" Escape
xdotool windowfocus --sync "$window"
# Preview the only fixture from the file list.
# Wait for the asynchronous directory load before selecting it.
sleep 1
xdotool key --window "$window" Escape ctrl+a ctrl+p
preview=''
for _ in $(seq 1 100); do
    preview=$(xdotool search --onlyvisible --pid "$pid" --name '^ZFM Preview — example.txt$' 2>/dev/null | head -n 1 || true)
    [ -z "$preview" ] || break
    if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited while opening preview:' >&2; cat "$work/stderr" >&2; exit 1; fi
    sleep 0.1
done
if [ -z "$preview" ]; then echo 'Text preview did not appear:' >&2; cat "$work/stderr" >&2; exit 1; fi
xdotool windowfocus --sync "$preview"
xdotool key --window "$preview" Escape
xdotool windowfocus --sync "$window"
# Tabs must remain usable without forcing the compact toolbar off-screen.
xdotool windowsize "$window" 900 600
for _ in $(seq 1 7); do xdotool key --window "$window" ctrl+t; done
sleep 1
width=$(xdotool getwindowgeometry --shell "$window" | sed -n 's/^WIDTH=//p')
if [ -z "$width" ] || [ "$width" -gt 920 ]; then
    echo "Tab strip expanded the window unexpectedly (width ${width:-unknown})." >&2
    cat "$work/stderr" >&2
    exit 1
fi
for _ in $(seq 1 7); do xdotool key --window "$window" ctrl+w; done
sleep 1
# Keyboard-triggered background menu should open and dismiss without a GTK critical.
xdotool key --window "$window" shift+F10
xdotool key Escape
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
# The X window can become visible before GTK has focused the entry widget.
xdotool windowfocus --sync "$dialog"
sleep 0.5
xdotool type --clearmodifiers 'created.txt'
xdotool key Return
for _ in $(seq 1 100); do
    [ ! -f "$work/created.txt" ] || break
    if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited while creating a file:' >&2; cat "$work/stderr" >&2; exit 1; fi
    sleep 0.1
done
if [ ! -f "$work/created.txt" ]; then echo 'Enter did not create the new file:' >&2; cat "$work/stderr" >&2; exit 1; fi
# Recursive search finds a nested file and previews its actual path.
mkdir -p "$work/search-fixture/nested"
printf 'recursive fixture\n' > "$work/search-fixture/nested/needle-search.txt"
xdotool windowfocus --sync "$window"
xdotool key --window "$window" ctrl+f
xdotool type --clearmodifiers 'needle-search.txt'
xdotool key --window "$window" ctrl+shift+f
sleep 1
xdotool key --window "$window" Escape ctrl+a ctrl+p
search_preview=''
for _ in $(seq 1 100); do
    search_preview=$(xdotool search --onlyvisible --pid "$pid" --name '^ZFM Preview — needle-search.txt$' 2>/dev/null | head -n 1 || true)
    [ -z "$search_preview" ] || break
    if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited during recursive search:' >&2; cat "$work/stderr" >&2; exit 1; fi
    sleep 0.1
done
if [ -z "$search_preview" ]; then echo 'Recursive search result did not open its preview:' >&2; cat "$work/stderr" >&2; exit 1; fi
xdotool windowfocus --sync "$search_preview"
xdotool key --window "$search_preview" Escape
xdotool windowfocus --sync "$window"
xdotool key --window "$window" ctrl+f ctrl+a BackSpace Return
sleep 1
# Exercise multi-item copy (including a folder) through native clipboard shortcuts.
mkdir -p "$work/folder-test/source/nested/empty"
printf 'nested content\n' > "$work/folder-test/source/nested/file.txt"
printf 'companion content\n' > "$work/folder-test/companion.txt"
xdotool windowfocus --sync "$window"
xdotool key --window "$window" ctrl+l
xdotool key --window "$window" ctrl+a
xdotool type --clearmodifiers "$work/folder-test"
xdotool key --window "$window" Return
sleep 1
xdotool key --window "$window" Escape ctrl+a ctrl+c ctrl+v
for _ in $(seq 1 100); do
    if [ -f "$work/folder-test/source copy/nested/file.txt" ] && [ -f "$work/folder-test/companion copy.txt" ]; then break; fi
    if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited during folder copy:' >&2; cat "$work/stderr" >&2; exit 1; fi
    sleep 0.1
done
if [ ! -f "$work/folder-test/source copy/nested/file.txt" ]; then cat "$work/stderr" >&2; find "$work/folder-test" >&2; exit 1; fi
cmp "$work/folder-test/source/nested/file.txt" "$work/folder-test/source copy/nested/file.txt"
cmp "$work/folder-test/companion.txt" "$work/folder-test/companion copy.txt"
[ -d "$work/folder-test/source copy/nested/empty" ]
# Trash multiple items through Delete, isolated from the user's real trash.
mkdir -p "$work/trash-test/batch-folder"
printf 'trash file\n' > "$work/trash-test/batch-file.txt"
printf 'trash nested\n' > "$work/trash-test/batch-folder/nested.txt"
xdotool windowfocus --sync "$window"
xdotool key --window "$window" ctrl+l ctrl+a
xdotool type --clearmodifiers "$work/trash-test"
xdotool key --window "$window" Return
sleep 1
xdotool key --window "$window" Escape ctrl+a Delete
for _ in $(seq 1 100); do
    if [ -f "$config/data/Trash/files/batch-file.txt" ] && [ -f "$config/data/Trash/files/batch-folder/nested.txt" ]; then break; fi
    if ! kill -0 "$pid" 2>/dev/null; then echo 'ZFM exited while trashing selection:' >&2; cat "$work/stderr" >&2; exit 1; fi
    sleep 0.1
done
[ ! -e "$work/trash-test/batch-file.txt" ] && [ ! -e "$work/trash-test/batch-folder" ]
grep -q 'trash file' "$config/data/Trash/files/batch-file.txt"
grep -q 'trash nested' "$config/data/Trash/files/batch-folder/nested.txt"
[ -f "$config/data/Trash/info/batch-file.txt.trashinfo" ] && [ -f "$config/data/Trash/info/batch-folder.trashinfo" ]
echo "GTK navigation, clipboard, trash and name dialog smoke tests passed (window $window)."
