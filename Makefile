BIN      := sqlv
VERSION  := 1.0.0
ARCH     := amd64
DEB_DIR  := dist/$(BIN)_$(VERSION)_$(ARCH)

.PHONY: build release deb install clean

## Build debug binary
build:
	cargo build

## Build optimised release binary
release:
	cargo build --release

## Build a .deb package (requires release binary)
deb: release
	mkdir -p $(DEB_DIR)/usr/bin
	mkdir -p $(DEB_DIR)/DEBIAN
	cp target/release/$(BIN) $(DEB_DIR)/usr/bin/$(BIN)
	chmod 755 $(DEB_DIR)/usr/bin/$(BIN)
	@printf "Package: $(BIN)\nVersion: $(VERSION)\nArchitecture: $(ARCH)\nMaintainer: Your Name <you@example.com>\nDepends:\nSection: utils\nPriority: optional\nDescription: A modern terminal SQLite viewer\n Zero-dependency TUI for browsing SQLite databases.\n" \
		> $(DEB_DIR)/DEBIAN/control
	dpkg-deb --build --root-owner-group $(DEB_DIR)
	@echo "\nPackage built: dist/$(BIN)_$(VERSION)_$(ARCH).deb"
	@echo "Install with: sudo dpkg -i dist/$(BIN)_$(VERSION)_$(ARCH).deb"

## Install binary to /usr/local/bin directly
install: release
	install -m 755 target/release/$(BIN) /usr/local/bin/$(BIN)
	@echo "Installed to /usr/local/bin/$(BIN)"

clean:
	cargo clean
	rm -rf dist/
