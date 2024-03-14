#!/usr/bin/env bash
set -euo pipefail

image="composable/emulated-light-client"

docker build \
    -f ./docker/Dockerfile \
    -t "${image}:latest" .
