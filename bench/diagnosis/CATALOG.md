# Mechanisms catalog

Every mechanism a benchmark loss has been traced to, with the cells that
showed it and where the work stands. A new diagnosis starts here: a loss
whose evidence matches an entry is that entry until shown otherwise. An entry
is added by the engineer who names a mechanism in a cell's `DIAGNOSIS.md`, in
the same change that files the work.

| Mechanism | What it is | Cells | Evidence | Status |
|---|---|---|---|---|
| `sort-quadratic` | A sort that makes n² comparisons: the native `List.sort`/`Array.sort` was an insertion sort, and `sorted_by` a bubble sort. | list-sort | 250000 numbers: 54 s against 50 ms in C, all in `numbers.sort()`; 499500 comparator calls for 1000 elements | Fixed: `d24a122a` (native sort), `9898ff07` (`sorted_by`) |
| `string-append-copies` | `text = text + piece` allocates a new string and copies all of `text`, though `text` is the only reference to it, so building a string piece by piece is quadratic. | string-builder (blocked), json-roundtrip | Doubling the pieces quadruples the time; writing a JSON document back: 140 ms at 3500 records, 8577 ms at 35000; json-roundtrip at S is 94× C, with 52 % of its samples in `memmove` and `String.concat` its hottest Miri function | Filed |
| `cow-copy-per-step` | A list rebound through a function that writes into it (`bodies = advance(bodies, dt)`, `var bodies = start` inside) is copied on every call instead of being written in place. | nbody | About 7 allocations per step where the algorithm makes none (10.5 M at 1.5 M steps); `miri_rt_list_clone`, `malloc` and `free` in the hot run's profile | Under diagnosis |
| `shared-check-per-field-write` | Every write into a field of a list element asks the runtime whether the list is shared. | nbody | `miri_rt_rc_is_shared` is 13 % of the hot run's samples | Filed |
| `growth-without-capacity` | A list whose final length the algorithm knows grows by doubling, because Miri has no way to reserve it; on the system allocator the freed buffers of each growth inflate the peak. | sieve (hot), list-sort, wordfreq, json-roundtrip, k-nucleotide | sieve hot mode peaks at 140 MB against 17 MB cold; a C program growing a buffer the same way peaks at 146 MB | Filed |
