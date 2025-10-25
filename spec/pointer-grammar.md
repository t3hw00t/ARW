# Pointer Grammar

ARW compression policies rely on explicit pointer tokens that expand at runtime. All pointer forms are ASCII and enclosed in `<@ … >`.

## General Structure

```
<@PREFIX:BODY[:QUALIFIER]>
```

- `PREFIX` identifies the pointer domain. Supported values: `blob`, `ocr`, `sigil`, `claim`, `graph`.
- `BODY` encodes the content-addressed identifier. It MUST be canonicalized before hashing.
- Optional `QUALIFIER` tail encodes spans, semantic hints, or parameters.

The parser MUST reject pointers that do not match the allowed prefix list, exceed 256 characters, or contain whitespace.

## Forms

### Blob Pointer

```
<@blob:sha256:HEX[:start..end]>
```

- `HEX` is a lowercase 64-hex digest of the normalized bytes.
- Optional byte-range `[start, end)` allows partial expansion.

### OCR Pointer

```
<@ocr:DOC#p{page}[:line={line}|:block={block}]>
```

- `DOC` is the OCR bundle identifier.
- `page`, `line`, `block` are zero-based integers.

### Sigil Pointer

```
<@sigil:NAME[:v=SEMVER]>
```

- `NAME` is a project-scoped macro token.
- Optional semantic version gate ensures compatibility.

### Claim Pointer

```
<@claim:ID>
```

- `ID` references a worldview claim or predicate cache entry.

### Graph Pointer

```
<@graph:ROOT?k={hops}&budget={tokens}>
```

- `ROOT` node identifier in the knowledge graph.
- `hops` sets traversal radius (default 1, max 4).
- `budget` sets the expansion token budget.

## Canonicalization Rules

1. Normalize Unicode inputs to NFKC.
2. Convert CRLF to LF.
3. Trim trailing whitespace outside of literal payloads.
4. Lowercase pointer prefixes.
5. Collapse repeated separators (`::`, `..`) into single instances; if any collapse changes semantics, reject.
6. Tokens must already conform to this canonical form when submitted; the server lowercases prefixes automatically and rejects any pointer whose domain does not match the declared metadata.

## Safety Checklist

- Enforce depth and fan-out limits before expansion.
- Validate referenced hashes prior to expansion; mismatches must hard-fail.
- Guard against pointer cycles by tracking visited identifiers.
- Log `{pointer, actor, decision}` to the audit trail after expansion.
- When `security.consent_gate` is enabled, every pointer referenced by a plan must carry explicit consent metadata (for example `private`, `shared`, or `public`).

These rules are mirrored in `crates/arw-contracts::pointer` and validated at ingress. Any new pointer prefix requires updating the shared contract crate and this document. 
