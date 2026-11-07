//! Tab lifecycle state machine. Pure Rust, no platform types.
//!
//! The machine decides *which* platform commands run; `platform` only executes them.
//! Rule 3.2 is enforced here: a protected tab is never `TrySuspend`ed, and the memory
//! target level is never touched while the webview is suspended.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// Webview exists and is visible.
    Active,
    /// Webview exists, hidden, nothing else done.
    Idle,
    /// Webview hidden and frozen with `TrySuspend`.
    Suspended,
    /// Protected tab hidden but kept running, `MemoryUsageTargetLevel = Low`.
    LowMemory,
    /// Webview dropped; URL/title/favicon kept by the app.
    Discarded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Activate,
    Deactivate,
    /// Hidden long enough for the mode's IDLE -> SUSPENDED threshold.
    SuspendDue,
    /// Discard requested (timeout, memory pressure or "Free inactive tab memory").
    DiscardDue,
    /// Protection signal changed (audio, download, call, dirty form, pinned...).
    ProtectionChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Show,
    Hide,
    TrySuspend,
    Resume,
    SetLevelLow,
    SetLevelNormal,
    CreateWebview,
    DropWebview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forbidden {
    /// Suspending or discarding a protected tab.
    Protected,
    /// Suspending or discarding the visible tab.
    Active,
    /// Event makes no sense in this state (e.g. suspend an already discarded tab).
    Invalid,
}

pub type Step = Result<(Lifecycle, Vec<Command>), Forbidden>;

/// Apply `event` to a tab in `state`. `protected` is the tab's current protection
/// verdict (uncertain signals must already have been turned into `true`).
pub fn transition(state: Lifecycle, event: Event, protected: bool) -> Step {
    use Command::*;
    use Event::*;
    use Lifecycle::*;
    let ok = |s, c: &[Command]| Ok((s, c.to_vec()));
    match (state, event) {
        (Active, Activate) => ok(Active, &[]),
        (Active, Deactivate) if protected => ok(LowMemory, &[Hide, SetLevelLow]),
        (Active, Deactivate) => ok(Idle, &[Hide]),
        (Active, SuspendDue | DiscardDue) => Err(Forbidden::Active),
        (Active, ProtectionChanged) => ok(Active, &[]),

        (Idle, Activate) => ok(Active, &[Show]),
        (Idle, ProtectionChanged) if protected => ok(LowMemory, &[SetLevelLow]),
        (Idle | LowMemory | Suspended, SuspendDue | DiscardDue) if protected => {
            Err(Forbidden::Protected)
        }
        (Idle, SuspendDue) => ok(Suspended, &[TrySuspend]),
        (Idle, DiscardDue) => ok(Discarded, &[DropWebview]),

        // Resume before touching the level: the two mechanisms are never mixed.
        (Suspended, Activate) => ok(Active, &[Resume, Show]),
        (Suspended, ProtectionChanged) if protected => ok(LowMemory, &[Resume, SetLevelLow]),
        (Suspended, DiscardDue) => ok(Discarded, &[DropWebview]),

        (LowMemory, Activate) => ok(Active, &[SetLevelNormal, Show]),
        (LowMemory, ProtectionChanged) if !protected => ok(Idle, &[SetLevelNormal]),

        (Discarded, Activate) => ok(Active, &[CreateWebview, Show]),

        (Idle | Suspended | LowMemory | Discarded, ProtectionChanged) => ok(state, &[]),
        _ => Err(Forbidden::Invalid),
    }
}

#[cfg(test)]
mod tests {
    use super::{Command::*, Event::*, Forbidden as F, Lifecycle::*, *};

    const STATES: [Lifecycle; 5] = [Active, Idle, Suspended, LowMemory, Discarded];
    const EVENTS: [Event; 5] = [Activate, Deactivate, SuspendDue, DiscardDue, ProtectionChanged];

    fn ok(s: Lifecycle, c: &[Command]) -> Step {
        Ok((s, c.to_vec()))
    }

