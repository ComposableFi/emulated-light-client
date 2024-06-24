#!/usr/bin/env bash
set -euo pipefail

image="composable/emulated-light-client"
tag="$(git describe --tags --exact-match HEAD 2>/dev/null || git rev-parse --short HEAD)"

docker build \
    -f ./docker/Dockerfile \
    -t "${image}:${tag}" .
