## Rule

A list was changed by the closure it was being sorted with. The runtime sorts by calling the comparator for pairs of elements, and holds the elements apart from the list while it does, so a comparator that reads the list sees it empty. A comparator that writes the list writes a copy, which replaces the list where the program holds it, and one that assigns another list replaces it outright; either way the sorted elements would reach no one, so the program stops here instead of dropping them unseen. `sorted_by` sorts a copy the comparator cannot reach, so only a `runtime` declaration of the sort handed a list the comparator can reach meets this.

## Messages

- `List Changed While Sorted`
- `the list was changed by the closure it was being sorted with`

## Before

```miri
use system.collections.list

runtime "core" fn miri_rt_list_sort_by(l [int], f fn(int, int) int)

class Log
    var l List<int>

fn noted(log Log, a int, b int) int
    log.l.push(a)
    return a - b

fn main()
    let log = Log(l: List([3, 1, 2]))
    miri_rt_list_sort_by(log.l, fn(a int, b int) int: noted(log, a, b))
    println(f"{log.l.length()}")
```

## After

```miri
use system.collections.list

class Log
    var l List<int>

fn noted(log Log, a int, b int) int
    log.l.push(a)
    return a - b

fn main()
    let log = Log(l: List([3, 1, 2]))
    let sorted = log.l.sorted_by(fn(a int, b int) int: noted(log, a, b))
    println(f"{sorted[0]} {log.l.length() > 3}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
