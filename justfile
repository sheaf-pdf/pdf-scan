# Development tasks. CI runs these same recipes, so a local `just ci` checks
# what CI checks, but for two variables: CI's Ubuntu job sets
# SANE_DL_REQUIRE_LIBSANE=1 and SANE_DL_REQUIRE_HEADER=1, so a test that
# needs libsane or sane.h fails where it would skip. On Linux, set them for
# a local run that matches it. CI's Windows TWAIN job also runs the
# twain-dsm recipes outside `ci` (`twain-dsm-test`, `twain-dsm-clippy`,
# `twain-bindings --check`, `twain-dsm-doc-codes`), which need Windows,
# MSVC, the sample source, twain.h or nightly, and sets
# TWAIN_DSM_REQUIRE_SAMPLE=1 and TWAIN_DSM_REQUIRE_HEADER=1.

# The oldest Rust each library supports, from its Cargo.toml.
sane_dl_msrv := `grep -m1 '^rust-version' crates/sane-dl/Cargo.toml | cut -d'"' -f2`
twain_dsm_msrv := `grep -m1 '^rust-version' crates/twain-dsm/Cargo.toml | cut -d'"' -f2`
scan_device_msrv := `grep -m1 '^rust-version' crates/scan-device/Cargo.toml | cut -d'"' -f2`
scan_worker_msrv := `grep -m1 '^rust-version' crates/scan-worker/Cargo.toml | cut -d'"' -f2`

# The nightly whose rustdoc writes the JSON `public-api` reads. The JSON's
# format moves between nightlies, so this moves only on purpose, with the
# format the recipe expects and scripts/public-api.jq read again. Keep in
# step with the `public-api` job's toolchain in .github/workflows/ci.yml.
public_api_nightly := "nightly-2026-10-16"

default:
    @just --list --unsorted

# What they need: `msrv` each crate's oldest Rust installed (`rustup
# toolchain install <version>`), `gates` jq and readelf, `sane-dl-package`
# and `twain-dsm-package` the network, `public-api` jq and the nightly
# pinned above, `public_api_nightly` (the recipe says how to install it).
# Everything CI runs.
ci: fmt clippy test doc gates sane-dl-package twain-dsm-package public-api msrv

# Check formatting; `cargo fmt --all` fixes it.
fmt:
    cargo fmt --all -- --check

# Every member, each built alone (-p) by the clippy and doc recipes below:
# cargo unifies features across the members one command selects, so a
# workspace run builds a library with what another member turns on of it
# (scan-worker turns on scan-device's serde), never as an app's build of
# that library alone has it. `just gates` checks the list is every member.
libraries := "sane-dl twain-dsm scan-device scan-worker fake-libsane"

# Every feature on, so a backend behind one (scan-device's fake) is linted
# as a user who turns it on meets it; then each library alone, with -p, its
# default features alone, as an app's build has it, which the first line
# never builds where a crate's tests turn a feature on (scan-device's fake,
# through its dev-dependency on itself), and a workspace run never builds
# where another member turns one on.
# Lint every crate, with its tests and every feature, and each library as shipped.
clippy:
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    for crate in {{ libraries }}; do cargo clippy --locked -p "$crate" --lib -- -D warnings || exit 1; done

# Unit tests, integration tests and doctests.
test:
    cargo test --locked --workspace

# Default features first, as docs.rs and an app's build have them, so a
# link to an item behind a feature fails here: each library alone, with -p,
# since a workspace run would build it with what another member turns on of
# it (`libraries`). Then every feature on, as a user who turns one on meets
# them, which is what's left in target/doc.
# Build the API docs, with default features and with every one, failing on warnings.
doc:
    for crate in {{ libraries }}; do RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps -p "$crate" || exit 1; done
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps --all-features

# Each library tested on its oldest supported Rust.
msrv: sane-dl-msrv twain-dsm-msrv scan-device-msrv scan-worker-msrv

# sane-dl's tests on its oldest supported Rust.
sane-dl-msrv:
    cargo +{{ sane_dl_msrv }} test --locked -p sane-dl

# Off Windows, only its pure parts build and test.
# twain-dsm's tests on its oldest supported Rust.
twain-dsm-msrv:
    cargo +{{ twain_dsm_msrv }} test --locked -p twain-dsm

# scan-device's tests on its oldest supported Rust.
scan-device-msrv:
    cargo +{{ scan_device_msrv }} test --locked -p scan-device

