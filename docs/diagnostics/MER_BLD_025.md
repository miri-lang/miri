## Rule

`miri test` finished without discovering a single test. Nothing was compiled and
nothing was executed, so the run has no verdict to give.

That is reported as a failure rather than as a passing suite of zero tests. A
run that discovers nothing looks exactly like a run that discovered everything
and found it all correct: both print no failures. The difference matters most to
whoever is least able to see it — an author who guessed at the syntax for
declaring a test, wrote a file, and would otherwise be told their guess worked.

The message distinguishes the two ways a run can come up empty. Having read
files and found no `@test` in any of them means the tests are not declared the
way the runner recognises. Having read no files at all means the path names a
directory that holds no `.mi` source, which is usually a path typed for a
different tree.

A file that declares tests and cannot be run is not this: it is reported by name
under `not run:`, because tests were discovered in it.

## Messages

- `read {count} .mi files, none of which declares a '@test' function`
- `read 1 .mi file, which declares no '@test' function`
- `found no .mi files to read`

## Help

- `a test is a function carrying the '@test' attribute; write '@test' on the line above its 'fn', and point 'miri test' at the file or directory that holds it`

## Before

```sh
# plain.mi declares functions, none of them marked @test
miri test --dir .
# error[MER_BLD_025]: No Tests Discovered
# exit status 2
```

## After

```sh
# Mark the function the runner should call.
#   @test
#   fn test_adds()
#       assert(1 + 1 == 2)
miri test --dir .
```

## Reference

[Build and Command Line](../reference/build.md)
