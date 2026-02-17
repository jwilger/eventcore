# EventCore Planning

**ALL planning, story tracking, and task management uses `dot` (dots CLI) and GitHub Issues.**

## Using dots for Planning

**View all tasks:**

```bash
dot ls
```

**View specific task details:**

```bash
dot show <task-id>
```

**Find ready work:**

```bash
dot ready
```

**Start working on a task:**

```bash
dot on <task-id>
```

**Complete a task:**

```bash
dot off <task-id> -r "Completed because..."
```

## For AI Assistants

- **DO NOT** maintain separate planning in Markdown files
- **DO** use `dot` commands for all task operations
- **DO** reference task IDs in commit messages and documentation
- **DO** query `dot ls` / `dot ready` for current status before starting work
