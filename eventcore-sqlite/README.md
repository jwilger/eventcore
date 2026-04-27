# eventcore-sqlite

SQLite backend for the [EventCore](https://github.com/jwilger/eventcore)
event-sourcing library, built on `rusqlite`.

## Cargo features

| Feature      | Default | What it does                                                                                                      |
| ------------ | ------- | ----------------------------------------------------------------------------------------------------------------- |
| `bundled`    | yes     | Vendors a vanilla SQLite C library through `rusqlite/bundled`. No system `libsqlite3` required.                   |
| `encryption` | no      | Enables SQLCipher with vendored OpenSSL via `rusqlite/bundled-sqlcipher-vendored-openssl` for at-rest encryption. |

The `encryption` feature pulls in native crypto code; only enable it if you
actually need encrypted databases. To link against a system-provided SQLite
or to bring your own `rusqlite` features, disable default features:

```toml
eventcore-sqlite = { version = "0.7", default-features = false }
```

When both `bundled` and `encryption` are active, `libsqlite3-sys` links
SQLCipher (which is itself a SQLite fork) — there is no link-time conflict,
but if you want encryption without also pulling in the vanilla vendored
SQLite source, disable defaults:

```toml
eventcore-sqlite = { version = "0.7", default-features = false, features = ["encryption"] }
```

## Version compatibility with `rusqlite`

`eventcore-sqlite` is built against a specific minor version of `rusqlite`
(currently `0.32.x`). The crate re-exports `rusqlite` at its crate root so
consumers do not need to declare a separate dependency:

```rust
use eventcore_sqlite::rusqlite;

let conn = rusqlite::Connection::open_in_memory()?;
```

Prefer the re-export over a direct `rusqlite` dependency. Cargo unifies
versions automatically when ranges overlap. If your declared range and
`eventcore-sqlite`'s range do not overlap, Cargo will resolve two
SemVer-incompatible copies of `rusqlite` (e.g. `0.31.x` and `0.32.x`) into
the dependency graph rather than failing resolution; the mismatch then
surfaces as a compile-time type error at the call site when you try to
hand a `Connection` from one version to an API that expects the other.
Using the re-export sidesteps the whole issue by guaranteeing you reference
the same `rusqlite` `eventcore-sqlite` was built against.

## Bring your own connection

For consumers that need fine-grained control over connection setup —
custom pragmas, attached databases, encryption keys configured at open
time, or pooling — use [`SqliteEventStore::from_connection`] (and the
matching constructor on `SqliteCheckpointStore`):

```rust
use eventcore_sqlite::{SqliteEventStore, rusqlite};

let conn = rusqlite::Connection::open("events.db")?;
// ...apply consumer-controlled pragmas here...
let store = SqliteEventStore::from_connection(conn);
store.migrate().await?;
```

The connection is taken as-is. The consumer is responsible for any pragmas
(journal mode, encryption key, etc.). If you want EventCore's default setup,
prefer [`SqliteEventStore::new`] with a `SqliteConfig` instead.
