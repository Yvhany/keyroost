# Factory reset (all applets) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One "Factory reset" action per multi-applet key that runs every manufacturer-intended applet reset in order (OATH → OpenPGP → PIV → Token2 OTP → FIDO last), in both the CLI and GUI.

**Architecture:** A pure planner in `keyroost-resolve` turns a device's `Caps` into an ordered `Vec<ResetStep>` shared by both frontends. A new `PivSession::force_reset()` handles PIV's block-PIN-then-block-PUK-then-RESET ceremony. The CLI `factory-reset` command and the GUI Overview danger card each own their sessions and loop the plan, recording a per-step `StepReport`; FIDO reuses the existing armed-reset flow last.

**Tech Stack:** Rust workspace (keyroost-resolve, keyroost-transport, keyroostctl clap CLI, keyroost egui GUI). No new dependencies.

## Global Constraints

- Commit UNSIGNED: `git commit --no-gpg-sign`, footer `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. Never push main, never create tags. Guard-tripping messages → `git commit -F <scratch-file>`.
- Gates before every commit: `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check`, `cargo test --workspace --offline`. MSRV 1.85 libs/CLI, 1.92 GUI.
- Vendor-over-depend: no new dependencies.
- Name is **"Factory reset"** (GUI) / `factory-reset` (CLI subcommand). Never "decommission".
- Manufacturer-intended resets ONLY. Anything that could permanently brick a key is stop-and-discuss, out of scope here.
- Branch: `feat/factory-reset` (already created off origin/main; the design spec is committed on it).
- Order is fixed: `Oath, OpenPgp, Piv, Token2Otp, Fido`.
- Continue-on-error: a failed step never aborts later steps; CLI exits nonzero on any failure.
- Hardware verification is deferred to the hardware session (no keys in-session); every task here is unit/property tested without hardware.

---

### Task 1: The shared reset planner in keyroost-resolve

**Files:**
- Modify: `crates/keyroost-resolve/src/device.rs` (add after the `Caps` impl / `Device` block; it already exports `Caps`, `Device`, `DeviceKind`)
- Test: same file, `#[cfg(test)] mod` at the end

**Interfaces:**
- Consumes: `Caps` (existing bitset with `FIDO2`, `OATH`, `PGP`, `PIV`, `OTP`, `has()`).
- Produces:
  - `pub enum ResetStep { Oath, OpenPgp, Piv, Token2Otp, Fido }` (derives `Debug, Clone, Copy, PartialEq, Eq`)
  - `pub fn factory_reset_plan(caps: Caps) -> Vec<ResetStep>`
  - `pub enum StepOutcome { Wiped, Failed(String), Skipped }` (derives `Debug, Clone, PartialEq, Eq`)
  - `pub struct StepReport { pub step: ResetStep, pub outcome: StepOutcome }` (derives `Debug, Clone, PartialEq, Eq`)
  - `impl ResetStep { pub fn label(self) -> &'static str }` → "OATH", "OpenPGP", "PIV", "OTP", "FIDO2"

- [ ] **Step 1: Write the failing tests**

Add at the end of `crates/keyroost-resolve/src/device.rs`:

