#!/usr/bin/env bash
set -e
set -u
set -o pipefail

# ANSI color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

REPO="dimzragil/doraivu"
BINARY="doraivu"

echo -e "${BLUE}==>${NC} Installing ${BINARY}..."

# 1. Detect OS
OS="$(uname -s)"
case "${OS}" in
    Linux*)     OS_LOWER=linux;;
    Darwin*)    OS_LOWER=darwin;;
    *)          echo -e "${RED}Error: Unsupported OS ${OS}${NC}"; exit 1;;
esac

# 2. Detect Architecture
ARCH="$(uname -m)"
case "${ARCH}" in
    x86_64)     ARCH_LOWER=x86_64;;
    arm64)      ARCH_LOWER=aarch64;;
    aarch64)    ARCH_LOWER=aarch64;;
    *)          echo -e "${RED}Error: Unsupported architecture ${ARCH}${NC}"; exit 1;;
esac

echo -e "${BLUE}==>${NC} Detected Platform: ${OS_LOWER}-${ARCH_LOWER}"

# 3. Fetch latest release tag
echo -e "${BLUE}==>${NC} Fetching latest release version..."
LATEST_TAG=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "${LATEST_TAG}" ]; then
    echo -e "${RED}Error: Failed to fetch the latest release tag from GitHub.${NC}"
    exit 1
fi

echo -e "${BLUE}==>${NC} Latest version is ${LATEST_TAG}"

# Form download URL (Assuming standard naming convention: doraivu-<os>-<arch>.tar.gz)
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/${BINARY}-${OS_LOWER}-${ARCH_LOWER}.tar.gz"

# 4. Download and extract
TMP_DIR=$(mktemp -d)
TAR_FILE="${TMP_DIR}/${BINARY}.tar.gz"

echo -e "${BLUE}==>${NC} Downloading ${DOWNLOAD_URL}..."
curl -sSfL "${DOWNLOAD_URL}" -o "${TAR_FILE}"

echo -e "${BLUE}==>${NC} Extracting..."
tar -xzf "${TAR_FILE}" -C "${TMP_DIR}"

# 5. Install Binary
INSTALL_DIR=""
if [ -d "$HOME/.local/bin" ] && [[ ":$PATH:" == *":$HOME/.local/bin:"* ]]; then
    INSTALL_DIR="$HOME/.local/bin"
else
    INSTALL_DIR="/usr/local/bin"
fi

echo -e "${BLUE}==>${NC} Installing binary to ${INSTALL_DIR}..."

if [ "$INSTALL_DIR" = "/usr/local/bin" ]; then
    if command -v sudo >/dev/null 2>&1; then
        sudo mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
        sudo chmod +x "${INSTALL_DIR}/${BINARY}"
    else
        echo -e "${YELLOW}Warning: 'sudo' not found. Attempting to install to ${INSTALL_DIR} without it...${NC}"
        mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
        chmod +x "${INSTALL_DIR}/${BINARY}"
    fi
else
    mv "${TMP_DIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    chmod +x "${INSTALL_DIR}/${BINARY}"
fi

# Cleanup
rm -rf "${TMP_DIR}"

echo -e "${GREEN}==>${NC} Successfully installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
echo -e "${GREEN}==>${NC} Run '${BINARY}' to get started!"
