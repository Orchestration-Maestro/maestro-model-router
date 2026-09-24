# Security rules in `maestro-model-router`

`maestro-model-router` follows the organization's [security
rules](https://github.com/Orchestration-Maestro/.github/blob/main/golden-rules/security.md).
This page is its rule map (C-001): for every rule, what holds it here, or why it
does not apply. A row may name a stricter local rule; none weakens one.

The organization's `scripts/golden-rules.py` writes the rows from the golden
rules and keeps what each row says here. A rule added there arrives as "Not
mapped yet", and the drift check fails until it is mapped.

## What this repository protects

The machine's memory and the model processes it starts. The router launches
`llama-server` children on loopback ports, holds them within a memory budget and
relays requests and streamed replies between clients and those children.
Untrusted input enters as the catalog, request bodies and the children's
replies.

## Rule map

| Rule | Held here by |
| --- | --- |
| SEC-001 Minimise sensitive data | Review: requests and replies are relayed, not stored |
| SEC-002 Treat input as data | Code: a request's model name only selects an entry of the validated catalog; a child's reply is relayed, never followed as an instruction |
| SEC-003 Validate boundaries | Code: children listen on loopback ports only; catalog paths resolve at run time |
| SEC-004 Use real authority | Organization: rulesets, workflow permissions and the organization bot's own App identity; automation borrows no person's credentials |
| SEC-005 Scope sensitive approvals | Review: a publication, release or settings change is approved in its own pull request |
| SEC-006 Inspect code safely | Code: the router starts only `llama-server` and the memory probes it names, with fixed arguments |
| SEC-007 Stop and escalate incidents | Review: a suspected exposure stops the work and goes to SECURITY.md's private channel; a leaked secret is revoked and rotated |
| SEC-008 Keep truthful evidence | Review: results are reported as run, with what was not checked |
| SEC-009 Preserve safe progress | Review: blocked work is reported as partial, never as done |
| SEC-010 Report vulnerabilities privately | Organization: private vulnerability reporting is on (`maestrolabs-baseline`), and SECURITY.md routes reports to it |
| SEC-011 Sign every release | Not applicable: no release is published yet |
