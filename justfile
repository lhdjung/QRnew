# Name of the application's binary.
name := 'qrnew'
# The unique ID of the application.
appid := 'com.github.pop-os.cosmic-app-template'

# Path to root file system, which defaults to `/`.
rootdir := ''
# The prefix for the `/usr` directory.
prefix := '/usr'
# The location of the cargo target directory.
cargo-target-dir := env('CARGO_TARGET_DIR', 'target')

# Application's appstream metadata
appdata := appid + '.metainfo.xml'
# Application's desktop entry
desktop := appid + '.desktop'
# Application's icon.
icon-svg := appid + '.svg'

# Install destinations
base-dir := absolute_path(clean(rootdir / prefix))
appdata-dst := base-dir / 'share' / 'appdata' / appdata
bin-dst := base-dir / 'bin' / name
desktop-dst := base-dir / 'share' / 'applications' / desktop
icons-dst := base-dir / 'share' / 'icons' / 'hicolor'
icon-svg-dst := icons-dst / 'scalable' / 'apps'

# Default recipe which runs `just build-release`
default: build-release

# Runs `cargo clean`
clean:
    cargo clean

# Removes vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# `cargo clean` and removes vendored dependencies
clean-dist: clean clean-vendor

# Compiles with debug profile
build-debug *args:
    cargo build --locked {{args}}

# Compiles with release profile
build-release *args: (build-debug '--release' args)

# Compiles release profile with vendored dependencies
build-vendored *args: vendor-extract (build-release '--frozen --offline' args)

# Runs a clippy check
check *args:
    cargo clippy --all-features --locked {{args}} -- -W clippy::pedantic

# Runs a clippy check with JSON message format
check-json: (check '--message-format=json')

# Run the application for testing purposes
run *args:
    env RUST_BACKTRACE=full cargo run --release --locked {{args}}

# Installs files
install:
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} {{bin-dst}}
    install -Dm0644 {{ 'resources' / desktop }} {{desktop-dst}}
    install -Dm0644 {{ 'resources' / appdata }} {{appdata-dst}}
    install -Dm0644 {{ 'resources' / 'icons' / 'hicolor' / 'scalable' / 'apps' / 'icon.svg' }} {{icon-svg-dst}}

# Uninstalls installed files
uninstall:
    rm {{bin-dst}} {{desktop-dst}} {{icon-svg-dst}}

# Creates a macOS .app bundle at QrNew.app/; drag it to /Applications to install
bundle-macos: build-release
    #!/usr/bin/env bash
    set -euo pipefail
    rm -rf QrNew.app
    mkdir -p QrNew.app/Contents/MacOS QrNew.app/Contents/Resources
    cp {{cargo-target-dir}}/release/{{name}} QrNew.app/Contents/MacOS/{{name}}
    icon=resources/icons/hicolor/scalable/apps/icon.svg
    iconset=/tmp/qrnew_$$.iconset
    mkdir "$iconset"
    for size in 16 32 128 256 512; do
        magick "$icon" -background none -resize "${size}x${size}"     "$iconset/icon_${size}x${size}.png"
        double=$((size * 2))
        magick "$icon" -background none -resize "${double}x${double}" "$iconset/icon_${size}x${size}@2x.png"
    done
    iconutil -c icns -o QrNew.app/Contents/Resources/AppIcon.icns "$iconset"
    rm -rf "$iconset"
    cat > QrNew.app/Contents/Info.plist << 'PLIST'
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
    <dict>
    <key>CFBundleName</key><string>QRnew</string>
    <key>CFBundleDisplayName</key><string>QRnew</string>
    <key>CFBundleIdentifier</key><string>dev.lhdjung.QrNew</string>
    <key>CFBundleVersion</key><string>0.1.0</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleExecutable</key><string>{{name}}</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    </dict>
    </plist>
    PLIST

# Installs to ~/.local for the current user (Linux); adds app to launcher with icon
bundle-linux: build-release
    #!/usr/bin/env bash
    set -euo pipefail
    install -Dm0755 {{cargo-target-dir}}/release/{{name}} ~/.local/bin/{{name}}
    install -Dm0644 resources/app.desktop \
        ~/.local/share/applications/dev.lhdjung.QrNew.desktop
    install -Dm0644 resources/icons/hicolor/scalable/apps/icon.svg \
        ~/.local/share/icons/hicolor/scalable/apps/dev.lhdjung.QrNew.svg
    update-desktop-database ~/.local/share/applications 2>/dev/null || true
    gtk-update-icon-cache -f ~/.local/share/icons/hicolor 2>/dev/null || true

# Creates a Windows package at QRnew-windows/ (run on Windows; requires magick)
bundle-windows: build-release
    #!/usr/bin/env bash
    set -euo pipefail
    rm -rf QRnew-windows
    mkdir QRnew-windows
    cp {{cargo-target-dir}}/release/{{name}}.exe QRnew-windows/QRnew.exe
    icon=resources/icons/hicolor/scalable/apps/icon.svg
    tmpdir=$(mktemp -d)
    for size in 16 32 48 64 128 256; do
        magick "$icon" -background none -resize "${size}x${size}" "$tmpdir/icon_${size}.png"
    done
    magick "$tmpdir/icon_16.png" "$tmpdir/icon_32.png" "$tmpdir/icon_48.png" \
        "$tmpdir/icon_64.png" "$tmpdir/icon_128.png" "$tmpdir/icon_256.png" \
        QRnew-windows/QRnew.ico
    rm -rf "$tmpdir"

# Vendor dependencies locally
vendor:
    mkdir -p .cargo
    cargo vendor | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    tar pcf vendor.tar vendor
    rm -rf vendor

# Extracts vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar

# Bump cargo version, create git commit, and create tag
tag version:
    find -type f -name Cargo.toml -exec sed -i '0,/^version/s/^version.*/version = "{{version}}"/' '{}' \; -exec git add '{}' \;
    cargo check
    cargo clean
    git add Cargo.lock
    git commit -m 'release: {{version}}'
    git commit --amend
    git tag -a {{version}} -m ''