```rust
#[cfg(test)]
mod plan_tests {
    use super::*;

    fn caps(bits: &[Caps]) -> Caps {
        let mut c = Caps::default();
        for b in bits {
            c.insert(*b);
        }
        c
    }

    #[test]
    fn plan_is_ordered_and_only_present_applets() {
        let full = caps(&[Caps::OATH, Caps::PGP, Caps::PIV, Caps::OTP, Caps::FIDO2]);
        assert_eq!(
            factory_reset_plan(full),
            vec![
                ResetStep::Oath,
                ResetStep::OpenPgp,
                ResetStep::Piv,
                ResetStep::Token2Otp,
                ResetStep::Fido,
            ]
        );
    }

    #[test]
    fn fido_is_always_last_when_present() {
        let c = caps(&[Caps::FIDO2, Caps::OATH]);
        let plan = factory_reset_plan(c);
        assert_eq!(plan.last(), Some(&ResetStep::Fido));
        assert_eq!(plan, vec![ResetStep::Oath, ResetStep::Fido]);
    }

    #[test]
    fn absent_applets_are_omitted() {
        let c = caps(&[Caps::PIV]);
        assert_eq!(factory_reset_plan(c), vec![ResetStep::Piv]);
        // TOTP (Molto2) and PROG are not applet-reset steps here.
        assert_eq!(factory_reset_plan(caps(&[Caps::TOTP])), Vec::<ResetStep>::new());
        assert_eq!(factory_reset_plan(Caps::default()), Vec::<ResetStep>::new());
    }

    #[test]
    fn labels_are_stable() {
        assert_eq!(ResetStep::Oath.label(), "OATH");
        assert_eq!(ResetStep::OpenPgp.label(), "OpenPGP");
        assert_eq!(ResetStep::Piv.label(), "PIV");
        assert_eq!(ResetStep::Token2Otp.label(), "OTP");
        assert_eq!(ResetStep::Fido.label(), "FIDO2");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p keyroost-resolve --offline plan_tests`
Expected: FAIL — `cannot find function factory_reset_plan` / `ResetStep`.

- [ ] **Step 3: Implement the planner**

Add before the `#[cfg(test)]` module in `crates/keyroost-resolve/src/device.rs`:

```rust
/// One applet-reset step in a whole-device factory reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetStep {
    Oath,
    OpenPgp,
    Piv,
    Token2Otp,
    Fido,
}

impl ResetStep {
    /// Short badge label — matches the capability vocabulary the CLI/GUI
    /// already show, so the reset summary reads consistently.
    pub fn label(self) -> &'static str {
        match self {
            ResetStep::Oath => "OATH",
            ResetStep::OpenPgp => "OpenPGP",
            ResetStep::Piv => "PIV",
            ResetStep::Token2Otp => "OTP",
            ResetStep::Fido => "FIDO2",
        }
    }
}

/// Ordered factory-reset steps for a key with these capabilities. Card
/// applets first (silent wipes), FIDO last (its reset needs a replug +
/// touch ceremony, so it ends the flow). Only applets the key advertises
/// appear. Pure — the single source of truth both the CLI and GUI consume,
/// so they can never disagree about what "everything" means.
pub fn factory_reset_plan(caps: Caps) -> Vec<ResetStep> {
    let mut steps = Vec::new();
    if caps.has(Caps::OATH) {
        steps.push(ResetStep::Oath);
    }
    if caps.has(Caps::PGP) {
        steps.push(ResetStep::OpenPgp);
    }
    if caps.has(Caps::PIV) {
        steps.push(ResetStep::Piv);
    }
    if caps.has(Caps::OTP) {
        steps.push(ResetStep::Token2Otp);
    }
    if caps.has(Caps::FIDO2) {
        steps.push(ResetStep::Fido);
    }
    steps
}

/// The outcome of one reset step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// The applet was reset to factory state.
    Wiped,
    /// The reset was attempted and failed; the string is the reason.
    Failed(String),
    /// The step was not run (applet not present) — reserved for callers that
    /// build a full report over all step kinds; `factory_reset_plan` simply
    /// omits absent applets.
    Skipped,
}

/// One line of a factory-reset report: which applet, and how it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReport {
    pub step: ResetStep,
    pub outcome: StepOutcome,
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p keyroost-resolve --offline plan_tests`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check
git add crates/keyroost-resolve/src/device.rs
git commit --no-gpg-sign -m "feat(resolve): shared factory-reset planner (caps -> ordered steps)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: PIV `force_reset` — block PIN, block PUK, then reset

**Files:**
- Modify: `crates/keyroost-transport/src/piv.rs` (add a method to `impl PivSession`, near `reset()` at line ~438)
- Modify: `crates/keyroost-transport/src/lib.rs` (re-export unchanged; `PivSession` is already public)
- Test: `crates/keyroost-transport/src/piv.rs` `#[cfg(test)]` module (pure counter-bound logic extracted so it tests without a card)

