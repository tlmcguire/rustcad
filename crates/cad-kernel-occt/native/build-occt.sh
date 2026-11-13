#!/usr/bin/env bash
# Builds the pinned OpenCascade (OCCT) release as shared libraries.
#
# Usage: bash build-occt.sh <work-dir>
#   <work-dir>/src      source checkout
#   <work-dir>/build    CMake build tree
#   <work-dir>/install  install prefix; point OCCT_ROOT here
#
# Needs git, cmake (3.16 or newer), ninja, and a C++17 compiler on PATH.
# Only the three modules the Kernel trait needs are built.
set -euo pipefail

OCCT_TAG="V7_9_3"
OCCT_COMMIT="a016080bf6738d6aeae020badee4e888ad1540a5"
OCCT_URL="https://github.com/Open-Cascade-SAS/OCCT.git"

work="${1:?usage: build-occt.sh <work-dir>}"
mkdir -p "$work"
work="$(cd "$work" && pwd)"

if [ ! -d "$work/src/.git" ]; then
  git clone --depth 1 --branch "$OCCT_TAG" "$OCCT_URL" "$work/src"
fi
actual="$(git -C "$work/src" rev-parse HEAD)"
if [ "$actual" != "$OCCT_COMMIT" ]; then
  echo "OCCT checkout is $actual, expected $OCCT_COMMIT ($OCCT_TAG)" >&2
  exit 1
fi

cmake -S "$work/src" -B "$work/build" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
  -DINSTALL_DIR="$work/install" \
  -DCMAKE_INSTALL_NAME_DIR="$work/install/lib" \
  -DBUILD_LIBRARY_TYPE=Shared \
  -DBUILD_MODULE_FoundationClasses=ON \
  -DBUILD_MODULE_ModelingData=ON \
  -DBUILD_MODULE_ModelingAlgorithms=ON \
  -DBUILD_MODULE_Visualization=OFF \
  -DBUILD_MODULE_ApplicationFramework=OFF \
  -DBUILD_MODULE_DataExchange=OFF \
  -DBUILD_MODULE_Draw=OFF \
  -DBUILD_MODULE_DETools=OFF \
  -DBUILD_DOC_Overview=OFF \
  -DUSE_FREETYPE=OFF -DUSE_TK=OFF -DUSE_TCL=OFF -DUSE_OPENGL=OFF \
  -DUSE_XLIB=OFF -DUSE_TBB=OFF -DUSE_VTK=OFF -DUSE_RAPIDJSON=OFF -DUSE_DRACO=OFF

start=$(date +%s)
cmake --build "$work/build"
cmake --install "$work/build" > "$work/install.log"
echo "OCCT $OCCT_TAG built and installed in $(( $(date +%s) - start )) s: $work/install"
