## Rule

The compiler encountered an internal inconsistency while setting up GPU launch metadata (grid and block dimensions). This occurs when constructing launch descriptors for GPU kernels and the dimension values fail validation. Reaching this error indicates a compiler bug.

## Before

This error has no source-level reproduction; it indicates an internal GPU launch setup failure.

## After

The launch dimensions in the source are not what failed — the descriptor built
from them was. Report it with a reproduction:

```sh
miri build program.mi
```

Attach the smallest kernel and launch that still trigger the error, together with
the grid and block dimensions used. As a workaround, try expressing the launch
bounds as plain literals rather than computed or constant-folded expressions;
the descriptor is assembled from those values, and a simpler form often avoids
the path that fails validation.

## Reference

[MIR and Lowering](../reference/mir.md)
