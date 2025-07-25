#!/bin/bash

set -euo pipefail

# Configuration
PKG_NAME="game-cheetah"
AUR_USERNAME="your-aur-username"  # Change this to your AUR username

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}==> Preparing AUR submission for $PKG_NAME${NC}"

# Check if PKGBUILD exists
if [ ! -f "PKGBUILD" ]; then
    echo -e "${RED}Error: PKGBUILD not found. Run build_arch.sh first.${NC}"
    exit 1
fi

# Create AUR directory
AUR_DIR="aur-$PKG_NAME"
rm -rf "$AUR_DIR"
mkdir -p "$AUR_DIR"

# Copy only necessary files for AUR
cp PKGBUILD "$AUR_DIR/"

# Generate .SRCINFO
cd "$AUR_DIR"
makepkg --printsrcinfo > .SRCINFO

# Initialize git repository
git init
git add PKGBUILD .SRCINFO
git commit -m "Initial commit"

echo -e "${GREEN}==> AUR package prepared in $AUR_DIR/${NC}"
echo ""
echo -e "${BLUE}==> Next steps:${NC}"
echo "1. Clone the AUR repository (if package doesn't exist yet):"
echo "   git clone ssh://aur@aur.archlinux.org/$PKG_NAME.git"
echo ""
echo "2. If this is a new package, you need to create it first:"
echo "   cd $PKG_NAME"
echo "   cp ../$AUR_DIR/* ."
echo "   git add ."
echo "   git commit -m 'Initial submission'"
echo "   git push"
echo ""
echo "3. For updates to existing package:"
echo "   cd $PKG_NAME"
echo "   git pull"
echo "   cp ../$AUR_DIR/* ."
echo "   git add ."
echo "   git commit -m 'Update to version X.Y.Z'"
echo "   git push"
