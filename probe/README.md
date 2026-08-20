# parallax-probe

Serves one machine's platform state so a cockpit on another can see it.

Run it on every machine that holds projects. It scans that machine's own
disk with the adapters `parallax-baseline` already provides, aggregates
exactly as a local frontend would, and serves the result as
`parallax/v1` JSON. It draws nothing, decides nothing, and links no
cockpit — a test asserts the last part, because the probe has to run on
a Pi with no terminal at all.

Design:
[`docs/design/specs/parallax/2026-08-19-remote-hosts-and-the-probe-design.md`](../docs/design/specs/parallax/2026-08-19-remote-hosts-and-the-probe-design.md).

## Two rules worth knowing

**It binds `127.0.0.1` and refuses anything else.** There is no TLS, no
authentication, and no authorization in this binary — and that is
defensible only because nothing off the machine can open a socket to it.
`tailscale serve` terminates TLS and forwards to the loopback port, and
the tailnet decides who may reach *that*. A test asserts the refusal
against `0.0.0.0`, a LAN address, and a **`100.x` tailnet address** — the
last is the one that looks safe, and binding it would publish the probe
to the whole tailnet directly, bypassing `tailscale serve` and its ACLs.

**`GET /state` never runs a build.** Verification checks that produce
state by running something — `cargo test` and friends — are reported as
`NotRun`, naming the command that would run them. They are not silently
dropped: a check that vanished reads as "this project has no tests"
rather than "nobody has run them". The rule lives in `split_by_cost`, in
baseline, shared with the cockpit's refresh thread. A probe with its own
idea of which checks are safe to poll is how three machines end up able
to run `cargo test` on a fourth.

## Running it

```
parallax-probe --projects-root ~/Dev
tailscale serve --bg --https=443 http://127.0.0.1:8737
```

| flag | |
|---|---|
| `--projects-root <dir>` | every child holding a `parallax.yaml` is registered |
| `--registry <file>` | the roots a registry file lists |
| `--port <n>` | loopback port, default 8737 |
| `--peer <name>` | how this machine names itself, default its hostname |
| `--github-token <tok>` | falls back to `$GITHUB_TOKEN`, then `$GH_TOKEN` |

With neither `--projects-root` nor `--registry` it still starts and
serves an empty envelope. A machine with no projects yet is still a
machine the cockpit should be able to reach, and a probe that exited
would be indistinguishable from one that was never installed.

| route | |
|---|---|
| `GET /state` | everything this machine knows, aggregated |
| `GET /health` | liveness with no scan, so a slow probe and a dead one differ |

Check it locally before publishing it:

```
curl -s 127.0.0.1:8737/health          # -> ok
curl -s 127.0.0.1:8737/state | head -c 200
```

## Installing on the Pi

Build on the Pi. `SESH/deploy/README.md` explains why at length and the
reasoning is unchanged here: cross-compiling to aarch64 from Windows
means fighting a linker, and a Pi 5 builds this crate in a couple of
minutes.

```bash
cargo build --release -p parallax-probe
install -Dm755 target/release/parallax-probe ~/.local/bin/parallax-probe
install -Dm644 probe/deploy/parallax-probe.service \
  ~/.config/systemd/user/parallax-probe.service

systemctl --user daemon-reload
systemctl --user enable --now parallax-probe
sudo loginctl enable-linger "$USER"

tailscale serve --bg --https=443 http://127.0.0.1:8737
```

**`enable-linger` is the step that is easy to miss.** The probe is a
*user* unit, like `seshd`, because the projects it scans live in this
user's home. Without lingering it starts at login and stops at logout —
so the Pi answers only while somebody is signed in, and the failure
looks exactly like a network problem.

The unit is deliberately **not** tied to `graphical-session.target`,
which is how `seshd` is wired. `seshd` needs `WAYLAND_DISPLAY` because it
puts things on a television. The probe answers a cockpit that may be in
another room, and stopping it when the TV session ends would stop it
precisely when you are most likely to be looking at the machine from
somewhere else.

## Pointing a cockpit at it

[`deploy/registry.yaml`](deploy/registry.yaml) is a worked three-machine
example. In short:

```yaml
apiVersion: parallax/v1
projects:
  - root: C:/Users/tatem/Dev/Parallax
peers:
  - url: https://pi5.tail-scale.ts.net
```

```
panopticon --registry ~/.parallax/registry.yaml
```

Peers are URLs, not inventories. The Pi knows what is on the Pi, so a
project added there appears in the cockpit with no edit here.

A peer is named by its registry entry and never by what it answers with,
so a machine can be named before it has ever replied — and one probe
cannot present itself as another. Its projects show up qualified:
`sesh@pi5`.

**Nothing a peer sends can claim to be live.** A filesystem read is
`Live` on the machine that performed it and cannot be on any other, so
every observation is re-stamped as polled on receipt. An unreachable
machine keeps its rows and acquires a reason; the values it served last
time stay on screen and go stale on their own.
