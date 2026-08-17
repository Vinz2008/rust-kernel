#!/bin/bash

cargo build-userspace "$@"
cargo build "$@"
cargo build "$@"