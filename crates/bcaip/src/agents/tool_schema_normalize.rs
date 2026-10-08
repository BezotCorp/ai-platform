use rmcp::model::JsonObject;
use serde_json::{Map, Value};
use std::collections::HashSet;
/// Normalize an rmcp tool `input_schema` in place, returning `true` if changed.
pub fn normalize_input_schema(schema: &mut JsonObject) -> bool {
    let mut value = Value::Object(std::mem::take(schema));
    let changed = collapse_const_unions(&mut value);
    if let Value::Object(obj) = value {
        *schema = obj;
    }
    changed
}

/// Collapse `oneOf`/`anyOf` unions whose members are all string `const`s into
/// a single `{type: "string", enum: [...]}`, folding per-variant descriptions
/// into the enclosing description, then inline and prune trivial `$defs`.
///
/// schemars emits documented unit enums as `$ref -> $defs -> oneOf` of consts:
/// ~9x larger than an equivalent `enum` and rejected outright by strict
/// validators (notably Moonshot's). Anything not provably equivalent is left
/// untouched: genuine unions, refs with conflicting siblings, identity-bearing
/// defs, sibling-carrying refs under draft-06/07 (where `$ref` siblings are
/// ignored), and anything declaring a dialect that predates `const` (draft-04
/// and earlier, where a const member just means `type: "string"`).
pub fn collapse_const_unions(schema: &mut Value) -> bool {
    if dialect_predates_const(schema) {
        return false;
    }
    let mut changed = collapse_node(schema);
    if inline_trivial_defs(schema) {
        changed = true;
    }
    changed
}

fn dialect_predates_const(schema: &Value) -> bool {
    schema
        .get("$schema")
        .and_then(Value::as_str)
        .is_some_and(|dialect| {
            ["draft-00", "draft-01", "draft-02", "draft-03", "draft-04"]
                .iter()
                .any(|old| dialect.contains(old))
        })
}

fn ref_siblings_ignored(schema: &Value) -> bool {
    schema
        .get("$schema")
        .and_then(Value::as_str)
        .is_some_and(|dialect| dialect.contains("draft-06") || dialect.contains("draft-07"))
}

fn collapse_node(node: &mut Value) -> bool {
    // The root or an embedded resource may declare a pre-`const` dialect.
    if dialect_predates_const(node) {
        return false;
    }
    let mut changed = false;
    if let Value::Object(obj) = node {
        for key in ["oneOf", "anyOf"] {
            // Inserting `type`/`enum` must not clobber sibling constraints;
            // this also limits collapsing to one union per node (the first
            // inserts `enum`, so the second stays as a sibling).
            let compatible = !obj.contains_key("enum")
                && obj.get("type").is_none_or(|t| t.as_str() == Some("string"));
            if !compatible {
                continue;
            }
            if let Some(collapsed) = try_collapse_union(obj, key) {
                obj.remove(key);
                let existing = obj
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                obj.insert("type".to_string(), Value::String("string".to_string()));
                obj.insert("enum".to_string(), Value::Array(collapsed.values));
                if let Some(merged) = merge_descriptions(existing, collapsed.descriptions) {
                    obj.insert("description".to_string(), Value::String(merged));
                }
                changed = true;
            }
        }
        for_each_subschema_mut(obj, &mut |child| {
            if collapse_node(child) {
                changed = true;
            }
        });
    }
    changed
}

/// Visit each child of `obj` in a subschema position. Mutating walks must not
/// descend into instance data (`default`/`examples`/`const`/`enum` values),
/// which may look like schemas but must pass through verbatim.
fn for_each_subschema_mut(obj: &mut Map<String, Value>, f: &mut dyn FnMut(&mut Value)) {
    for (key, value) in obj.iter_mut() {
        match key.as_str() {
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas" => {
                if let Value::Object(map) = value {
                    for child in map.values_mut() {
                        f(child);
                    }
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                if let Value::Array(members) = value {
                    for child in members {
                        f(child);
                    }
                }
            }
            "items"
            | "additionalItems"
            | "additionalProperties"
            | "unevaluatedItems"
            | "unevaluatedProperties"
            | "contains"
            | "propertyNames"
            | "not"
            | "if"
            | "then"
            | "else" => match value {
                // draft-07 `items` takes an array of schemas
                Value::Array(members) => {
                    for child in members {
                        f(child);
                    }
                }
                child => f(child),
            },
            _ => {}
        }
    }
}

struct CollapsedUnion {
    values: Vec<Value>,
    descriptions: Vec<String>,
}

