# Marginalia — task runner
#
# A mirror of the justfile, for anyone who would rather not install `just`.
# Target names are identical in both, so instructions can say `make test` or
# `just test` interchangeably.
#
#   make            list everything
#   make check      everything CI runs
#   make device     what you can do to a connected reMarkable

.DEFAULT_GOAL := help
SHELL := /usr/bin/env bash
.SHELLFLAGS := -euo pipefail -c

RM_TARGET := armv7-unknown-linux-gnueabihf
RM_HOST   ?= 10.11.99.1
PORTABLE  := -p marginalia-core -p marginalia-safety -p marginalia-observability \
             -p marginalia-remarkable -p marginalia-platform -p marginalia-zotero \
             -p marginalia-library-folder -p marginalia-annotations
# Not $TMPDIR: on macOS that lives under /var, which the agent refuses to write
# to. The refusal is correct -- /var belongs to the device -- so the dev home
# goes somewhere unambiguously ours instead.
AGENT_DEV_HOME := $(HOME)/.marginalia-dev

# Every target that produces no file of its own.
.PHONY: help setup setup-rust setup-cross \
        tui agent agent-init \
        check test test-safety test-arch test-characterization test-device-faults \
        test-zotero-live lint fmt fmt-check \
        build cross-check build-device build-device-docker verify-device-binary \
        device device-doctor device-install-dry device-install device-status \
        device-check device-reset-dry device-reset \
        clean docs stats

## help: list every target
help:
	@echo "Marginalia"
	@echo ""
	@awk 'BEGIN {section=""} \
	     /^# ===/ {next} \
	     /^##@/ {printf "\n\033[1m%s\033[0m\n", substr($$0, 5); next} \
	     /^## / {split(substr($$0,4), p, ": "); printf "  \033[36m%-24s\033[0m %s\n", p[1], p[2]}' \
	     $(MAKEFILE_LIST)
	@echo ""

##@ Setup

## setup: everything needed to build and test — run once
setup: setup-rust
	@echo ""
	@echo "Ready. Try: make tui"

## setup-rust: Rust toolchain plus the reMarkable's ARM target
setup-rust:
	rustup target add $(RM_TARGET)
	@echo "Rust ready ($$(rustc --version))"

## setup-cross: install `cross`, which builds for the device (needs Docker)
setup-cross:
	cargo install cross

##@ Develop

## tui: the terminal interface — install, check, configure, remove
tui:
	cargo run -q -p marginalia-tui

## agent: run the on-device agent locally — make agent ARGS=doctor
agent: ARGS ?= status
agent:
	MARGINALIA_HOME="$(AGENT_DEV_HOME)" cargo run -q -p marginalia-agent -- $(ARGS)

## agent-init: create the agent's local scratch home and database
agent-init:
	$(MAKE) agent ARGS=init

##@ Verify

## check: everything CI runs — do this before opening a pull request
check: fmt-check lint test test-safety cross-check
	@echo ""
	@echo "All checks passed."

## test: the whole Rust test suite
test:
	cargo test --workspace --all-features

## test-safety: the mandatory safety suite, with output
test-safety:
	cargo test -p marginalia-safety-suite --all-features -- --nocapture

## test-arch: dependency-direction and forbidden-import rules
test-arch:
	cargo test -p marginalia-architecture-tests

## test-characterization: Phase 0 behaviour, pinned before it moves
test-characterization:
	cargo test -p marginalia-characterization-tests

## test-device-faults: power loss, corruption, storage pressure, clock skew
test-device-faults:
	cargo test -p marginalia-simulator --test device_faults -- --nocapture

## test-zotero-live: talk to the real Zotero API (needs MARGINALIA_ZOTERO_API_KEY)
test-zotero-live:
	cargo test -p marginalia-zotero --features http -- --ignored --nocapture

## lint: clippy, warnings denied
lint:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

## fmt: format everything
fmt:
	cargo fmt --all

## fmt-check: fail if anything is unformatted
fmt-check:
	cargo fmt --all -- --check

##@ Build

## build: release build of everything that builds here
build:
	cargo build --release --workspace

## cross-check: prove the portable crates still compile for the reMarkable
cross-check:
	cargo check --target $(RM_TARGET) $(PORTABLE)

