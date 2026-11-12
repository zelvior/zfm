#!/usr/bin/env bash
# Build native Linux plus 32/64-bit Linux and Windows GNU binaries where possible.
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) || exit 1
cd "$ROOT" || exit 1

usage() {
    cat <<'HELP'
Usage: ./scripts/build-linux.sh [--native | --linux-only] [--install-deps] [--toolchain VERSION] [--skip-tests]

By default, builds 32-bit and 64-bit Linux and Windows GNU executables, plus
this host's Linux target if different. Missing targets are reported as failures;
successful executables are kept in a new artifacts/build-*/ directory.

  --native         Build only the host Linux executable.
  --linux-only     Build 32/64-bit Linux binaries and the host Linux binary;
                   skip Windows targets (useful in Windows Subsystem for Linux).
  --install-deps   Install build/runtime prerequisites with apt-get (uses sudo
                   if necessary; currently supported on Debian-family x86_64).
  --toolchain VER  Rustup toolchain, default: stable (updated before building).
                   A version override is useful on older systems.
  --skip-tests     Skip the native automated tests.
  -h, --help       Show this help.

Requires network access to the official rustup/Rust distribution servers.
The default stable toolchain is the latest stable Rust, not a guarantee of
compatibility with older Linux distributions or Windows 7.
HELP
}
native_only=0
linux_only=0
install_deps=0
run_tests=1
toolchain=stable
while [ "$#" -gt 0 ]; do
    case "$1" in
        --native) native_only=1 ;;
        --linux-only) linux_only=1 ;;
        --install-deps) install_deps=1 ;;
        --skip-tests) run_tests=0 ;;
        --toolchain) shift; if [ "$#" -eq 0 ] || [ -z "$1" ]; then echo 'Missing toolchain version.' >&2; exit 2; fi; toolchain=$1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done
if [ "$native_only" -eq 1 ] && [ "$linux_only" -eq 1 ]; then echo 'Choose either --native or --linux-only.' >&2; exit 2; fi
if [ "$(uname -s)" != Linux ]; then echo 'This script requires Linux.' >&2; exit 1; fi
case "$(uname -m)" in
    x86_64|amd64) host_arch=x86_64; host_target=x86_64-unknown-linux-gnu ;;
    i?86) host_arch=i686; host_target=i686-unknown-linux-gnu ;;
    aarch64|arm64) host_arch=aarch64; host_target=aarch64-unknown-linux-gnu ;;
    *) echo "Unsupported host architecture: $(uname -m). Use a supported Linux Rust host." >&2; exit 1 ;;
esac

if [ "$install_deps" -eq 1 ]; then
    if ! command -v apt-get >/dev/null 2>&1 || [ "$host_arch" != x86_64 ]; then
        echo '--install-deps currently supports Debian/Ubuntu-family x86_64 hosts only.' >&2; exit 1
    fi
    if [ "$(id -u)" -eq 0 ]; then sudo_cmd=();
    elif command -v sudo >/dev/null 2>&1; then sudo_cmd=(sudo);
    else echo 'Install sudo or run --install-deps as root.' >&2; exit 1; fi
    packages=(build-essential curl ca-certificates libgtk-3-0 libarchive13 xdg-utils)
    if [ "$native_only" -eq 0 ]; then packages+=(gcc-multilib libc6-dev-i386); fi
    if [ "$native_only" -eq 0 ] && [ "$linux_only" -eq 0 ]; then packages+=(gcc-mingw-w64-x86-64 gcc-mingw-w64-i686); fi
    printf 'Installing system packages: %s\n' "${packages[*]}"
    "${sudo_cmd[@]}" apt-get update && "${sudo_cmd[@]}" apt-get install -y "${packages[@]}" || { echo 'System package installation failed.' >&2; exit 1; }
fi

if ! command -v cc >/dev/null 2>&1; then
    echo 'A native C compiler/linker (cc) is required for native tests and builds. Install build-essential or equivalent, or use --install-deps on a supported system.' >&2
    exit 1
fi
if command -v ldconfig >/dev/null 2>&1; then
    if ! ldconfig -p 2>/dev/null | grep -q 'libgtk-3.so.0'; then
        echo 'Warning: GTK 3 runtime was not detected; Linux binaries may not start without it.' >&2
    fi
    if ! ldconfig -p 2>/dev/null | grep -q 'libarchive.so.13'; then
        echo 'Warning: libarchive runtime was not detected; ZIP and compressed TAR features need it.' >&2
    fi
fi
if ! command -v xdg-open >/dev/null 2>&1; then
    echo 'Warning: xdg-open was not detected; opening files from the Linux GUI needs it.' >&2
fi

# Rustup uses its normal user-local home. Do not modify shell startup files.
if command -v rustup >/dev/null 2>&1; then
    rustup_cmd=$(command -v rustup)
elif [ -x "${CARGO_HOME:-$HOME/.cargo}/bin/rustup" ]; then
    rustup_cmd="${CARGO_HOME:-$HOME/.cargo}/bin/rustup"
else
    if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
        echo 'Install curl or wget to download rustup, then retry.' >&2; exit 1
    fi
    installer=$(mktemp "${TMPDIR:-/tmp}/zfm-rustup.XXXXXXXX") || exit 1
    trap 'rm -f "$installer"' EXIT
    echo 'Downloading the official rustup installer from https://sh.rustup.rs ...'
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$installer" || { echo 'Could not download rustup.' >&2; exit 1; }
    else
        wget -q https://sh.rustup.rs -O "$installer" || { echo 'Could not download rustup.' >&2; exit 1; }
    fi
    sh "$installer" -y --no-modify-path --profile minimal --default-toolchain none || { echo 'Rustup installation failed.' >&2; exit 1; }
    rustup_cmd="${CARGO_HOME:-$HOME/.cargo}/bin/rustup"
    [ -x "$rustup_cmd" ] || { echo "Cannot find installed rustup at $rustup_cmd" >&2; exit 1; }
    rm -f "$installer"; trap - EXIT
