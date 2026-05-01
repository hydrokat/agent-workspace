# Task 601: Setup Playwright & Local Supabase Test Harness

## Description
Establish the Playwright configuration and the global setup/teardown scripts required to run tests against a localized, isolated Supabase instance for the Nexus application. This harness must spin up Supabase, run the migrations/seeds, execute tests, and cleanly shut down and remove temporary data.

## Objectives
- [ ] Install and configure Playwright in the Nexus web application (`task-tracker-web`).
- [ ] Create a Playwright global setup script to execute `npx supabase start` and apply necessary migrations/seeds for a clean state.
- [ ] Create a Playwright global teardown script to execute `npx supabase stop` and delete/cleanup any temporary test database files.
- [ ] Configure Playwright to automatically capture screenshots on test failures and define a utility for capturing screenshots on significant actions.
- [ ] Verify the harness by running a simple dummy test.

## Status
- [ ] Pending
