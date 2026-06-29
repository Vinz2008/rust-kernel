#!/bin/bash

cargo build-init "$@"
cargo build-userspace "$@"
#cargo build "$@"