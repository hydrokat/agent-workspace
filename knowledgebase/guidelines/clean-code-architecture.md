---
name: nextjs-strict-types-clean-architecture-zod-agent
description: Enforces strict TypeScript, Clean Code Architecture, and Zod-based validation in Next.js applications. Ensures type-safe, validated, and layered architecture across the stack.

triggers:
  - nextjs project detected
  - typescript usage in frontend/backend
  - creating API routes, forms, hooks, services, or domain logic
  - data validation required
  - refactoring unsafe or untyped code

principles:
  - strict TypeScript typing is mandatory
  - Clean Code Architecture must be enforced
  - Zod is the single source of truth for runtime validation
  - all external input is untrusted
  - validation happens at system boundaries only
  - business logic is independent from framework and validation library

architecture_rules:
  layers:
    presentation_layer:
      responsibility: UI only (React components, pages)
      rules:
        - no business logic
        - no direct API calls (use hooks/application layer)
        - props must be strictly typed (inferred from domain/application types)
        - must not contain Zod schemas directly (only use inferred types)

    application_layer:
      responsibility: orchestration and use cases
      rules:
        - contains use cases
        - validates input using Zod schemas at entry points
        - coordinates domain + infrastructure
        - no UI logic allowed

    domain_layer:
      responsibility: core business rules
      rules:
        - pure TypeScript only
        - no Zod dependency allowed (important separation rule)
        - defines entities, value objects, business rules
        - enforces invariants internally

    infrastructure_layer:
      responsibility: external systems integration
      rules:
        - API, DB, third-party services
        - uses application layer contracts
        - never contains business rules

dependency_rules:
  - presentation → application → domain
  - infrastructure → application/domain via interfaces
  - domain must never depend on Zod or any framework
  - Zod is allowed ONLY in application/infrastructure boundary layers
  - no circular dependencies allowed

strict_typescript_rules:
  - tsconfig must enforce:
      - strict: true
      - noImplicitAny: true
      - strictNullChecks: true
      - noUncheckedIndexedAccess: true

  - never use `any`
  - prefer `unknown` for unsafe external inputs
  - always explicitly type public APIs and functions
  - infer types from Zod schemas when possible

zod_validation_rules:
  - Zod is the ONLY validation library allowed
  - all external inputs must pass through Zod schemas
  - validation must occur at boundaries:
      - API routes
      - server actions
      - form submissions
      - external API responses

  - schemas must be colocated with use cases or domain entry points

  - schema design rules:
      - schemas define runtime contract
      - types are derived from schemas using:
          - z.infer<typeof schema>
      - never duplicate schema types manually

  - failure handling:
      - always return structured validation errors
      - never throw raw Zod errors to UI
      - normalize errors at application layer

nextjs_rules:
  api_routes:
    - must validate request body with Zod
    - must call application use cases only
    - must not contain business logic

  server_components:
    - must not validate using Zod directly
    - rely on application layer outputs only

  client_components:
    - use inferred types only
    - no Zod usage in UI layer

  forms:
    - Zod schemas define form validation rules
    - reuse same schema for API + UI validation consistency

  hooks:
    - must call application layer
    - must not define business rules

data_flow:
  - input enters system (API / form / external service)
  - validate using Zod schema at boundary
  - convert validated data into domain objects
  - execute application use case
  - domain enforces business rules internally
  - return typed result to UI

file_structure_guideline:
  preferred_structure:
    - /domain
        - entities
        - value-objects
        - repositories (interfaces)
    - /application
        - use-cases
        - schemas (Zod)
        - services
    - /infrastructure
        - api
        - db
        - external-services
    - /presentation
        - components
        - pages
        - hooks
    - /types
        - inferred shared types only (z.infer outputs)

anti_patterns:
  - using Zod inside React components for business logic
  - duplicating validation logic outside Zod schemas
  - mixing domain rules with validation schemas
  - using any instead of z.infer
  - skipping validation at API boundaries
  - letting UI interpret raw Zod errors
  - placing business logic inside infrastructure layer

workflow:
  - identify feature scope (UI / API / domain / infra)
  - design domain model first (pure TS)
  - define Zod schema at application boundary
  - infer types from schema (z.infer)
  - implement use case using validated input
  - implement infrastructure adapters if needed
  - implement presentation layer last
  - ensure all inputs validated at boundaries
  - run TypeScript + lint checks

output_rules:
  - always specify architecture layer affected
  - always show Zod schema separately from domain types
  - explicitly show where validation occurs
  - highlight any schema-to-type inference usage
  - flag any violation of “domain must be pure”