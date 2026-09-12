#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# install-linux.sh — Standalone Linux installer for SpectraLang
#
# This script is the Linux equivalent of install-extension.ps1.
# It can be run from:
#   • A cloned git repo (will build from source)
#   • An extracted release tarball (uses pre-built binaries)
#
# What it does:
#   1. Builds or locates spectralang + spectra-lsp binaries
#   2. Installs them to ~/.local/bin (or another directory you choose)
#   3. Adds the directory to your shell PATH if missing
#   4. Registers the .spectra MIME type and file association
#   5. Installs the VS Code extension (VSIX or from source)
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

function print_step() {
    echo -e "${CYAN}==>${NC} $1"
}

function print_success() {
    echo -e "${GREEN}==>${NC} $1"
}

function print_warning() {
    echo -e "${YELLOW}==>${NC} $1"
}

function print_error() {
    echo -e "${RED}==>${NC} $1"
}

# ── Detect environment ───────────────────────────────────────────────────────
IS_REPO=false
if [[ -f "${REPO_ROOT}/Cargo.toml" ]] && [[ -d "${REPO_ROOT}/tools/vscode-extension" ]]; then
    IS_REPO=true
fi

# ── Resolve install directory ────────────────────────────────────────────────
DEFAULT_DEST="${HOME}/.local/bin"
read -rp "Install directory [${DEFAULT_DEST}]: " DEST
DEST="${DEST:-${DEFAULT_DEST}}"
mkdir -p "${DEST}"

# ── Resolve VS Code CLI ──────────────────────────────────────────────────────
function resolve_code_cli() {
    for cmd in code code-insiders; do
        if command -v "${cmd}" >/dev/null 2>&1; then
            echo "${cmd}"
            return 0
        fi
    done
    echo ""
}

CODE_CMD="$(resolve_code_cli)"

# ── Build or locate binaries ─────────────────────────────────────────────────
SPECTRALANG_BIN=""
SPC_BIN=""
SPECTRA_LSP_BIN=""
VSIX_PATH=""

if [[ "${IS_REPO}" == true ]]; then
    print_step "Repository detected. Building from source..."

    if ! command -v cargo >/dev/null 2>&1; then
        print_error "Rust/Cargo not found. Install Rust: https://rustup.rs/"
        exit 1
    fi

    cd "${REPO_ROOT}"
    cargo build --release -p spectra-cli -p spectra-lsp

    SPECTRALANG_BIN="${REPO_ROOT}/target/release/spectralang"
    SPC_BIN="${REPO_ROOT}/target/release/spc"
    SPECTRA_LSP_BIN="${REPO_ROOT}/target/release/spectra-lsp"

    if [[ ! -f "${SPECTRALANG_BIN}" ]]; then
        print_error "spectralang binary not found after build."
        exit 1
    fi
    if [[ ! -f "${SPC_BIN}" ]]; then
        print_error "spc binary not found after build."
        exit 1
    fi
    if [[ ! -f "${SPECTRA_LSP_BIN}" ]]; then
        print_error "spectra-lsp binary not found after build."
        exit 1
    fi

    # Build VSIX from source
    EXT_DIR="${REPO_ROOT}/tools/vscode-extension"
    if [[ ! -d "${EXT_DIR}/node_modules" ]]; then
        print_step "Installing npm dependencies for VS Code extension..."
        (cd "${EXT_DIR}" && npm install)
    fi
    print_step "Compiling VS Code extension..."
    (cd "${EXT_DIR}" && npm run compile)

    print_step "Packaging VS Code extension..."
    (cd "${EXT_DIR}" && npx @vscode/vsce package --no-git-tag-version)

    VSIX_PATH="$(ls "${EXT_DIR}"/spectra-vscode-extension-*.vsix | head -1)"
else
    print_step "Using pre-built binaries from ${SCRIPT_DIR}"

    SPECTRALANG_BIN="${SCRIPT_DIR}/bin/spectralang"
    SPC_BIN="${SCRIPT_DIR}/bin/spc"
    SPECTRA_LSP_BIN="${SCRIPT_DIR}/bin/spectra-lsp"
    VSIX_PATH="${SCRIPT_DIR}/extension/spectra-vscode-extension.vsix"

    if [[ ! -f "${SPECTRALANG_BIN}" ]]; then
        print_error "Pre-built spectralang not found at ${SPECTRALANG_BIN}"
        print_error "Run this script from an extracted release tarball or clone the repo."
        exit 1
    fi
    if [[ ! -f "${SPC_BIN}" ]]; then
        print_error "Pre-built spc not found at ${SPC_BIN}"
        exit 1
    fi
    if [[ ! -f "${SPECTRA_LSP_BIN}" ]]; then
        print_error "Pre-built spectra-lsp not found at ${SPECTRA_LSP_BIN}"
        exit 1
    fi
