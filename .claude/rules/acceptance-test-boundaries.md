# Acceptance Test Boundaries

Acceptance tests must exercise the system from the actor's perspective. The
actor is either a human using a browser or a human using a CLI.

## UI Features

Step definitions for UI-facing features MUST use Playwright browser automation.
The test opens a real browser, navigates pages, fills forms, clicks buttons,
and asserts on visible page content.

Raw HTTP client calls (reqwest, curl, fetch, etc.) against internal endpoints
are NOT acceptance tests for UI features. They may exist as supplementary
integration tests, but they do not satisfy the acceptance test requirement.

## CLI Features

Step definitions for CLI features MUST invoke the CLI binary as a subprocess
and assert on stdout, stderr, and exit codes. This is the real boundary a
CLI user interacts with.

## The Litmus Test

> "Could a real user perform this action the way the test does?"

If a real user would open a browser and fill out a form, the test must open a
browser and fill out a form. If the test posts JSON to an internal API
endpoint, it is not testing the user's experience.

## API-Level Tests

Tests that call internal HTTP endpoints directly are integration tests. They
are permitted and sometimes valuable, but they:

- Do NOT count as acceptance tests for user-visible features
- Do NOT replace the requirement for a Playwright-based acceptance test
- Should be used only when testing internal API contracts that are not
  exercised through the UI

## Why

The gap between "the API works" and "the user can do the thing" is where
bugs hide. Missing routes, broken form actions, unlinked pages, and
JavaScript errors are invisible to API-level tests. Playwright tests catch
all of these because they exercise the same path a real user takes.
