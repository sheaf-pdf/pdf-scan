# Development tasks. CI runs these same recipes, so a local `just ci` checks
# what CI checks.

# The oldest Rust sane-dl supports, from its Cargo.toml.
sane_dl_msrv := `grep -m1 '^rust-version' crates/sane-dl/Cargo.toml | cut -d'"' -f2`

default:
    @just --list --unsorted

# Everything CI runs. `msrv` needs each crate's oldest Rust installed
# (`rustup toolchain install <version>`); `gates` needs jq and readelf.
ci: fmt clippy test doc gates msrv

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
msrv: sane-dl-msrv

sane-dl-msrv:
    cargo +{{ sane_dl_msrv }} test --locked -p sane-dl

# TWAIN driven through the TWAIN Working Group's sample data source, in the
# bitness `target` builds for (x86_64-pc-windows-msvc or
# i686-pc-windows-msvc), three pages a batch. Windows only, with the source
# installed (`scripts/twain-sample.ps1 Install`), so not in `ci`: CI's
# twain-spike job runs it.
twain-spike target scenario="all":
    CAP_XFERCOUNT=3 cargo run --locked -p twain-spike --target {{ target }} -- {{ scenario }}

# The boundaries of the layers, checked over every target's dependency
# graph; that nothing links libsane; and what files may name.
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
    for crate in sane-dl; do
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
    read_count=0
    while read -r binary; do
        [[ -n $binary ]] || continue
        read_count=$((read_count + 1))
        readelf -d "$binary" | grep -q 'NEEDED.*libsane' && fail "$binary links libsane"
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
