# ADR-017: Reserved Characters for StreamId and StreamPrefix

## Status

accepted

## Context

EventCore's domain model uses `StreamId` as the aggregate identity type (ADR-012), representing business concepts like accounts, orders, or shopping carts. The subscription infrastructure uses `SubscriptionQuery` for projection queries (ADR-016), enabling filtering by stream prefixes to build read models.

**Current State:**

`StreamId` validation (from implementation) enforces:
- Non-empty (after trimming)
- Maximum 255 characters
- Leading/trailing whitespace sanitized via trim

`StreamPrefix` was recently added for subscription filtering, currently with no explicit validation beyond being a wrapped `String`.

**The Problem:**

To enable future POSIX glob pattern matching in `SubscriptionQuery` (e.g., `account-*`, `order-[0-9]*`), we need clarity on which characters are permitted in domain identifiers versus query patterns. Without this distinction, ambiguity arises:

- Does `account-*` mean the literal identifier "account-*" or a wildcard pattern matching "account-123", "account-456", etc.?
- Must we implement complex escaping mechanisms to represent literal asterisks in identifiers?
- Can projection queries unambiguously distinguish between literal prefixes and glob patterns?

**Key Forces:**

1. **Domain vs Infrastructure Separation**: `StreamId` is a domain concept (aggregate identity); `SubscriptionQuery` is infrastructure (projection queries). These should not leak concerns into each other.

2. **Parse, Don't Validate (ADR-003)**: Invalid characters should be rejected at construction time, not discovered during query execution.

3. **Future-Proofing**: Glob pattern support is planned but not yet implemented. Reserving characters now prevents breaking changes later.

4. **Developer Experience**: Simple, understandable rules. No complex escaping syntax required for common use cases.

5. **POSIX Glob Familiarity**: POSIX glob patterns (`*`, `?`, `[...]`) are widely understood, simpler than regex, and sufficient for stream filtering needs.

6. **Type Safety**: The type system should prevent invalid identifiers from being constructed, not rely on runtime checks scattered throughout the codebase.

**Why This Decision Now:**

`StreamPrefix` was just introduced for subscription filtering (I-017). Validation rules must be established before the API stabilizes and before glob pattern support is added in future work. Reserving characters now avoids breaking changes to StreamId validation later.

## Decision

Reserve glob metacharacters in `StreamId` and `StreamPrefix` to enable unambiguous future pattern matching:

1. **`StreamId` validation will reject glob metacharacters**: `*`, `?`, `[`, `]`
   - `StreamId::try_new("account-*")` returns `Err(StreamIdError::InvalidCharacter)`
   - Valid identifiers are literals only: `account-123`, `order-2024-12-10`, `cart_abc_def`

2. **`StreamPrefix` validation will mirror `StreamId` constraints**:
   - Non-empty (after trimming)
   - Maximum 255 characters
   - Leading/trailing whitespace sanitized
   - **No glob metacharacters** (`*`, `?`, `[`, `]`)
   - `StreamPrefix::try_new("account-")` is valid
   - `StreamPrefix::try_new("account-*")` returns error

3. **Future glob pattern support will use a distinct type**:
   - `SubscriptionQuery` will add methods accepting glob patterns (not yet implemented)
   - Pattern type (e.g., `StreamPattern`) will explicitly permit metacharacters
   - Clear separation: literals (`StreamId`, `StreamPrefix`) vs patterns (`StreamPattern`)

4. **Validation via nutype**:
   - Custom validation function checks for reserved characters
   - Errors provide clear feedback: "StreamId cannot contain glob metacharacters: *, ?, [, ]"

## Rationale

**Why Reserve Characters Now vs. Later:**

Adding character restrictions is a breaking change—existing code using prohibited characters would break. Reserving characters *before* API stabilization allows future glob support without breaking changes. The cost now (restricting some identifiers) is small; the cost later (breaking existing applications) is unacceptable.

**Why No Escaping Complexity:**

Alternative approaches (e.g., "use `\*` for literal asterisk") add cognitive overhead, parsing complexity, and error-prone API surface. Event sourcing identifiers are typically structured strings like UUIDs, hierarchical paths (`tenant/account/123`), or composite keys (`order-2024-12-10-001`)—none require glob metacharacters.

If a legitimate use case emerges for literal asterisks in identifiers, that's a signal to revisit the identifier structure, not to complicate the API with escaping.

