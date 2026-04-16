#!/bin/bash
set -e

# dusk install script
# Usage: curl -fsSL https://raw.githubusercontent.com/lostf1sh/dusk/main/install.sh | bash

REPO="lostf1sh/dusk"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
FORCE="${FORCE:-false}"

detect_os() {
    case "$(uname -s)" in
        Linux*)     echo "linux" ;;
        Darwin*)    echo "macos" ;;
        *)          echo "unsupported" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64)     echo "x86_64" ;;
        aarch64)    echo "aarch64" ;;
        armv7l)     echo "armv7" ;;
        *)          echo "unsupported" ;;
    esac
}

get_latest_version() {
    curl -sL "https://api.github.com/repos/$REPO/releases/latest" | grep -o '"tag_name": ".*"' | cut -d'"' -f4
}

resolve_target() {
    local os="$1"
    local arch="$2"

    case "$os/$arch" in
        linux/x86_64) echo "x86_64-unknown-linux-musl" ;;
        macos/x86_64) echo "x86_64-apple-darwin" ;;
        macos/aarch64) echo "aarch64-apple-darwin" ;;
        *) echo "" ;;
    esac
}

download_release() {
    local version="$1"
    local os="$2"
    local arch="$3"
    local target
    target=$(resolve_target "$os" "$arch")

    if [ -z "$target" ]; then
        echo "No prebuilt release for $os/$arch. Falling back to source install..."
        install_from_source
        return $?
    fi

    local filename="dusk-${target}.tar.gz"
    local url="https://github.com/$REPO/releases/download/${version}/${filename}"
    local tmpdir
    tmpdir=$(mktemp -d)
    local archive="$tmpdir/$filename"

    echo "Downloading dusk $version for $target..."
    curl -fSL "$url" -o "$archive" || {
        rm -rf "$tmpdir"
        return 1
    }

    echo "Extracting..."
    tar xzf "$archive" -C "$tmpdir"

    local binary="$tmpdir/dusk"
    if [ ! -f "$binary" ]; then
        echo "Error: Could not find extracted binary in $filename"
        rm -rf "$tmpdir"
        return 1
    fi

    echo "Installing to $INSTALL_DIR..."
    if [ "$FORCE" = "true" ] || [ -w "$INSTALL_DIR" ]; then
        mv "$binary" "$INSTALL_DIR/dusk"
        chmod +x "$INSTALL_DIR/dusk"
        echo "✓ Installed dusk to $INSTALL_DIR/dusk"
    else
        echo "Error: Cannot write to $INSTALL_DIR (try with sudo)"
        echo "  sudo mv $binary $INSTALL_DIR/dusk"
        rm -rf "$tmpdir"
        return 1
    fi

    rm -rf "$tmpdir"
    return 0
}

install_from_source() {
    echo "Building from source (requires Rust 1.77+)..."
    
    if ! command -v cargo &> /dev/null; then
        echo "Error: cargo not found. Install Rust from https://rustup.rs"
        return 1
    fi
    
    local tmpdir=$(mktemp -d)
    git clone --depth 1 "https://github.com/$REPO.git" "$tmpdir/dusk"
    cd "$tmpdir/dusk"
    cargo build --release
    
    if [ "$FORCE" = "true" ] || [ -w "$INSTALL_DIR" ]; then
        cp "target/release/dusk" "$INSTALL_DIR/dusk"
        chmod +x "$INSTALL_DIR/dusk"
        echo "✓ Installed dusk to $INSTALL_DIR/dusk"
    else
        echo "Error: Cannot write to $INSTALL_DIR (try with sudo)"
        echo "  sudo cp target/release/dusk $INSTALL_DIR/dusk"
        return 1
    fi
    
    rm -rf "$tmpdir"
    return 0
}

main() {
    echo "=== dusk installer ==="
    
    local os=$(detect_os)
    local arch=$(detect_arch)
    
    if [ "$os" = "unsupported" ] || [ "$arch" = "unsupported" ]; then
        echo "Unsupported platform. Installing from source instead..."
        install_from_source
        exit $?
    fi
    
    # Check for --source flag
    if [ "$1" = "--source" ]; then
        install_from_source
        exit $?
    fi
    
    local version
    version=$(get_latest_version) || {
        echo "Warning: Could not fetch latest version, trying from source..."
        install_from_source
        exit $?
    }
    
    echo "Latest version: $version"
    
    download_release "$version" "$os" "$arch"
}

main "$@"