# Run with -p, the crate alone: its tests ask for scan-device's `fake`
# themselves, which a workspace run would turn on for them anyway.
# scan-worker's tests on its oldest supported Rust.
scan-worker-msrv:
    cargo +{{ scan_worker_msrv }} test --locked -p scan-worker

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
    CARGO_TARGET_DIR="$dir" cargo package --list --locked -p sane-dl | tr '\\' '/' | tr -d '\r' > "$dir/files.txt"
    if grep -v -E '^(src/.*\.rs|build\.rs|README\.md|CHANGELOG\.md|Cargo\.toml|Cargo\.toml\.orig|Cargo\.lock|\.cargo_vcs_info\.json)$' "$dir/files.txt"; then
        echo "the package holds more than its sources, build script, README, CHANGELOG, manifest and lock (above)" >&2
        exit 1
    fi
    for file in src/lib.rs build.rs README.md CHANGELOG.md Cargo.toml Cargo.lock; do
        grep -qxF -- "$file" "$dir/files.txt" || { echo "the package lacks $file" >&2; exit 1; }
    done
    echo "sane-dl-package: $(wc -l < "$dir/files.txt") files, each a source, the build script, the README, the CHANGELOG, the manifest or the lock ($dir/files.txt)"

# Package twain-dsm as a publish would, in a fresh target directory of its
# own, and check what the package holds: its sources, README, CHANGELOG,
# manifest and lock, each there and nothing else. The dry run asks the
# registry, so it needs the network, and it never publishes.
# twain-dsm packaged as a publish would, and what the package holds checked.
twain-dsm-package:
    #!/usr/bin/env bash
    set -euo pipefail
    dir=${CARGO_TARGET_DIR:-$PWD/target}/twain-dsm-package
    rm -rf "$dir"
    CARGO_TARGET_DIR="$dir" cargo publish --dry-run --locked -p twain-dsm
    CARGO_TARGET_DIR="$dir" cargo package --list --locked -p twain-dsm | tr '\\' '/' | tr -d '\r' > "$dir/files.txt"
    if grep -v -E '^(src/.*\.rs|README\.md|CHANGELOG\.md|Cargo\.toml|Cargo\.toml\.orig|Cargo\.lock|\.cargo_vcs_info\.json)$' "$dir/files.txt"; then
        echo "the package holds more than its sources, README, CHANGELOG, manifest and lock (above)" >&2
        exit 1
    fi
    for file in src/lib.rs README.md CHANGELOG.md Cargo.toml Cargo.lock; do
        grep -qxF -- "$file" "$dir/files.txt" || { echo "the package lacks $file" >&2; exit 1; }
    done
    echo "twain-dsm-package: $(wc -l < "$dir/files.txt") files, each a source, the README, the CHANGELOG, the manifest or the lock ($dir/files.txt)"