**Interfaces:**
- Consumes: existing `PivSession::verify_pin(&[u8])`, `PivSession::unblock_pin(puk, new_pin)`, `PivSession::reset()`, `PivSession::status()`, and `TransportError::{PivPinRejected{tries_remaining}, PivResetNotAllowed}`.
- Produces:
  - `pub fn force_reset(&mut self) -> Result<(), TransportError>` on `PivSession`.
  - `fn block_attempts_cap(reported: Option<u8>) -> u32` (private, pure, tested): the max wrong-credential attempts to try — `reported.map(|n| n as u32).unwrap_or(10).min(20) + 2`, so we always exceed the real retry count but never loop unboundedly.

**Background (verified in the code):** `verify_pin` with a wrong PIN returns `Err(PivPinRejected{ tries_remaining })`; when the card is already blocked it returns `PivPinRejected{ tries_remaining: Some(0) }`. `unblock_pin(wrong_puk, …)` behaves the same on the PUK counter. `reset()` returns `Err(PivResetNotAllowed)` (maps card `6983`) until BOTH are blocked, then `Ok(())`.

- [ ] **Step 1: Write the failing test (pure bound logic)**

Add to the existing `#[cfg(test)] mod tests` in `crates/keyroost-transport/src/piv.rs` (create the module if none — check first with `grep -n "mod tests" crates/keyroost-transport/src/piv.rs`; if absent, add `#[cfg(test)] mod tests { use super::*; … }` at end of file):

```rust
#[test]
fn block_attempts_cap_exceeds_reported_but_is_bounded() {
    // A card reporting 3 tries: we try a few more than 3 to guarantee a block.
    assert_eq!(block_attempts_cap(Some(3)), 5);
    // Unknown count: default to 10 (max PIV retry the spec allows) + margin.
    assert_eq!(block_attempts_cap(None), 12);
    // A pathological huge count is clamped so the loop can't run away.
    assert_eq!(block_attempts_cap(Some(200)), 22);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p keyroost-transport --offline block_attempts_cap`
Expected: FAIL — `cannot find function block_attempts_cap`.

- [ ] **Step 3: Implement the cap helper and `force_reset`**

Add the pure helper near the top of `piv.rs` (module scope, not in the impl):

```rust
/// How many wrong-credential attempts to make when intentionally blocking a
/// PIN or PUK during a factory reset: always more than the card's reported
/// retry count so a block is guaranteed, but hard-capped so a card that
/// misreports (or never decrements) cannot loop forever.
fn block_attempts_cap(reported: Option<u8>) -> u32 {
    reported.map(u32::from).unwrap_or(10).min(20) + 2
}
```

Add to `impl PivSession` (right after `reset()`):

```rust
/// Factory-reset the PIV applet the manufacturer-intended way even when the
/// PIN/PUK are unknown: deliberately exhaust the PIN retry counter with wrong
/// values, then the PUK counter, then send RESET (which the card only accepts
/// once BOTH are blocked). This is the documented decommission path; it wipes
/// all PIV keys, certificates, and PINs and leaves the applet at defaults.
///
/// Used only by the whole-device factory reset — the single-applet PIV reset
/// keeps requiring an already-blocked card (that path is a user who knows the
/// card is blocked, not one asking us to block it).
pub fn force_reset(&mut self) -> Result<(), TransportError> {
    // Wrong values that satisfy the 6–8 byte length rule so the card actually
    // evaluates (and decrements) rather than rejecting on length. The real
    // PIN/PUK is never these, so each attempt consumes exactly one try.
    const WRONG_A: &[u8] = b"00000000";
    const WRONG_B: &[u8] = b"99999999";

    // 1. Block the PIN.
    let pin_tries = self.status().ok().and_then(|s| s.pin_retries);
    let mut blocked = false;
    for i in 0..block_attempts_cap(pin_tries) {
        let guess = if i % 2 == 0 { WRONG_A } else { WRONG_B };
        match self.verify_pin(guess) {
            Ok(()) => { /* absurd: the wrong PIN "worked"; keep going */ }
            Err(TransportError::PivPinRejected { tries_remaining: Some(0) }) => {
                blocked = true;
                break;
            }
            Err(TransportError::PivPinRejected { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    if !blocked {
        return Err(TransportError::MalformedResponse(
            "PIV PIN would not block after the attempt cap".into(),
        ));
    }

    // 2. Block the PUK (via unblock-pin, whose wrong PUK decrements the PUK
    //    counter). new_pin is irrelevant — the unblock never succeeds.
    let mut puk_blocked = false;
    for i in 0..block_attempts_cap(None) {
        let guess = if i % 2 == 0 { WRONG_A } else { WRONG_B };
        match self.unblock_pin(guess, b"00000000") {
            Ok(()) => {}
            Err(TransportError::PivPinRejected { tries_remaining: Some(0) }) => {
                puk_blocked = true;
                break;
            }
            Err(TransportError::PivPinRejected { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    if !puk_blocked {
        return Err(TransportError::MalformedResponse(
            "PIV PUK would not block after the attempt cap".into(),
        ));
    }

    // 3. Both blocked — RESET now succeeds.
    self.reset()
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p keyroost-transport --offline block_attempts_cap`
Expected: PASS. Also `cargo build -p keyroost-transport --offline` compiles `force_reset`.

