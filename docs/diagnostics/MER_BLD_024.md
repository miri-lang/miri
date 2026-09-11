## Rule

`miri agent` read a message from stdin that was not a frame: a
`Content-Length: N` header line, a blank line, then exactly N bytes of UTF-8
JSON. Every message on the session's stdin must take that shape; there is no
other way to say where one message ends and the next begins.

The session refuses the message rather than skipping it. Line-delimited JSON is
the framing a client reaches for first, and a session that read it, found no
frame, and exited 0 would have told the client its message was handled. So a
message that is not framed ends the session: what was framed before it is
answered, the fault is reported on stderr under this code, and the process exits
1. Nothing else can be done with the stream, because after such a message
nothing says where the next one starts.

## Messages

- `a message arrived without a Content-Length header: the line `{line}` is not a header`
- `input ended inside a message's headers, before the blank line that ends them`
- `a message's headers ended without a Content-Length this session can read`
- `a header line ran past the {limit} bytes this session reads`
- `a message declared {length} bytes, over the {limit} byte limit`
- `input ended before the {length} bytes a message declared had arrived`
- `a message body is not UTF-8`

## Before

```sh
# Line-delimited JSON: no length header, so no frame.
echo '{"jsonrpc":"2.0","id":1,"method":"initialize"}' | miri agent
# error[MER_BLD_024]: Message Not Framed
# exit status 1
```

## After

```sh
# Frame the message: the header, a blank line, then exactly that many bytes.
printf 'Content-Length: 46\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize"}' | miri agent

# Or drive the session with the reference client, which frames for you.
python3 tools/agent_client.py --help
```

## Reference

[Build and Command Line](../reference/build.md)
