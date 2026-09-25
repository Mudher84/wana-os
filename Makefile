# Wana OS top-level build entry point. `make help` lists targets.
# Build output goes to out/ (git-ignored); Rust output goes to target/.

CARGO ?= cargo

.PHONY: help check fmt fmt-check lint test repo-check clean

help:
	@echo "Wana OS build targets:"
	@echo "  make check       fmt-check + lint + test + repo-check (run before every push)"
	@echo "  make fmt         format Rust code"
	@echo "  make fmt-check   verify Rust formatting"
	@echo "  make lint        clippy, warnings are errors"
	@echo "  make test        unit tests"
	@echo "  make repo-check  repository hygiene (no tracked build output, required docs)"
	@echo "  make clean       remove build output"

check: fmt-check lint test repo-check

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --workspace --all-targets --locked -- -D warnings

test:
	$(CARGO) test --workspace --locked

repo-check:
	tools/check-repo.sh

clean:
	$(CARGO) clean
	rm -rf out
