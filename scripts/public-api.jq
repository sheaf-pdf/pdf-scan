# A crate's public API, read from rustdoc's JSON, for `just public-api`.
#
# Input: the JSON `cargo rustdoc --lib -- -Z unstable-options --output-format
# json --document-hidden-items` writes for one crate, so `#[doc(hidden)]`
# items are in it, and every re-export is a `use` item rather than inlined.
# Arguments: `$format`, the `format_version` this was written against, and
# `$floor`, the least number of the crate's own items a whole read finds.
#
# Output: one object. `shape` lists what isn't as this program expects; when
# it's not empty, nothing else is read, since a field renamed or moved would
# otherwise read as an API with nothing unsafe in it. Otherwise `failures`
# names each item that breaks the rule (no `unsafe` in what a caller uses),
# `unsafe_impls` each `unsafe impl` of the crate's (its own promise, never
# its caller's: listed, not failed), `list` every public item by path and
# kind, and `counts`.
#
# What fails, over every item of the crate's own (`crate_id` 0):
# - a function or method whose header is `unsafe`;
# - a trait declared `unsafe`;
# - an item holding an `unsafe` function pointer anywhere in it (a type
#   alias, a field, a constant, a signature);
# - a function or method with a raw pointer in its signature, the crate's own
#   type aliases followed (rustdoc keeps a public alias as a path);
# - a public re-export of another crate's item, which this can't read.
# Raw pointers in public fields are allowed: plain data, read through only
# by the reader's own `unsafe`.

def kind: .inner | keys[0];

# The word Rust spells a kind with.
def word:
  {function: "fn", type_alias: "type", constant: "const", module: "mod",
   assoc_const: "const", assoc_type: "type", struct_field: "field"}[.] // .;

