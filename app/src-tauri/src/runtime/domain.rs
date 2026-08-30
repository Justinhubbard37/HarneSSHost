use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimePhase {
    Inactive,
    Starting,
    Ready,
    Stopping,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Reserved for later runtime gates; it deliberately remains separate from phase.
pub(crate) enum RuntimeOwnership {
    Unowned,
    HostOwned { generation: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeState {
    phase: RuntimePhase,
    ownership: RuntimeOwnership,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            phase: RuntimePhase::Inactive,
            ownership: RuntimeOwnership::Unowned,
        }
    }
}

impl RuntimeState {
    pub(crate) fn phase(&self) -> RuntimePhase {
        self.phase
    }

    pub(crate) fn ownership(&self) -> RuntimeOwnership {
        self.ownership
    }

    pub(crate) fn claim_host_ownership(
        &mut self,
        generation: u64,
    ) -> Result<(), RuntimeOwnershipError> {
        Self::validate_host_generation(generation)?;
        if self.phase != RuntimePhase::Starting || self.ownership != RuntimeOwnership::Unowned {
            return Err(RuntimeOwnershipError);
        }
        self.ownership = RuntimeOwnership::HostOwned { generation };
        Ok(())
    }

    pub(crate) fn validate_host_generation(generation: u64) -> Result<(), RuntimeOwnershipError> {
        if generation == 0 {
            return Err(RuntimeOwnershipError);
        }
        Ok(())
    }

    pub(crate) fn release_host_ownership(&mut self) {
        self.ownership = RuntimeOwnership::Unowned;
    }

    pub(crate) fn transition_to(
        &mut self,
        next: RuntimePhase,
    ) -> Result<(), RuntimeTransitionError> {
        if is_allowed_transition(self.phase, next) {
            self.phase = next;
            Ok(())
        } else {
            Err(RuntimeTransitionError {
                from: self.phase,
                to: next,
            })
        }
    }
}

fn is_allowed_transition(from: RuntimePhase, to: RuntimePhase) -> bool {
    matches!(
        (from, to),
        (RuntimePhase::Inactive, RuntimePhase::Starting)
            | (RuntimePhase::Starting, RuntimePhase::Ready)
            | (RuntimePhase::Starting, RuntimePhase::Failed)
            | (RuntimePhase::Starting, RuntimePhase::Stopping)
            | (RuntimePhase::Ready, RuntimePhase::Stopping)
            | (RuntimePhase::Ready, RuntimePhase::Failed)
            | (RuntimePhase::Stopping, RuntimePhase::Inactive)
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeTransitionError {
    from: RuntimePhase,
    to: RuntimePhase,
}

impl Display for RuntimeTransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "runtime transition from {:?} to {:?} is not allowed",
            self.from, self.to
        )
    }
}

impl Error for RuntimeTransitionError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeOwnershipError;

impl Display for RuntimeOwnershipError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("runtime ownership can only be claimed by a starting host launch")
    }
}

impl Error for RuntimeOwnershipError {}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOWED: &[(RuntimePhase, RuntimePhase)] = &[
        (RuntimePhase::Inactive, RuntimePhase::Starting),
        (RuntimePhase::Starting, RuntimePhase::Ready),
        (RuntimePhase::Starting, RuntimePhase::Failed),
        (RuntimePhase::Starting, RuntimePhase::Stopping),
        (RuntimePhase::Ready, RuntimePhase::Stopping),
        (RuntimePhase::Ready, RuntimePhase::Failed),
        (RuntimePhase::Stopping, RuntimePhase::Inactive),
    ];

    #[test]
    fn accepts_every_allowed_transition_without_changing_ownership() {
        for &(from, to) in ALLOWED {
            let mut state = RuntimeState {
                phase: from,
                ownership: RuntimeOwnership::HostOwned { generation: 7 },
            };

            state.transition_to(to).unwrap();

            assert_eq!(state.phase(), to);
            assert_eq!(
                state.ownership(),
                RuntimeOwnership::HostOwned { generation: 7 }
            );
        }
    }

    #[test]
    fn rejects_every_other_transition_without_coercing_state() {
        let phases = [
            RuntimePhase::Inactive,
            RuntimePhase::Starting,
            RuntimePhase::Ready,
            RuntimePhase::Stopping,
            RuntimePhase::Failed,
        ];

        for from in phases {
            for to in phases {
                if ALLOWED.contains(&(from, to)) {
                    continue;
                }

                let mut state = RuntimeState {
                    phase: from,
                    ownership: RuntimeOwnership::Unowned,
                };
                let error = state.transition_to(to).unwrap_err();

                assert_eq!(error, RuntimeTransitionError { from, to });
                assert_eq!(state.phase(), from);
                assert_eq!(state.ownership(), RuntimeOwnership::Unowned);
            }
        }
    }

    #[test]
    fn ownership_requires_an_explicit_starting_host_launch() {
        let mut state = RuntimeState::default();
        assert_eq!(
            RuntimeState::validate_host_generation(0),
            Err(RuntimeOwnershipError)
        );
        RuntimeState::validate_host_generation(1).unwrap();
        assert_eq!(state.claim_host_ownership(1), Err(RuntimeOwnershipError));
        assert_eq!(state.ownership(), RuntimeOwnership::Unowned);

        state.transition_to(RuntimePhase::Starting).unwrap();
        assert_eq!(state.claim_host_ownership(0), Err(RuntimeOwnershipError));
        state.claim_host_ownership(1).unwrap();
        assert_eq!(
            state.ownership(),
            RuntimeOwnership::HostOwned { generation: 1 }
        );
        assert_eq!(state.claim_host_ownership(2), Err(RuntimeOwnershipError));

        state.release_host_ownership();
        assert_eq!(state.ownership(), RuntimeOwnership::Unowned);
    }
}
