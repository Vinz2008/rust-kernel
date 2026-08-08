#!/bin/bash

FRONTEND_THREADS=8

RUSTFLAGS="-Z threads=${FRONTEND_THREADS}" cargo build-init "$@"
RUSTFLAGS="-Z threads=${FRONTEND_THREADS}" cargo build-userspace "$@"
RUSTFLAGS="-Z threads=${FRONTEND_THREADS}" cargo build "$@"
RUSTFLAGS="-Z threads=${FRONTEND_THREADS}" cargo build "$@"