- [ ] **Step 5: Commit**

```bash
cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check
git add crates/keyroost-transport/src/piv.rs
git commit --no-gpg-sign -m "feat(piv): force_reset — block PIN+PUK then reset (decommission path)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: CLI `factory-reset` command

**Files:**
- Modify: `crates/keyroostctl/src/main.rs` — add a `Cmd::FactoryReset` variant (the top-level `enum Cmd` at line ~380), its match arm in the command dispatch, and a `run_factory_reset` fn. Add a `#[cfg(test)]` grammar test alongside the existing `cli_tests`.

**Interfaces:**
- Consumes: `keyroost_resolve::{factory_reset_plan, ResetStep, StepOutcome, StepReport, enumerate, Device, Caps}`; the existing per-applet session openers and resets — `OathSession::factory_reset`, `OpenPgpSession::factory_reset`, `PivSession::force_reset`, the Token2 OTP `erase_all` path, and `keyroost_ctap::reset`; `sanitize_terminal`; the `--device`/`SELECTED_KEY_NAME` resolution helpers.
- Produces: `keyroostctl factory-reset [--reader <sub>] --yes`, printing a per-step report and exiting 1 if any step failed.

- [ ] **Step 1: Write the failing grammar test**

In the `cli_tests` module (near `oath_reset_requires_explicit_yes`) add:

```rust
#[test]
fn factory_reset_requires_explicit_yes() {
    match parse(&["keyroostctl", "factory-reset", "--yes"]).unwrap().command {
        Some(Cmd::FactoryReset { yes, .. }) => assert!(yes),
        _ => panic!("expected factory-reset"),
    }
    match parse(&["keyroostctl", "factory-reset"]).unwrap().command {
        Some(Cmd::FactoryReset { yes, .. }) => assert!(!yes),
        _ => panic!("expected factory-reset"),
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p keyroostctl --offline factory_reset_requires_explicit_yes`
Expected: FAIL — `no variant named FactoryReset`.

- [ ] **Step 3: Add the command variant**

In `enum Cmd` (after the last existing variant, before the closing brace):

```rust
    /// Factory-reset EVERY resettable applet on the selected key: OATH,
    /// OpenPGP, PIV, Token2 OTP, then FIDO2 (which needs an unplug/replug +
    /// touch at the end). Wipes all credentials, codes, keys, and PINs; the
    /// key stays fully usable afterward. Irreversible.
    FactoryReset {
        /// Substring of the PC/SC reader name (skips auto-detection for the
        /// smart-card applets).
        #[arg(long)]
        reader: Option<String>,
        /// Confirm the wipe. Required — without it the command refuses.
        #[arg(long)]
        yes: bool,
    },
```

- [ ] **Step 4: Run to verify the grammar test passes**