fi
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
echo "Installing/updating Rust toolchain: $toolchain"
"$rustup_cmd" toolchain install "$toolchain" --profile minimal || { echo 'Rust toolchain installation failed. Check network, architecture and OS compatibility; try --toolchain VERSION for an older supported toolchain.' >&2; exit 1; }
"$rustup_cmd" run "$toolchain" rustc --version || exit 1
"$rustup_cmd" run "$toolchain" cargo --version || exit 1

# Use an isolated Cargo target directory so user configuration cannot make us
# copy an old executable from a different build target or profile.
export CARGO_TARGET_DIR="$ROOT/target/zfm-build"
if [ "$run_tests" -eq 1 ]; then
    if ! "$rustup_cmd" target add --toolchain "$toolchain" "$host_target"; then
        echo "Cannot install native Rust target $host_target." >&2; exit 1
    fi
    echo 'Running native automated tests...'
    if ! "$rustup_cmd" run "$toolchain" cargo test --locked --target "$host_target"; then
        echo 'Native tests failed; no executables were published. Use --skip-tests only if you understand the risk.' >&2
        exit 1
    fi
fi

mkdir -p "$ROOT/artifacts" || exit 1
output=$(mktemp -d "$ROOT/artifacts/build-XXXXXXXX") || exit 1
test_dir=$(mktemp -d "${TMPDIR:-/tmp}/zfm-link-test.XXXXXXXX") || exit 1
trap 'rm -rf "$test_dir"' EXIT
printf 'int main(void) { return 0; }\n' > "$test_dir/test.c"
if [ "$native_only" -eq 1 ]; then targets=("$host_target");
else
    targets=(x86_64-unknown-linux-gnu i686-unknown-linux-gnu)
    if [ "$linux_only" -eq 0 ]; then targets+=(x86_64-pc-windows-gnu i686-pc-windows-gnu); fi
    if [ "$host_arch" = aarch64 ]; then targets+=("$host_target"); fi
fi
successes=()
failures=()
for target in "${targets[@]}"; do
    echo; echo "=== $target ==="
    linker=''; flag=''
    case "$target" in
        x86_64-unknown-linux-gnu)
            if [ "$host_arch" = x86_64 ] || [ "$host_arch" = i686 ]; then linker=cc; flag=-m64; else linker=x86_64-linux-gnu-gcc; fi ;;
        i686-unknown-linux-gnu)
            if [ "$host_arch" = x86_64 ] || [ "$host_arch" = i686 ]; then linker=cc; flag=-m32; else linker=i686-linux-gnu-gcc; fi ;;
        aarch64-unknown-linux-gnu) linker=cc ;;
        x86_64-pc-windows-gnu) linker=x86_64-w64-mingw32-gcc ;;
        i686-pc-windows-gnu) linker=i686-w64-mingw32-gcc ;;
    esac
    if ! command -v "$linker" >/dev/null 2>&1; then
        echo "Missing linker: $linker. Install its compiler/toolchain and retry." >&2
        failures+=("$target (missing $linker)"); continue
    fi
    args=(); [ -n "$flag" ] && args+=("$flag")
    if ! "$linker" "${args[@]}" "$test_dir/test.c" -o "$test_dir/test-bin"; then
        echo "Linker test failed for $target; install its C runtime/development libraries." >&2
        failures+=("$target (linker test failed)"); continue
    fi
    if ! "$rustup_cmd" target add --toolchain "$toolchain" "$target"; then
        echo "Rust target installation failed for $target." >&2
        failures+=("$target (Rust target unavailable)"); continue
    fi
    # Override Cargo linker explicitly for cross builds, not globally.
    case "$target" in
        x86_64-unknown-linux-gnu) linker_var=CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER ;;
        i686-unknown-linux-gnu) linker_var=CARGO_TARGET_I686_UNKNOWN_LINUX_GNU_LINKER ;;
        aarch64-unknown-linux-gnu) linker_var=CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER ;;
        x86_64-pc-windows-gnu) linker_var=CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER ;;
        i686-pc-windows-gnu) linker_var=CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER ;;
    esac
    if ! env "$linker_var=$linker" "$rustup_cmd" run "$toolchain" cargo build --release --locked --target "$target"; then
        echo "Build failed for $target." >&2; failures+=("$target (build failed)"); continue
    fi
    name=zfm; case "$target" in *-windows-*) name=zfm.exe ;; esac
    binary="$CARGO_TARGET_DIR/$target/release/$name"
    if [ ! -f "$binary" ] || ! mkdir -p "$output/$target" || ! cp "$binary" "$output/$target/$name"; then
        echo "Could not publish executable for $target." >&2; failures+=("$target (publication failed)"); continue
    fi
    successes+=("$output/$target/$name")
    echo "Created $output/$target/$name"
done
printf '\nBuild output: %s\n' "$output"
if [ "${#successes[@]}" -gt 0 ]; then printf 'Built:\n'; printf '  %s\n' "${successes[@]}"; fi
if [ "${#failures[@]}" -gt 0 ]; then printf 'Failed or unavailable:\n' >&2; printf '  %s\n' "${failures[@]}" >&2; echo 'Install the missing prerequisites or use --native, then run the script again.' >&2; exit 1; fi
echo 'All requested builds succeeded. These binaries are not a substitute for testing on their target systems.'
