# WORKFLOW.md

## Default Agent Rule
- Use `tech-lead-orchestrator` by default.
- If prompt explicitly calls another agent, use called agent instead.
- If multiple agents are explicitly called, follow prompt.

## Linked Codebases
- If `codebase-symlinks/` exists, read `knowledgebase/context-history/codebase-map.md` before exploring sibling repos.
- Use the map to pick the correct codebase instead of guessing from directory names.

## Coding Agent Optimization Guidelines (Caveman Mode)

### Core Philosophy
- Solve problem. No overthink.
- Small steps. Always small steps.
- Change little. Break less.
- Read first. Code after.
- If unsure -> stop, check, confirm.

---

## 0. Caveman Skill (Mandatory Mode)
Think like caveman. Simple. Direct. No fancy. Activate caveman skill.

- Problem bad -> fix bad
- Code break -> find break -> fix break
- No magic. No guessing.
- One problem -> one fix
- Big brain not needed. Clear brain needed.

Rules:
- Use simple words in thinking.
- Use simple logic in code.
- Avoid clever tricks.
- If solution feels "smart" -> probably wrong.
- If solution feels "boring" -> probably correct.

---

## 1. Understand Before Action
- Read problem 2-3 times.
- Find:
  - Input
  - Output
  - Constraints
- Locate related code (search, grep, scan files).
- Do not assume. Verify.

Caveman check:
> "What go in? What come out? What go wrong?"

---

## 2. Minimal Impact Changes
- Fix only what is broken.
- Do not refactor unless required.
- Do not rename things unless necessary.
- Avoid touching unrelated files.

Rule:
> If it works -> leave it.

Caveman rule:
> "Good part -> no touch"

---

## 3. Trace the Flow
- Start from entry point (API, UI, CLI, job, etc.)
- Follow execution path step by step:
  - Input -> Processing -> Output
- Add logs if needed to see flow.

Caveman thinking:
> "Data go here -> then here -> then break here"

---

## 4. Reproduce First
- Reproduce bug before fixing.
- If cannot reproduce -> do not fix blindly.
- Create simple test or scenario:
  - Small input
  - Clear expected result

Caveman rule:
> "No see bug -> no fix bug"

---

## 5. Fix Strategy (Simple First)
- Try simplest fix first.
- Avoid complex abstractions.
- Prefer:
  - Inline fix
  - Guard clauses
  - Null/edge handling

Avoid:
- New frameworks
- Big rewrites
- Premature optimization

Caveman rule:
> "Small rock fix problem. No need big machine."

---

## 6. Validate Immediately
- After change:
  - Run tests (if exist)
  - Run app
  - Check logs
- Confirm:
  - Bug is gone
  - No new bug introduced

Caveman check:
> "Still broken? If yes -> fix again."

---

## 7. Use Existing Patterns
- Copy existing style in codebase.
- Follow:
  - Naming conventions
  - Folder structure
  - Error handling patterns

Rule:
> When in doubt -> mimic nearby code.

Caveman rule:
> "Other human do like this -> do same"

---

## 8. Safe Iteration
- One change at a time.
- Commit often (small commits).
- If broken -> revert fast.

Caveman rule:
> "Step. Check. Step. Check."

---

## 9. Edge Case Awareness
Check for:
- Null / undefined
- Empty values
- Large inputs
- Timeouts / async issues
- Race conditions

Caveman thinking:
> "What if nothing? What if too big? What if slow?"

---

## 10. Logging Over Guessing
- Add temporary logs to understand behavior.
- Remove or reduce after fix.

Caveman rule:
> "See truth in logs. Not in head."

---

## 11. Documentation Check
- Check:
  - README
  - Inline comments
  - API contracts
- If mismatch -> trust code more, but note inconsistency.

Caveman rule:
> "Docs lie sometimes. Code show truth."

---

## 12. Communication (If Human-in-the-Loop)
- Ask when:
  - Requirements unclear
  - Multiple valid approaches
- Provide:
  - What you found
  - What you plan to change
  - Why

Caveman style:
> "I see this. I do this. Because this."

---

## 13. Definition of Done
Task is done when:
- Bug fixed OR feature works
- No regressions observed
- Code matches project style
- Basic edge cases handled

Caveman check:
> "Work? Yes. Break others? No. Done."

---

## 14. Anti-Patterns (Avoid)
- Blind coding without reading
- Large refactors during bug fix
- Ignoring failing tests
- Adding unnecessary dependencies
- Overengineering simple logic

Caveman rule:
> "Too complex -> throw away"

---

## 15. Caveman Checklist
Before finishing:
- Problem understood? Yes
- Bug reproduced? Yes
- Minimal fix applied? Yes
- Works after change? Yes
- No extra breakage? Yes

If all Yes -> done.

---

## Final Mental Model
> "See problem. Track path. Fix small. Test fast. Leave cave clean."
