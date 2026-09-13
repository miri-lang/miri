//! A small task tracker.

use std::env;
use std::fs;

struct Task {
    id: i64,
    title: String,
    done: bool,
}

fn find(tasks: &mut [Task], id: i64) -> Option<&mut Task> {
    tasks.iter_mut().find(|task| task.id == id)
}

fn main() {
    let path = env::args().nth(1).expect("a command file");
    let text = fs::read_to_string(&path).expect("a readable command file");
    let mut tasks: Vec<Task> = Vec::new();
    let mut next_id = 1;

    for line in text.trim_end_matches('\n').split('\n') {
        if line.is_empty() {
            continue;
        }
        let (command, rest) = match line.split_once(' ') {
            Some((command, rest)) => (command, rest),
            None => (line, ""),
        };

        if command == "add" {
            tasks.push(Task { id: next_id, title: rest.to_string(), done: false });
            println!("added {}", next_id);
            next_id += 1;
        } else if command == "done" {
            let id: i64 = rest.parse().expect("a task id");
            match find(&mut tasks, id) {
                None => println!("no such task {}", id),
                Some(task) => {
                    task.done = true;
                    println!("done {}", id);
                }
            }
        } else if command == "list" {
            for task in tasks.iter().filter(|task| !task.done) {
                println!("{} {}", task.id, task.title);
            }
        }
    }
}
