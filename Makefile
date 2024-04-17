SHELL := /bin/bash

repro:
	RUST_LOG=keom_clob=info cargo run --release