## build-device: build the agent for the reMarkable (needs `cross`)
build-device:
	@if command -v cross >/dev/null 2>&1; then \
	    cross build --release --target $(RM_TARGET) -p marginalia-agent; \
	elif command -v arm-linux-gnueabihf-gcc >/dev/null 2>&1; then \
	    cargo build --release --target $(RM_TARGET) -p marginalia-agent; \
	else \
	    echo "No ARM cross-compiler. Run: make setup-cross" >&2; exit 1; \
	fi
	@ls -lh target/$(RM_TARGET)/release/marginalia

## build-device-docker: build the agent for the reMarkable in a container (needs only Docker)
build-device-docker:
	./tools/device/build-in-docker.sh

## verify-device-binary: run the built ARM agent under emulation
verify-device-binary:
	@BIN=target/device/$(RM_TARGET)/release/marginalia; \
	 [ -f "$$BIN" ] || { echo "Build it first: make build-device-docker" >&2; exit 1; }; \
	 file "$$BIN"; \
	 docker run --rm --platform linux/arm/v7 \
	   -v "$(PWD)/target/device/$(RM_TARGET)/release":/x:ro \
	   -e MARGINALIA_HOME=/data/.marginalia \
	   debian:bookworm-slim bash -c '/x/marginalia init && /x/marginalia doctor'

##@ Device — everything below talks to a connected reMarkable

## device: what you can do to a connected reMarkable
device:
	@echo "  make device-doctor       check everything, change nothing"
	@echo "  make device-install-dry  show what installing would do"
	@echo "  make device-install      install"
	@echo "  make device-status       ask the installed agent how it is"
	@echo "  make device-reset-dry    show what removing would take"
	@echo "  make device-reset        remove it, and verify it is gone"
	@echo ""
	@echo "  RM_HOST=$(RM_HOST)  (set it to use Wi-Fi)"

## device-doctor: read-only checks, on your machine and your device
device-doctor:
	./tools/device/doctor.sh

## device-install-dry: show every install step without performing any
device-install-dry:
	./tools/device/install.sh --dry-run

## device-install: install the agent on the connected reMarkable
device-install:
	./tools/device/install.sh

## device-status: ask the installed agent to report its state
device-status:
	ssh "root@$(RM_HOST)" '/home/root/.marginalia/bin/marginalia status'

## device-check: ask the installed agent to check itself
device-check:
	ssh "root@$(RM_HOST)" '/home/root/.marginalia/bin/marginalia doctor'

## device-reset-dry: list what removal would take, without taking it
device-reset-dry:
	./tools/device/reset.sh --dry-run

## device-reset: remove Marginalia and return the device to stock
device-reset:
	./tools/device/reset.sh

##@ Housekeeping

## clean: remove build artefacts
clean:
	cargo clean

## docs: the documents worth reading first
docs:
	@echo "  README.md                                    what this is"
	@echo "  docs/INSTALL.md                              install on your computer"
	@echo "  docs/INSTALL_REMARKABLE.md                   install on your reMarkable"
	@echo "  docs/USING_MARGINALIA.md                     how to actually use it"
	@echo "  ROADMAP.md                                   what is built and what is next"
	@echo ""
	@echo "  docs/architecture/ARCHITECTURE.md            the design"
	@echo "  docs/safety/SAFETY_MODEL.md                  what protects your device"
	@echo "  docs/safety/DEVICE_WRITE_POLICY.md           exactly what may be written"
	@echo "  docs/adr/                                    decisions, including the open ones"
	@echo "  docs/development/OPEN_QUESTIONS.md           what is still unknown"
	@echo "  docs/remarkable/HARDWARE_VALIDATION.md       what a real device did"

## stats: count what exists, for a sense of scale
stats:
	@echo "Rust:  $$(find packages apps tests -name '*.rs' | xargs wc -l | tail -1 | awk '{print $$1}') lines"
	@echo "Tests: $$(grep -rc '#\[test\]' --include='*.rs' packages apps tests | awk -F: '{s+=$$2} END {print s}')"
	@echo "Docs:  $$(find docs -name '*.md' | wc -l | tr -d ' ') documents"
