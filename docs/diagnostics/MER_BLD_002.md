## Rule

Repairs are classified by safety level to protect you from unintended changes. The `miri fix --apply` command automatically applies only repairs that are safe to apply unconditionally: format-only, behavior-preserving, or local-edit. Repairs classified as api-changing, target-changing, or requires-human-review are refused unless you explicitly approve them with the `--allow-risky` flag.

An api-changing repair modifies a public API surface that other functions and modules observe — changing a module-scope binding from immutable to mutable affects callers. A target-changing repair alters where or how the program runs (GPU residency, target capability, scalar width). A requires-human-review repair is ambiguous or carries sufficient risk that a human should inspect the edit before it applies.

## Messages

- `Refused: Repairs Require Human Review`

## Help

- `Pass --allow-risky to apply these repairs anyway.`

## Before

`main.mi`:

```miri
let counter = 0

fn bump()
    counter = counter + 1

bump()
println(f"{counter}")
```

```sh
miri fix --apply --yes main.mi
```

The repair would make the module-scope `counter` mutable, which changes a surface other code observes, so it is refused.

## After

Review the proposed repair, then approve it explicitly:

```sh
miri fix main.mi
miri fix --apply --yes --allow-risky main.mi
```

## Reference

[Build and Command Line](../reference/build.md)
