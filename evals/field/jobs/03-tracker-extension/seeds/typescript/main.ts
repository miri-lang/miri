// A small task tracker.

class Task {
  id: number;
  title: string;
  done: boolean;

  constructor(id: number, title: string) {
    this.id = id;
    this.title = title;
    this.done = false;
  }
}

function find(tasks: Task[], id: number): Task | undefined {
  return tasks.find((task) => task.id == id);
}

function main() {
  const text = Deno.readTextFileSync(Deno.args[0]);
  const tasks: Task[] = [];
  let next_id = 1;

  for (const line of text.replace(/\n+$/, "").split("\n")) {
    if (line == "") {
      continue;
    }
    const space = line.indexOf(" ");
    const command = space < 0 ? line : line.slice(0, space);
    const rest = space < 0 ? "" : line.slice(space + 1);

    if (command == "add") {
      tasks.push(new Task(next_id, rest));
      console.log(`added ${next_id}`);
      next_id = next_id + 1;
    } else if (command == "done") {
      const id = Number(rest);
      const task = find(tasks, id);
      if (task === undefined) {
        console.log(`no such task ${id}`);
      } else {
        task.done = true;
        console.log(`done ${id}`);
      }
    } else if (command == "list") {
      for (const task of tasks) {
        if (!task.done) {
          console.log(`${task.id} ${task.title}`);
        }
      }
    }
  }
}

main();
