## Rule

Repairs are classified by safety level to protect you from unintended changes. The `miri fix --apply` command automatically applies only repairs that are safe to apply unconditionally: format-only, behavior-preserving, or local-edit. Repairs classified as api-changing, target-changing, or requires-human-review are refused unless you explicitly approve them with the `--allow-risky` flag.

An api-changing repair modifies a surface other functions and modules observe — renaming a parameter changes the names callers pass arguments by, and making a public module-scope binding mutable changes what its importers may do with it. A target-changing repair alters where or how the program runs (GPU residency, target capability, scalar width). A requires-human-review repair is ambiguous or carries sufficient risk that a human should inspect the edit before it applies.

## Messages

- `Refused: Repairs Require Human Review`

## Help

- `Pass --allow-risky to apply these repairs anyway.`

## Before

`main.mi`:

```miri
fn greet(name String)
    println("hi")

fn main()
    greet("a")
```

```sh
miri fix --apply --yes main.mi
```

The repair for the unread parameter would rename `name` to `_name`. A caller can pass that argument by name (`greet(name: "a")`), so the rename changes the function's signature and is refused.

## After

Review the proposed repair, then approve it explicitly:

```sh
miri fix main.mi
miri fix --apply --yes --allow-risky main.mi
```

## Reference

[Build and Command Line](../reference/build.md)
