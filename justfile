# Development tasks. CI runs these same recipes, so a local `just ci` checks
# what CI checks, but for two variables: CI's Ubuntu job sets
# SANE_DL_REQUIRE_LIBSANE=1 and SANE_DL_REQUIRE_HEADER=1, so a test that
# needs libsane or sane.h fails where it would skip. On Linux, set them for
# a local run that matches it.

# The oldest Rust each library supports, from its Cargo.toml.
sane_dl_msrv := `grep -m1 '^rust-version' crates/sane-dl/Cargo.toml | cut -d'"' -f2`
twain_dsm_msrv := `grep -m1 '^rust-version' crates/twain-dsm/Cargo.toml | cut -d'"' -f2`

default:
    @just --list --unsorted

# What they need: `msrv` each crate's oldest Rust installed (`rustup
# toolchain install <version>`), `gates` jq and readelf, `sane-dl-package`
# the network.
# Everything CI runs.
ci: fmt clippy test doc gates sane-dl-package msrv

# Check formatting; `cargo fmt --all` fixes it.
fmt:
    cargo fmt --all -- --check

# Lint every crate, with its tests.
clippy:
    cargo clippy --locked --workspace --all-targets -- -D warnings

# Unit tests, integration tests and doctests.
test:
    cargo test --locked --workspace

# Build the API docs, failing on warnings.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps

# Each library tested on its oldest supported Rust.
msrv: sane-dl-msrv twain-dsm-msrv

# sane-dl's tests on its oldest supported Rust.
sane-dl-msrv:
    cargo +{{ sane_dl_msrv }} test --locked -p sane-dl

# Off Windows, only its pure parts build and test.
twain-dsm-msrv:
    cargo +{{ twain_dsm_msrv }} test --locked -p twain-dsm

# Package sane-dl as a publish would, in a target directory of its own made
# fresh each time (a stale one passes or fails falsely): sane-dl-package
# under target/, or under CARGO_TARGET_DIR where that names another, so the
# recipe never removes more than its own directory. Then check what the
# package holds: its sources, build script, README, CHANGELOG, manifest and
# lock, each there and nothing else. The tests are outside `include`, so
# cargo says it leaves each of them out of the packaged manifest, which is
# expected. The dry run asks the registry, so it needs the network, and it
# never publishes.
# sane-dl packaged as a publish would, and what the package holds checked.
sane-dl-package:
    #!/usr/bin/env bash
    set -euo pipefail
    dir=${CARGO_TARGET_DIR:-$PWD/target}/sane-dl-package
    rm -rf "$dir"
    CARGO_TARGET_DIR="$dir" cargo publish --dry-run --locked -p sane-dl
    CARGO_TARGET_DIR="$dir" cargo package --list --locked -p sane-dl > "$dir/files.txt"
    if grep -v -E '^(src/.*\.rs|build\.rs|README\.md|CHANGELOG\.md|Cargo\.toml|Cargo\.toml\.orig|Cargo\.lock|\.cargo_vcs_info\.json)$' "$dir/files.txt"; then
        echo "the package holds more than its sources, build script, README, CHANGELOG, manifest and lock (above)" >&2
        exit 1
    fi
    for file in src/lib.rs build.rs README.md CHANGELOG.md Cargo.toml Cargo.lock; do
        grep -qxF -- "$file" "$dir/files.txt" || { echo "the package lacks $file" >&2; exit 1; }
    done
    echo "sane-dl-package: $(wc -l < "$dir/files.txt") files, each a source, the build script, the README, the CHANGELOG, the manifest or the lock ($dir/files.txt)"

# TWAIN driven through the TWAIN Working Group's sample data source, in the
# bitness `target` builds for (x86_64-pc-windows-msvc or
# i686-pc-windows-msvc), three pages a batch. Windows only, with the source
# installed (`scripts/twain-sample.ps1 Install`), so not in `ci`: CI's
# twain-spike job runs it. From Git Bash: just runs recipes with `sh`.
twain-spike target scenario="all":
    CAP_XFERCOUNT=3 cargo run --locked -p twain-spike --target {{ target }} -- {{ scenario }}

# twain-spike's unit tests for `target`: the 32-bit one too, whose `usize` a
# driver's sizes can overflow.
twain-spike-test target:
    cargo test --locked -p twain-spike --target {{ target }}

# twain-dsm's tests for `target` (x86_64-pc-windows-msvc or
# i686-pc-windows-msvc). Its header contract needs twain.h at
# TWAIN_DSM_TWAIN_H, and says it skipped without, unless
# TWAIN_DSM_REQUIRE_HEADER=1; CI's TWAIN job fetches the header and sets both.
twain-dsm-test target:
    cargo test --locked -p twain-dsm --target {{ target }}

# twain-dsm's constants generated again from TWAINH.cs and checked against
# twain.h, both fetched at their pinned commits; `just twain-bindings --check`
# changes nothing, and fails if the file differs from what it would write.
twain-bindings *args:
    python -I scripts/twain-bindings.py {{ args }}

