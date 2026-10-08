#!/usr/bin/env python3

"""Generate BDK API reference data from the UniFFI surface in bcaip-sdk.

`crates/bcaip-sdk/src/bindings.rs` is the single source of truth for the Rust,
Python, and Kotlin BDK APIs, so the docs are derived from it instead of being
written by hand. Output is `documentation/src/data/bdk-api.json`, holding one
entry per BDK release series, consumed by the BdkApiReference component.

Usage:
    python3 documentation/automation/bdk-api/generate.py [--check]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import TypedDict, cast

REPO_ROOT = Path(__file__).resolve().parents[3]
BINDINGS = REPO_ROOT / "crates/bcaip-sdk/src/bindings.rs"
CARGO_TOML = REPO_ROOT / "crates/bcaip-sdk/Cargo.toml"
OUT_FILE = REPO_ROOT / "documentation/src/data/bdk-api.json"

@dataclass
class Param:
    name: str
    type: str
    default: str | None = None
    docs: str = ""

def empty_params() -> list[Param]:
    return []

@dataclass
class Func:
    name: str
    docs: str = ""
    params: list[Param] = field(default_factory=empty_params)
    returns: str | None = None
    throws: str | None = None
    is_async: bool = False

def empty_funcs() -> list[Func]:
    return []

@dataclass
class Variant:
    name: str
    fields: list[Param] = field(default_factory=empty_params)

def empty_variants() -> list[Variant]:
    return []

@dataclass
class Item:
    name: str
    kind: str
    docs: str = ""
    fields: list[Param] = field(default_factory=empty_params)
    variants: list[Variant] = field(default_factory=empty_variants)
    methods: list[Func] = field(default_factory=empty_funcs)

class ParsedBindings(TypedDict):
    items: list[Item]
    functions: list[Func]

class SerializedParam(TypedDict):
    name: str
    type: str
    default: str | None
    docs: str

class SerializedVariant(TypedDict):
    name: str
    fields: list[SerializedParam]

class SerializedFunc(TypedDict):
    name: str
    docs: str
    params: list[SerializedParam]
    returns: str | None
    throws: str | None
    isAsync: bool

class SerializedItem(TypedDict):
    name: str
    kind: str
    docs: str
    fields: list[SerializedParam]
    variants: list[SerializedVariant]
    methods: list[SerializedFunc]

class ApiVersion(TypedDict):
    version: str
    docVersion: str
    source: str
    functions: list[SerializedFunc]
    items: list[SerializedItem]

class ApiPayload(TypedDict):
    versions: list[ApiVersion]

def crate_version() -> str:
    match = re.search(
        r'^version\s*=\s*"([^"]+)"',
        CARGO_TOML.read_text(),
        re.MULTILINE,
    )

    if match is None:
        sys.exit(f"could not read version from {CARGO_TOML}")

    return match.group(1)

def doc_version(version: str) -> str:
    """Docs are versioned per release series, e.g. 0.1.0-alpha.6 -> 0.1."""
    parts: list[str] = version.split(".")

    if len(parts) < 2:
        raise ValueError(f"invalid crate version: {version}")

    return f"{parts[0]}.{parts[1]}"

def split_top_level(text: str, sep: str = ",") -> list[str]:
    parts: list[str] = []
    depth = 0
    current = ""

    for char in text:
        if char in "<([{":
            depth += 1
        elif char in ">)]}":
            depth -= 1

        if char == sep and depth == 0:
            parts.append(current)
            current = ""
        else:
            current += char

    if current.strip():
        parts.append(current)

    return [
        part.strip()
        for part in parts
        if part.strip()
    ]

def unwrap(type_text: str, wrapper: str) -> str | None:
    match = re.fullmatch(
        rf"{wrapper}\s*<(.+)>",
        type_text.strip(),
        re.DOTALL,
    )

    if match is None:
        return None

    return match.group(1).strip()

def clean_type(type_text: str) -> str:
    type_text = re.sub(
        r"\s+",
        " ",
        type_text,
    ).strip()

    while True:
        inner = (
            unwrap(type_text, r"(?:std::sync::)?Arc")
            or unwrap(type_text, r"Box<\s*dyn")
        )

        if inner is None:
            inner = unwrap(type_text, "Box")

        if inner is None:
            break

        type_text = re.sub(
            r"^dyn\s+",
            "",
            inner,
        )

    return type_text

class Scanner:
    """Line scanner that pairs doc comments and attributes with the next item."""

    def __init__(self, source: str) -> None:
        self.lines: list[str] = source.splitlines()
        self.index: int = 0
        self.docs: list[str] = []
        self.attrs: list[str] = []

    def take_docs(self) -> str:
        docs = "\n".join(self.docs).strip()
        self.docs = []
        return docs

    def block(self) -> str:
        """Consume from the current line through its balanced brace block."""
        text = ""
        depth = 0
        started = False

        while self.index < len(self.lines):
            line: str = self.lines[self.index]
            self.index += 1

            text += line + "\n"
            depth += line.count("{") - line.count("}")
            started = started or "{" in line

            if started and depth <= 0:
                break

            if not started and line.rstrip().endswith(";"):
                break

        return text

def parse_fields(block: str) -> list[Param]:
    fields: list[Param] = []
    docs: list[str] = []
    default: str | None = None

    for line in block.splitlines():
        stripped: str = line.strip()

        if stripped.startswith("///"):
            docs.append(stripped[3:].strip())
            continue

        default_match = re.match(
            r"#\[uniffi\(default\s*=\s*(.+?)\)\]",
            stripped,
        )

        if default_match is not None:
            default = default_match.group(1).strip()
            continue

        field_match = re.match(
            r"pub\s+([a-z_0-9]+)\s*:\s*(.+?),?$",
            stripped,
        )

        if field_match is None:
            continue

        fields.append(
            Param(
                name=field_match.group(1),
                type=clean_type(field_match.group(2)),
                default=default,
                docs=" ".join(docs).strip(),
            )
        )

        docs = []
        default = None

    return fields

def parse_variants(block: str) -> list[Variant]:
    body = block[
        block.index("{") + 1 :
        block.rindex("}")
    ]

    variants: list[Variant] = []

    chunks: list[str] = split_top_level(
        re.sub(
            r"#\[[^\]]*\]",
            "",
            body,
        )
    )

    for chunk in chunks:
        chunk = chunk.strip()

        struct_match = re.match(
            r"^([A-Z]\w*)\s*\{(.*)\}$",
            chunk,
            re.DOTALL,
        )

        if struct_match is not None:
            fields: list[Param] = []
            field_parts: list[str] = split_top_level(
                struct_match.group(2)
            )

            for part in field_parts:
                name, separator, type_text = part.partition(":")

                if separator == "":
                    continue

                field_name = name.strip()

                if not field_name or field_name.startswith("#"):
                    continue

                fields.append(
                    Param(
                        name=field_name,
                        type=clean_type(type_text),
                    )
                )

            variants.append(
                Variant(
                    name=struct_match.group(1),
                    fields=fields,
                )
            )
            continue

        if re.fullmatch(r"[A-Z]\w*", chunk) is not None:
            variants.append(
                Variant(name=chunk)
            )

    return variants

def parse_arg_defaults(attrs: str) -> dict[str, str]:
    """Read argument defaults from `#[uniffi::export(default(arg = value))]`."""
    defaults: dict[str, str] = {}

    groups: list[str] = re.findall(
        r"default\s*\(([^()]*)\)",
        attrs,
    )

    for group in groups:
        parts: list[str] = split_top_level(group)

        for part in parts:
            name, separator, value = part.partition("=")

            if separator == "":
                continue

            defaults[name.strip()] = value.strip()

    return defaults

def parse_signature(
    signature: str,
    docs: str,
    defaults: dict[str, str] | None = None,
) -> Func:
    actual_defaults: dict[str, str]

    if defaults is None:
        actual_defaults = {}
    else:
        actual_defaults = defaults

    signature = (
        re.sub(
            r"\s+",
            " ",
            signature,
        )
        .strip()
        .rstrip("{;")
        .strip()
    )

    is_async = " async fn " in f" {signature} "

    match = re.search(
        r"fn\s+(\w+)\s*\((.*)\)\s*(?:->\s*(.+))?$",
        signature,
        re.DOTALL,
    )

    if match is None:
        return Func(
            name=signature,
            docs=docs,
        )

    name: str = match.group(1)
    raw_params: str = match.group(2)
    raw_return: str | None = match.group(3)

    params: list[Param] = []
    parameter_parts: list[str] = split_top_level(raw_params)

    for part in parameter_parts:
        if re.fullmatch(
            r"&?\s*(mut\s+)?self",
            part,
        ) is not None:
            continue

        param_name, separator, type_text = part.partition(":")

        if separator == "":
            continue

        name_text = param_name.strip()

        params.append(
            Param(
                name=name_text,
                type=clean_type(type_text),
                default=actual_defaults.get(name_text),
            )
        )

    returns: str | None = None
    throws: str | None = None

    if raw_return is not None:
        result: str = clean_type(raw_return)
        inner: str | None = unwrap(result, "Result")

        if inner is not None:
            result_parts: list[str] = split_top_level(inner)

            if result_parts:
                returns = clean_type(result_parts[0])

            if len(result_parts) > 1:
                throws = clean_type(result_parts[1])
            else:
                throws = "BcaipError"
        else:
            returns = result

    if returns in ("()", ""):
        returns = None

    return Func(
        name=name,
        docs=docs,
        params=params,
        returns=returns,
        throws=throws,
        is_async=is_async,
    )

def parse_required_name(
    pattern: str,
    text: str,
    description: str,
) -> str:
    match = re.search(
        pattern,
        text,
    )

    if match is None:
        raise ValueError(
            f"could not parse {description}"
        )

    return match.group(1)

def parse_bindings(source: str) -> ParsedBindings:
    source = source.split(
        "#[cfg(test)]",
        1,
    )[0]

    scanner = Scanner(source)
    items: list[Item] = []
    functions: list[Func] = []

    while scanner.index < len(scanner.lines):
        line: str = scanner.lines[scanner.index]
        stripped: str = line.strip()

        if stripped.startswith("///"):
            scanner.docs.append(
                stripped[3:].strip()
            )
            scanner.index += 1
            continue

        if stripped.startswith("#["):
            scanner.attrs.append(stripped)
            scanner.index += 1
            continue

        if not stripped or stripped.startswith("//"):
            scanner.index += 1
            scanner.docs = []
            continue

        attrs: str = " ".join(scanner.attrs)
        scanner.attrs = []

        docs: str = scanner.take_docs()
        exported: bool = "uniffi::export" in attrs

        if (
            "uniffi::Record" in attrs
            and stripped.startswith("pub struct")
        ):
            block: str = scanner.block()

            name = parse_required_name(
                r"pub struct\s+(\w+)",
                block,
                "UniFFI record name",
            )

            items.append(
                Item(
                    name=name,
                    kind="record",
                    docs=docs,
                    fields=parse_fields(block),
                )
            )
            continue

        if (
            "uniffi::Object" in attrs
            and stripped.startswith("pub struct")
        ):
            block = scanner.block()

            name = parse_required_name(
                r"pub struct\s+(\w+)",
                block,
                "UniFFI object name",
            )

            items.append(
                Item(
                    name=name,
                    kind="object",
                    docs=docs,
                )
            )
            continue

        if (
            (
                "uniffi::Enum" in attrs
                or "uniffi::Error" in attrs
            )
            and stripped.startswith("pub enum")
        ):
            block = scanner.block()

            name = parse_required_name(
                r"pub enum\s+(\w+)",
                block,
                "UniFFI enum name",
            )

            kind = (
                "error"
                if "uniffi::Error" in attrs
                else "enum"
            )

            items.append(
                Item(
                    name=name,
                    kind=kind,
                    docs=docs,
                    variants=parse_variants(block),
                )
            )
            continue

        if (
            exported
            and stripped.startswith("pub trait")
        ):
            block = scanner.block()

            name = parse_required_name(
                r"pub trait\s+(\w+)",
                block,
                "UniFFI trait name",
            )

            signatures: list[str] = re.findall(
                r"fn\s+\w+\s*\([^;]*?\)\s*(?:->[^;]+)?;",
                block,
            )

            methods: list[Func] = [
                parse_signature(
                    signature,
                    "",
                )
                for signature in signatures
            ]

            items.append(
                Item(
                    name=name,
                    kind="callback",
                    docs=docs,
                    methods=methods,
                )
            )
            continue

        if (
            exported
            and stripped.startswith("impl ")
        ):
            block = scanner.block()

            target = parse_required_name(
                r"impl\s+(\w+)",
                block,
                "UniFFI impl target",
            )

            owner: Item | None = next(
                (
                    item
                    for item in items
                    if item.name == target
                ),
                None,
            )

            if owner is not None:
                owner.methods.extend(
                    parse_impl_methods(block)
                )

            continue

        if (
            exported
            and re.match(
                r"pub\s+(async\s+)?fn",
                stripped,
            ) is not None
        ):
            block = scanner.block()

            functions.append(
                parse_signature(
                    block.split("{", 1)[0],
                    docs,
                    parse_arg_defaults(attrs),
                )
            )
            continue

        scanner.index += 1

    return {
        "items": items,
        "functions": functions,
    }

def parse_impl_methods(block: str) -> list[Func]:
    methods: list[Func] = []
    lines: list[str] = block.splitlines()
    docs: list[str] = []
    index = 0

    while index < len(lines):
        stripped: str = lines[index].strip()

        if stripped.startswith("///"):
            docs.append(
                stripped[3:].strip()
            )
            index += 1
            continue

        if re.match(
            r"pub\s+(async\s+)?fn",
            stripped,
        ) is not None:
            signature = ""
            depth = 0

            while index < len(lines):
                current_line: str = lines[index]

                signature += current_line + "\n"

                depth += (
                    current_line.count("(")
                    - current_line.count(")")
                )

                if (
                    depth <= 0
                    and (
                        "{" in current_line
                        or ";" in current_line
                    )
                ):
                    break

                index += 1

            methods.append(
                parse_signature(
                    signature.split("{", 1)[0],
                    "\n".join(docs).strip(),
                )
            )

            docs = []

        elif (
            stripped
            and not stripped.startswith("#")
        ):
            docs = []

        index += 1

    return methods

def serialize_param(
    param: Param,
) -> SerializedParam:
    return {
        "name": param.name,
        "type": param.type,
        "default": param.default,
        "docs": param.docs,
    }

def serialize_variant(
    variant: Variant,
) -> SerializedVariant:
    serialized_fields: list[SerializedParam] = [
        serialize_param(field_)
        for field_ in variant.fields
    ]

    return {
        "name": variant.name,
        "fields": serialized_fields,
    }

def serialize_func(
    func: Func,
) -> SerializedFunc:
    serialized_params: list[SerializedParam] = [
        serialize_param(param)
        for param in func.params
    ]

    return {
        "name": func.name,
        "docs": func.docs,
        "params": serialized_params,
        "returns": func.returns,
        "throws": func.throws,
        "isAsync": func.is_async,
    }

def serialize_item(
    item: Item,
) -> SerializedItem:
    serialized_fields: list[SerializedParam] = [
        serialize_param(field_)
        for field_ in item.fields
    ]

    serialized_variants: list[SerializedVariant] = [
        serialize_variant(variant)
        for variant in item.variants
    ]

    serialized_methods: list[SerializedFunc] = [
        serialize_func(method)
        for method in item.methods
    ]

    return {
        "name": item.name,
        "kind": item.kind,
        "docs": item.docs,
        "fields": serialized_fields,
        "variants": serialized_variants,
        "methods": serialized_methods,
    }

def build(version: str) -> ApiVersion:
    parsed: ParsedBindings = parse_bindings(
        BINDINGS.read_text()
    )

    items: list[Item] = parsed["items"]
    functions: list[Func] = parsed["functions"]

    if not items or not functions:
        sys.exit(
            "parsed no API items; "
            "the bindings layout likely changed"
        )

    sorted_functions: list[Func] = sorted(
        functions,
        key=lambda function: function.name,
    )

    serialized_functions: list[SerializedFunc] = [
        serialize_func(func)
        for func in sorted_functions
    ]

    serialized_items: list[SerializedItem] = [
        serialize_item(item)
        for item in items
    ]

    return {
        "version": version,
        "docVersion": doc_version(version),
        "source": "crates/bcaip-sdk/src/bindings.rs",
        "functions": serialized_functions,
        "items": serialized_items,
    }

def load_existing_payload() -> ApiPayload:
    if not OUT_FILE.exists():
        return {
            "versions": [],
        }

    raw: object = json.loads(
        OUT_FILE.read_text()
    )

    return cast(
        ApiPayload,
        raw,
    )

def merge(
    current: ApiVersion,
) -> ApiPayload:
    """Upsert the current release series, keeping older series newest-first."""
    existing_payload: ApiPayload = load_existing_payload()
    existing: list[ApiVersion] = existing_payload["versions"]

    versions: list[ApiVersion] = [
        entry
        for entry in existing
        if entry["docVersion"] != current["docVersion"]
    ]

    versions.append(current)

    versions.sort(
        key=lambda entry: [
            int(part)
            for part in entry["docVersion"].split(".")
        ],
        reverse=True,
    )

    return {
        "versions": versions,
    }

def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
    )

    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if output is stale",
    )

    args = parser.parse_args()
    check_mode = bool(
        getattr(
            args,
            "check",
            False,
        )
    )

    version: str = crate_version()
    current: ApiVersion = build(version)
    merged: ApiPayload = merge(current)

    payload: str = (
        json.dumps(
            merged,
            indent=2,
        )
        + "\n"
    )

    relative: Path = OUT_FILE.relative_to(
        REPO_ROOT
    )

    if check_mode:
        if (
            not OUT_FILE.exists()
            or OUT_FILE.read_text() != payload
        ):
            print(
                f"{relative} is out of date; "
                f"run {Path(__file__).name}"
            )
            return 1

        print(
            f"{relative} is up to date"
        )
        return 0

    OUT_FILE.parent.mkdir(
        parents=True,
        exist_ok=True,
    )

    OUT_FILE.write_text(payload)

    print(
        f"wrote {relative} "
        f"for bcaip-sdk {version}"
    )

    return 0

if __name__ == "__main__":
    raise SystemExit(main())