---
description: Capture the scenarios this change touches and run the blinded multi-lens review, producing a GO / NO-GO / HOLD verdict.
argument-hint: [--scenario <name>]
---

Run Plumb's blinded multi-lens visual review by invoking the
`visual-review` skill now, and follow its orchestration procedure
exactly — do not shortcut, reorder, or reimplement any of its steps
here.

If this command was invoked with `--scenario <name>` in `$ARGUMENTS`,
pass that scenario name through to the skill's Select step so only
that scenario is reviewed, ignoring `touches`. Otherwise let the skill
select scenarios from the current branch's diff against its merge
base, as its procedure describes.
