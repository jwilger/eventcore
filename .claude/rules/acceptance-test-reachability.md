# Reachability Tests for Every Page

Every page or screen in the application must have at least one acceptance test
proving a user can reach it through natural browser navigation from the
application entry point.

## What Reachability Means

A page is "reachable" when a user starting at the application's root URL can
arrive at it through a sequence of visible interactions: clicking links,
submitting forms, following redirects, or being redirected after an action.

## What Is Required

For every page:

1. **A reachability test** that starts at the entry point (e.g., the setup
   token redemption screen for setup flows, the login screen for authenticated
   flows) and navigates to the page through the UI.
2. **A rendering test** that verifies the page displays the correct content
   for its state.

Direct-URL navigation in a test proves rendering but NOT reachability. Both
are needed.

## What This Catches

- Missing navigation links between pages
- Broken redirects after form submissions
- Orphaned pages that exist but have no path leading to them
- Incorrect route wiring that serves 404 for pages that should be linked

## Applies To

All pages, including:

- Primary workflow screens (setup, login, configuration)
- Error states and validation feedback
- Confirmation and success screens
- Redirect targets after form submissions

## Why

A page that renders correctly but cannot be reached through the UI is dead
code from the user's perspective. Reachability tests ensure the application
is navigable end-to-end, not just renderable page-by-page.