Run: `cargo test -p keyroostctl --offline factory_reset_requires_explicit_yes`
Expected: FAIL still — the dispatch match is now non-exhaustive (compile error). That is expected; Step 5 adds the arm. (If you prefer a green checkpoint, do Steps 4–5 together.)

- [ ] **Step 5: Implement the handler**

Find the top-level command dispatch (where other `Cmd::` arms are handled — `grep -n "Cmd::Oath" crates/keyroostctl/src/main.rs`) and add:

```rust
        Cmd::FactoryReset { reader, yes } => run_factory_reset(reader.as_deref(), yes, debug),
```

Then add the function (near `run_oath`):

```rust
/// Whole-device factory reset: run every applet reset the key supports, in
/// planner order, continue on failure, print a per-step report, and exit
/// nonzero if anything failed. FIDO2 is last and needs a physical replug +
/// touch, prompted interactively.
fn run_factory_reset(
    reader: Option<&str>,
    yes: bool,
    debug: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use keyroost_resolve::{factory_reset_plan, Caps, ResetStep, StepOutcome, StepReport};

    if !yes {
        return Err("refusing to factory-reset without --yes (wipes ALL applets: \
                    OATH, OpenPGP, PIV, Token2 OTP, and FIDO2; the key stays \
                    usable but every credential, code, key, and PIN is erased)"
            .into());
    }

    // Resolve the one selected device (or a lone key) via the shared model,
    // so a name/`--device` binds exactly like the other commands.
    let devices = keyroost_resolve::enumerate()?;
    let name = SELECTED_KEY_NAME.get().and_then(|o| o.as_deref());
    let dev = resolve_single_device(&devices, name)?; // see helper below
    let plan = factory_reset_plan(dev.caps);
    if plan.is_empty() {
        return Err(format!(
            "'{}' exposes no resettable applet (nothing to factory-reset)",
            sanitize_terminal(&dev.model)
        )
        .into());
    }

    eprintln!(
        "\u{2192} factory-resetting {} ({})",
        sanitize_terminal(&dev.serial),
        plan.iter().map(|s| s.label()).collect::<Vec<_>>().join(", ")
    );

    let mut reports: Vec<StepReport> = Vec::new();
    for step in &plan {
        let outcome = match step {
            ResetStep::Fido => {
                // Interactive replug + touch; on its own so a card-step
                // failure above never skips the FIDO offer.
                println!("FIDO2  unplug the key, plug it back in, then press Enter\u{2026}");
                let mut _line = String::new();
                std::io::stdin().read_line(&mut _line).ok();
                println!("FIDO2  touch the key now\u{2026}");
                match run_fido_reset(None) {
                    Ok(()) => StepOutcome::Wiped,
                    Err(e) => StepOutcome::Failed(sanitize_terminal(&e.to_string())),
                }
            }
            other => reset_one_card_applet(*other, reader, debug),
        };
        let label = step.label();
        match &outcome {
            StepOutcome::Wiped => println!("{label:<8} wiped"),
            StepOutcome::Failed(e) => println!("{label:<8} failed: {e}"),
            StepOutcome::Skipped => println!("{label:<8} skipped"),
        }
        reports.push(StepReport { step: *step, outcome });
    }

    let failed = reports
        .iter()
        .filter(|r| matches!(r.outcome, StepOutcome::Failed(_)))
        .count();
    let wiped = reports.len() - failed;
    println!("factory reset: {wiped} wiped, {failed} failed");
    if failed > 0 {
        return Err(format!("{failed} applet(s) failed to reset").into());
    }
    Ok(())
}

/// Run one card-applet reset step, mapping its result to a StepOutcome so a
/// single failure is recorded, not propagated (continue-on-error).
fn reset_one_card_applet(
    step: keyroost_resolve::ResetStep,
    reader: Option<&str>,
    debug: bool,
) -> keyroost_resolve::StepOutcome {
    use keyroost_resolve::{ResetStep, StepOutcome};
    let run = || -> Result<(), Box<dyn std::error::Error>> {
        match step {
            ResetStep::Oath => {
                let by_name = reader_from_name()?;
                let name = resolve_oath_reader(reader.or(by_name.as_deref()))?;
                let mut s = keyroost_transport::OathSession::open(&name)?;
                s.set_debug(debug);
                s.factory_reset()?;
            }
            ResetStep::OpenPgp => {
                let mut s = open_openpgp_for_reset(reader, debug)?; // thin wrapper over existing opener
                s.factory_reset()?;
            }
            ResetStep::Piv => {
                let mut s = open_piv(reader, debug)?;
                s.force_reset()?;
            }
            ResetStep::Token2Otp => {
                let mut s = open_otp(OtpTransportArg::Auto, debug)?;
                s.erase_all()?;
            }
            ResetStep::Fido => unreachable!("FIDO handled by the interactive path"),
        }
        Ok(())
    };
    match run() {
        Ok(()) => StepOutcome::Wiped,
        Err(e) => StepOutcome::Failed(sanitize_terminal(&e.to_string())),
    }
}
```

