# The `model-experiments` fixture project

A recorded copy of a real checkout, and the first project in this
fixture set whose **output** is the point rather than its development.
Every other project here answers a question about software being built;
this one answers what an experiment concluded.

## What is recorded, and from where

| File | Recorded from |
|---|---|
| `parallax.yaml` | `tatemeyer/Model-Experiments@main:parallax.yaml`, verbatim |
| `projects/jepa/results.jsonl` | that repository's checked-in feed, verbatim — 318 records |
| `github/issues.json` | the repository's seven open issues, trimmed to the fields the work adapter reads |
| `github/pulls.json` | empty, because it was |

Verbatim matters for the first two. This project is here because the
pane had only ever been shown feeds written to exercise it, and two
defects survived that — see below.

## Why the feed is JSONL and not the CSV

`results.csv` is that project's record of truth and `results.jsonl` is a
*projection* of it: `mx_viz.feed` writes the JSONL, and a test in that
repository fails CI if the two drift. So the feed is real, backed, and
guarded.

It exists at all because of a defect in **this** repository, argued out
in `Model-Experiments#112` and filed here as
[#53](https://github.com/tatemeyer/parallax/issues/53):
`ArtifactAdapterKind` is a closed enum, so `adapter: csv` does not
deserialize, and the only way to declare a long-format CSV was to
transcode it and check in a second copy. The projection is provisional.
When #53 lands, that repository can point at `results.csv` and delete
both the projection and its sync test.

## What it caught

The feed is not tidy. 318 records become 106 series across six metrics
with ragged coverage — a record carries whichever parameters its
experiment varied, so `momentum` is a dimension in `002` and absent from
`001`. Two things the pane shipped with did not survive contact with it:

- **Dimension labels ran 80 to 197 characters**, so a row rendered as a
  truncated list of parameters with the numbers and the band pushed off
  the right edge. Every row of a metric began with the same characters
  and none of them showed a measurement.
- **The pane could not be moved through.** `detail_len` counted feeds,
  of which there is one, so `j` did nothing; and the detail list draws
  from the top, so 88 of the 113 lines were unreachable and nothing said
  so.

- **A band narrower than one cell rendered as a `├` and no `┤`** — an
  interval with a left end and no right one, which reads as running off
  the row. Several of `002`'s cells are that tight on a scale that spans
  the whole metric. They now get `┼`, a single mark, which is what is
  true at that resolution.

All three are fixed in the slice that added this directory, and
`panopticon/tests/real_feed.rs` asserts against this recording so they
cannot come back quietly. None of the three was visible against a
nine-record fixture, which is the whole argument for recording a real
one.

## Why the producer age reads `unknown`

A checked-in file's modification time is whenever the clone happened.
That is later than this set's frozen `clock.txt` and unknowable in
advance, so the pane says **unknown** rather than inventing a duration
— which is the rendering the foreign-clock slice exists to make
possible. `0s` would put a producer that has not run in years at the top
of the screen as the freshest thing on it.

There is no `clock.txt` for a producer, and there should not be one: the
age of a *file* is not something a fixture set can freeze.
