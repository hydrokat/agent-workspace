# Context History: 2026-04-28 Supabase Migrations and Lint Fixes

## 1. Supabase Migrations "Relation does not exist" Fix
During the setup of Supabase migrations, a `relation "profiles" does not exist` error occurred when executing the `01_create_teams.sql` migration, despite `profiles` being created in the `00_init_schema.sql` migration.

**Root Cause:**
The active database connection executing the migrations lacked the `public` schema in its default `search_path`, or the `anon`/`authenticated`/`service_role` API roles had lost `USAGE` and `ALL` access on the `public` schema.

**Resolution:**
To ensure the migrations are robust against different database connection settings and environments:
- We created a new migration `20260426190308_fix_permissions_and_schema.sql`.
- This migration explicitly executes `GRANT USAGE ON SCHEMA public TO anon, authenticated, service_role;` and `GRANT ALL` on all tables, routines, and sequences.
- It also uses `ALTER DEFAULT PRIVILEGES` to ensure future objects inherit these permissions.
- It uses `ALTER TABLE ... SET SCHEMA public` to explicitly set the schema of existing tables, preventing search path ambiguity.

## 2. React `useEffect` Cascading Renders (`react-hooks/set-state-in-effect`)
A lint error was raised in `TaskManagement` due to `setState` being called synchronously within a `useEffect` body. This causes cascading renders and hurts performance.

**Resolution:**
We replaced the `useEffect` hook with the "set state during render" pattern (recommended by React for adjusting state based on prop/param changes). 
- We initialize the state directly from search parameters.
- We keep track of previous parameters in a state variable `prevParams`.
- During render, if the parameters change compared to `prevParams`, we update `prevParams` and the target state (e.g. `setIsDialogOpen(true)`). React will then re-render immediately before painting, avoiding cascading render cycles.

## 3. Strict Typing (`no-explicit-any`)
Multiple test files contained `any` types which violated strict typing rules.

**Resolution:**
We replaced `any` with `unknown` or `Record<string, unknown>[]` depending on the expected shape of the data. This enforces stricter type assertions down the line and adheres to the project's engineering standards.