Implementation notes for the engineer (find exact existing signatures with grep before wiring):
- `resolve_single_device(&devices, name)` — if no such helper exists, add a small one mirroring `resolve_otp_target`'s name-match/ambiguity logic: exactly one device (named, or the lone connected key) or an error. Fail closed on zero/ambiguous.
- `open_openpgp_for_reset` — reuse the existing OpenPGP reader resolution (`selected_openpgp_reader` / `OpenPgpSession::open`); if a ready-made opener exists (`grep -n "OpenPgpSession::open" crates/keyroostctl/src/main.rs`), call it directly instead of adding a wrapper.
- `open_piv`, `open_otp`, `run_fido_reset`, `reader_from_name`, `resolve_oath_reader` all already exist — confirm arg shapes with grep.

- [ ] **Step 6: Run tests + full gates**

Run: `cargo test -p keyroostctl --offline factory_reset_requires_explicit_yes && cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check && cargo test --workspace --offline`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/keyroostctl/src/main.rs
git commit --no-gpg-sign -m "feat(cli): factory-reset — every resettable applet, FIDO last

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: GUI Overview danger card + confirmation + execution

**Files:**
- Modify: `crates/keyroost/src/main.rs` — the Overview renderer (`fn overview`, reached via `CapTab::Overview => self.overview(...)` at line ~7370), an `App`/state field for the confirmation + running report, a `render_factory_reset_confirm` modal, and a `run_factory_reset_gui` job. Add a `#[cfg(test)]` test for the confirm-message builder (pure).

**Interfaces:**
- Consumes: `keyroost_resolve::{factory_reset_plan, ResetStep, StepOutcome, StepReport}`; `self.selected_device`; `completion_still_valid`; the existing per-applet reset helpers already on `App` (`reset_oath_applet` job body, `reset_openpgp`, `piv_reset`/`force_reset` path, OTP `erase_all` job, and the armed FIDO `ResetDialog` flow); `theme::card_frame`, `BtnKind::Danger`.
- Produces: an Overview "Factory reset" card; a two-click device-bound modal; a live per-step status list; FIDO arming handoff at the end.

- [ ] **Step 1: Write the failing test (pure confirm-summary builder)**

Add near the GUI `#[cfg(test)] mod tests`:

```rust
#[test]
fn factory_reset_summary_lists_applets_and_flags_piv_and_fido() {
    use keyroost_resolve::{factory_reset_plan, Caps};
    let mut caps = Caps::default();
    for c in [Caps::OATH, Caps::PIV, Caps::FIDO2] {
        caps.insert(c);
    }
    let msg = factory_reset_confirm_summary("SN123", "Token2 PIN+", &factory_reset_plan(caps));
    assert!(msg.contains("SN123") && msg.contains("Token2 PIN+"));
    assert!(msg.contains("OATH") && msg.contains("PIV") && msg.contains("FIDO2"));
    // PIV disclosure and FIDO replug note are present.
    assert!(msg.contains("PIN and PUK"));
    assert!(msg.to_lowercase().contains("unplug"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p keyroost --offline factory_reset_summary`
Expected: FAIL — `cannot find function factory_reset_confirm_summary`.

