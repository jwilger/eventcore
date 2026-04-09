# Prefer Borrows Over Clones

Use references (`&T`) instead of `clone()` whenever the caller does not
need ownership of the value.

## The Rule

Before writing `.clone()`, ask: does the receiver need to own this value,
or can it borrow it?

- If the value is only read, pass `&T`
- If the value must be stored or moved, `clone()` is appropriate
- If the value implements `Copy`, cloning is free and either form is fine

## Common Unnecessary Clones

```rust
// Wrong: cloning just to pass to a function that reads
let name = self.name.clone();
do_something(&name);

// Right: borrow directly
do_something(&self.name);

// Wrong: cloning to compare
if self.email.clone().into_inner() == other {

// Right: borrow for comparison
if self.email.as_ref() == other {
// or define PartialEq to compare without unwrapping
```

## When Clone Is Appropriate

- Constructing a new struct that takes ownership
- Sending data across thread boundaries (`Send`)
- Event payloads that must be owned by the event
- Test code where clarity matters more than efficiency

## Why

Unnecessary clones waste allocations and obscure intent. A `clone()` call
signals "this value needs independent ownership" — when it doesn't, the
signal is misleading and the allocation is wasted.
