# Best Practices: React State and Strict TypeScript

## 1. Avoid Cascading Renders (`react-hooks/set-state-in-effect`)

Setting state synchronously inside a `useEffect` can trigger a cascading render cycle, causing performance degradation and unexpected bugs. This commonly happens when syncing state with prop changes or URL search parameters.

**Anti-Pattern:**
```tsx
const [isOpen, setIsOpen] = useState(false);
const autoOpen = searchParams.get("autoOpen");

// This causes an extra render cycle
useEffect(() => {
  if (autoOpen === "true") {
    setIsOpen(true);
  }
}, [autoOpen]);
```

**Best Practice (Set State During Render):**
React recommends adjusting state based on props or external values directly during the render phase. This way, React catches the state update *before* painting the DOM.

```tsx
const autoOpen = searchParams.get("autoOpen");
const [isOpen, setIsOpen] = useState(autoOpen === "true");
const [prevAutoOpen, setPrevAutoOpen] = useState(autoOpen);

if (autoOpen !== prevAutoOpen) {
  // Update the tracker
  setPrevAutoOpen(autoOpen);
  // Set the new state synchronously
  if (autoOpen === "true") {
    setIsOpen(true);
  }
}
```
This pattern is safer and avoids the `react-hooks/set-state-in-effect` linting error.

## 2. Strict Typing vs `any`
Using `any` disables TypeScript's safety features and defeats the purpose of the type system. The `no-explicit-any` linting rule enforces this.

**Anti-Pattern:**
```ts
const processData = (data: any) => { ... }
```

**Best Practice:**
Always prefer `unknown` over `any`. When using `unknown`, TypeScript forces you to explicitly assert or narrow the type before you can access its properties. For more specific complex structures where you don't know the exact schema, use `Record<string, unknown>`.

```ts
const processData = (data: unknown) => {
    if (typeof data === 'object' && data !== null) {
        // Safe casting or type guards here
    }
}

// For mocked APIs or responses:
type MockResponse = { data: Record<string, unknown>[], error: null };
```
