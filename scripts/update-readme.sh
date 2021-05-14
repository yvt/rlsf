#!/bin/sh

cd "`dirname "$0"`/../crates/rlsf"
cargo readme -t ../../README.tpl > ../../README.md
