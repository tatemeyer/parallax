# Recorded machines

One `<name>.json` per machine, each holding exactly what that machine's
probe would have served from `GET /state`. The file name is the machine
name, and the URL is synthesized from it — which is why a name here must
be a bare label. A dotted name or an address is refused at load, because
the only thing standing between a fixture run and a real host is this
directory listing.

## `<name>.control.json`

Optional, and its presence is the fixture-set spelling of
`--allow-control`. A machine with one can be *asked* to do something; a
machine without one is watched and not acted on, which is the default
here exactly as it is in a deployment.

```json
{
  "submit": { ... the body POST /action answers with ... },
  "status": { "<action id>": { ... the body GET /action/<id> answers with ... } }
}
```

Ids are `fixture-0-1`, `fixture-0-2`, and so on: a fixture cockpit
submits under a pinned client and run, so that a recording can name in
advance the id it is answering about.

## Why `pi5` answers with something nonsensical

**It is not a mistake, and it should not be corrected.**

`pi5.control.json` records `"result": "queued"`, which is not a reply
this version of the cockpit understands — it knows `accepted` and
`refused`. That is the point. A probe that answers something
unparseable has told the cockpit *nothing* about whether the action ran,
and the cockpit has a distinct and important behaviour for that case: it
reports the action's fate as **unknown**, marked `??`, rather than as a
failure.

The distinction is the reason the control arc exists. A failure means
nothing happened. An unknown means something may well have happened and
nobody can say — and an operator who is told "failed" when the truth is
"unknown" will retry a merge that already went through.

Recording a *plausible future* reply rather than random bytes is
deliberate too: this is what a probe one version ahead of its cockpit
would really send, which is the way this case actually turns up.

`tates-laptop` has no control file, so it is the machine the cockpit can
watch and not act on. Between the two, the fixture set covers both
answers a real deployment gives.