/// If `obj[key]` is an array where every member is a bare `{type:"string",
/// const:X, description?}`, return the collected consts and descriptions.
fn try_collapse_union(obj: &Map<String, Value>, key: &str) -> Option<CollapsedUnion> {
    let members = obj.get(key)?.as_array()?;
    if members.is_empty() {
        return None;
    }

    let mut values = Vec::with_capacity(members.len());
    let mut descriptions = Vec::new();
    let mut seen = HashSet::new();
    for member in members {
        let member = member.as_object()?;
        let allowed = member
            .keys()
            .all(|k| matches!(k.as_str(), "type" | "const" | "description"));
        if !allowed {
            return None;
        }
        if member.get("type").and_then(Value::as_str) != Some("string") {
            return None;
        }
        let konst = member.get("const")?;
        let konst_str = konst.as_str()?;
        if !seen.insert(konst_str.to_string()) {
            // A duplicated const matches two oneOf branches, which oneOf
            // rejects and an enum cannot express; an anyOf duplicate is
            // just dropped.
            if key == "oneOf" {
                return None;
            }
            continue;
        }
        values.push(konst.clone());
        if let Some(d) = member.get("description").and_then(Value::as_str) {
            descriptions.push(format!("{konst_str}: {d}"));
        }
    }

    Some(CollapsedUnion {
        values,
        descriptions,
    })
}

fn merge_descriptions(existing: Option<String>, variant_descs: Vec<String>) -> Option<String> {
    let variants = if variant_descs.is_empty() {
        None
    } else {
        Some(format!("One of: {}", variant_descs.join("; ")))
    };
    match (existing, variants) {
        (Some(base), Some(v)) => Some(format!("{base}. {v}")),
        (Some(base), None) => Some(base),
        (None, Some(v)) => Some(v),
        (None, None) => None,
    }
}

/// Inline `$defs` entries that are leaf string enums at their `$ref` sites,
/// then drop defs nothing references anymore.
fn inline_trivial_defs(schema: &mut Value) -> bool {
    let Some(defs) = schema.get("$defs").and_then(Value::as_object).cloned() else {
        return false;
    };

    let inlinable: Map<String, Value> = defs
        .iter()
        .filter(|(_, def)| is_leaf_string_enum(def))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if inlinable.is_empty() {
        return false;
    }

    let mut changed = false;
    inline_refs(
        schema,
        &inlinable,
        ref_siblings_ignored(schema),
        &mut changed,
    );

    let still_used = collect_used_defs(schema);
    if let Some(Value::Object(defs_obj)) = schema.get_mut("$defs") {
        let len_before = defs_obj.len();
        defs_obj.retain(|name, _| still_used.contains(name));
        if defs_obj.len() != len_before {
            changed = true;
        }
        if defs_obj.is_empty() {
            schema.as_object_mut().unwrap().remove("$defs");
        }
    }
    changed
}

fn is_leaf_string_enum(def: &Value) -> bool {
    let Some(obj) = def.as_object() else {
        return false;
    };
    obj.get("type").and_then(Value::as_str) == Some("string")
        && obj.get("enum").is_some_and(Value::is_array)
        && !obj.contains_key("$ref")
        && !obj.contains_key("$defs")
        // Copying an identity keyword to the ref site would change how
        // anchor-based references resolve, and copying a redeclared
        // `$schema` would pull the ref's siblings into another dialect.
        && !contains_identity_keyword(def)
        && !contains_key_deep(def, "$schema")
}

fn contains_identity_keyword(node: &Value) -> bool {
    ["$id", "$anchor", "$dynamicAnchor", "$recursiveAnchor"]
        .iter()
        .any(|key| contains_key_deep(node, key))
}

fn contains_key_deep(node: &Value, key: &str) -> bool {
    match node {
        Value::Object(obj) => {
            obj.contains_key(key) || obj.values().any(|v| contains_key_deep(v, key))
        }
        Value::Array(arr) => arr.iter().any(|v| contains_key_deep(v, key)),
        _ => false,
    }
}

fn inline_refs(
    node: &mut Value,
    inlinable: &Map<String, Value>,
    legacy_refs: bool,
    changed: &mut bool,
) {
    inline_refs_scoped(node, inlinable, legacy_refs, changed, true, false);
}

