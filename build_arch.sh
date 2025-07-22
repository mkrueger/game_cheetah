#!/bin/bash
# filepath: /home/mkrueger/work/game_cheetah/build_arch.sh

set -euo pipefail

# --- Configuration ---
APP_NAME="game-cheetah"
PKG_NAME="game-cheetah"
BUILD_DIR="arch-build"

# --- Colors for output ---
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# --- Pre-flight checks ---
if [ "$EUID" -eq 0 ]; then
    echo -e "${RED}Error: This script must not be run as root. Please run as a regular user.${NC}"
    exit 1
fi

if ! command -v makepkg &> /dev/null; then
    echo -e "${RED}Error: 'makepkg' command not found. Is 'base-devel' package group installed?${NC}"
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: 'cargo' command not found. Is Rust installed?${NC}"
    exit 1
fi

# --- Main script ---
echo -e "${BLUE}==> Building Arch Linux package for $APP_NAME${NC}"

# More robust version fetching
VERSION=$(cargo pkgid | sed 's/.*#//' | sed 's/[^0-9.]//g')
echo -e "${GREEN}==> Version: $VERSION${NC}"

# Get absolute path of project root
PROJECT_ROOT=$(pwd)

# Clean and create build directory
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

# Create PKGBUILD in the build directory
echo -e "${BLUE}==> Creating PKGBUILD...${NC}"
cat > "$BUILD_DIR/PKGBUILD" << EOF
# Maintainer: Mike Krüger <mkrueger@posteo.de>
pkgname=$PKG_NAME
pkgver=$VERSION
pkgrel=1
pkgdesc="High-performance memory scanner/editor and game trainer"
arch=('x86_64')
url="https://github.com/mkrueger/game_cheetah"
license=('Apache')
depends=('gtk3' 'libxcb' 'libxkbcommon' 'wayland' 'libgl' 'fontconfig' 'freetype2')
makedepends=('rust' 'cargo')
options=('!strip') # Optional: keep debug symbols for better crash reports
source=()
sha256sums=()

prepare() {
    cd "\$srcdir"
    # Copy the project files, excluding the build directory
    rsync -a --exclude="$BUILD_DIR" --exclude="target" "$PROJECT_ROOT"/ .
}

build() {
    cd "\$srcdir"
    export RUSTFLAGS="-C target-cpu=x86-64-v2"
    cargo build --release --locked
}

check() {
    cd "\$srcdir"
    cargo test --release --locked || true
}

package() {
    cd "\$srcdir"
    
    # Install binary
    install -Dm755 "target/release/$APP_NAME" "\$pkgdir/usr/bin/$PKG_NAME"
    
    # Install desktop file
    install -Dm644 "build/linux/${APP_NAME}.desktop" "\$pkgdir/usr/share/applications/$PKG_NAME.desktop"
    
    # Fix the desktop file to use correct binary and icon names
    sed -i "s/Exec=.*$/Exec=$PKG_NAME/" "\$pkgdir/usr/share/applications/$PKG_NAME.desktop"
    sed -i "s/Icon=.*$/Icon=$PKG_NAME/" "\$pkgdir/usr/share/applications/$PKG_NAME.desktop"
    sed -i "s/StartupWMClass=.*$/StartupWMClass=$PKG_NAME/" "\$pkgdir/usr/share/applications/$PKG_NAME.desktop"
    
    # Install icons with the correct name (game-cheetah instead of game_cheetah)
    install -Dm644 "build/linux/128x128.png" "\$pkgdir/usr/share/icons/hicolor/128x128/apps/$PKG_NAME.png"
    if [ -f "build/linux/256x256.png" ]; then
        install -Dm644 "build/linux/256x256.png" "\$pkgdir/usr/share/icons/hicolor/256x256/apps/$PKG_NAME.png"
    fi
    
    # Install license if it exists
    if [ -f "LICENSE" ]; then
        install -Dm644 "LICENSE" "\$pkgdir/usr/share/licenses/\$pkgname/LICENSE"
    fi
    
    # Install documentation
    install -Dm644 "README.md" "\$pkgdir/usr/share/doc/\$pkgname/README.md"
}
EOF

# Navigate to build dir and run makepkg
cd "$BUILD_DIR"
echo -e "${BLUE}==> Building package with makepkg...${NC}"
# -f: overwrite existing package
# -s: install missing dependencies
# -r: remove build dependencies after build
# --noconfirm: don't ask for confirmation
makepkg -fsr --noconfirm

# --- Post-build ---
PACKAGE_FILE=$(find . -maxdepth 1 -name "*.pkg.tar.*" | head -n1)
if [ -z "$PACKAGE_FILE" ]; then
    echo -e "${RED}Error: No package file found. Build may have failed.${NC}"
    exit 1
fi

echo -e "${GREEN}==> Package created successfully!${NC}"
echo -e "${GREEN}==> File created: $PACKAGE_FILE${NC}"

# Generate .SRCINFO for AUR submission
echo -e "${BLUE}==> Generating .SRCINFO...${NC}"
makepkg --printsrcinfo > .SRCINFO 2>/dev/null || true

# Move files to project root
mv "$PACKAGE_FILE" ../
cp PKGBUILD ../
cp .SRCINFO ../ 2>/dev/null || true

cd ..
rm -rf "$BUILD_DIR"

FINAL_PACKAGE=$(basename "$PACKAGE_FILE")
echo -e "${BLUE}==> To install the package, run:${NC}"
echo -e "    sudo pacman -U $FINAL_PACKAGE"
echo ""
echo -e "${BLUE}==> Package details:${NC}"
echo -e "    Package: $FINAL_PACKAGE"
echo -e "    Size: $(du -h "$FINAL_PACKAGE" | cut -f1)"