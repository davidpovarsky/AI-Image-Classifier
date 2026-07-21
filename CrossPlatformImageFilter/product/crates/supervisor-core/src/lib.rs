#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SupervisorState {
    Uninstalled,
    NeedsActivation,
    NeedsOnboarding,
    Stopped,
    Starting,
    Running,
    Degraded,
    Updating,
    Stopping,
    Recovering,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSnapshot {
    pub state: SupervisorState,
    pub engine_healthy: bool,
    pub capture_healthy: bool,
    pub policy_revision: u64,
    pub degraded_reason: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SupervisorError {
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: SupervisorState,
        to: SupervisorState,
    },
    #[error("start preflight failed at {stage}: {message}")]
    Preflight {
        stage: &'static str,
        message: String,
    },
    #[error("platform operation failed at {stage}: {message}")]
    Platform {
        stage: &'static str,
        message: String,
    },
}

pub trait LifecyclePlatform {
    fn acquire_lock(&mut self) -> Result<(), String>;
    fn validate_license(&mut self) -> Result<(), String>;
    fn verify_policy(&mut self) -> Result<u64, String>;
    fn verify_models_and_engine(&mut self) -> Result<(), String>;
    fn verify_certificate(&mut self) -> Result<(), String>;
    fn start_engine(&mut self) -> Result<(), String>;
    fn engine_health(&mut self) -> Result<(), String>;
    fn backup_proxy(&mut self) -> Result<(), String>;
    fn enable_capture(&mut self) -> Result<(), String>;
    fn capture_health(&mut self) -> Result<(), String>;
    fn disable_capture(&mut self) -> Result<(), String>;
    fn restore_proxy(&mut self) -> Result<(), String>;
    fn stop_engine(&mut self) -> Result<(), String>;
    fn release_lock(&mut self);
}

pub struct Supervisor<P> {
    platform: P,
    state: SupervisorState,
    policy_revision: u64,
    degraded_reason: Option<String>,
}

impl<P: LifecyclePlatform> Supervisor<P> {
    pub fn new(platform: P, state: SupervisorState) -> Self {
        Self {
            platform,
            state,
            policy_revision: 0,
            degraded_reason: None,
        }
    }

    pub fn state(&self) -> SupervisorState {
        self.state
    }

    pub fn start(&mut self) -> Result<HealthSnapshot, SupervisorError> {
        self.transition(SupervisorState::Starting)?;
        if let Err(error) = self.start_transaction() {
            self.state = SupervisorState::Recovering;
            self.rollback_start();
            self.state = SupervisorState::Error;
            return Err(error);
        }
        self.state = SupervisorState::Running;
        Ok(self.health())
    }

    pub fn stop(&mut self) -> Result<HealthSnapshot, SupervisorError> {
        self.transition(SupervisorState::Stopping)?;
        let capture =
            self.platform
                .disable_capture()
                .map_err(|message| SupervisorError::Platform {
                    stage: "disableCapture",
                    message,
                });
        let restore = self
            .platform
            .restore_proxy()
            .map_err(|message| SupervisorError::Platform {
                stage: "restoreProxy",
                message,
            });
        let engine = self
            .platform
            .stop_engine()
            .map_err(|message| SupervisorError::Platform {
                stage: "stopEngine",
                message,
            });
        self.platform.release_lock();
        capture?;
        restore?;
        engine?;
        self.state = SupervisorState::Stopped;
        Ok(self.health())
    }

    pub fn mark_engine_crashed(&mut self, reason: impl Into<String>) {
        self.degraded_reason = Some(reason.into());
        self.state = SupervisorState::Recovering;
        let _ = self.platform.disable_capture();
        let _ = self.platform.restore_proxy();
        let _ = self.platform.stop_engine();
        self.state = SupervisorState::Degraded;
    }

    pub fn health(&self) -> HealthSnapshot {
        HealthSnapshot {
            state: self.state,
            engine_healthy: self.state == SupervisorState::Running,
            capture_healthy: self.state == SupervisorState::Running,
            policy_revision: self.policy_revision,
            degraded_reason: self.degraded_reason.clone(),
        }
    }

    fn start_transaction(&mut self) -> Result<(), SupervisorError> {
        self.preflight("singleInstanceLock", |platform| platform.acquire_lock())?;
        self.preflight("license", |platform| platform.validate_license())?;
        self.policy_revision = self.preflight("policy", |platform| platform.verify_policy())?;
        self.preflight("modelsAndEngine", |platform| {
            platform.verify_models_and_engine()
        })?;
        self.preflight("certificate", |platform| platform.verify_certificate())?;
        self.operation("startEngine", |platform| platform.start_engine())?;
        self.operation("engineHealth", |platform| platform.engine_health())?;
        self.operation("backupProxy", |platform| platform.backup_proxy())?;
        self.operation("enableCapture", |platform| platform.enable_capture())?;
        self.operation("captureHealth", |platform| platform.capture_health())?;
        Ok(())
    }