# Each crate made ready to publish, its public API read from rustdoc's JSON
# on the pinned nightly, hidden items included, so `#[doc(hidden)]` hides
# nothing (scripts/public-api.jq says what it reads and why). It fails,
# naming each, on an `unsafe` function, method or trait, an `unsafe`
# function pointer anywhere, a raw pointer or `NonNull` in a public
# signature (the crate's own type aliases, and the public fields of its own
# types, followed), a `#[target_feature]` function, a `static mut` or an
# extern static, and a re-export of another crate's item, which it can't
# read (a `use`, a `pub extern crate`, or an alias of that item); it lists
# each `unsafe impl`, the crate's own promise and never its caller's, and
# each union, whose fields only `unsafe` reads, without failing. First it
# checks the JSON is the shape the jq was written against, so a moved pin
# fails rather than passes unread. Built under public-api/ in the target
# directory (under CARGO_TARGET_DIR where that names another), apart from
# stable's builds, where it writes every public item, by path and kind, to
# <crate>.txt. The host's target only: an item behind another system's
# `cfg` isn't in its JSON. A crate joins the list as it's made ready to
# publish, with the least number of its own items a whole read finds.
# No unsafe in a published crate's public API, from rustdoc's JSON.
public-api:
    #!/usr/bin/env bash
    set -euo pipefail
    toolchain={{ public_api_nightly }}
    format=61
    command -v jq >/dev/null || { echo "public-api: jq is missing, and the gate needs it" >&2; exit 1; }
    toolchains=$(rustup toolchain list)
    if ! grep -q "^$toolchain-" <<< "$toolchains"; then
        echo "public-api: $toolchain isn't installed (rustup toolchain install $toolchain)" >&2
        exit 1
    fi
    dir=${CARGO_TARGET_DIR:-$PWD/target}/public-api
    host=$(rustc +"$toolchain" -vV | sed -n 's/^host: //p')
    # twain-dsm's Windows items are #[cfg(windows)], so its full API is read
    # on a Windows host; sane-dl's items are unconditional.
    case "$host" in
        *-windows-*) entries="sane-dl:1000 twain-dsm:3000" ;;
        *)           entries="sane-dl:1000" ;;
    esac
    failed=0
    for entry in $entries; do
        crate=${entry%%:*}
        floor=${entry##*:}
        json=$dir/doc/${crate//-/_}.json
        # Removed first, so what's read and listed is this run's.
        rm -f "$json" "$dir/$crate.txt"
        CARGO_TARGET_DIR="$dir" cargo +"$toolchain" rustdoc --locked -p "$crate" --lib \
            -- -Z unstable-options --output-format json --document-hidden-items
        if ! report=$(jq --argjson format "$format" --argjson floor "$floor" -f scripts/public-api.jq "$json"); then
            echo "public-api: $crate: jq couldn't read rustdoc's JSON ($json): has its shape changed?" >&2
            exit 1
        fi
        if jq -e 'has("shape")' <<< "$report" >/dev/null; then
            echo "public-api: $crate: rustdoc's JSON isn't the shape scripts/public-api.jq was written against (format $format), so nothing was read:" >&2
            jq -r '.shape[] | "  " + .' <<< "$report" >&2
            exit 1
        fi
        {
            echo "# $crate's public API: rustdoc's JSON (format $format) on $toolchain, for $host, hidden items included; blanket impls left out."
            jq -r '.list[]' <<< "$report"
        } > "$dir/$crate.txt"
        jq -r --arg crate "$crate" \
            '.unsafe_impls[] | "public-api: \($crate): \(.) (listed, not failed: the crate keeps that promise, not its caller)"' <<< "$report"
        jq -r --arg crate "$crate" \
            '.unions[] | "public-api: \($crate): union \(.): its fields are read only in unsafe (listed, not failed: plain data, as raw pointers in fields are)"' <<< "$report"
        if jq -e '.failures | length > 0' <<< "$report" >/dev/null; then
            echo "public-api: $crate's public API holds unsafe:" >&2
            jq -r '.failures[] | "  " + .' <<< "$report" >&2
            failed=1
        fi
        jq -r --arg crate "$crate" --arg file "$dir/$crate.txt" \
            '.counts as $c | "public-api: \($crate): \($c.own) items of its own read, \($c.public) public by path, \($c.functions) functions, \($c.unsafe_functions) unsafe; failures \(.failures | length); unsafe impls \(.unsafe_impls | length) written, \($c.derived_unsafe_impls) derived; unions \(.unions | length); blanket impls \($c.blanket_impls), left out of the list; every public item listed in \($file)"' <<< "$report"
    done
    if [[ $failed -ne 0 ]]; then exit 1; fi
    echo "public-api: no unsafe in the public API"

# twain-dsm's tests for `target` (x86_64-pc-windows-msvc or
# i686-pc-windows-msvc), the 32-bit one too, whose `usize` a driver's sizes
# can overflow. Windows only. Its header contract needs twain.h at
# TWAIN_DSM_TWAIN_H, and the tests that scan need the TWAIN Working Group's
# sample source (`scripts/twain-sample.ps1 Install`); without them they say
# they skipped, unless TWAIN_DSM_REQUIRE_HEADER=1 and
# TWAIN_DSM_REQUIRE_SAMPLE=1. CI's TWAIN job installs both and sets both.
# From Git Bash: just runs recipes with `sh`.
# twain-dsm's tests for one Windows target, with the sample source and twain.h.
twain-dsm-test target:
    cargo test --locked -p twain-dsm --target {{ target }}

# twain-dsm's lints for `target`, its tests included: the 32-bit one too,
# whose code the 64-bit build doesn't compile.
# twain-dsm's lints for one Windows target, its tests included.
twain-dsm-clippy target:
    cargo clippy --locked -p twain-dsm --all-targets --target {{ target }} -- -D warnings

# twain-dsm's doctests on nightly, whose rustdoc checks that each
# `compile_fail` example fails with the error code it names (stable's checks
# only that it fails). Needs `rustup toolchain install nightly`.
# twain-dsm's compile_fail doctests on nightly, their error codes checked.
twain-dsm-doc-codes:
    cargo +nightly test --locked -p twain-dsm --doc

# twain-dsm's constants generated again from TWAINH.cs and checked against
# twain.h, both fetched at their pinned commits; `just twain-bindings --check`
# changes nothing, and fails if the file differs from what it would write.
# twain-dsm's constants generated from TWAINH.cs and checked against twain.h.
twain-bindings *args:
    python -I scripts/twain-bindings.py {{ args }}

# Over every target's dependency graph, every feature on, the layers'
# boundaries, and that fake-libsane takes no dependency; that nothing built
# with every feature on links libsane; that fake-libsane is never published
# or depended on; and what files may name.
# The layers' boundaries, nothing linking libsane, fake-libsane, and names.
gates:
    #!/usr/bin/env bash
    set -euo pipefail
    failed=0
    fail() { echo "gate: $*" >&2; failed=1; }
    for tool in jq readelf; do
        command -v "$tool" >/dev/null || { echo "gate: $tool is missing, and the gates need it" >&2; exit 1; }
    done
    # A crate's dependencies, normal and build, on every target and with
    # every feature on, so what a feature brings is read too: one name a line.
    deps() { cargo tree --locked -p "$1" -e normal,build --target all --all-features --prefix none -f '{p}' | awk '{print $1}' | sort -u; }
    codec='^(image|tiff|png|zune-.*|jpeg-.*|fax|hayro-.*)$'
    pdf='^(pdf-.*|lopdf|pdfium-render|mupdf.*|hayro-syntax|hayro-interpret|krilla|pdf-writer|printpdf)$'
    ocr='^(ort|ort-sys|onnxruntime.*|tract-.*|candle-.*|rten|burn|tesseract.*|leptess|leptonica-sys)$'
    gui='^(gpui|egui|eframe|iced|gtk4?|winit|slint)$'
    # The devices layer: no pixel work beyond assembling what a driver sends,
    # no PDF, no OCR, no GUI.
    for crate in sane-dl twain-dsm scan-device scan-worker; do
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
    # ...and no binary built needs it: neither a test binary, nor a program
    # (a `bin` target, such as a worker a crate's tests start), nor a shared
    # library (the fake's cdylib, which only `cargo build` builds), each read
    # for libsane among the NEEDED entries of its dynamic section. Built with
    # every feature on, as the graphs are read, so a backend's feature that
    # links it is caught, and a program behind a feature is built.
    binaries=$({
        cargo test --locked --workspace --all-features --no-run --message-format=json
        cargo build --locked --workspace --all-features --message-format=json
    } | jq -r 'select(.reason == "compiler-artifact")
        | if .profile.test then .executable
          elif (.target.kind | index("bin")) then .executable
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
    # of any kind: tests build it and load it as SANE, through the system
    # loader's search path.
    members=$(cargo metadata --locked --format-version 1 --no-deps)
    publish=$(jq -c '.packages[] | select(.name == "fake-libsane") | .publish' <<< "$members")
    [[ $publish == '[]' ]] || fail "fake-libsane could be published (publish: ${publish:-not a member})"
    users=$(jq -r '.packages[] | select(any(.dependencies[]; .name == "fake-libsane")) | .name' <<< "$members")
    [[ -z $users ]] || fail "these take fake-libsane as a dependency: $users"
    # The clippy and doc recipes build each member alone, from `libraries`:
    # a member left out of it would go unbuilt there as an app has it.
    listed=$(tr ' ' '\n' <<< "{{ libraries }}" | sort)
    workspace=$(jq -r '.packages[].name' <<< "$members" | sort)
    [[ $listed == "$workspace" ]] || fail "the justfile's libraries ($(tr '\n' ' ' <<< "$listed")) aren't the workspace's members ($(tr '\n' ' ' <<< "$workspace"))"
    # No file git tracks or would track outside docs/pm uses the downstream
    # app's name (docs/pm/plan.md, "Standing rules"), but as the org in a URL.
    # git grep exits 1 for no match, and above 1 when it couldn't search.
    status=0
    named=$(git grep -IilP --untracked 'shea[f](?!-pdf/)' -- ':!docs/pm') || status=$?
    [[ $status -le 1 ]] || fail "git grep couldn't search the files for the downstream app's name (exit $status)"
    [[ -z $named ]] || fail "these use the downstream app's name: $named"
    if [[ $failed -ne 0 ]]; then exit 1; fi
    echo "gates: all hold"
