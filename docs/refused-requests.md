# Refused requests

## Quick start

With the router [serving a catalog](../README.md#-serving), ask for a path it
has no endpoint for:

```sh
curl -i http://127.0.0.1:8080/not-an-endpoint
```

The reply has status `404` and a JSON error whose `code` is `path_not_found`.
Use the status and code to handle the failure; the message explains it to the
reader.

## Reference

A refusal comes before the router forwards anything to a model. It declares its
body length and closes the connection after the reply.

The caller may still be sending a request the router has refused to read. To
avoid discarding the reply through a connection reset, the router closes its
write side first, then reads and drops the remaining request bytes. This drain
is best effort. It stops at EOF, a read error, one mebibyte, or its one-second
deadline checked between reads. A blocked read also has a one-second timeout.
The router does not promise to consume the whole request after refusing it.

### Testing the drain

The drain loop takes a reader and an expiry predicate. Production supplies the
caller socket and a monotonic deadline. Unit tests hold expiry fixed while
reading to EOF, or supply the expiry event explicitly. Byte-bound and read-error
cases use controlled input. None of these tests needs a sender to finish within
a wall-clock window.

The on-wire refusal test checks that an oversized request head receives `431`
and that the reply ends cleanly rather than with a connection reset.

## Examples

Run the drain cases and the on-wire refusal case:

```sh
cargo nextest run -p maestro-model-router -E 'test(proxy::reply::tests::) | test(a_refused_request_ends_with_its_reply_rather_than_a_reset)'
```
