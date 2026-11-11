# Architecture and Code Quality Guidelines

## Clean Architecture

- Keep domain rules, application orchestration, infrastructure, and presentation
  concerns in separate layers.
- Depend inward on stable domain abstractions. Business rules must not import
  framework controllers, ORM models, or HTTP primitives.
- Keep interfaces small, typed, and focused on a single concern.

## No God Objects

- Do not create god services or god controllers.
- A controller orchestrates: parse request, authorize, validate, call service,
  shape response. It does not contain business logic.
- A service owns one coherent application capability.
- Split files when unrelated responsibilities or different dependency sets
  accumulate in one place.

## File and Line Limits

- **500 lines** maximum per source file.
- **110 columns** maximum per line.
- If a file must temporarily exceed either limit, document the reason in the
  task and schedule a split before release.

## KISS and DRY

- **KISS:** Prefer the simplest solution that meets requirements. Avoid
  premature abstraction, over-engineering, or design patterns that add
  complexity without a demonstrated need.
- **DRY:** Extract duplicate logic into shared helpers or traits. When a
  pattern appears three or more times, it should be formalized.

## Reusable Code

- Use traits, helpers, or base classes for shared behavior across modules.
- A trait or helper should have a single responsibility and be testable in
  isolation.
- Do not import shared helpers from god modules. Organize by domain.

## Refactoring and Blast Radius

On every refactor, assess the blast radius before making changes:

1. **Map dependents.** Before touching a function, class, or module, identify
   every caller, consumer, or test that depends on it.
2. **Existing tests must still pass.** A refactor that breaks existing tests is
   not a refactor — it is a behavior change. If existing tests must change,
   the change is a redesign, not a refactor.
3. **Prefer additive over subtractive.** Add a new interface before removing
   the old one. Deprecate before deleting. Run a full test suite between
   each step.
4. **Shared code has exponential blast radius.** Changing a utility, trait,
   or helper used in N places risks N callers. Add tests for every call site
   before modifying shared code.
5. **Public API changes must be multi-phase.** Add the new parameter or
   overload in phase 1, migrate callers in phase 2, remove the old signature
   in phase 3.
6. **Run the full suite.** Never rely on a subset of tests. Run the entire
   test suite before and after the refactor to confirm nothing regressed.