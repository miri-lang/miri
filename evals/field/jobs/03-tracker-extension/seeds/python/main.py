#!/usr/bin/env python3

"""A small task tracker."""

import sys


class Task:
    def __init__(self, id, title):
        self.id = id
        self.title = title
        self.done = False


def find(tasks, id):
    for task in tasks:
        if task.id == id:
            return task
    return None


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
        elif command == "list":
            for task in tasks:
                if not task.done:
                    print(f"{task.id} {task.title}")


main()
