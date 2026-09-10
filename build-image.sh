#!/bin/bash

set -euo pipefail

# 构建主机扫描套件镜像。源码仓库只负责镜像制品，.vsp 交付包由 vust-suites 维护。
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERSION="${1:-${RELEASE_VERSION:-0.1.0-alpha.1}}"
IMAGE_REPOSITORY="${IMAGE_REPOSITORY:-vustcc/host-scanner}"
IMAGE_NAME="$IMAGE_REPOSITORY:$VERSION"

echo "Building frontend..."
if [ ! -d "$REPO_ROOT/frontend/node_modules" ]; then
    pnpm -C "$REPO_ROOT/frontend" install --frozen-lockfile
fi
pnpm -C "$REPO_ROOT/frontend" build

echo "Building Rust binary..."
cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --release

echo "Building Docker image: $IMAGE_NAME"
docker build -t "$IMAGE_NAME" "$REPO_ROOT"

echo "Image built: $IMAGE_NAME"
