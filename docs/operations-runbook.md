# Operations runbook: exe.dev deployment, isolation, backup, daily use

Prepared for Robert Guss · September 14, 2026 · Design for discussion, not an
implemented system

"Two supervised processes" and "test restoring backups" are requirements, not a
plan. This document is the plan. Items marked **verify** were not checked on the
target VM and must be confirmed before implementation depends on them.

## 1. Target and layout

Target: one exe.dev VM, single owner. exe.dev's
[HTTP proxy documentation](https://exe.dev/docs/proxy) states that
`https://<vmname>.exe.xyz/` is **private by default**: only users with access to
the VM can reach it, and first-time visitors are redirected to log into exe.dev.
Ports 3000–9999 are forwarded to VM-access users at
`https://<vmname>.exe.xyz:<port>/`; only one port can be made public. Requests
arrive with `X-Forwarded-Proto`, `X-Forwarded-Host`, and `X-Forwarded-For`.
exe.dev also documents
[HTTPS tokens for VMs](https://exe.dev/docs/https-tokens-for-vms) for
programmatic access; its exact semantics were not reviewed here (**verify**).

Filesystem layout, separating what changes for different reasons:

| Path                        | Contents                                              | Backed up      | Owner                  |
| --------------------------- | ----------------------------------------------------- | -------------- | ---------------------- |
| `/opt/research/bin/`        | Pinned release binary, versioned subdirectories       | No             | root                   |
| `/opt/research/tools/`      | Pinned Python/Docling env, renderer, agent CLI        | No             | root                   |
| `/etc/research/config.toml` | Non-secret configuration                              | Yes            | root, 0644             |
| `/etc/research/secrets.env` | Provider keys, Telegram token, API bearer             | Yes, encrypted | root, 0600             |
| `/var/lib/research/db/`     | SQLite database (WAL mode)                            | Yes            | `research`             |
| `/var/lib/research/store/`  | Content-addressed sources, extracts, report revisions | Yes            | `research`             |
| `/var/lib/research/work/`   | Per-job scratch; deleted on completion                | No             | `research`             |
| `/var/lib/research/agent/`  | The agent-CLI worker's HOME and auth (see section 3)  | Yes, encrypted | `research-agent`, 0700 |
| `/var/log/research/`        | Structured logs, rotated                              | No             | `research`             |

## 2. Supervision and resource limits

The VM's init system is expected to be systemd (**verify**). Two units from one
binary:

- `research-api.service`: `research serve`. Restart always. `MemoryMax` sized
  for the API only.
- `research-worker.service`: `research worker`. Restart always. Separate
  `MemoryMax` and `CPUQuota`; its subprocesses (Docling, renderer, agent CLI)
  run inside the worker's cgroup and are bounded by it, plus their own
  per-process limits set at spawn.
- Both units read `EnvironmentFile=/etc/research/secrets.env`; the worker passes
  an **allow-listed** subset to each child, never its whole environment.

Boot behaviour: the API runs migrations and refuses to start on a schema it does
not know. The worker refuses to start if the API's schema version differs from
its own. Both log their version, config hash, and instruction version at
startup.

Disk-full behaviour: the worker checks free space before claiming heavy-lane
work and refuses below a threshold (2 GB;
[D-015](decisions.md#d-015-operating-numbers)), entering all queued heavy jobs
into `blocked` with reason `disk`. The API keeps serving reads. A Telegram
message is sent once, not per job.

Upgrade procedure:

1. Place the new binary in a new versioned directory.
2. `research worker drain`: the worker finishes or checkpoints heavy-lane tasks,
   keeps the control lane running, then exits when heavy work is empty or a
   timeout passes.
3. Restart the API on the new binary (runs migrations).
4. Restart the worker on the new binary. Runs that were `running` at drain
   resume as a new attempt and record the version change.
5. Run the regression subset from the
   [evaluation harness](evaluation-harness.md#5-regression-runs) if the
   instruction file, adapters, or renderer changed.

Keep the previous binary directory for rollback. Migrations are forward-only;
rollback of a migration requires restore from backup.

## 3. Agent-CLI worker isolation

"Dedicated job workspace" is not isolation. When the official-CLI backend is
enabled:

- A separate OS user `research-agent` with its own HOME at
  `/var/lib/research/agent/`. Robert's own HOME, shell history, SSH keys, and
  credentials are not visible to it.
- The CLI's saved authentication lives in that HOME with mode 0700. It is
  treated as a password: backed up encrypted, never logged, never copied into a
  job workspace.
- Child environment is an explicit allow-list: `PATH`, `HOME`, locale, the
  run-scoped service credential, and nothing else. `ANTHROPIC_API_KEY`,
  `OPENAI_API_KEY`, Exa and Telegram secrets are absent by construction, not by
  hoping they were unset.
- The run-scoped service credential is minted per attempt, can only call the
  retrieval and report-submit endpoints for its own run, and expires with the
  lease.
- Filesystem access: the job workspace, read-only access to the pinned tools,
  and nothing else. Network egress is the same policy as the fetcher.
- The CLI version is pinned. Upgrades go through a canary: run the regression
  subset on the new version with the worker pointed at a scratch database before
  the pin moves.
- Parser and renderer children (Docling, the HTML→PDF renderer) run under a
  third user, `research-tools`, with **no network and no credentials**: no
  service environment, no secrets directory, no database path, outbound traffic
  denied at the systemd unit (`IPAddressDeny=any`; **verify** the exe.dev kernel
  and systemd version support it, otherwise a network namespace). They receive
  one input file and one output directory.
- Every completion manifest a child writes is validated before the parent
  references anything from it: each path resolves inside the output directory
  after following symlinks, is a regular file, is under the declared size limit,
  is not open for writing by any process, and matches its stated hash. Failure
  is a job failure with a named reason, not a warning.

Failure handling tests that must exist before this backend is enabled:

| Scenario                             | Expected result                                                             |
| ------------------------------------ | --------------------------------------------------------------------------- |
| Saved auth expired                   | Run `blocked`, reason `reauth`; Telegram says which CLI and how to fix      |
| Quota or rate limit exhausted        | Run `blocked`, reason `quota`; no account switch, no API-key fallback       |
| CLI prompts for permission           | Detected as a hang, killed at timeout, run `failed` with a clear diagnostic |
| CLI hangs silently                   | Heartbeat expires, process group killed, attempt marked, run `failed`       |
| Malformed or partial JSON output     | Preserved verbatim; run `failed` with `output_format` error, not retried    |
| Output format changed after upgrade  | Caught by canary regression, pin does not move                              |
| Billing mode is API not subscription | Detected at startup by an explicit check; worker refuses to run             |

## 4. Backup, restore, and what recovery means

Objectives ([D-015](decisions.md#d-015-operating-numbers)):

- **Recovery point objective:** completed reports and their evidence within 1
  hour; everything else within 24 hours.
- **Recovery time objective:** a working service with all completed reports
  within 2 hours on a fresh VM.

Mechanism:

- Do not `cp` the live SQLite file. Take a consistent snapshot with
  `VACUUM INTO` or the online backup API into `/var/lib/research/backup/`, then
  ship it.
- Before snapshotting, pause artifact garbage collection so every file the
  snapshot references still exists; resume after the artifact sync completes.
- Sync `store/` (immutable, content-addressed, so incremental sync is cheap) and
  the snapshot with **restic** to a **Cloudflare R2** bucket over its S3 API
  ([D-012](decisions.md#d-012-backup-destination-and-key-custody)). restic
  encrypts client-side before anything leaves the VM, deduplicates the store,
  and enforces the retention rule with `forget --prune`.
- The VM's R2 token is **Object Read & Write scoped to this one bucket**. R2 has
  no write-without-delete permission, so a compromised VM could issue deletes;
  what stops them is a **bucket lock**: a 30-day retention rule on the `data/`,
  `snapshots/`, `keys/`, and `secrets/` prefixes, and an indefinite rule on
  `config`. `locks/` and `index/` are left unlocked because restic deletes and
  rewrites them on every run; a deleted index is rebuilt with
  `restic repair index`, and a deleted lock file is harmless. The lock is
  shorter than the 180-day retention, so `forget` only ever removes snapshots
  that are already unlocked.
- `forget --prune` runs **monthly from Robert's machine**, not from the VM.
  Prune deletes packs made obsolete by repacking; a pack written inside the lock
  window cannot be deleted until it expires, so the first restore rehearsal must
  run `forget --prune` against the locked bucket and confirm it completes
  (**verify**; if it fails on locked packs, lengthen the prune interval past the
  lock window).
- Hourly: snapshot plus incremental `store/` sync. Daily: full verification that
  every artifact referenced by the snapshot exists in the remote.
- Secrets and the agent HOME are backed up separately, encrypted with a
  different key, on a different schedule.

A restored service starts in **reconciliation mode**. The snapshot is up to an
hour old, so it can contain jobs that were `running`, `unknown`, or waiting to
collect a hosted result at snapshot time, and whose real outcome happened after
it. In reconciliation mode the worker:

1. Makes no paid submission of any kind, including standalone `search` and
   `sources` calls.
2. Lists every job not in a terminal state and, for each, asks the provider
   about the stored external task ID or idempotency key: completed (collect the
   result if it is still retrievable), still running (adopt and poll), gone or
   never seen (mark `unknown` for Robert to `reconcile`).
3. Runs `research verify-store` and marks any report whose artifacts are missing
   from the store as `artifacts: missing` rather than deleting the report.
4. Sends one Telegram summary: how many jobs were adopted, collected, marked
   unknown, and how many reports lost artifacts.
5. Leaves reconciliation mode only when an operator runs
   `research reconcile --clear-restore`, which is recorded as an event.

Any job that remains `unknown` after this stays blocked with the explicit
possibility of an existing paid task, exactly as in the
[contracts](run-and-report-contracts.md#unknown-is-resolved-by-reconciliation-not-by-resume).

Restore rehearsal: once before the first slice is declared in daily use, and
after any change to the layout, perform a restore on a clean VM from the remote
only, then run `research verify-store` (every referenced hash present, every
report renders) and open three reports on the phone. The rehearsal snapshot must
include at least one job that was `running` when the snapshot was taken, so
reconciliation mode is exercised and not only the happy path. Record the elapsed
time against the objective.

## 5. Phone workflow

The architecture's default Telegram message is a job ID and a status, and the
API is behind bearer auth over SSH. That is not a finished daily loop.

- The API serves a small **read-only report page** per run at `/r/{run_id}`:
  label, summary, limitations, the answered and unanswered questions, source
  list with retrieval times, and a PDF download. No editing, no administration.
- The page is reached through exe.dev's private proxy on a forwarded port, so
  Robert's phone browser logs into exe.dev once and then opens links directly.
  The service additionally checks `X-Forwarded-Host` matches its configured host
  and rejects requests that did not arrive via the proxy (**verify** how to
  distinguish proxied from direct traffic on the VM's network).
- The Telegram message contains the run ID, the label, a one-line status, and
  the report page link. Never a bearer token in the URL. Whether the message
  also carries the report title or a summary is a per-context setting, default
  off, because run titles can be sensitive.
- Tested end to end on Robert's phone: Telegram notification → tap → exe.dev
  login if needed → report page → PDF opens. This is an acceptance criterion for
  the first slice, not a later polish item.

Agents continue to use the bearer-authenticated API and CLI over SSH or the
proxy; the report page is for the human.

## 6. Operator view

Structured logs are for debugging. Daily operation needs one screen. Both the
CLI (`research dashboard`) and the report page's index (`/r/`) render
`GET /v1/summary`:

- Runs in the last 7 days: ID, brief title, execution state, completeness,
  label, last progress time.
- Anything `blocked` or `unknown`, with its reason and the one command that
  resolves it.
- Spend today and this month against caps, with unknown spend as its own line.
- Worker health: last heartbeat, heavy-lane occupancy, free disk.
- Last successful backup and last successful restore rehearsal.

A dead VM cannot report its own death. An external heartbeat check (any uptime
service pinging `/healthz` through the proxy, or a scheduled job on another
machine) alerts by Telegram if the service is unreachable for more than a
configurable window. This is required before the service replaces the incumbent.

## 7. Observability minimum

Per request and per job: request ID, run ID, job ID, attempt, backend, provider
timing, estimated and reported spend, queue delay, heavy-lane wait, extraction
warnings, resource-limit events, and child exit status. UTC in storage;
America/New_York on display when the client asks.

Never logged by default: source content, prompts, briefs, credentials, Telegram
message bodies. A debug flag can enable prompt logging for one run, recorded on
the run.

## 8. Testing strategy

| Layer                 | Method                                                                                                                                                                                                                                                               |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Job lifecycle         | Deterministic fake provider with scripted responses: accept-then-crash, duplicate completion, throttling, cancel races, malformed results. Every state transition in the [contracts](run-and-report-contracts.md) has a test that a wrong implementation would fail. |
| Fencing               | Two workers, one lease: the stale one must be unable to publish                                                                                                                                                                                                      |
| Publication atomicity | Kill between file rename and DB commit; orphan sweep finds the file; no report references a missing file                                                                                                                                                             |
| SSRF                  | Fixture DNS and redirect chains to private, link-local, and metadata ranges; all rejected at connect time                                                                                                                                                            |
| Source versions       | Same URL, changed content → new version; old evidence still resolves to old version                                                                                                                                                                                  |
| Envelope and label    | Label computed correctly from every combination of completeness, review, and open clarification                                                                                                                                                                      |
| Rendering             | Golden HTML/PDF fixtures for wide tables, footnotes, page breaks, the label header; visual diff on change                                                                                                                                                            |
| Target VM             | Restart during fetch, parse, export, poll; OOM of a child; disk full; drain and upgrade                                                                                                                                                                              |
| Live providers        | Budgeted, explicitly authorised integration runs; never in the default test suite                                                                                                                                                                                    |

The deterministic tests below must pass completely before the first slice is
declared in daily use. Each names a wrong implementation that would fail it.
Most come from the independent review and were adopted unchanged.

| Test                                                                 | Required behaviour                                                                                           |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Evidence contains a real quote but not the claimed conclusion        | Mechanical check passes; the claim stays `unsupported` until a review record qualifies or rejects it         |
| Material table value lacks support                                   | Report label is `draft` or `needs_review`; the cell is visibly unresolved in HTML and PDF                    |
| Question requires missing geography or version                       | `clarification` block or a recorded assumption; never an invented locale                                     |
| Hosted answer returns citations but no quotations                    | Provider provenance kept as `urls_only`; no passage evidence is fabricated                                   |
| Provider accepts a task and the worker dies before saving its ID     | Reconciliation finds it or the run enters `unknown`; no blind resubmission                                   |
| Old worker calls report or evidence APIs after losing its lease      | Every write rejected by the attempt-epoch credential                                                         |
| Local cancellation races with remote completion                      | External outcome preserved; charge and result status honest                                                  |
| Deadline arrives and the provider has no partial output              | Honest `partial` status with what exists; no fabricated report                                               |
| Publication interrupted between revision write and notification row  | Impossible by construction (one transaction); the test kills the process at every step and checks invariants |
| Several jobs reserve the remaining budget simultaneously             | Aggregate reservations never exceed the cap; exactly one provider call per reservation                       |
| Parser emits a path escape, symlink, or keeps a file open            | Manifest rejected; nothing referenced; child terminated                                                      |
| Source contains instructions to expose secrets or access another run | The run-scoped credential cannot reach them; the attempt is logged                                           |
| Redirect targets a private address                                   | Denied at connect time, not by URL inspection                                                                |
| Parser OOM overlaps with API requests and a cancellation             | API responds; control lane processes the cancel within its latency budget                                    |
| Disk fills during export                                             | No published manifest points to an incomplete artifact; run `blocked` with reason `disk`                     |
| Report is re-extracted, revised, or corrected                        | Old revision's citations still resolve to the extraction they were made against                              |
| Restore into an empty environment                                    | Retained revisions resolve; no paid submission until reconciliation mode is cleared                          |
| Provider schema or CLI output changes                                | Contract test fails visibly; no silent downgrade of provenance or billing                                    |
| Brief classification not permitted for the backend                   | Rejected before any outbound request; recording proxy sees no traffic                                        |
| Same publication retried                                             | One notification per (run, revision, kind)                                                                   |
| `refresh` finds a changed source                                     | Affected claims marked; label recomputed with a new assessment; old revision untouched                       |

## 9. Privacy, classification, and outbound policy

Public sources do not make a query public. A brief can name a client, an
acquisition target, an internal system, a security concern, or a family
circumstance while asking only about public information. The policy below is
about what may leave the VM, not about what may be fetched.

**Classification.** Every brief carries a `classification`, defaulting from its
`scope`:

| Classification      | Default for scope | Meaning                                                         |
| ------------------- | ----------------- | --------------------------------------------------------------- |
| `public`            | —                 | The brief could be posted publicly without harm                 |
| `personal`          | `personal`        | Reveals something about Robert or his family                    |
| `work_confidential` | `work`            | Reveals something about an employer, client, or unreleased work |

The default is the conservative one; an agent must say `public` explicitly.

**Provider routing.** Each backend adapter declares which classifications it may
receive and under what provider-side retention setting. The initial table is a
proposal for Robert to edit:

| Destination                              | `public` | `personal`            | `work_confidential`                            |
| ---------------------------------------- | -------- | --------------------- | ---------------------------------------------- |
| Exa search/contents (queries and URLs)   | yes      | yes                   | yes, query only, with ZDR enabled (**verify**) |
| Exa Agent (full brief)                   | yes      | yes, with ZDR enabled | no, until Robert approves a per-run exception  |
| Official CLI under Robert's subscription | yes      | yes                   | yes, subject to that provider's terms          |
| Live fetch of a public URL               | yes      | yes                   | yes                                            |
| Hosted extractor (URL only)              | yes      | yes                   | yes                                            |
| Telegram                                 | title    | title                 | run ID only                                    |

A run whose classification the backend does not accept is rejected at submission
with a stable error, before any outbound request. The `--override` path requires
a reason and is recorded on the run as an event.

**Minimisation.** The worker sends the backend the brief and only the standing
context fields the brief's scope permits, not the whole context. Internal names
in a brief are not turned into search queries by the service's own retrieval
tools unless the brief marks them as searchable; agents are told this in the
`research backends` output so they phrase briefs accordingly.

**Local handling.** Briefs, extractions, reports, and debug payloads are
readable only by the `research` service user. The prompt-logging debug flag
writes to a separate directory with its own retention (7 days) and is never
included in backups.

**Encryption and key custody.** The VM disk is whatever exe.dev provides
(**verify** whether it is encrypted at rest). Backups are encrypted client-side
by restic; Cloudflare sees ciphertext. The repository password is a generated
32-byte secret stored in Robert's password manager and on one printed copy kept
with his other recovery codes; it is never on the VM in plaintext outside the
service user's 0700 secrets directory, and never in the Cloudflare account.
Losing the password loses the backups. The secrets directory (API keys, Telegram
token, restic password, R2 token) is backed up separately as a dated
age-encrypted file under `secrets/` in the same bucket, with the age identity
also in the password manager
([D-012](decisions.md#d-012-backup-destination-and-key-custody)). Secrets live
in `/etc/research/secrets/` mode 0700, owned by the service user, never in
environment files readable by other users.

**Deletion semantics.** `research runs delete <id>` removes the run, its
reports, and evidence from the live database and store, and records a tombstone
so a restore re-deletes it. It does not remove the data from existing backups;
backups age out under the retention rule (180 days). It does not remove anything
from a provider; the adapter declares what the provider retains and for how
long, and the deletion output says so in plain words.

**Scopes.** `personal` and `work` are two standing-context profiles, two default
classifications, and a filter in `research find`. They are not two accounts, two
databases, or two users; the single-user model stands.

**Provider terms and Telegram.** Each adapter documents the terms it operates
under and any retention limit the service must enforce. Telegram bot messages
are not end-to-end encrypted, and the message content per classification above
reflects that.