**Why POSIX Glob Over Regex:**

POSIX glob patterns are:
- **Simpler**: `account-*` is immediately understandable; regex equivalents are cryptic
- **Sufficient**: Stream filtering needs basic wildcard matching, not full regex power
- **Standard**: Developers already know glob patterns from file systems and shells
- **Safer**: Limited syntax prevents catastrophic backtracking and regex injection concerns

Regex would add complexity without corresponding value for projection query use cases.

**Why StreamPrefix Mirrors StreamId Validation:**

`StreamPrefix` represents a literal prefix for filtering—it's conceptually a partial `StreamId`. Diverging validation rules would create confusion:

- Why can `StreamId` contain character X but `StreamPrefix` cannot (or vice versa)?
- Mental model clarity: "prefixes are beginnings of stream IDs"

Consistent validation maintains conceptual alignment and prevents edge cases where prefixes cannot match any valid StreamId.

**Why Separate Pattern Type in Future:**

By distinguishing literal prefixes (`StreamPrefix`) from glob patterns (`StreamPattern`), the type system makes intent explicit:

- `filter_stream_prefix(StreamPrefix::try_new("account-")?)` filters literally
- `filter_stream_pattern(StreamPattern::new("account-*"))` matches wildcard (future)

Developers cannot accidentally confuse literal filtering with pattern matching. The API guides correct usage through types (ADR-003 principle).

**Trade-offs Accepted:**

- **Restricted Identifier Space**: Some character combinations are prohibited in StreamId
  - _Acceptable because_: Event sourcing identifiers rarely need glob metacharacters; structured formats (UUIDs, timestamps, hierarchical paths) are standard practice

- **Deferred Pattern Implementation**: Glob pattern matching is designed for but not implemented now
  - _Acceptable because_: Literal prefix filtering (already implemented) covers immediate needs; patterns are an enhancement, not a requirement

- **Potential Future Incompatibility**: If unforeseen use cases require reserved characters
  - _Acceptable because_: Rare edge cases can be addressed via alternative identifier structures; the common case (no metacharacters needed) is optimized

**Alignment with EventCore Principles:**

- **Type-Driven Development (ADR-003)**: Invalid characters rejected at construction; types prevent illegal states
- **Domain-First Design (ADR-012)**: `StreamId` remains pure domain concept; infrastructure patterns confined to query types
- **Parse, Don't Validate**: Single validation at creation; type system guarantees validity thereafter

## Consequences

### Positive

- **Future-Proof API**: Glob pattern support can be added without breaking changes to `StreamId` or `StreamPrefix` validation
- **Unambiguous Semantics**: Clear distinction between literal identifiers (domain) and query patterns (infrastructure)
- **No Escaping Complexity**: Developers never need to escape metacharacters in identifiers; simpler mental model
- **Type-Safe Filtering**: Compile-time prevention of invalid patterns; IDE autocomplete guides valid usage
- **Consistent Validation**: `StreamId` and `StreamPrefix` have aligned constraints; conceptual clarity maintained
- **Parse-Don't-Validate**: Invalid characters caught at construction; no runtime query parsing failures

### Negative

- **Restricted Identifier Characters**: Applications cannot use `*`, `?`, `[`, `]` in stream identifiers
  - _Mitigation_: These characters are rarely needed; structured identifiers (UUIDs, timestamps, paths) remain fully supported

- **Breaking Change Risk**: If currently-valid identifiers use reserved characters, migration required
  - _Mitigation_: Early adoption phase; few (if any) existing users; better now than after widespread use

- **Deferred Pattern Matching**: Full glob support not implemented in this decision
  - _Mitigation_: Literal prefix filtering (already implemented) covers immediate projection needs; patterns are enhancement

### Enabled Future Decisions

- **Glob Pattern Implementation**: Add `StreamPattern` type and `filter_stream_pattern()` method to `SubscriptionQuery` using reserved metacharacters
- **Advanced Pattern Syntax**: Extend pattern support to brace expansion (`{account,order}-*`), character classes (`[a-z]*`), or negation (`[!0-9]*`)
- **Pattern Compilation**: Pre-compile patterns for efficient matching in high-throughput subscriptions
- **Pattern-Based Indexing**: Backends can optimize storage indexes knowing identifier character constraints

### Constrained Future Decisions

