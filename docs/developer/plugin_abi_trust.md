---
title: Plugin ABI & Trust
---

# Plugin/Extension ABI & Trust Model

Manifests
- Signed plugin manifests declare capabilities, UI contributions (panels/commands), background tasks, and spec compatibility.

Sandboxing
- Default to sandboxed execution (WASI/containers); deny network unless declared.

Compatibility
- Tool ids and schemas must pass schema validation and contract tests.

Provenance
- Record tool versions and signatures in artifacts and episodes for audit.

See also: Plugins & Extensions, Threat Model, Contract Tests.

