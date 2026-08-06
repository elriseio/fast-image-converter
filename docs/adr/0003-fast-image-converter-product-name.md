---
project_slug: fast-image-converter
doc_slug: adr_0003_fast_image_converter_product_name
doc_type: adr
applicable_roles: [architect, developer, lamport]
version: 1
date: 2026-08-04
status: accepted
supersedes: adr_0001-multi-format-cli-scope.md
summary: "Rename the product and canonical CLI from fast-image-converter to fast-image-converter while retaining compatibility aliases for existing operators."
source_artifacts:
  - Cargo.toml
  - README.md
  - docs/architecture.md
  - docs/integration-contract.md
  - operator directive received 2026-08-04
tags: [adr, rename, breaking-change, cli, compatibility]
---

# ADR-0003 — Adopt fast-image-converter as the product name

## Status

**Accepted** (2026-08-04) by explicit operator directive.

## Date

2026-08-04

## Authors

- Architect (system)
- Operator (decision authority)

## Context

The repository currently uses `fast-image-converter` as its crate name,
canonical binary name, documentation identity, release artifact name,
and test discovery name. The product now supports JPG, PNG, and WebP
conversion in both directions, so the existing name is narrower than
the actual capability.

The rename affects public CLI invocation, Cargo package metadata, binary
artifacts, release assets, documentation, test environment variables,
operator runbooks, and consumer-facing integration contracts. Existing
operators may still invoke `fast-image-converter` or `gallery-compress`.

The MCP operational project identity is a separate coordination key. It
must not be changed as a side effect of the product rename; a separate
identity migration is required if the operator wants that key changed.

## Decision

1. The canonical product and binary name becomes `fast-image-converter`.
2. The Cargo package name, default binary, release artifact, examples,
   and consumer-facing documentation use `fast-image-converter`.
3. `fast-image-converter` remains a deprecated compatibility alias for at
   least one major version and forwards unchanged arguments and exit
   status to the canonical binary.
4. `gallery-compress` remains a legacy compatibility alias and forwards
   to the canonical binary with a deprecation hint.
5. JSON, exit-code, stdin/stdout, report-fd, and image-conversion
   semantics do not change solely because of the name rename.
6. Every old-name reference must be classified as one of: compatibility
   alias, historical ADR/issue evidence, generated build output, or
   stale product reference. Stale product references must be renamed.
7. The operational project slug remains unchanged until a separately
   approved identity migration is completed.

## Consequences

### Positive

- The public name describes the multi-format product accurately.
- Existing operator scripts continue to work through compatibility aliases.
- Release artifacts and documentation have one clear canonical identity.

### Negative / Risks

- Cargo binary environment variables and downstream package scripts must
  change from the old canonical target to the new target.
- Consumers that assert the old executable name may need migration before
  the next major release.
- Historical documents and compatibility messages will still contain old
  names by design.

## Migration and Compatibility Contract

- New documentation and examples must invoke `fast-image-converter`.
- Compatibility aliases must be tested for argument forwarding, exit
  status, stdout purity, stderr deprecation messaging, and JSON behavior.
- The release workflow must publish the canonical binary first and list
  compatibility aliases explicitly.
- The next major version may remove compatibility aliases only after an
  operator-approved migration review.

## Follow-up

- `Issues/open/developer/AR-009_rename_runtime_and_cargo_identity.md`
- `Issues/open/developer/AR-010_rename_release_and_deploy_surfaces.md`
- `Issues/open/lamport/AR-011_rename_product_documentation.md`
