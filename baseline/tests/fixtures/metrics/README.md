# Metrics fixtures

Two feeds, deliberately of different **shapes**. The shape is the point:
a metrics feed is not one thing, and the parser has to tell them apart.

## `loss.jsonl` — wide

One record per timestep; each numeric field is a metric.

```json
{"step": 0, "loss": 2.7183, "probe_acc": 0.11, "note": "warmup"}
```

Successive records are successive steps, so record order **is** an
order, and a curve drawn over it is a claim the data supports. Ragged by
design — `spectral_err` appears only in the last record, and `note` is a
string annotation — because a real producer emits both.

## `sweep.jsonl` — long

One record per *measurement*, with the metric's name in a field.

```json
{"issue": 69, "variant": "full", "seed": 0, "metric": "effective_rank", "value": 2.779}
```

**Not invented.** These are 27 real rows of
`projects/jepa/results.csv` from `tatemeyer/Model-Experiments`, the
`001-baseline-collapse-avoidance` experiment — the checked-in record
Arc 1 concluded in. Three metrics (`effective_rank`, `embedding_std`,
`probe_r2_superseded_104`) across three variants (`full`, `no_ema`,
`random_init`) and three seeds.

They are here rather than hand-written because the defects Slice 1 fixes
were found by running the shipping parser against this file, and a tidy
imitation would not have shown them:

- `issue` and `seed` are numeric, and nothing marks them as identifiers;
- `metric` and `variant` are strings, and they are the two columns the
  finding lives in;
- record order is the writing loop's nesting — metric, then variant,
  then seed — so a curve drawn over it charts the loop, not the data.

The values carry Arc 1's actual conclusion, which is a **null result**:
grouped by metric and variant, `full` (2.352..2.791) sits almost
entirely inside `random_init` (2.437..2.934) — the trained model is not
distinguishable from an untrained one on `effective_rank` — while
`no_ema` (1.250..1.459) separates cleanly below it. Tests assert those
bands by value, so a regression that flattens the grouping fails
loudly rather than rendering a plausible picture.