    fn preflight<T>(
        &mut self,
        stage: &'static str,
        operation: impl FnOnce(&mut P) -> Result<T, String>,
    ) -> Result<T, SupervisorError> {
        operation(&mut self.platform)
            .map_err(|message| SupervisorError::Preflight { stage, message })
    }

    fn operation<T>(
        &mut self,
        stage: &'static str,
        operation: impl FnOnce(&mut P) -> Result<T, String>,
    ) -> Result<T, SupervisorError> {
        operation(&mut self.platform)
            .map_err(|message| SupervisorError::Platform { stage, message })
    }

    fn rollback_start(&mut self) {
        let _ = self.platform.disable_capture();
        let _ = self.platform.restore_proxy();
        let _ = self.platform.stop_engine();
        self.platform.release_lock();
    }

    fn transition(&mut self, to: SupervisorState) -> Result<(), SupervisorError> {
        let valid = matches!(
            (self.state, to),
            (SupervisorState::Stopped, SupervisorState::Starting)
                | (SupervisorState::Degraded, SupervisorState::Starting)
                | (SupervisorState::Running, SupervisorState::Stopping)
                | (SupervisorState::Degraded, SupervisorState::Stopping)
        );
        if !valid {
            return Err(SupervisorError::InvalidTransition {
                from: self.state,
                to,
            });
        }
        self.state = to;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakePlatform {
        fail_at: Option<&'static str>,
        calls: Vec<&'static str>,
    }

    impl FakePlatform {
        fn call(&mut self, name: &'static str) -> Result<(), String> {
            self.calls.push(name);
            if self.fail_at == Some(name) {
                Err("injected failure".into())
            } else {
                Ok(())
            }
        }
    }

    impl LifecyclePlatform for FakePlatform {
        fn acquire_lock(&mut self) -> Result<(), String> {
            self.call("lock")
        }
        fn validate_license(&mut self) -> Result<(), String> {
            self.call("license")
        }
        fn verify_policy(&mut self) -> Result<u64, String> {
            self.call("policy")?;
            Ok(42)
        }
        fn verify_models_and_engine(&mut self) -> Result<(), String> {
            self.call("models")
        }
        fn verify_certificate(&mut self) -> Result<(), String> {
            self.call("certificate")
        }
        fn start_engine(&mut self) -> Result<(), String> {
            self.call("startEngine")
        }
        fn engine_health(&mut self) -> Result<(), String> {
            self.call("engineHealth")
        }
        fn backup_proxy(&mut self) -> Result<(), String> {
            self.call("backupProxy")
        }
        fn enable_capture(&mut self) -> Result<(), String> {
            self.call("enableCapture")
        }
        fn capture_health(&mut self) -> Result<(), String> {
            self.call("captureHealth")
        }
        fn disable_capture(&mut self) -> Result<(), String> {
            self.call("disableCapture")
        }
        fn restore_proxy(&mut self) -> Result<(), String> {
            self.call("restoreProxy")
        }
        fn stop_engine(&mut self) -> Result<(), String> {
            self.call("stopEngine")
        }
        fn release_lock(&mut self) {
            self.calls.push("releaseLock");
        }
    }

    #[test]
    fn transactional_start_and_stop() {
        let mut supervisor = Supervisor::new(FakePlatform::default(), SupervisorState::Stopped);
        assert_eq!(supervisor.start().unwrap().policy_revision, 42);
        assert_eq!(supervisor.stop().unwrap().state, SupervisorState::Stopped);
    }

    #[test]
    fn capture_failure_rolls_back_and_restores_network() {
        let platform = FakePlatform {
            fail_at: Some("captureHealth"),
            calls: vec![],
        };
        let mut supervisor = Supervisor::new(platform, SupervisorState::Stopped);
        assert!(supervisor.start().is_err());
        assert_eq!(supervisor.state(), SupervisorState::Error);
        assert!(supervisor.platform.calls.ends_with(&[
            "disableCapture",
            "restoreProxy",
            "stopEngine",
            "releaseLock"
        ]));
    }

    #[test]
    fn engine_crash_fails_safe_without_stranding_proxy() {
        let mut supervisor = Supervisor::new(FakePlatform::default(), SupervisorState::Stopped);
        supervisor.start().unwrap();
        supervisor.mark_engine_crashed("exit 101");
        assert_eq!(supervisor.state(), SupervisorState::Degraded);
        assert!(supervisor.platform.calls.ends_with(&[
            "disableCapture",
            "restoreProxy",
            "stopEngine"
        ]));
    }
}