- [ ] **Step 3: Implement the summary builder (pure, free fn near the other pure helpers like `fido_settings_available`)**

```rust
/// The confirmation body for the Factory reset modal: what key, and exactly
/// which applets get wiped, with the two ceremonies that aren't a plain wipe
/// spelled out (PIV blocks its PIN+PUK first; FIDO needs a replug + touch).
fn factory_reset_confirm_summary(
    serial: &str,
    model: &str,
    plan: &[keyroost_resolve::ResetStep],
) -> String {
    use keyroost_resolve::ResetStep;
    let applets = plan
        .iter()
        .map(|s| s.label())
        .collect::<Vec<_>>()
        .join(", ");
    let mut msg = format!(
        "Factory-reset {model} (serial {serial})?\n\nWipes: {applets}.\nEvery \
         credential, code, key, and PIN is erased. The key stays fully usable."
    );
    if plan.contains(&ResetStep::Piv) {
        msg.push_str(
            "\n\nPIV: the PIN and PUK are intentionally blocked, then the applet \
             is wiped (the standard reset path).",
        );
    }
    if plan.contains(&ResetStep::Fido) {
        msg.push_str("\n\nFinishes with a step to unplug the key, plug it back in, and touch it.");
    }
    msg
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p keyroost --offline factory_reset_summary`
Expected: PASS.

- [ ] **Step 5: Add state, the Overview card, the modal, and the job**

Add to the OATH-adjacent state on `App` (or the security_keys/device state struct — place beside `oath.confirm_reset`):

```rust
    /// Pending whole-device factory-reset confirmation, bound to the device it
    /// was opened for (KEY-008 posture). None unless the modal is open.
    factory_reset_confirm: Option<DeviceId>,
    /// Live per-step report while a factory reset runs (empty when idle).
    factory_reset_report: Vec<keyroost_resolve::StepReport>,
```

In `fn overview`, after the existing device summary, render the danger card ONLY for `DeviceKind::Key` with a non-empty plan (Molto2 keeps its own reset in its pane; ProgToken shows nothing):

```rust
        if dev.kind == keyroost_resolve::DeviceKind::Key {
            let plan = keyroost_resolve::factory_reset_plan(dev.caps);
            if !plan.is_empty() {
                ui.add_space(14.0);
                let mut arm = false;
                theme::card_frame(p)
                    .stroke(egui::Stroke::new(1.0, theme::tint(p.err, 90)))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Factory reset")
                                    .font(theme::f_sb(14.5))
                                    .color(p.err),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if theme::button(ui, p, BtnKind::Danger, "Factory reset\u{2026}")
                                    .clicked()
                                {
                                    arm = true;
                                }
                            });
                        });
                        ui.label(
                            egui::RichText::new(format!(
                                "Resets every applet on this key ({}) to factory state. \
                                 The key stays fully usable.",
                                plan.iter().map(|s| s.label()).collect::<Vec<_>>().join(", ")
                            ))
                            .font(theme::f_reg(12.5))
                            .color(p.txt2),
                        );
                    });
                if arm {
                    self.factory_reset_confirm = self.selected_device.clone();
                }
            }
        }
```

Add `render_factory_reset_confirm(&mut self, ctx)` mirroring `render_oath_reset_confirm` (device-bound; dies on selection change), using `factory_reset_confirm_summary(...)` for the body and a "Yes, wipe this key" primary-danger button that calls `self.run_factory_reset_gui()`. Call it from the same top-level place `render_oath_reset_confirm` is called (`grep -n "render_oath_reset_confirm(ctx)"`).

Implement `run_factory_reset_gui`: build the plan from `self.selected_device`'s caps, then `spawn_job` a sequential loop that runs each card applet's reset (reusing the transport calls the existing per-applet job bodies use) recording `StepReport`s into `self.factory_reset_report` via the apply closure (guarded by `completion_still_valid`), and when the card steps finish, if the plan ends in `Fido`, open the existing `ResetDialog` (the armed replug+touch flow) so the finale reuses tested code. Render `factory_reset_report` as a live list under the card.

