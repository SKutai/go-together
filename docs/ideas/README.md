# Ideas

Proposals, deferred decisions, and things considered and deliberately not
done. **Nothing in this folder describes how Go Together behaves.**

The rest of `docs/` is the settled record: `ARCHITECTURE.md`, `GAME_RULES.md`,
`SECURITY.md`, `TESTING.md` and `DISTRIBUTION.md` each describe the project as
it is, and a claim in one of them is meant to be true right now. That is what
makes them worth trusting and worth correcting when a change makes one stale.

This folder is the opposite: everything here is a maybe. Mixing the two would
cost the settled documents the property that makes them useful, which is why
these live apart rather than as a "Future work" section at the bottom of
`ARCHITECTURE.md`.

## The rule that matters

**Never cite a file here as evidence of what the game does.** If you are
reading this folder to find out how something works, you are in the wrong
place - the answer is in the settled documents, in a doc comment, or in the
code. A reviewer who finds an idea file quoted as justification for a change
should treat that as a defect in the change.

## What belongs here

- An idea that has been discussed and parked, with enough context that picking
  it up later does not mean rediscovering the whole discussion.
- A decision that was deliberately deferred, and what it is waiting on.
- An approach that was considered and rejected, and why - so nobody proposes
  it again without knowing what it costs.
- Constraints a future implementation would have to respect, where those are
  already known.

## What does not belong here

- Anything true about the current code. That goes in the settled documents or
  a doc comment.
- Work somebody is actually doing, with its assumptions, dead ends and loose
  ends. That belongs with the work itself, in its pull request.
- A bug. A bug is either fixed or filed as work, not parked as an idea.

## Writing one

Give each idea its own file, named for the thing rather than the ticket. Start
it with a status line carrying an absolute date, so a reader can tell how stale
the thinking is:

```
Status: deferred 2026-09-19. Waiting on a decision about the warning threshold.
```

Say plainly what is decided and what is not. An idea file whose reader cannot
tell which parts are settled is worse than no file. Where the idea rests on a
fact about the current code, cite the file so a future reader can check whether
it is still true - and expect that it might not be.

## Retiring one

When an idea is adopted, its reasoning moves into the settled document that
now describes the behaviour, and the idea file is deleted. When an idea is
abandoned for good, say so in the file and keep it: "we tried this and it does
not work" is worth more than silence, and it is the one case where a file here
earns its place indefinitely.

Nothing here is deleted on a schedule, but an idea file that has gone unread
and unreferenced for a long time is usually telling you something.