# Over every target's dependency graph, the layers' boundaries, and that
# fake-libsane takes no dependency; that nothing links libsane; that
# fake-libsane is never published or depended on; and what files may name.
# The layers' boundaries, nothing linking libsane, fake-libsane, and names.
gates:
    #!/usr/bin/env bash
    set -euo pipefail
    failed=0
    fail() { echo "gate: $*" >&2; failed=1; }
    for tool in jq readelf; do
        command -v "$tool" >/dev/null || { echo "gate: $tool is missing, and the gates need it" >&2; exit 1; }
    done
    # A crate's dependencies, normal and build, on every target: one name a line.
    deps() { cargo tree --locked -p "$1" -e normal,build --target all --prefix none -f '{p}' | awk '{print $1}' | sort -u; }
    codec='^(image|tiff|png|zune-.*|jpeg-.*|fax|hayro-.*)$'
    pdf='^(pdf-.*|lopdf|pdfium-render|mupdf.*|hayro-syntax|hayro-interpret|krilla|pdf-writer|printpdf)$'
    ocr='^(ort|ort-sys|onnxruntime.*|tract-.*|candle-.*|rten|burn|tesseract.*|leptess|leptonica-sys)$'
    gui='^(gpui|egui|eframe|iced|gtk4?|winit|slint)$'
    # The devices layer: no pixel work beyond assembling what a driver sends,
    # no PDF, no OCR, no GUI.
    for crate in sane-dl twain-dsm; do
        # The list first, so a `cargo tree` that fails (a crate named here
        # that isn't a member) fails the gate, rather than leaving the loop
        # nothing to read; and the crate itself must be in its own list.
        if ! list=$(deps "$crate"); then
            fail "cargo tree couldn't list $crate's dependencies"
            continue
        fi
        grep -qxF -- "$crate" <<< "$list" || { fail "$crate isn't in its own dependency list, so it wasn't read"; continue; }
        while read -r d; do
            [[ $d =~ $codec ]] && fail "$crate depends on $d, an image codec"
            [[ $d == imageproc ]] && fail "$crate depends on imageproc, which is pixel work"
            [[ $d =~ $pdf ]] && fail "$crate depends on $d, a PDF crate"
            [[ $d =~ $ocr ]] && fail "$crate depends on $d, an OCR or model runtime"
            [[ $d =~ $gui ]] && fail "$crate depends on $d, a GUI toolkit"
        done <<< "$list"
        echo "gates: $crate's graph read for the devices layer ($(wc -l <<< "$list") packages)"
    done
    # fake-libsane takes no dependency at all: a shared object loaded into
    # the process of the test that loads it, it carries nothing into it
    # (image codecs least of all, which the page fixtures will take).
    if ! fake_list=$(deps fake-libsane); then
        fail "cargo tree couldn't list fake-libsane's dependencies"
    elif [[ $fake_list != fake-libsane ]]; then
        fail "fake-libsane takes dependencies: $(grep -vxF fake-libsane <<< "$fake_list" | tr '\n' ' ')"
    else
        echo "gates: fake-libsane's graph read: no dependency"
    fi
    # Nothing links libsane: it's loaded at run time. No package declares it
    # as the native library it links...
    metadata=$(cargo metadata --locked --format-version 1)
    linkers=$(jq -r '.packages[] | select(.links == "sane") | .name' <<< "$metadata")
    [[ -z $linkers ]] || fail "these declare links = \"sane\": $linkers"
    # ...and no binary built needs it: neither a test binary nor a shared
    # library (the fake's cdylib, which only `cargo build` builds), each read
    # for libsane among the NEEDED entries of its dynamic section.
    binaries=$({
        cargo test --locked --workspace --no-run --message-format=json
        cargo build --locked --workspace --message-format=json
    } | jq -r 'select(.reason == "compiler-artifact")
        | if .profile.test then .executable
          elif (.target.kind | index("cdylib")) then (.filenames[] | select(test("\\.so$")))
          else empty end // empty' | sort -u)
    # Each read whole before it's matched, so a binary readelf can't read
    # fails the gate rather than passing unread.
    read_count=0
    while read -r binary; do
        [[ -n $binary ]] || continue
        if ! dynamic=$(readelf -d "$binary"); then
            fail "readelf couldn't read $binary"
            continue
        fi
        read_count=$((read_count + 1))
        grep -q 'NEEDED.*libsane' <<< "$dynamic" && fail "$binary links libsane"
    done <<< "$binaries"
    [[ $read_count -gt 0 ]] || fail "no binary was read for libsane"
    echo "gates: $read_count binaries read for libsane"
    # fake-libsane: never published, and no crate takes it as a dependency
    # of any kind: tests build it and load it by its path.
    members=$(cargo metadata --locked --format-version 1 --no-deps)
    publish=$(jq -c '.packages[] | select(.name == "fake-libsane") | .publish' <<< "$members")
    [[ $publish == '[]' ]] || fail "fake-libsane could be published (publish: ${publish:-not a member})"
    users=$(jq -r '.packages[] | select(any(.dependencies[]; .name == "fake-libsane")) | .name' <<< "$members")
    [[ -z $users ]] || fail "these take fake-libsane as a dependency: $users"
    # No file git tracks or would track outside docs/pm uses the downstream
    # app's name (docs/pm/plan.md, "Standing rules"), but as the org in a URL.
    # git grep exits 1 for no match, and above 1 when it couldn't search.
    status=0
    named=$(git grep -IilP --untracked 'shea[f](?!-pdf/)' -- ':!docs/pm') || status=$?
    [[ $status -le 1 ]] || fail "git grep couldn't search the files for the downstream app's name (exit $status)"
    [[ -z $named ]] || fail "these use the downstream app's name: $named"
    if [[ $failed -ne 0 ]]; then exit 1; fi
    echo "gates: all hold"
