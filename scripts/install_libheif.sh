#!/usr/bin/env bash
# Install a libheif version satisfying libheif-sys' minimum requirement
# (>= 1.21). The script is a no-op when the system pkg-config already
# reports a libheif at or above the minimum, which keeps local developer
# hosts with a newer libheif (Arch, current rolling distros) on the
# fast path.
#
# Used by:
#   - .github/workflows/ci.yml (gate job)
#   - .github/workflows/release.yml (gate + build jobs)
#   - docs/RUNBOOK.md § 2.4 / § 7.1 (operator-facing local install)
#
# The build-from-source path is required because Ubuntu 22.04/24.04 LTS
# and Debian 12 ship libheif 1.17.x or 1.18.x in apt, which is below
# the libheif-sys 5.x system_deps floor (v1_17..v1_21..v1_23 require
# >= 1.17/1.21/1.23 respectively). The `latest` feature of
# libheif-sys (default in Cargo.lock via libheif-rs 2.7 + image crate)
# activates v1_23, so the build must produce a libheif.pc reporting
# >= 1.21 to satisfy any of the activated features. We pin the rebuild
# to the lowest accepted version (1.21.x, pinned to 1.21.2 for the
# GCC 14 / Ubuntu 24.04 INT_MAX fix) to keep CI deterministic.
#
# Environment overrides:
#   LIBHEIF_MIN_VERSION       minimum acceptable version (default 1.21)
#   LIBHEIF_SOURCE_VERSION    version of the source tarball to fetch
#                             (default 1.21.2)
#   LIBHEIF_INSTALL_PREFIX    destination prefix for the rebuilt libheif
#                             (default /usr/local; CI passes an explicit
#                             value to avoid touching system paths)
#   LIBHEIF_KEEP_BUILD_DIR    when set, the extracted source + build
#                             directory is preserved at $LIBHEIF_KEEP_BUILD_DIR
#                             instead of being removed on exit; useful
#                             for offline debugging of the build.

set -euo pipefail

MIN_VERSION="${LIBHEIF_MIN_VERSION:-1.21}"
# Pinned to 1.21.2 rather than 1.21.0: 1.21.0 fails to build with
# GCC 14+ on Ubuntu 24.04 / Arch because heif_image_handle.cc uses
# INT_MAX without including <climits>. 1.21.1 / 1.21.2 carry the
# upstream fix. 1.21.2 is the highest 1.21.x patch release; it is
# fully ABI-compatible with 1.21.0 and satisfies libheif-sys' v1_21
# system_deps floor.
SOURCE_VERSION="${LIBHEIF_SOURCE_VERSION:-1.21.2}"
INSTALL_PREFIX="${LIBHEIF_INSTALL_PREFIX:-/usr/local}"
SOURCE_URL="https://github.com/strukturag/libheif/releases/download/v${SOURCE_VERSION}/libheif-${SOURCE_VERSION}.tar.gz"
SOURCE_SHA256="75f530b7154bc93e7ecf846edfc0416bf5f490612de8c45983c36385aa742b42"

CHECK_ONLY=0
ASSUME_YES=0
usage() {
    cat <<EOF
Usage: $0 [--version <min>] [--check] [--yes] [--prefix <path>]

  --version <min>   minimum acceptable libheif version (default: ${MIN_VERSION})
  --check           report whether the system libheif already satisfies the
                    minimum and exit; do not rebuild, do not install
  --yes             assume yes for any interactive prompt (apt-get -y)
  --prefix <path>   install prefix when rebuilding from source
                    (default: ${INSTALL_PREFIX})
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version) MIN_VERSION="$2"; shift 2 ;;
        --check)   CHECK_ONLY=1; shift ;;
        --yes)     ASSUME_YES=1; shift ;;
        --prefix)  INSTALL_PREFIX="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
    esac
done

log() { printf 'install_libheif: %s\n' "$*"; }
err() { printf 'install_libheif: %s\n' "$*" >&2; exit 1; }

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || err "required command '$1' not found on PATH"
}

# version_compare a b  --  returns 0 if a >= b (semver-ish, dot-separated)
version_compare() {
    [ "$1" = "$2" ] && return 0
    local highest
    highest=$(printf '%s\n%s' "$1" "$2" | sort -V | tail -n 1)
    [ "$highest" = "$1" ]
}

current_version() {
    if command -v pkg-config >/dev/null 2>&1 && pkg-config --exists libheif; then
        pkg-config --modversion libheif
    else
        printf ''
    fi
}