Implementation note: keep the job body's device I/O off the UI thread exactly like `reset_oath_applet`; the FIDO handoff sets `self.security_keys.reset = ResetDialog { open: true, ..Default::default() }` after the card steps, so no new FIDO logic is written.

- [ ] **Step 6: Full gates**

Run: `cargo clippy --workspace --all-targets --offline -- -D warnings && cargo fmt --all --check && cargo test --workspace --offline`
Expected: all pass. Then `cargo build --release -p keyroost --offline` (PATH symlink refresh).

- [ ] **Step 7: Commit**

```bash
git add crates/keyroost/src/main.rs
git commit --no-gpg-sign -m "feat(gui): Factory reset card on the device Overview tab

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Docs + deferred hardware item

**Files:**
- Modify: `README.md` (the CLI command list / features — add `factory-reset` and, per the earlier decision, the libpcsclite note for tarball/binstall users if not already present)
- Modify: `TODO-v0.7.5.md` (hardware-verification list)
- Modify: `CHANGELOG.md` (`[Unreleased]`)

- [ ] **Step 1: Changelog + README**

Add under `## [Unreleased]` in CHANGELOG.md:

```markdown
### Added
- **Factory reset (all applets):** one action — `keyroostctl factory-reset`
  and a card on the GUI device Overview tab — resets every resettable applet
  on a key (OATH, OpenPGP, PIV, Token2 OTP, then FIDO2). Uses only
  manufacturer-intended resets; the key stays fully usable. Per-applet resets
  remain available individually.
```

Add `factory-reset` to the README command list next to the other destructive commands, and (earlier decision) a one-line note in the tarball/binstall install section: "the prebuilt binaries need `libpcsclite` at runtime — on a FIDO-only machine install it (`apt install libpcsclite1` / `dnf install pcsc-lite`) or use a package that declares the dependency."

- [ ] **Step 2: Hardware TODO**

Add to the hardware-verification list in `TODO-v0.7.5.md`:

```markdown
- [ ] **Factory reset (all applets), flagship (v0.7.7):** one full run on a
      disposable multi-applet test key — confirm each applet's reset fires in
      order, the PIV PIN+PUK retry burn completes and RESET succeeds, the FIDO
      replug+touch finale works, and the per-step summary matches reality
      (including a deliberately-induced mid-sequence failure showing
      continue-on-error).
```

- [ ] **Step 3: Commit**

```bash
git add README.md TODO-v0.7.5.md CHANGELOG.md
git commit --no-gpg-sign -m "docs: factory reset — changelog, README, deferred hardware check

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** planner in keyroost-resolve ✅ (Task 1); PIV force_reset with bounded blocking loops ✅ (Task 2); CLI `factory-reset --yes`, continue-on-error, nonzero exit, FIDO-last interactive ✅ (Task 3); GUI Overview card, two-click device-bound confirm showing model/serial/applet-list with PIV + FIDO disclosures, live report, FIDO handoff via existing ResetDialog ✅ (Task 4); Molto2 excluded (card gated to `DeviceKind::Key`) and ProgToken excluded (empty plan → no card) ✅ (Task 4); "Factory reset"/`factory-reset` naming ✅; manufacturer-intent-only ✅ (no brick paths); hardware verification deferred ✅ (Task 5).
- **Placeholder scan:** the two grep-for-exact-signature notes (Task 3 openers, Task 4 job body) point at named existing functions the implementer must wire, with fallbacks specified — not vague "handle it" placeholders. All code steps carry code.
- **Type consistency:** `ResetStep`/`StepOutcome`/`StepReport`/`factory_reset_plan` names and the `label()` strings are identical across Tasks 1, 3, 4; `force_reset`/`block_attempts_cap` consistent between Task 2 and its caller in Task 3; `factory_reset_confirm_summary` signature identical in Task 4 Steps 1/3.
- **Ordering caveat (Task 3 Step 4):** the grammar test can't pass until the dispatch arm exists (non-exhaustive match). Flagged in-step — do Steps 3–5 as one unit for a green checkpoint.
