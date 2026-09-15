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

.PHONY: run
run: ## Run against your kubeconfig
	$(CARGO) run

.PHONY: install
install: ## Install k9mux into ~/.cargo/bin
	$(CARGO) install --path .

.PHONY: clean
clean: ## Remove build artifacts
	$(CARGO) clean
