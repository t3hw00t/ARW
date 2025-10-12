---
title: Interface Release Notes
---

# Interface Release Notes
Updated: 2025-10-12
Type: Reference

Base: `origin/main`

## OpenAPI (REST)

### New Endpoints: None
-----------------------

### Deleted Endpoints: None
---------------------------

### Modified Endpoints: 1
-------------------------
GET /orchestrator/mini_agents
- Summary changed from 'List available mini-agents (placeholder).' to 'List cataloged mini-agents.'
- Description changed from 'List available mini-agents (placeholder).' to 'Return the curated mini-agent catalog with training defaults, requirements, and documentation pointers.'
- Responses changed
  - Modified response: 200
    - Description changed from '' to 'Mini-agent catalog'
    - Content changed
      - Modified media type: application/json
        - Schema changed
          - Type changed from '' to 'object'
          - Required changed
            - New required property: items
            - New required property: version
          - Properties changed
            - New property: generated_at
            - New property: items
            - New property: schema
            - New property: version

## AsyncAPI (Events)

No changes

