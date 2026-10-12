CARGO ?= cargo

.DEFAULT_GOAL := help

.PHONY: help
help: ## Show this help
	@awk 'BEGIN {FS = ":.*?## "} /^[a-z][a-z-]*:.*?## / {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

.PHONY: ci
ci: fmt-check lint test ## Everything CI runs

.PHONY: fmt
fmt: ## Format the code
	$(CARGO) fmt --all

.PHONY: fmt-check
fmt-check: ## Fail if the code is not formatted
	$(CARGO) fmt --all -- --check

.PHONY: lint
lint: ## Clippy, warnings are errors
	$(CARGO) clippy --all-targets --all-features -- -D warnings

.PHONY: test
test: ## Run the test suite
	$(CARGO) test

.PHONY: build
build: ## Release build
	$(CARGO) build --release

# What a release ships. Linux is built static against musl so one binary
# runs on any distribution.
DARWIN_TARGETS := aarch64-apple-darwin x86_64-apple-darwin
LINUX_TARGETS  := aarch64-unknown-linux-musl x86_64-unknown-linux-musl

ifeq ($(shell uname -s),Darwin)
DIST_TARGETS := $(DARWIN_TARGETS)
else
DIST_TARGETS := $(shell uname -m)-unknown-linux-musl
endif

DOCKER ?= docker
LINUX_IMAGE ?= rust:1-alpine

.PHONY: dist
dist: ## Release tarballs for this OS, every arch it can build
	@for t in $(DIST_TARGETS); do ./scripts/dist.sh $$t; done

.PHONY: dist-linux
dist-linux: ## Linux tarballs, both arches, in docker
	@for t in $(LINUX_TARGETS); do \
		case $$t in \
			aarch64-*) platform=linux/arm64 ;; \
			x86_64-*)  platform=linux/amd64 ;; \
		esac; \
		echo "==> $$t on $$platform"; \
		$(DOCKER) run --rm --platform $$platform \
			--user "$$(id -u):$$(id -g)" \
			-v "$(CURDIR)":/src -w /src \
			-e CARGO_HOME=/tmp/cargo -e CARGO_TARGET_DIR=/tmp/target \
			$(LINUX_IMAGE) ./scripts/dist.sh $$t; \
	done

.PHONY: dist-all
dist-all: dist dist-linux ## Every platform a release ships

.PHONY: run
run: ## Run against your kubeconfig
	$(CARGO) run

.PHONY: install
install: ## Install k9mux into ~/.cargo/bin
	$(CARGO) install --path .

.PHONY: clean
clean: ## Remove build artifacts
	$(CARGO) clean
	rm -rf dist
