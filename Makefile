BIN_NAME := onec-download-rs
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo dev)
CARGO ?= cargo
CARGO_FLAGS ?= --locked
CARGO_TARGET_DIR ?= target
DIST_DIR ?= dist
PROFILE ?= release

LINUX_X64_TARGET := x86_64-unknown-linux-gnu
WINDOWS_X64_MSVC_TARGET := x86_64-pc-windows-msvc
WINDOWS_X64_GNU_TARGET := x86_64-pc-windows-gnu
MACOS_X64_TARGET := x86_64-apple-darwin
MACOS_AARCH64_TARGET := aarch64-apple-darwin

ifeq ($(PROFILE),release)
PROFILE_FLAG := --release
PROFILE_DIR := release
else
PROFILE_FLAG :=
PROFILE_DIR := debug
endif

export CARGO_TARGET_DIR

.PHONY: help
help:
	@printf '%s\n' \
		'Targets:' \
		'  make check                  Run fmt, clippy, and tests' \
		'  make build                  Build host release binary' \
		'  make build-linux-x64        Build Linux x64 binary' \
		'  make build-windows-x64      Build Windows x64 MSVC binary' \
		'  make build-windows-x64-gnu  Build Windows x64 GNU binary' \
		'  make build-macos-x64        Build macOS x64 binary' \
		'  make build-macos-aarch64    Build macOS ARM64 binary' \
		'  make dist-linux-x64         Copy portable Linux binary to dist/' \
		'  make dist-windows-x64       Copy portable Windows MSVC binary to dist/' \
		'  make dist                   Build all release portable binaries' \
		'' \
		'Cross-platform builds require the matching Rust target and linker.'

.PHONY: fmt
fmt:
	$(CARGO) fmt --all -- --check

.PHONY: clippy
clippy:
	$(CARGO) clippy $(CARGO_FLAGS) --all-targets --all-features -- -D warnings

.PHONY: test
test:
	$(CARGO) test $(CARGO_FLAGS) --all-targets --all-features

.PHONY: check
check: fmt clippy test

.PHONY: build
build:
	$(CARGO) build $(CARGO_FLAGS) $(PROFILE_FLAG)

.PHONY: install-targets
install-targets:
	rustup target add \
		$(LINUX_X64_TARGET) \
		$(WINDOWS_X64_MSVC_TARGET) \
		$(WINDOWS_X64_GNU_TARGET) \
		$(MACOS_X64_TARGET) \
		$(MACOS_AARCH64_TARGET)

define RUST_BUILD_RULES
.PHONY: build-$(1)
build-$(1):
	$$(CARGO) build $$(CARGO_FLAGS) $$(PROFILE_FLAG) --target $(2)

.PHONY: dist-$(1)
dist-$(1): build-$(1)
	mkdir -p "$$(DIST_DIR)"
	cp "$$(CARGO_TARGET_DIR)/$(2)/$$(PROFILE_DIR)/$$(BIN_NAME)$(3)" "$$(DIST_DIR)/$$(BIN_NAME)-$$(VERSION)-$(1)$(3)"
	$(if $(3),,@chmod +x "$$(DIST_DIR)/$$(BIN_NAME)-$$(VERSION)-$(1)$(3)")
endef

$(eval $(call RUST_BUILD_RULES,linux-x64,$(LINUX_X64_TARGET),))
$(eval $(call RUST_BUILD_RULES,windows-x64-msvc,$(WINDOWS_X64_MSVC_TARGET),.exe))
$(eval $(call RUST_BUILD_RULES,windows-x64-gnu,$(WINDOWS_X64_GNU_TARGET),.exe))
$(eval $(call RUST_BUILD_RULES,macos-x64,$(MACOS_X64_TARGET),))
$(eval $(call RUST_BUILD_RULES,macos-aarch64,$(MACOS_AARCH64_TARGET),))

.PHONY: build-windows-x64
build-windows-x64: build-windows-x64-msvc

.PHONY: dist-windows-x64
dist-windows-x64: dist-windows-x64-msvc

.PHONY: build-all
build-all: build-linux-x64 build-windows-x64-msvc build-macos-x64 build-macos-aarch64

.PHONY: dist
dist: dist-linux-x64 dist-windows-x64-msvc dist-macos-x64 dist-macos-aarch64

.PHONY: clean-dist
clean-dist:
	rm -rf "$(DIST_DIR)"
