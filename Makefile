# Entry points for the whole workspace. Every target is safe to re-run.

PY := uv run --with numpy python3

.PHONY: all test fixtures reference cli wasm web lint clean

all: test wasm

## Rust unit and fixture tests.
test:
	cargo test --workspace

## Rebuild the MP3 corpus. Needs lame and ffmpeg.
fixtures:
	$(PY) fixtures/generate.py

## Compare our decode against ffmpeg's, sample for sample.
reference: cli
	$(PY) fixtures/compare_reference.py

cli:
	cargo build --release -p pimp3-cli

wasm:
	wasm-pack build crates/pimp3-wasm --target web --out-dir ../../web/src/wasm --release

web: wasm
	cd web && npm install && npm run build

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

clean:
	cargo clean
	rm -rf web/dist web/src/wasm web/node_modules fixtures/*.reference.wav
