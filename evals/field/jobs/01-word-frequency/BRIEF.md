# Job — Word frequency

Write a program in {{LANGUAGE}} that reports the most frequent words in a text.

Start from an empty directory. The program's source lives at `{{ENTRY_POINT}}`.
Build and run it with {{TOOLCHAIN}}.

## Contract

- The first command-line argument is the path of a UTF-8 text file. Read it.
- A **word** is a maximal run of the characters `A`–`Z`, `a`–`z` and `0`–`9`.
  Every other byte separates words.
- Words are counted without regard to case and reported in lower case.
- Write the ten most frequent words to standard output, one per line: the word,
  a single space, then its count.
- Order by count, descending. Break ties by the word, ascending, comparing
  bytes.
- Fewer than ten distinct words: report every one of them. No words at all:
  write nothing.
- Write nothing else to standard output, and exit with status 0.

## Done

The job is finished when the program builds and satisfies the contract for any
input, not only the one you tried. There are no tests in the directory; the
contract above is the specification.
