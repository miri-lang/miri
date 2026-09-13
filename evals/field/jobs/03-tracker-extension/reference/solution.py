#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Reference solution for the tracker-extension job.

This is the committed program with the four additions the brief asks for. It is
never copied into a subject's directory; it exists to generate what the hidden
tests expect.
"""

import sys


class Task:
    def __init__(self, id, title):
        self.id = id
        self.title = title
        self.done = False
        self.priority = 5


def find(tasks, id):
    for task in tasks:
        if task.id == id:
            return task
    return None


def ordered(tasks):
    return sorted(tasks, key=lambda task: (task.priority, task.id))


def set_priority(tasks, rest):
    id_text, _, priority_text = rest.partition(" ")
    task = find(tasks, int(id_text))
    if task is None:
        print(f"no such task {id_text}")
    elif len(priority_text) != 1 or priority_text < "0" or priority_text > "9":
        print(f"bad priority {priority_text}")
    else:
        task.priority = int(priority_text)
        print(f"prio {task.id} {task.priority}")


def main():
    lines = open(sys.argv[1], "rb").read().decode("utf-8").rstrip("\n").split("\n")
    tasks = []
    next_id = 1

    for line in lines:
        if line == "":
            continue
        parts = line.split(" ", 1)
        command = parts[0]
        rest = parts[1] if len(parts) > 1 else ""

        if command == "add":
            tasks.append(Task(next_id, rest))
            print(f"added {next_id}")
            next_id = next_id + 1
        elif command == "done":
            id = int(rest)
            task = find(tasks, id)
            if task is None:
                print(f"no such task {id}")
            else:
                task.done = True
                print(f"done {id}")
        elif command == "prio":
            set_priority(tasks, rest)
        elif command == "list" and rest == "all":
            for task in ordered(tasks):
                print(f"{'x' if task.done else '-'} {task.id} {task.title}")
        elif command == "list":
            for task in ordered(tasks):
                if not task.done:
                    print(f"{task.id} {task.title}")
        else:
            print(f"? {command}")


main()
