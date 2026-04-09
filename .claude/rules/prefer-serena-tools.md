# Prefer Serena Semantic Tools

When exploring, navigating, or modifying code, use serena's semantic tools
instead of reading entire files or grepping blindly.

## Tool Selection

| Task                          | Use                                            | Not                                |
| ----------------------------- | ---------------------------------------------- | ---------------------------------- |
| Understand file structure     | `get_symbols_overview`                         | `Read` the whole file              |
| Find a function/type          | `find_symbol`                                  | `Grep` or `Read` + scan            |
| Read a specific function body | `find_symbol` with `include_body: true`        | `Read` the whole file              |
| Modify a function body        | `replace_symbol_body`                          | `Edit` with large old_string       |
| Find callers/references       | `find_referencing_symbols`                     | `Grep` for the name                |
| Insert code near a symbol     | `insert_before_symbol` / `insert_after_symbol` | `Edit` with surrounding context    |
| Rename across codebase        | `rename_symbol`                                | `Edit` with `replace_all` per file |

## When Raw Tools Are Appropriate

- **Read**: When you need the full file in context to Edit it, or for small
  files (< 30 lines), config files, feature files, or non-code files.
- **Edit**: When the change is not localized to a symbol body (e.g., adding
  imports, modifying match arms across multiple functions, editing non-code).
- **Grep/Glob**: When searching for patterns that are not symbol names (e.g.,
  string literals, comments, config values).

## Why

Serena tools provide precise, scope-aware navigation that avoids flooding
context with irrelevant code. Reading entire files wastes context window and
makes it harder to stay focused on the relevant code path.
