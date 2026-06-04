# Architecture and Code Quality Guidelines

These standards keep implementation portable across frameworks while protecting maintainability.

## Clean Architecture

- Follow clean code architecture by keeping domain rules, application orchestration,
  infrastructure, and presentation concerns separate.
- Depend inward on stable domain abstractions instead of binding business rules to frameworks,
  controllers, jobs, or UI components.
- Keep interfaces clear, typed where the language supports it, and small enough to be
  understood without reading unrelated modules.

## Service and Controller Size

- Do not create god services, god controllers, or catch-all modules.
- A controller should coordinate request parsing, authorization handoff, validation handoff,
  application service calls, and response shaping.
- A service should own one coherent application capability or workflow.
- Split files when unrelated responsibilities, branching workflows, or dependency sets start
  accumulating in one place.

## File and Line Limits

- Keep source files at or below 500 lines of code.
- Keep lines at or below 110 columns.
- If a file must temporarily exceed the limit, document the reason in the task and create
  a follow-up split task before release.