fi

# ── Install binaries ─────────────────────────────────────────────────────────
print_step "Installing binaries to ${DEST}..."
cp "${SPECTRALANG_BIN}" "${DEST}/spectralang"
cp "${SPC_BIN}" "${DEST}/spc"
cp "${SPECTRA_LSP_BIN}" "${DEST}/spectra-lsp"
chmod +x "${DEST}/spectralang" "${DEST}/spc" "${DEST}/spectra-lsp"

# ── Update PATH ──────────────────────────────────────────────────────────────
if ! echo "$PATH" | tr ':' '\n' | grep -qx "${DEST}"; then
    print_step "Adding ${DEST} to your PATH..."

    SHELL_RC=""
    if [[ -n "${ZSH_VERSION:-}" ]] || [[ "${SHELL}" == */zsh ]]; then
        SHELL_RC="${HOME}/.zshrc"
    elif [[ -n "${BASH_VERSION:-}" ]] || [[ "${SHELL}" == */bash ]]; then
        SHELL_RC="${HOME}/.bashrc"
    else
        SHELL_RC="${HOME}/.profile"
    fi

    if ! grep -q "export PATH=\".*${DEST}.*\"" "${SHELL_RC}" 2>/dev/null; then
        echo "export PATH=\"${DEST}:\$PATH\"" >> "${SHELL_RC}"
        print_success "Added ${DEST} to ${SHELL_RC}"
        print_warning "Run 'source ${SHELL_RC}' or open a new terminal to use spectralang."
    fi
else
    print_success "${DEST} is already in your PATH."
fi

# ── Install VS Code extension ────────────────────────────────────────────────
if [[ -f "${VSIX_PATH}" ]]; then
    if [[ -n "${CODE_CMD}" ]]; then
        print_step "Installing VS Code extension via ${CODE_CMD}..."
        "${CODE_CMD}" --install-extension "${VSIX_PATH}" --force || true
        print_success "VS Code extension installed."
    else
        print_warning "VS Code CLI not found. Install the extension manually:"
        print_warning "  code --install-extension '${VSIX_PATH}'"
    fi
else
    print_warning "VSIX not found. Skipping VS Code extension installation."
fi

# ── Register .spectra file association (best effort) ─────────────────────────
if command -v xdg-mime >/dev/null 2>&1; then
    print_step "Registering .spectra file association..."

    MIME_DIR="${HOME}/.local/share/mime"
    APP_DIR="${HOME}/.local/share/applications"
    mkdir -p "${MIME_DIR}/packages" "${APP_DIR}"

    cat > "${MIME_DIR}/packages/spectra.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="text/x-spectra">
    <comment>SpectraLang source file</comment>
    <glob pattern="*.spectra"/>
    <sub-class-of type="text/plain"/>
  </mime-type>
</mime-info>
EOF

    cat > "${APP_DIR}/spectra.desktop" <<EOF
[Desktop Entry]
Name=SpectraLang
Comment=SpectraLang compiler and runtime
Exec=${DEST}/spectralang run %f
Icon=utilities-terminal
Type=Application
Terminal=true
MimeType=text/x-spectra;
Categories=Development;IDE;
NoDisplay=true
EOF

    update-mime-database "${MIME_DIR}" 2>/dev/null || true
    update-desktop-database "${APP_DIR}" 2>/dev/null || true

    print_success ".spectra files associated with SpectraLang."
else
    print_warning "xdg-mime not found. Skipping .spectra file association."
fi

# ── Done ─────────────────────────────────────────────────────────────────────
echo ""
print_success "SpectraLang installed successfully!"
echo ""
echo "  Binaries:     ${DEST}/spectralang"
echo "                ${DEST}/spc  (alias)"
echo "                ${DEST}/spectra-lsp"
echo "  VSIX:         ${VSIX_PATH}"
if [[ -n "${CODE_CMD}" ]]; then
    echo "  VS Code:      ${CODE_CMD}"
fi
echo ""
echo "Quick start:"
echo "  spectralang --help"
echo "  spc --help"
echo "  spectralang version"
echo ""