    #[test]
    fn full_table() {
        // (state, event, protected) -> expected, for every combination.
        let table: &[(Lifecycle, Event, bool, Step)] = &[
            (Active, Activate, false, ok(Active, &[])),
            (Active, Activate, true, ok(Active, &[])),
            (Active, Deactivate, false, ok(Idle, &[Hide])),
            (Active, Deactivate, true, ok(LowMemory, &[Hide, SetLevelLow])),
            (Active, SuspendDue, false, Err(F::Active)),
            (Active, SuspendDue, true, Err(F::Active)),
            (Active, DiscardDue, false, Err(F::Active)),
            (Active, DiscardDue, true, Err(F::Active)),
            (Active, ProtectionChanged, false, ok(Active, &[])),
            (Active, ProtectionChanged, true, ok(Active, &[])),
            (Idle, Activate, false, ok(Active, &[Show])),
            (Idle, Activate, true, ok(Active, &[Show])),
            (Idle, Deactivate, false, Err(F::Invalid)),
            (Idle, Deactivate, true, Err(F::Invalid)),
            (Idle, SuspendDue, false, ok(Suspended, &[TrySuspend])),
            (Idle, SuspendDue, true, Err(F::Protected)),
            (Idle, DiscardDue, false, ok(Discarded, &[DropWebview])),
            (Idle, DiscardDue, true, Err(F::Protected)),
            (Idle, ProtectionChanged, false, ok(Idle, &[])),
            (Idle, ProtectionChanged, true, ok(LowMemory, &[SetLevelLow])),
            (Suspended, Activate, false, ok(Active, &[Resume, Show])),
            (Suspended, Activate, true, ok(Active, &[Resume, Show])),
            (Suspended, Deactivate, false, Err(F::Invalid)),
            (Suspended, Deactivate, true, Err(F::Invalid)),
            (Suspended, SuspendDue, false, Err(F::Invalid)),
            (Suspended, SuspendDue, true, Err(F::Protected)),
            (Suspended, DiscardDue, false, ok(Discarded, &[DropWebview])),
            (Suspended, DiscardDue, true, Err(F::Protected)),
            (Suspended, ProtectionChanged, false, ok(Suspended, &[])),
            (Suspended, ProtectionChanged, true, ok(LowMemory, &[Resume, SetLevelLow])),
            (LowMemory, Activate, false, ok(Active, &[SetLevelNormal, Show])),
            (LowMemory, Activate, true, ok(Active, &[SetLevelNormal, Show])),
            (LowMemory, Deactivate, false, Err(F::Invalid)),
            (LowMemory, Deactivate, true, Err(F::Invalid)),
            (LowMemory, SuspendDue, false, Err(F::Invalid)),
            (LowMemory, SuspendDue, true, Err(F::Protected)),
            (LowMemory, DiscardDue, false, Err(F::Invalid)),
            (LowMemory, DiscardDue, true, Err(F::Protected)),
            (LowMemory, ProtectionChanged, false, ok(Idle, &[SetLevelNormal])),
            (LowMemory, ProtectionChanged, true, ok(LowMemory, &[])),
            (Discarded, Activate, false, ok(Active, &[CreateWebview, Show])),
            (Discarded, Activate, true, ok(Active, &[CreateWebview, Show])),
            (Discarded, Deactivate, false, Err(F::Invalid)),
            (Discarded, Deactivate, true, Err(F::Invalid)),
            (Discarded, SuspendDue, false, Err(F::Invalid)),
            (Discarded, SuspendDue, true, Err(F::Invalid)),
            (Discarded, DiscardDue, false, Err(F::Invalid)),
            (Discarded, DiscardDue, true, Err(F::Invalid)),
            (Discarded, ProtectionChanged, false, ok(Discarded, &[])),
            (Discarded, ProtectionChanged, true, ok(Discarded, &[])),
        ];
        assert_eq!(table.len(), STATES.len() * EVENTS.len() * 2);
        for (s, e, p, want) in table {
            assert_eq!(&transition(*s, *e, *p), want, "{s:?} + {e:?} (protected={p})");
        }
    }

    /// Rule 3.2 as invariants over every reachable command list.
    #[test]
    fn never_mixes_suspend_and_memory_level() {
        for s in STATES {
            for e in EVENTS {
                for p in [false, true] {
                    let Ok((_, cmds)) = transition(s, e, p) else { continue };
                    assert!(!(p && cmds.contains(&TrySuspend)), "{s:?}+{e:?}: suspended a protected tab");
                    let mut frozen = s == Suspended;
                    let mut low = s == LowMemory;
                    for c in &cmds {
                        match c {
                            Resume => frozen = false,
                            TrySuspend => {
                                assert!(!low, "{s:?}+{e:?}: TrySuspend while level is Low");
                                frozen = true
                            }
                            SetLevelLow | SetLevelNormal => {
                                assert!(!frozen, "{s:?}+{e:?}: level set while suspended");
                                low = *c == SetLevelLow;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn protected_tab_is_never_discarded() {
        for s in STATES {
            for e in EVENTS {
                if let Ok((_, cmds)) = transition(s, e, true) {
                    assert!(!cmds.contains(&DropWebview), "{s:?}+{e:?}");
                }
            }
        }
    }
}
