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

Enabling `encryption` together with `bundled` is unsupported — pick one
bundled variant. Most consumers should disable defaults when opting in to
encryption:

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

Prefer the re-export over a direct `rusqlite` dependency. Cargo will unify
versions automatically when both crates declare compatible ranges; if the
ranges do not overlap, Cargo emits a clear compile-time error rather than
linking two incompatible copies.

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