verdict="$(current_version)"
if [ -n "$verdict" ]; then
    if version_compare "$verdict" "$MIN_VERSION"; then
        log "system libheif ${verdict} satisfies >= ${MIN_VERSION} — no rebuild needed"
        exit 0
    fi
    log "system libheif ${verdict} is below ${MIN_VERSION}; rebuilding from source v${SOURCE_VERSION}"
else
    log "system libheif not discoverable via pkg-config; building from source v${SOURCE_VERSION}"
fi

if [ "$CHECK_ONLY" -eq 1 ]; then
    log "--check requested and a rebuild would be required; exiting non-zero"
    exit 1
fi

APT_GET=(apt-get)
if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        APT_GET=(sudo apt-get)
    else
        err "apt-get requires root and sudo is not available"
    fi
fi

WORK_DIR="${LIBHEIF_KEEP_BUILD_DIR:-$(mktemp -d)}"
if [ -z "${LIBHEIF_KEEP_BUILD_DIR:-}" ]; then
    trap 'rm -rf "${WORK_DIR}"' EXIT
fi

TARBALL="${WORK_DIR}/libheif-${SOURCE_VERSION}.tar.gz"
SOURCE_DIR="${WORK_DIR}/libheif-${SOURCE_VERSION}"
BUILD_DIR="${WORK_DIR}/build"

APT_FLAGS=()
if [ "$ASSUME_YES" -eq 1 ] || [ "${LIBHEIF_ASSUME_YES:-0}" = "1" ]; then
    APT_FLAGS=(-y)
fi

# Step 1: install build prerequisites (libde265-dev / libdav1d-dev are
# already pulled in by the CI workflow step that precedes this script;
# listed here for local developers running the script standalone).
log "installing build prerequisites via apt-get"
"${APT_GET[@]}" update "${APT_FLAGS[@]}"
"${APT_GET[@]}" install "${APT_FLAGS[@]}" \
    build-essential pkg-config cmake \
    zlib1g-dev \
    libde265-dev libdav1d-dev

# Step 2: fetch and verify the source tarball.
log "fetching libheif ${SOURCE_VERSION} source tarball"
require_cmd curl
curl -fsSL "${SOURCE_URL}" -o "${TARBALL}"
printf '%s  %s\n' "${SOURCE_SHA256}" "${TARBALL}" | sha256sum -c -

# Step 3: extract.
log "extracting source"
tar -xzf "${TARBALL}" -C "${WORK_DIR}"

# Step 4: configure with CMake. The libheif 1.21.x line still uses
# CMake as the upstream build system (meson support landed in 1.19
# but 1.21.x kept CMake as the primary). We build with the system
# libde265 + dav1d as internal codecs (static link into libheif) so
# the resulting library works without depending on the .heif
# plugin search path at runtime.
log "configuring with CMake (prefix=${INSTALL_PREFIX})"
cmake \
    -S "${SOURCE_DIR}" \
    -B "${BUILD_DIR}" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="${INSTALL_PREFIX}" \
    -DBUILD_TESTING=OFF \
    -DBUILD_DOCUMENTATION=OFF \
    -DWITH_EXAMPLES=OFF \
    -DWITH_GDK_PIXBUF=OFF \
    -DWITH_LIBDE265=ON \
    -DWITH_DAV1D=ON \
    -DENABLE_PLUGIN_LOADING=OFF \
    -DWITH_REDUCED_VISIBILITY=ON \
    -DWITH_LIBSHARPYUV=ON

# Step 5: compile.
log "compiling"
cmake --build "${BUILD_DIR}" --parallel "$(nproc)"

# Step 6: install.
log "installing to ${INSTALL_PREFIX}"
if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        sudo cmake --install "${BUILD_DIR}"
    else
        err "install requires root and sudo is not available"
    fi
else
    cmake --install "${BUILD_DIR}"
fi

# Step 7: refresh the dynamic linker cache so the freshly installed
# libheif.so is resolvable immediately.
if command -v ldconfig >/dev/null 2>&1; then
    if [ "$(id -u)" -ne 0 ] && command -v sudo >/dev/null 2>&1; then
        sudo ldconfig || true
    else
        ldconfig || true
    fi
fi

# Step 8: confirm the freshly installed library is visible to pkg-config.
export PKG_CONFIG_PATH="${INSTALL_PREFIX}/lib/pkgconfig:${INSTALL_PREFIX}/share/pkgconfig:${PKG_CONFIG_PATH:-}"
installed_version="$(pkg-config --modversion libheif)"
log "libheif ${installed_version} is now discoverable via pkg-config"
if ! version_compare "$installed_version" "$MIN_VERSION"; then
    err "installed libheif ${installed_version} still does not satisfy >= ${MIN_VERSION}"
fi
