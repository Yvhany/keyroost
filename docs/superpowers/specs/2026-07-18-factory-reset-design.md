# Factory reset (all applets) — design

Decided with the user, 2026-07-18. One action that resets every
manufacturer-intended-resettable applet on the selected key. Individual
per-applet resets remain exactly where they are; this composes them.

## Decisions log

| Question | Decision |
|---|---|
| FIDO's replug+touch ceremony | **Last.** Card applets wipe silently first; the flow ends by arming the existing FIDO reset (unplug → replug → touch). |
| PIV precondition (card only accepts RESET with PIN **and** PUK blocked) | **Included, disclosed.** The flow deliberately exhausts PIN then PUK retries with wrong values, then RESETs — the documented decommission path. The confirmation dialog states this in plain words. |
| One step fails mid-sequence | **Keep going.** Per-applet outcomes tracked independently; summary shows Wiped / Failed(reason) / Skipped; FIDO arming still offered; CLI exits nonzero on any failure. |
| Confirmation ceremony | **Two affirmative clicks, no typing.** The dialog shows model + serial + the exact applet list; protection against wrong-key wipes comes from showing identity plus the device-bound dialog (dies on selection change, KEY-008/KEY-013 posture), not from transcription. CLI: existing `--yes` convention. |
| Name | **"Factory reset."** The key is fully usable afterward; "decommission" (rejected) implied otherwise. CLI subcommand: `keyroostctl factory-reset`. |

## Scope

- **Applies to multi-applet keys** (`DeviceKind::Key`). Steps are derived
  from the device's detected `Caps` — only applets the key advertises appear.
- **Molto2 (`DeviceKind::Token`):** the existing whole-device factory reset
  *is* this feature; the Overview card routes to it unchanged.
- **Single-profile programmable token (`DeviceKind::ProgToken`): excluded.**
  The protocol has no reset instruction; overwriting the seed/config is the
  only mutation the manufacturer provides. The Overview card does not render
  for this kind.
- Both frontends: GUI (device **Overview** tab danger card — device-level
  action, so it lives outside every capability pane) and CLI
  (`keyroostctl factory-reset --yes`, device via `--device` or a lone key).

## The plan model (testable core)

A pure planner beside `Caps` in **keyroost-resolve** — the same
pure-predicate pattern as the #81 fix — consumed by both binaries so they
can never disagree about what "everything" means:

```rust
pub enum ResetStep { Oath, OpenPgp, Piv, Token2Otp, Fido }

/// Ordered steps for a factory reset of `dev`: card applets first
/// (silent wipes), FIDO last (replug + touch ceremony).
pub fn factory_reset_plan(caps: Caps) -> Vec<ResetStep>
```

Order: `Oath → OpenPgp → Piv → Token2Otp → Fido`. Report type shared too:

```rust
pub enum StepOutcome { Wiped, Failed(String), Skipped }
pub struct StepReport { pub step: ResetStep, pub outcome: StepOutcome }
```

## Per-step execution (existing machinery, composed)

| Step | Mechanism | Notes |
|---|---|---|
| OATH | `OathSession::factory_reset()` (v0.7.7) | No precondition. |
| OpenPGP | `OpenPgpSession::factory_reset()` | Blocks its own PINs; no precondition. |
| PIV | New `PivSession::force_reset()`: loop wrong `VERIFY` until PIN blocked, loop wrong `RESET RETRY COUNTER` until PUK blocked, then existing `reset()` | Wrong values are fixed junk (e.g. `"\xFF"*8` form); loop bounds = retry counts from status plus a hard cap (~20) so a pathological card cannot loop forever. |
| Token2 OTP | `erase_all()` via the OTP session (existing builder) | Uses the same HID-then-reader binding as the OTP pane. |
| FIDO | Existing armed-reset flow | GUI: `ResetDialog`/reinsert-match watcher. CLI: print the summary, prompt "unplug, replug, press Enter", then `keyroost_ctap::reset` with the touch wait. |

Each frontend owns its sessions and loops over the plan; there is no
cross-crate orchestrator (transport can't see FIDO's HID stack, and doesn't
need to).

## UX

**GUI:** "Factory reset" danger card on the Overview tab (red stroke, house
style). Click 1 opens the device-bound dialog: model, serial, and the
per-applet list with the PIV disclosure line ("PIN and PUK will be
intentionally blocked, then the applet wiped") and, when FIDO is present,
"finishes with an unplug-replug-touch step." Click 2 ("Yes, wipe this key")
starts the job. A live status list shows each step as it runs; the summary
stays visible; the FIDO arming banner appears when card steps finish. All
pane states for the wiped applets are re-initialized afterward (same
fresh-state pattern as the OATH reset apply).

**CLI:**

```
$ keyroostctl --device work-key factory-reset --yes
→ key SN 15731286 (Token2 PIN+): OATH, OpenPGP, PIV, OTP, FIDO2
OATH      wiped
OpenPGP   wiped
PIV       blocking PIN (3 tries)… blocking PUK (3 tries)… wiped
OTP       wiped
FIDO2     unplug the key, plug it back in, then press Enter…
          touch the key now…
FIDO2     wiped
factory reset complete: 5 wiped, 0 failed
```

Refuses without `--yes` (existing convention). Any failure → per-step
`failed: <reason>` row, sequence continues, exit code 1.

## Error handling

- Step outcomes are independent; a failure never aborts later steps.
- Transport hangs are already bounded (v0.7.5 read budgets); a yanked key
  fails the remaining card steps with transport errors.
- The PIV blocking loops are bounded by reported retry counts + hard cap;
  hitting the cap = `Failed("PIN would not block after N attempts")`.
- GUI job binds to the device at confirm time (`completion_still_valid`);
  a selection change discards the completion, never repaints another key's
  outcome.

## Testing

- TDD: `factory_reset_plan` (every Caps combination, ordering, FIDO-last,
  empty caps → empty plan), CLI `--yes` grammar decode, summary formatting,
  PIV blocking-loop bound logic (pure counter math extracted).
- Execution reuses per-applet resets that carry their own tests.
- **Hardware session (deferred list):** one full run on a disposable
  test key — including the PIV retry burn and the FIDO finale — is the
  flagship item; also verify the summary matches reality per applet.

## Out of scope

- Molto2 gets no new flow (routes to its existing reset).
- ProgToken excluded (no manufacturer reset exists).
- No attempt to wipe anything a vendor doesn't expose a reset for
  (explicit user constraint: manufacturer intent only).
