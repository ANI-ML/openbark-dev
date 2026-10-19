#!/usr/bin/env python3
"""Validate a drafted note against a template's JSON Schema, with the
standard library only (no jsonschema package on the runner or in WSL).

Covers the keywords OpenBark's template schemas use: $ref (local #/$defs),
type, properties, required, additionalProperties, items, anyOf, enum, const,
minimum, maximum, minItems, maxItems, minLength, maxLength. Any other
validation keyword makes it fail loudly rather than pass silently.

Usage: validate-note.py <schema.json> <note.json | result.json>
A result written by `openbark note --json` is accepted too (its "note").
"""

import json
import sys

ANNOTATIONS = {"$schema", "$id", "title", "description", "$defs", "definitions", "$comment", "examples", "default"}
CHECKED = {
    "$ref", "type", "properties", "required", "additionalProperties", "items", "anyOf", "enum", "const",
    "minimum", "maximum", "minItems", "maxItems", "minLength", "maxLength",
}


def type_ok(value, t):
    if isinstance(t, list):
        return any(type_ok(value, x) for x in t)
    return {
        "object": lambda v: isinstance(v, dict),
        "array": lambda v: isinstance(v, list),
        "string": lambda v: isinstance(v, str),
        "integer": lambda v: isinstance(v, int) and not isinstance(v, bool),
        "number": lambda v: isinstance(v, (int, float)) and not isinstance(v, bool),
        "boolean": lambda v: isinstance(v, bool),
        "null": lambda v: v is None,
    }[t](value)


def resolve(root, ref):
    if not ref.startswith("#/"):
        raise SystemExit(f"unsupported $ref {ref}")
    node = root
    for part in ref[2:].split("/"):
        node = node[part]
    return node


def validate(root, schema, value, path, errors):
    if schema is True or schema == {}:
        return
    if schema is False:
        errors.append(f"{path}: not allowed")
        return
    unknown = set(schema) - ANNOTATIONS - CHECKED
    if unknown:
        raise SystemExit(f"validate-note.py doesn't support {sorted(unknown)} (at {path})")
    if "$ref" in schema:
        validate(root, resolve(root, schema["$ref"]), value, path, errors)
    if "type" in schema and not type_ok(value, schema["type"]):
        errors.append(f"{path}: expected {schema['type']}, got {type(value).__name__}")
        return
    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{path}: {value!r} not in {schema['enum']}")
    if "const" in schema and value != schema["const"]:
        errors.append(f"{path}: {value!r} != {schema['const']!r}")
    if "anyOf" in schema:
        if not any(not collect(root, s, value, path) for s in schema["anyOf"]):
            errors.append(f"{path}: matches none of anyOf")
    if isinstance(value, dict):
        props = schema.get("properties", {})
        for key in schema.get("required", []):
            if key not in value:
                errors.append(f"{path}: missing {key}")
        for key, v in value.items():
            if key in props:
                validate(root, props[key], v, f"{path}/{key}", errors)
            elif schema.get("additionalProperties") is False:
                errors.append(f"{path}: unexpected {key}")
            elif isinstance(schema.get("additionalProperties"), dict):
                validate(root, schema["additionalProperties"], v, f"{path}/{key}", errors)
    if isinstance(value, list):
        if "minItems" in schema and len(value) < schema["minItems"]:
            errors.append(f"{path}: fewer than {schema['minItems']} items")
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            errors.append(f"{path}: more than {schema['maxItems']} items")
        if isinstance(schema.get("items"), dict):
            for i, v in enumerate(value):
                validate(root, schema["items"], v, f"{path}/{i}", errors)
    if isinstance(value, str):
        if "minLength" in schema and len(value) < schema["minLength"]:
            errors.append(f"{path}: shorter than {schema['minLength']}")
        if "maxLength" in schema and len(value) > schema["maxLength"]:
            errors.append(f"{path}: longer than {schema['maxLength']}")
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in schema and value < schema["minimum"]:
            errors.append(f"{path}: below {schema['minimum']}")
        if "maximum" in schema and value > schema["maximum"]:
            errors.append(f"{path}: above {schema['maximum']}")


def collect(root, schema, value, path):
    errors = []
    validate(root, schema, value, path, errors)
    return errors


def main():
    schema = json.load(open(sys.argv[1], encoding="utf-8"))
    doc = json.load(open(sys.argv[2], encoding="utf-8"))
    note = doc["note"] if isinstance(doc, dict) and "note" in doc and "template" in doc else doc
    errors = collect(schema, schema, note, "")
    if errors:
        print("the note does NOT match the schema:")
        for e in errors[:40]:
            print("  " + e)
        sys.exit(1)
    print(f"the note matches {sys.argv[1]}")


if __name__ == "__main__":
    main()
