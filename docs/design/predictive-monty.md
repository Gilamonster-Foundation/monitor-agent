# Predictive Monty: forecasting, and the harness around it

> Status: **design record** · 2026-09-06 · the reference behind epics #23–#27.
> Scope: how Monty grows from a threshold alerter into something that predicts,
> what has to exist first, and the single measurement the whole thing is gated
> on.

## 0. The fantasy pitch

Written down as the target, not as a claim:

> *Introducing monitor-agent. Monty reads Prometheus and other data sources
> directly and uses them together with TimesFM to predict failures before they
> happen, and warns you in time to act. Future versions may take action on your
> behalf.*

It is one measurement away from being either true or marketing. Epic #24 is
that measurement, and epics #25–#27 are conditional on it.

## 1. TL;DR

Monty's README promises alerts "before problems become crises." The engine does
not do that, and cannot: `Condition::evaluate(&self, value: f64)`
(`monitor-core/src/alert.rs:38`) takes a **single scalar** — `GreaterThan`,
`LessThan`, `Equals`. There is no time in the alert model. And there is nothing
to give it — metric history is a `VecDeque<f64>` **capped at 60 samples**, in
RAM, for sparklines (`monitor-presence/src/model.rs:65`, `:96`), lost on
restart.

The plan, in one line each:

