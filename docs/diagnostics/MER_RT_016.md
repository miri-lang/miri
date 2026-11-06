## Rule

A list was changed by the code it was being sorted with: the closure handed to the sort, or the `compare` of the elements `sort()` orders. The runtime sorts by calling the comparator for pairs of elements, and holds the elements apart from the list while it does, so a comparator that reads the list sees it empty. A comparator that writes the list writes a copy, which replaces the list where the program holds it, and one that assigns another list replaces it outright; either way the sorted elements would reach no one, so the program stops here instead of dropping them unseen. `sorted_by` sorts a copy the comparator cannot reach. An element's `compare` reaches the list only through an element that refers back to the object holding it; `sort()` on such a list meets this when `compare` changes the list, as does a `runtime` declaration of the sort handed a list its closure can reach.

## Messages

- `List Changed While Sorted`
- `the list was changed by the closure it was being sorted with`
- `the list was changed by the compare of an element it was being sorted by`

## Before

```miri
use system.collections.list

@writes("l")
@closure("f, 2, l")
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
