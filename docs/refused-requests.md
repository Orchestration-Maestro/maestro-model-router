# Refused requests

## Quick start

With the router [serving a catalog](../README.md#-serving), ask for a path it
has no endpoint for:

```sh
curl -i http://127.0.0.1:8080/not-an-endpoint
```

The reply has status `404` and a JSON error whose `code` is `path_not_found`.
Handle the failure by its status and code; the message is for the reader.

## Reference

A refusal comes before the router forwards anything to a model. The reply
declares its body length, and the router closes the connection after it. The
[README](../README.md) lists every cause with its status and code.

### The rest of a refused request

The router often refuses before it has read the whole request: a head larger
than 64 KiB, or a body larger than it takes. Closing a connection that still
holds unread bytes resets it. A client that receives the reset may discard the
reply, and a Windows client does discard it. So the router closes its write side
once the reply is written, which tells the caller the reply is complete, then
reads and drops what the caller sends after it. The router reads to its end a
caller that has already finished sending and closed its side.

The drain is best effort and bounded. It stops at the end of the request, at a
read error, after one mebibyte, or after one second, and a single read waits at
most one second. The router resets a caller that sends more than that.

## Examples

Send a head larger than the router reads:

```sh
curl -i -H "X-Big: $(head -c 70000 /dev/zero | tr '\0' x)" http://127.0.0.1:8080/v1/models
```

The reply has status `431` and `code` `request_head_too_large`, and the
connection ends after it rather than with a reset.