. as $doc
| $doc.index as $index
| def item($id): $index[$id | tostring];
  def own: .crate_id == 0;
  def where: if .span then " (\(.span.filename):\(.span.begin[0]))" else "" end;

  # A type, written roughly as Rust spells it, for the list's impls.
  def render:
    def args:
      if . == null then ""
      elif .angle_bracketed then
        [ (.angle_bracketed.args[]
           | if type == "object" and .type then (.type | render)
             elif type == "object" and .lifetime then .lifetime
             elif type == "object" and .const then (.const.expr // "_")
             else "_" end),
          (.angle_bracketed.constraints[]? | "\(.name) = …") ]
        | if length == 0 then "" else "<" + join(", ") + ">" end
      elif .parenthesized then
        "(" + (.parenthesized.inputs | map(render) | join(", ")) + ")"
        + (if .parenthesized.output then " -> " + (.parenthesized.output | render) else "" end)
      else "" end;
    if type != "object" then tostring
    elif .resolved_path then .resolved_path.path + (.resolved_path.args | args)
    elif .primitive then .primitive
    elif .generic then .generic
    elif .borrowed_ref then
      .borrowed_ref | "&" + (if .lifetime then .lifetime + " " else "" end)
        + (if .is_mutable then "mut " else "" end) + (.type | render)
    elif .raw_pointer then
      .raw_pointer | (if .is_mutable then "*mut " else "*const " end) + (.type | render)
    elif .slice then "[" + (.slice | render) + "]"
    elif .array then "[" + (.array.type | render) + "; " + (.array.len | tostring) + "]"
    elif .tuple then "(" + (.tuple | map(render) | join(", ")) + ")"
    elif .function_pointer then
      .function_pointer | (if .header.is_unsafe then "unsafe " else "" end)
        + "fn(" + (.sig.inputs | map(.[1] | render) | join(", ")) + ")"
        + (if .sig.output then " -> " + (.sig.output | render) else "" end)
    elif .qualified_path then
      .qualified_path | "<" + (.self_type | render)
        + (if .trait then " as " + .trait.path else "" end) + ">::" + .name
    elif .impl_trait then "impl …"
    elif .dyn_trait then "dyn " + (.dyn_trait.traits | map(.trait.path) | join(" + "))
    else "_" end;

  # The type an alias of the crate's own stands for.
  def own_alias($id):
    item($id) | select(. != null and own and .inner.type_alias != null) | .inner.type_alias.type;

  # Whether a type (or a signature) holds a raw pointer, through the crate's
  # own aliases too. Rust has no cycle of aliases; past 64 the chain is
  # taken as holding one, which fails rather than passes unread.
  def holds_raw($depth):
    if $depth > 64 then true
    else any(.. | objects; has("raw_pointer"))
      or any(.. | objects | select(has("resolved_path")) | .resolved_path.id // empty
             | own_alias(.); holds_raw($depth + 1))
    end;

  def holds_unsafe_fn:
    any(.. | objects | select(has("function_pointer")); .function_pointer.header.is_unsafe == true);

  # rustdoc's JSON (format 61) gives `is_unsafe: false` for every impl,
  # a written `unsafe impl` too, so an impl is taken as unsafe by its trait:
  # one of the crate's own declared `unsafe`, or one of std's stable unsafe
  # traits, by its path. The flag is read as well, should it come to be set.
  def std_unsafe: ["core::marker::Send", "core::marker::Sync", "core::alloc::global::GlobalAlloc"];
  def trait_path($id): $doc.paths[$id | tostring] // null | if . then .path | join("::") else null end;
  def trait_crate($id):
    ($doc.paths[$id | tostring].crate_id // null) as $c
    | if $c == 0 then "own" elif $c == null then "unknown"
      else ($doc.external_crates[$c | tostring].name // "unknown") end;
  def impl_is_unsafe:
    .inner.impl as $i
    | ($i.is_unsafe == true)
      or ($i.trait != null and ($i.is_negative | not)
          and ((item($i.trait.id) | . != null and own and .inner.trait.is_unsafe == true)
               or (trait_path($i.trait.id) as $p | std_unsafe | index([$p]) != null)));

  def shape:
    [ if .format_version != $format then
        "format_version is \(.format_version), and this was written against \($format)"
      else empty end,
      if (.index | type) != "object" or (.paths | type) != "object" then
        "index or paths isn't an object"
      else
        ([.index[] | select(own)]) as $own
        | if ($own | length) <= $floor then
            "the crate's own items number \($own | length), not more than the floor, \($floor)"
          else empty end,
          ($own[] | select((.inner | type) != "object" or (.inner | length) != 1)
           | "item \(.id) hasn't one kind"),
          ($own[] | select(.inner.function)
           | select((.inner.function.header.is_unsafe | type) != "boolean")
           | "function \(.name) (\(.id)) has no boolean header.is_unsafe"),
          ($own[] | select(.inner.trait) | select((.inner.trait.is_unsafe | type) != "boolean")
           | "trait \(.name) (\(.id)) has no boolean is_unsafe"),
          ($own[] | select(.inner.impl)
           | select((.inner.impl.is_synthetic | type) != "boolean" or (.inner.impl | has("blanket_impl") | not))
           | "impl \(.id) has no boolean is_synthetic or no blanket_impl"),
          ($own[] | select(.inner.use)
           | select((.inner.use | has("id") and has("is_glob") and has("name")) | not)
           | "use \(.id) lacks id, is_glob or name"),
          ($own[] | .inner | .. | objects | select(has("function_pointer"))
           | select((.function_pointer.header.is_unsafe | type) != "boolean")
           | "a function pointer has no boolean header.is_unsafe"),
          (item($doc.root) | if . == null or (.inner.module.is_crate != true) then
             "the root isn't the crate's module" else empty end)
      end ]
    | unique;

  # Every public item from the root down, its path as a user writes it:
  # re-exports of the crate's own items followed, globs too. A re-export of
  # another crate's item, and what can't be read, come back marked.
  def walk($prefix; $module; $depth):
    if $depth > 16 then {path: $prefix, problem: "a re-export cycle, or modules nested past 16"}
    else
      item($module).inner.module.items[]? as $child
      | item($child) as $c
      | if $c == null then {path: "\($prefix)::?", problem: "item \($child) isn't in the index"}
        elif $c.inner.use then
          $c.inner.use as $u
          | (if $u.id == null then null else item($u.id) end) as $t
          | if $t == null or ($t | own | not) then
              {path: (if $u.is_glob then "\($prefix)::*" else "\($prefix)::\($u.name)" end),
               foreign: $u.source}
            elif $u.is_glob then
              if $t.inner.module then walk($prefix; $t.id; $depth + 1)
              elif $t.inner.enum then
                ($t.inner.enum.variants[] | item(.) | {id, path: "\($prefix)::\(.name)"})
              else {path: "\($prefix)::*", problem: "a glob re-export of \($u.source), not a module or enum"}
              end
            else
              {id: $t.id, path: "\($prefix)::\($u.name)"},
              (if $t.inner.module then walk("\($prefix)::\($u.name)"; $t.id; $depth + 1) else empty end)
            end
        elif ($c | own | not) then {path: "\($prefix)::\($c.name)", foreign: $c.name}
        else
          {id: $c.id, path: "\($prefix)::\($c.name)"},
          (if $c.inner.module then walk("\($prefix)::\($c.name)"; $c.id; $depth + 1) else empty end)
        end
    end;

  (shape) as $shape
  | if $shape != [] then {shape: $shape}
    else
      item($doc.root).name as $crate
      | [{id: $doc.root, path: $crate}, walk($crate; $doc.root; 0)] as $walked
      | [$walked[] | select(.id != null)] as $public
      # Each public item's lines: the item, then what's under it.
      | [ $public[] | . as $p | item($p.id) as $it | ($it | kind) as $k
          | { id: $p.id, path: $p.path, item: $it,
              impls: ($it.inner[$k].impls // [] | map(item(.)) | map(select(. != null))) } ] as $types
      # Each item's path, for naming it: methods, trait items and fields
      # under their owner's path.
      | def field_ids:
          (.inner.struct.kind // .inner.variant.kind // null) as $shape_of
          | (if .inner.union then .inner.union.fields
             elif ($shape_of | type) != "object" then []
             elif $shape_of.plain then $shape_of.plain.fields
             elif $shape_of.struct then $shape_of.struct.fields
             elif $shape_of.tuple then $shape_of.tuple
             else [] end)[]
          | select(. != null);
        ([$types[] | .path as $path
          | (.impls[] | select(.inner.impl.blanket_impl == null) | .inner.impl.items[]
             | {key: tostring, value: "\($path)::\(item(.).name)"}),
            (.item | field_ids | {key: tostring, value: "\($path)::\(item(.).name)"}),
            (.item.inner.enum.variants[]? | item(.) | .name as $variant
             | field_ids | {key: tostring, value: "\($path)::\($variant)::\(item(.).name)"}),
            (.item.inner.trait.items[]? | {key: tostring, value: "\($path)::\(item(.).name)"})]
         + [$public[] | {key: (.id | tostring), value: .path}]
         | from_entries) as $names
      | def name: $names[.id | tostring] // (.name // "an item");
        def unsafe_word:
          if .inner.function.header.is_unsafe == true or .inner.trait.is_unsafe == true
          then "unsafe " else "" end;
        def field_lines($indent): field_ids | item(.) | "\($indent)field \(.name)";
        def impl_line:
          .inner.impl as $i | ($i.trait.id) as $t
          | "  \(if impl_is_unsafe then "unsafe " else "" end)impl "
            + (if $i.is_negative then "!" else "" end)
            + $i.trait.path + ($i.trait | {resolved_path: .} | render | ltrimstr($i.trait.path))
            + (if trait_crate($t) | IN("own", "core", "alloc", "std") then ""
               else " (another crate's trait: its safety unread)" end);
        # The stable auto traits, on one line; the compiler's others left out.
        def auto_line:
          [.impls[] | .inner.impl | select(.is_synthetic)
           | select(.trait.path | IN("Send", "Sync", "Unpin", "UnwindSafe", "RefUnwindSafe"))
           | (if .is_negative then "!" else "" end) + .trait.path]
          | if length > 0 then "  auto " + join(", ") else empty end;
        def lines:
          .item as $it | ($it | kind) as $k
          | "\($it | unsafe_word)\($k | word) \(.path)",
            ($it | field_lines("  ")),
            (if $k == "enum" then
               $it.inner.enum.variants[] | item(.) | "  variant \(.name)", field_lines("    ")
             else empty end),
            (if $k == "trait" then
               $it.inner.trait.items[] | item(.) | "  \(unsafe_word)\(kind | word) \(.name)"
             else empty end),
            (.impls[] | select(.inner.impl.trait == null) | .inner.impl.items[] | item(.)
             | "  \(unsafe_word)\(kind | word) \(.name)"),
            auto_line,
            (.impls[] | select(.inner.impl.trait != null and .inner.impl.blanket_impl == null
                               and (.inner.impl.is_synthetic | not)) | impl_line);
        ([$types[].impls[].id] | map(tostring) | unique) as $under
      | [$index[] | select(own and .inner.impl and .inner.impl.blanket_impl == null
                           and (.inner.impl.is_synthetic | not))
          | select((.id | tostring) as $i | $under | index([$i]) == null)] as $others
      | ([$index[] | select(own)]) as $own
      | {
          failures: [
            ($own[] | select(.inner.function.header.is_unsafe == true)
             | "an unsafe fn: \(name)\(where)"),
            ($own[] | select(.inner.trait.is_unsafe == true)
             | "an unsafe trait: \(name)\(where)"),
            ($own[] | select(.inner | holds_unsafe_fn)
             | "an unsafe fn pointer in \(kind | word) \(name)\(where)"),
            ($own[] | select(.inner.function)
             | select(.inner.function | {sig, generics} | holds_raw(0))
             | "a raw pointer in the signature of \(name)\(where)"),
            ($walked[] | select(.foreign) | "a re-export of another crate's item: \(.path) (\(.foreign))"),
            ($walked[] | select(.problem) | "unread: \(.path): \(.problem)")
          ],
          unsafe_impls: [
            $own[] | select(.inner.impl and (.inner.impl.is_synthetic | not)
                            and .inner.impl.blanket_impl == null)
            | select(impl_is_unsafe)
            | "unsafe impl \(.inner.impl.trait.path) for \(.inner.impl.for | render)\(where)"
          ],
          list: (
            [$types[] | lines]
            + [$others[] | "impl \(.inner.impl.trait.path // "") for \(.inner.impl.for | render)"
                           + (if impl_is_unsafe then " (unsafe impl)" else "" end)]
          ),
          counts: {
            own: ($own | length),
            public: ($public | length),
            functions: ([$own[] | select(.inner.function)] | length),
            unsafe_functions: ([$own[] | select(.inner.function.header.is_unsafe == true)] | length),
            blanket_impls: ([$own[] | select(.inner.impl.blanket_impl != null)] | length)
          }
        }
    end
