## Rule

`miri view --fn <NAME> --around <TEXT>` returned a region no smaller than the
whole declaration, so the anchor narrowed nothing.

Narrowing walks the blocks inside a function and returns the innermost one that
holds the anchor text. Two anchors have no smaller block to return: text that
only the signature holds belongs to no block at all, and text at the top level
of the body is held by the body, which is the whole function.

The read still answers, because the region it found is the honest one. What is
reported is that the answer is wider than the request implies. A read that
silently returned all of `main` under the name of a narrowed read is the reason
this warning exists: nothing in the returned text says how much of the function
came back.

## Messages

- `` `{anchor}` did not narrow the read: no block holds it, so the whole declaration came back ``
- `` `{anchor}` did not narrow the read: it sits at the top level of the body, so the whole body came back ``

## Before

```sh
# `float` occurs only in the signature, so no block holds it.
miri view app.mi --fn Inventory.total_value --around "float"
# warning[MER_BLD_023]: Read Could Not Be Narrowed
```

## After

```sh
# Anchor inside the block you want to see.
miri view app.mi --fn Inventory.total_value --around "total = total + it.price"

# Or read the declaration whole, with the file's own bytes.
miri view app.mi --fn Inventory.total_value --raw
```

## Reference

[Build and Command Line](../reference/build.md)
