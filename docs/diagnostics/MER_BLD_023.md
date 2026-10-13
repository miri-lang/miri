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

- `` `{anchor}` did not narrow the read: {reason} ``

## Help

- `anchor on text inside the block you want, such as a statement within a loop or a branch`

## Before

`app.mi`:

```miri
use system.collections.list

class Item
    var price float

class Inventory
    var items List<Item>

    fn total_value() float
        var total = 0.0
        for it in self.items
            total = total + it.price
        return total
```

```sh
miri view app.mi --fn Inventory.total_value --around "float"
```

`float` occurs only in the signature, so no block holds it.

## After

```sh
# Anchor inside the block you want to see.
miri view app.mi --fn Inventory.total_value --around "total = total + it.price"

# Or read the declaration whole, with the file's own bytes.
miri view app.mi --fn Inventory.total_value --raw
```

## Reference

[Build and Command Line](../reference/build.md)