1. **Record** (#23 F1–F2) — a content-addressed journal, and alert firings
   beside it. Time-gated: three weeks of wall clock cannot be compressed.
2. **Measure** (#24) — does TimesFM beat a straight line *on our disks*? Kill
   criteria stated up front.
3. **Forecast** (#25) — only if it does. A forecast is a falsifiable claim,
   scored when its horizon elapses.
4. **Explain, propose, act** (#26, #27) — the agency ladder, one rung at a time.

The architectural move underneath: **Monty becomes Python that uses
newt-agent's Rust crates**, the shape `shea` proved on 2026-09-05. Not fashion
— TimesFM is a PyTorch library, so in a Python host it is `import timesfm3`,
in-process, and the HTTP sidecar and all its failure modes disappear.

## 2. Three passes, two of them dead

Recorded so they are not re-proposed.

**Pass 1 — Rust-native + HTTP sidecar.** Add `Condition::ForecastCrosses`, run
TimesFM behind FastAPI on dgx1, call it with `reqwest`.
**Died because:** there is nothing to forecast *from* (§3), and the sidecar
existed only to work around Rust not being able to `import torch` — which stops
being a constraint under Pass 3.

**Pass 2 — build retention and a content-addressed forecast store in
`monitor-core`.** Design a metric journal, a disk `NodeStore`, a forecast
record.
**Died because:** every one of those already exists (§4). This is precisely the
failure the `provenance-audit` skill's STEP ZERO was written for — grounding a
design in the code it *changes* is not the same as inventorying what is already
*available to it*.

**Pass 3 — below.**

## 3. Provenance audit — the repo as found

**Rung 0 of 5.** `grep -rn "blake3\|sha2\|content_addressable\|CID"` over the
workspace comes back clean.

| Question | Answer |
|---|---|
| Identity derived from bytes? | **No.** `Alert.uuid: Uuid` (`monitor-core/src/alert.rs:150`), minted `Uuid::new_v4()` (`:272`). Assigned and random. |
| History tamper-evident? | **No.** Nothing is persisted at all. |
| Evidence actually read? | **N/A** — no evidence exists. |
| Invertible? | **N/A.** |

The upside of Rung 0 is real: there are **zero existing violations to ratchet
down**. No `KNOWN_VIOLATIONS` list, no migration, no legacy dispatch arm. The
first persisted structure gets designed right once — which is what PR #22 does.

## 4. Inventory — what is being reused

| Need | Already exists | Where |
|---|---|---|
| Metric collection | `LocalCollector`, **`PrometheusCollector`**, `SshCollector`, `NatsCollector` | `monitor-collect/src/` |
| Bulk numeric store | `newt-data` — headless SQLite: CSV ingest, SQL, summarize | newt-agent, PyO3 |
| Content-addressed log | `MerkleLog` — crate-minted nodes, verified on read, replayable from HEAD | `shea/python/shea/merkle_log.py` |
| Identity primitives | `ContentId`, `RawContentId`, `MerkleNode`, `VerifiedStore`, `ClassifiedCid` | `content-addressable` 0.1.2 |
| Inner/outer harness | `Inner`, `Rail`, `Outer` with Goal Mode + `revise` | `shea/python/shea/loop.py` |
| Python-over-Rust pattern | PyO3 git-dep assembly, proven live 2026-09-05 | `shea` |
| Agent substrate | `newt-core`, `newt-inference`, `newt-tools`, `newt-eval`, `newt-acp-worker` | newt-agent |
| TUI components | `newtui` core — `Component`/`Flow`/`View`/`Explorer`, empty dep closure | newtui (§9) |
| Alert dispatch | `AlertDispatcher` — bell, voice, webhook, nats | `monitor-alert/` |
| Skins | `PresenceModel`/`DataEvent`/`OutputSink` fan-out | `monitor-presence/`, ROADMAP P1–P4a |

**Designed here — the gap only:** the metric journal (PR #22), the claim and
verdict records, the scoring loop, and the predictive alert condition.

## 5. Architecture

```
   ┌─────────────────── Python — the new work ───────────────────┐
   │  timesfm3 forecaster · scoring · ops harness (Inner/Outer)  │
   └──────────────────────────┬──────────────────────────────────┘
                              │ PyO3  (git deps on main, shea's policy)
   ┌──────────────────────────┴──────────────────────────────────┐
   │ newt-agent   newt-data (SQLite)  newt-core  newt-inference   │
   │              newt-eval  newt-tools  newt-acp-worker          │
   ├──────────────────────────────────────────────────────────────┤
   │ monitor-*    monitor-collect (Local/Prometheus/Ssh/Nats)      │
   │              monitor-core types · monitor-alert dispatchers   │
   │              monitor-journal (PR #22)                         │
   ├──────────────────────────────────────────────────────────────┤
   │ newtui       components + (eventually) chart widgets          │
   │ content-addressable   mints the Merkle nodes                  │
   └──────────────────────────────────────────────────────────────┘
```

Rust keeps what it is good at and what is already written. Python takes the
model, the statistics, and the loop. `monitor-*` gains `pyo3` features exactly
as newt's crates did; `newt-*` and `newtui` arrive as git deps on `main` under
shea's "source, not shelves" policy, with `Cargo.lock` as the ratchet.

This is ROADMAP §6's "Model A → B" arriving with a payload.

**A constraint that decides where code lives:** shea's Findings record that the
`content-addressable` **Python binding cannot encode a `ContentId` as a
dag-cbor link**, so Merkle nodes are minted in Rust behind `unstable-merkle`.
The claim and verdict types below are therefore Rust structs with a PyO3
surface, not Python dataclasses.

### 5.1 Two stores, two jobs

Do **not** put 16k floats in a Merkle log.

- **`newt-data` SQLite** — the bulk metric series. Cheap, queryable, decimatable.
- **Merkle log** — the *claims about* that series. Small rows, high value.

The link between them is a digest, not a copy: a claim carries a
`RawContentId` over the exact input window it was computed from, so which bytes
produced a prediction is provable without storing them twice.

## 6. The spine — a forecast is a falsifiable claim

A prediction is a statement about the future that **becomes checkable when the
future arrives**. That is the whole design.

```
ForecastClaim                    # MerkleNode, parent = prior claim for this series
  series        (target, metric_path)
  window_cid    RawContentId     # over the canonical bytes of the input window
  window_span   (t_start, t_end, n, cadence_s)
  model         { repo, revision, weights_cid }   # RawContentId over safetensors
  config_cid    ContentId
  horizon_s     u32
  issued_at     i64
  quantiles     [[f64; 9]; horizon]

ForecastVerdict                  # MerkleNode, parents = [claim_cid, prior verdict]
  claim_cid     ContentId
  actual_cid    RawContentId     # over what actually happened
  pinball       f64
  predicted     bool             # did we raise an alert
  crossed       bool             # did the threshold actually get crossed
  lead_time_s   Option<i64>
```

Four things fall out of that one pairing:

1. **The bake-off harness** — baselines run through the same machinery, so the
   experiment is not separate code.
2. **Trust** — calibration is measured continuously, not asserted at launch.
3. **The audit's third question** — the verifier sits on the production path
   *because scoring is the product*, not as a bolt-on nobody calls.
4. **The outer loop's input** — a series whose verdicts plateau at bad
   calibration is a harness problem (§9).

`weights_cid` is worth its line: because the model loads in-process we can
digest the safetensors at startup. `gilamonster-bench` found the dgx1 llama.cpp
router exposes no weights hash at all; here, "which model said this" is
answerable.

## 7. Foundations, and what each actually blocks

| | What | Blocks |
|---|---|---|
| **F1** | Persistence — the journal + daemon wiring | **Everything. Time-gated.** |
| **F2** | Incident labels — record alert firings | The measurement |
| **F3** | Claim/verdict record types, in Rust | #25 |
| **F4** | The Python host — `pyo3` on `monitor-*` | #25, #26 |
| **F5** | CI — there is **no `.github/workflows/` today** | Should precede F4 |
| **F6** | Upstream: shea's Python→newtui bridge; newtui's widgets | A *good* UI, not a forecast |
| **F7** | Deferred on purpose: authority model; TimesFM licensing | L4; shipping |

> **F1 + F2 are the critical path, and they are a data recorder.**
> F3–F5 are ordinary engineering that runs in parallel while it records.

The trap this ordering avoids: building the host, the records, the harness and
the UI first, *then* starting a three-week clock. That costs a month of
calendar for nothing, and front-loads exactly the work a negative #24 would
throw away.

## 8. The experiment, and the kill criteria

Contenders: seasonal-naive (t−1 week), linear extrapolation, EWMA/Holt,
TimesFM-2.5 (Apache-2.0, univariate, 2k ctx), TimesFM-3.0 (non-commercial,
multivariate + covariates, 16k ctx).

**The deciding metric is not MASE.** It is:

> **Median lead time on real incidents, at a fixed false-alarm budget**
> (proposed: ≤ 1 false alert per machine per week).

False alarms are how a monitoring tool gets muted, and a muted Monty has
negative value.

**The precise hypothesis:** disk fill is often a *step function* — a build
lands and 20 GB appears. Linear extrapolation fails exactly there. A foundation
model should win **if and only if** the steps are patterned (nightly builds,
weekly jobs). That is falsifiable about our machines specifically.

**Kill criteria:**

- No ≥2× lead-time improvement over linear + seasonal-naive at the same
  false-alarm budget → **stop.** Keep thresholds, close #25–#27.
- 2.5 ≈ 3.0 → **use 2.5**, and the licensing problem disappears.
- 3.0 wins *only* via covariates → escalate; the non-commercial license becomes
  a product decision rather than a footnote.

**Licensing.** TimesFM source is Apache-2.0; the **3.0 weights** ship under
`timesfm-non-commercial-license-v1.0` — non-commercial, non-production — and
the HF repo is gated. monitor-agent is Apache-2.0. 3.0 is fine for the
experiment; anything that ships needs 2.5 or another model.

## 9. Downstream design notes

**Degradation is mandatory.** Forecast conditions that cannot be evaluated must
fail silent while threshold conditions keep firing. A monitoring tool that goes
quiet because its model died is worse than one that never had a model.

**Quantiles map onto severity for free.** TimesFM returns 9 deciles: q0.9
crosses → Warn (plausible), q0.5 crosses → Critical (likely). Graded urgency a
fixed threshold cannot express. Ship `Condition::ForecastCrosses` **last**,
after calibration is shown.

**The outer loop.** shea proved the move live on 2026-09-05: a failed round,
a `revise` that escalated the model, a corrected round — three rows in a Merkle
log, chain verified. Monty's version is **claim → verdict → revision**. Read
NeMo Switchyard's escalation router before hand-rolling more of it
(`knowledge/board/homelab/2026-09-06_nvidia-pair-switchyard-PARKED.md`).

**newtui.** Its stated destination is *"a Grafana you can drive from a terminal,
over data sources you bring"* — a description of Monty. But the chart widgets
are *"Landing next"* (`newtui/src/lib.rs:8`) and shea's Python bridge is a link
smoke, so **Monty is the forcing function, not the beneficiary**: PR #19's
hand-rolled heatmap/heat-graph/heat-meter are the vocabulary newtui exists to
own, written a second time, and the move is to donate them upstream. Two
constraints: newtui's **LEAF invariant** (empty dependency closure, asserted by
`tests/leaf.rs`) is not negotiable, and newtui has its own merge authority and
release train.

## 10. The agency ladder

| Rung | Monty | Where |
|---|---|---|
| L0 Observe | Thresholds on present state | today |
| L1 Predict | Forecast + `forecast_gt`, with lead time and confidence | #25 |
| L2 Explain | Says *why*, from recorded provenance | #26 |
| L3 Propose | Suggests a remediation, does not run it | #26 (ROADMAP P6) |
| L4 Act | Runs pre-approved, narrowly-scoped remediations | #27 |

L4 is an authority question, not a feature, and the two candidate hosts
disagree today: `monitor-station` mints **read-only object-capability** identity
via `agent-mesh-protocol` (PR #2), while `shea` uses **ambient authority** and
delegates confinement to OpenShell. Both are defensible — they are two halves
of a deliberate competition. Whichever Monty inherits must be settled **before
any code that can write**.

## 11. Open decisions

1. **Repo shape.** Restructure in place, or split the Python host into a sibling
   repo (ROADMAP §6)? Sub-question: is it a third sibling to `shea`, or does
   Monty become a **profile of shea** — same host, different loops and tools?
   shea's charter is "customizable agentic loops, not only coder loops," and an
   ops loop is exactly the second loop that would test that claim.
   *Recommendation:* decide later — F1 needs none of it, and must not wait.
2. **Authority for L4.** OCAP or ambient + OpenShell. Before any write path.
3. **False-alarm budget.** Is 1/machine/week right? It sets the entire gate.
4. **Prometheus as the front door.** `PrometheusCollector` already exists —
   should F1 record *through* it uniformly rather than mixing collector types?
5. **newtui timing.** Donate PR #19's charts upstream now, or keep the
   hand-rolled versions until forecasting proves out? *Recommendation:* keep
   them; revisit when there is a calibration view worth drawing well.
