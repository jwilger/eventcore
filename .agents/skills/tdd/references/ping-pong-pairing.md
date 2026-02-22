# Ping-Pong TDD Pairing Protocol

For use when persistent agent teams are available. Two engineers alternate
driver (writes failing test) and navigator (makes it pass) roles.

## Pair Selection

Track pairing history in `.team/pairing-history.json`. Neither of the last 2
pairings may repeat. If only 2 engineers exist, this constraint is relaxed.

**Schema** (`.team/pairing-history.json`):

```json
{
  "pairings": [
    {
      "driver": "name",
      "navigator": "name",
      "slice": "slice-id",
      "date": "ISO-date"
    }
  ]
}
```

Create this file if it does not exist. Append a new entry when a session begins.

## Session Lifecycle

1. **Establish the pair session.** Create a persistent pair for the vertical
   slice (e.g., `pair-<slice-id>`). Both engineers are activated once and
   persist for the entire TDD cycle.
2. **Bootstrap both engineers** with full initial context:
   - Their persona profile
   - The scenario being implemented (GWT acceptance criteria)
   - Current codebase context (file paths, test output, domain types)
   - Starting role assignment (driver or navigator)
   - The ping-pong protocol
3. **Engineers work and hand off directly** via structured messages. The
   orchestrator does NOT relay handoffs between engineers.
4. **Orchestrator monitors** via task updates and status notifications.
   Intervenes only for: external clarification routing, blocking
   disagreements, or workflow gate verification.
5. **End the session** when the acceptance test passes and the slice is
   complete.

## Ping-Pong Rhythm

1. **Engineer A (driver)** writes a failing test (RED step).
2. **Both engineers discuss** domain concerns (DOMAIN step). Each reviews
   within their own context and exchanges findings via structured messages.
3. **Engineer B (navigator)** either:
   - (a) Writes minimal green implementation (GREEN step), OR
   - (b) Drills down with a lower-level failing test if the next green step
     is not obvious.
     The engineer who writes green also performs the refactor step.
4. **Roles swap.** B becomes driver, A becomes navigator.
5. Repeat until the acceptance test passes.

## Structured Handoff Messages

When roles swap, the outgoing driver sends a message containing:

- **Failing test:** name and file path
- **Intent:** what behavior the test specifies
- **Domain context:** relevant constraints or decisions from domain discussion
- **Current output:** exact test failure or error message
- **New role assignment:** who is now driver, who is navigator

Because both engineers persist, the handoff provides the delta -- not a full
re-bootstrap.

## Drill-Down Ownership

When the navigator drills down instead of going green, roles swap at that
level too:

1. Navigator writes a lower-level failing test (now driver at this level).
2. Original driver writes the green for it (now navigator at this level).
3. When this level goes green, pop back up with swapped roles.

The person who wrote a failing test never writes its green implementation.

## Hub-and-Spoke Topology

The orchestrator manages the pair directly. No intermediate coordination
layer. All external clarification requests from the pair route through the
orchestrator to the appropriate team role or the user.