- **Metacharacters Permanently Reserved**: `*`, `?`, `[`, `]` cannot be reclaimed for literal use in `StreamId` or `StreamPrefix` without breaking change
- **StreamId Validation Cannot Relax**: Removing character restrictions would break pattern matching semantics
- **Pattern Type Required**: Glob patterns must use distinct type (cannot reuse `StreamPrefix` for patterns)

## Alternatives Considered

### Alternative 1: No Character Restrictions (Defer to Pattern Implementation)

**Description**: Allow all characters in `StreamId` and `StreamPrefix`; implement escaping when glob patterns are added.

**Why Rejected**:

- Escaping adds complexity: developers must remember to escape metacharacters in literals
- Error-prone: forgetting to escape `*` causes unintended wildcard matching
- API surface bloat: need both escaped and unescaped variants of filtering methods
- Cognitive overhead: "When do I escape? How do I escape?"
- Runtime parsing failures: escaped syntax errors discovered during query execution, not construction

Complexity imposed on every API consumer to solve a problem most will never encounter.

### Alternative 2: Use Regex Instead of POSIX Glob

**Description**: Reserve regex metacharacters (`.`, `*`, `+`, `?`, `^`, `$`, `[`, `]`, `(`, `)`, `{`, `}`, `|`, `\`) and implement regex-based pattern matching.

**Why Rejected**:

- **Over-engineering**: Stream filtering needs simple wildcards, not full regex power
- **Larger reserved character set**: Prohibits more identifier characters (`.`, `-` often used in stream IDs)
- **Catastrophic backtracking risk**: Poorly-constructed regex patterns can hang backend queries
- **Regex injection concerns**: User-provided patterns could exploit regex engine vulnerabilities
- **Steeper learning curve**: Regex is powerful but cryptic; glob is simple and familiar
- **Overkill**: No clear use cases require regex-level matching for stream subscriptions

POSIX glob provides 80% of the value with 20% of the complexity.

### Alternative 3: Context-Dependent Parsing (Magic Strings)

**Description**: Use single string API where context determines interpretation: `subscribe("account-123")` is literal, `subscribe("account-*")` is pattern.

**Why Rejected**:

- **Ambiguity**: Is `order-*` a literal stream ID or a wildcard pattern? Cannot determine from type alone.
- **Error-prone**: Typos in identifiers could accidentally create patterns (or vice versa)
- **No compile-time safety**: All validation deferred to runtime query parsing
- **IDE cannot help**: Autocomplete cannot distinguish literal vs pattern context
- **Violates ADR-003**: Type system should make intent explicit; magic strings hide it

Type-based separation (`StreamPrefix` vs `StreamPattern`) eliminates ambiguity.

### Alternative 4: Different Validation for StreamId vs StreamPrefix

**Description**: Allow metacharacters in `StreamId` but prohibit in `StreamPrefix` (or vice versa).

**Why Rejected**:

- **Conceptual mismatch**: Prefixes are beginnings of stream IDs; why would validation differ?
- **Edge case bugs**: Prefix could never match any valid StreamId (if validation diverges)
- **Mental model confusion**: Developers must remember two different character rules
- **Inconsistent API**: Same conceptual thing (stream identifier) has different constraints
- **No clear benefit**: Divergence adds complexity without solving a real problem

Consistency maintains clarity and prevents subtle bugs.

### Alternative 5: Runtime Validation Only (No Construction-Time Checks)

**Description**: Accept any characters in `StreamId`/`StreamPrefix`; validate during query execution.

**Why Rejected**:

- **Violates Parse-Don't-Validate (ADR-003)**: Core EventCore principle is validation at boundaries
- **Late error discovery**: Invalid identifiers discovered during query execution, not construction
- **Repeated validation overhead**: Must check on every query operation
- **Poor error messages**: Failures occur deep in subscription logic, not at clear API boundary
- **Type system underutilized**: Rust's strength is compile-time safety; runtime checks waste this

Construction-time validation catches errors early when they're easiest to fix.

## References

- **ADR-003**: Type System Patterns for Domain Safety (parse-don't-validate principle)
- **ADR-012**: Event Trait for Domain-First Design (StreamId as aggregate identity)
- **ADR-016**: Event Subscription Model (SubscriptionQuery for projection filtering)
- **POSIX Glob Pattern Syntax**: IEEE Std 1003.1-2017 (glob pattern matching specification)
- **I-017**: Subscription Foundation (implementation context for StreamPrefix addition)
