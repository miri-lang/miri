# Job — Extend the task tracker

The directory holds a working program in {{LANGUAGE}}: a task tracker that
reads commands from a file and reports what it did. Extend it.

The program's source is at `{{ENTRY_POINT}}`. Build and run it with
{{TOOLCHAIN}}.

## What it already does

The first command-line argument is the path of a text file holding one command
per line. Blank lines are ignored.

| Command | What it does |
|---|---|
| `add <title>` | Records a task, open, and writes `added <id>`. Ids start at 1 and count up. The title is the rest of the line and may hold spaces. |
| `done <id>` | Marks that task finished and writes `done <id>`; writes `no such task <id>` when no task has that id. |
| `list` | Writes every open task as `<id> <title>`, in the order the tasks were added. |

## What to add

1. `prio <id> <p>` sets a task's priority, where `p` is a digit from 0 to 9,
   and writes `prio <id> <p>`. A task nobody set a priority on has priority 5.
   An id no task has writes `no such task <id>`; a `p` outside 0 to 9 writes
   `bad priority <p>` and changes nothing.
2. `list` now orders the open tasks by priority, ascending, and breaks ties by
   id, ascending. Its line format does not change.
3. `list all` writes **every** task, finished ones included, under the same
   order, as `<mark> <id> <title>` — where the mark is `-` for an open task and
   `x` for a finished one.
4. A line whose first word is none of these commands writes `? <word>`, naming
   that first word. Today the program ignores such a line.

Everything the program already does keeps working exactly as it does now.

## Done

The job is finished when the program builds, the four additions behave as
written, and the original three commands are unchanged. There are no tests in
the directory; the contract above is the specification.