fn inline_refs_scoped(
    node: &mut Value,
    inlinable: &Map<String, Value>,
    legacy_refs: bool,
    changed: &mut bool,
    is_root: bool,
    in_nested_resource: bool,
) {
    let Value::Object(obj) = node else {
        return;
    };
    // A non-root `$id` (or a redeclared `$schema`) starts a new schema
    // resource: `#/$defs/...` below it no longer resolves against the
    // root's defs.
    let in_nested_resource = in_nested_resource
        || (!is_root && (obj.contains_key("$id") || obj.contains_key("$schema")));
    // `$ref` siblings still apply under draft 2020-12 (schemars emits
    // `{$ref, default, description}`), so they are merged with the target;
    // a ref whose siblings conflict with it stays in place, and
    // collect_used_defs keeps its target alive. Draft-06/07 instead ignore
    // `$ref` siblings entirely, so merging would activate dead keywords
    // there; only bare refs are inlined under those dialects.
    let mergeable = !legacy_refs || obj.keys().all(|k| k == "$ref");
    let target = if in_nested_resource || !mergeable {
        None
    } else {
        obj.get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix("#/$defs/"))
            // Multi-token pointers target something inside a def, and escapes
            // could make the raw suffix collide with an unrelated def name;
            // only plain single-token names are inlined.
            .filter(|name| !name.contains(['/', '~', '%']))
            .and_then(|name| inlinable.get(name))
            .cloned()
    };
    if let Some(Value::Object(target)) = target {
        let conflict = obj.iter().any(|(k, v)| {
            k != "$ref" && k != "description" && target.get(k).is_some_and(|tv| tv != v)
        });
        if !conflict {
            obj.remove("$ref");
            for (k, v) in &target {
                if k != "description" && !obj.contains_key(k) {
                    obj.insert(k.clone(), v.clone());
                }
            }
            if let Some(inner) = target.get("description").and_then(Value::as_str) {
                let merged = match obj.get("description").and_then(Value::as_str) {
                    Some(outer) => format!("{outer}. {inner}"),
                    None => inner.to_string(),
                };
                obj.insert("description".to_string(), Value::String(merged));
            }
            *changed = true;
            return;
        }
    }
    for_each_subschema_mut(obj, &mut |child| {
        inline_refs_scoped(
            child,
            inlinable,
            legacy_refs,
            changed,
            false,
            in_nested_resource,
        );
    });
}

fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(hi << 4 | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Defs reachable from outside `$defs`, followed transitively through kept
/// defs - a def referenced only by another live def must survive the prune.
fn collect_used_defs(schema: &Value) -> HashSet<String> {
    fn insert_ref(key: &str, value: &Value, used: &mut HashSet<String>) {
        // `$dynamicRef`/`$recursiveRef` with a pointer fragment resolve like
        // `$ref` when the target carries no matching anchor.
        if !matches!(key, "$ref" | "$dynamicRef" | "$recursiveRef") {
            return;
        }
        let Some(raw) = value.as_str() else {
            return;
        };
        // The first pointer token names the def that must stay alive. Split
        // at the literal `#` before percent-decoding (a decoded `%23` in the
        // base URI is not a fragment delimiter), accept any base URI before
        // the fragment (absolute self-references), check the verbatim and
        // decoded fragment forms, and record tokens both raw and
        // JSON-Pointer-unescaped: extra entries only ever retain more defs,
        // never prune live ones.
        let fragment = match raw.split_once('#') {
            Some((_, fragment)) => format!("#{fragment}"),
            None => raw.to_string(),
        };
        for form in [fragment.clone(), percent_decode(&fragment)] {
            if let Some(first) = form
                .strip_prefix("#/$defs/")
                .and_then(|pointer| pointer.split('/').next())
            {
                used.insert(first.to_string());
                used.insert(first.replace("~1", "/").replace("~0", "~"));
            }
        }
    }
    fn add_refs(node: &Value, used: &mut HashSet<String>) {
        match node {
            Value::Object(obj) => {
                for (k, v) in obj {
                    insert_ref(k, v, used);
                    add_refs(v, used);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    add_refs(v, used);
                }
            }
            _ => {}
        }
    }

    let mut used = HashSet::new();
    if let Some(obj) = schema.as_object() {
        for (k, v) in obj {
            insert_ref(k, v, &mut used);
            if k != "$defs" {
                add_refs(v, &mut used);
            }
        }
    }

    if let Some(defs) = schema.get("$defs").and_then(Value::as_object) {
        // Anchor/$id based references resolve without a `#/$defs/` pointer,
        // so identity-bearing defs are never safe to prune.
        for (name, def) in defs {
            if contains_identity_keyword(def) {
                used.insert(name.clone());
            }
        }
        let mut queue: Vec<String> = used.iter().cloned().collect();
        while let Some(name) = queue.pop() {
            if let Some(def) = defs.get(&name) {
                let mut inner = HashSet::new();
                add_refs(def, &mut inner);
                for n in inner {
                    if used.insert(n.clone()) {
                        queue.push(n);
                    }
                }
            }
        }
    }
    used
}
