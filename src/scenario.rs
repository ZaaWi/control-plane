use std::{error::Error, fmt};

use crate::simulation::SimulationConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    SteadyState,
    PressureRamp,
    RetryStorm,
    DualFailure,
}

impl Scenario {
    pub const ALL: [Self; 4] = [
        Self::SteadyState,
        Self::PressureRamp,
        Self::RetryStorm,
        Self::DualFailure,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::SteadyState => "steady-state",
            Self::PressureRamp => "pressure-ramp",
            Self::RetryStorm => "retry-storm",
            Self::DualFailure => "dual-failure",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::SteadyState => "balanced load that should remain healthy",
            Self::PressureRamp => "sustained pressure that fills the queue over time",
            Self::RetryStorm => "failed services create bounded retry pressure",
            Self::DualFailure => "aggressive load drives both services to failure",
        }
    }

    pub fn config(self) -> SimulationConfig {
        match self {
            Self::SteadyState => SimulationConfig {
                requests_per_tick: 4,
                service_capacity_per_tick: 2,
                max_queue_size: 24,
                overload_threshold_ticks: 4,
                failure_threshold_ticks: 10,
                max_request_retries: 2,
                backpressure_threshold_ticks: 2,
                overload_recovery_ticks: 3,
                history_limit: 30,
            },
            Self::PressureRamp => SimulationConfig {
                requests_per_tick: 6,
                service_capacity_per_tick: 2,
                max_queue_size: 24,
                overload_threshold_ticks: 4,
                failure_threshold_ticks: 10,
                max_request_retries: 2,
                backpressure_threshold_ticks: 2,
                overload_recovery_ticks: 3,
                history_limit: 30,
            },
            Self::RetryStorm => SimulationConfig {
                requests_per_tick: 8,
                service_capacity_per_tick: 1,
                max_queue_size: 16,
                overload_threshold_ticks: 1,
                failure_threshold_ticks: 2,
                max_request_retries: 3,
                backpressure_threshold_ticks: 1,
                overload_recovery_ticks: 4,
                history_limit: 30,
            },
            Self::DualFailure => SimulationConfig {
                requests_per_tick: 10,
                service_capacity_per_tick: 1,
                max_queue_size: 12,
                overload_threshold_ticks: 1,
                failure_threshold_ticks: 2,
                max_request_retries: 1,
                backpressure_threshold_ticks: 1,
                overload_recovery_ticks: 4,
                history_limit: 30,
            },
        }
    }

    pub fn from_name(name: &str) -> Result<Self, ScenarioError> {
        Self::ALL
            .into_iter()
            .find(|scenario| scenario.name() == name)
            .ok_or_else(|| ScenarioError::Unknown(name.to_string()))
    }
}

impl Default for Scenario {
    fn default() -> Self {
        Self::SteadyState
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioError {
    Unknown(String),
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(name) => {
                write!(formatter, "unknown scenario: {name} (use --list-scenarios)")
            }
        }
    }
}

impl Error for ScenarioError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_selection_resolves_known_names() {
        for scenario in Scenario::ALL {
            assert_eq!(Scenario::from_name(scenario.name()), Ok(scenario));
        }
    }

    #[test]
    fn unknown_scenarios_are_rejected_clearly() {
        let error = Scenario::from_name("missing").expect_err("scenario should be rejected");

        assert_eq!(
            error.to_string(),
            "unknown scenario: missing (use --list-scenarios)"
        );
    }

    #[test]
    fn scenario_presets_have_deterministic_config_values() {
        assert_eq!(Scenario::SteadyState.config().requests_per_tick, 4);
        assert_eq!(Scenario::SteadyState.config().service_capacity_per_tick, 2);
        assert_eq!(Scenario::PressureRamp.config().requests_per_tick, 6);
        assert_eq!(Scenario::PressureRamp.config().max_queue_size, 24);
        assert_eq!(Scenario::RetryStorm.config().max_request_retries, 3);
        assert_eq!(Scenario::RetryStorm.config().failure_threshold_ticks, 2);
        assert_eq!(Scenario::DualFailure.config().requests_per_tick, 10);
        assert_eq!(Scenario::DualFailure.config().max_queue_size, 12);
    }

    #[test]
    fn default_scenario_is_stable_baseline() {
        let scenario = Scenario::default();
        let config = scenario.config();

        assert_eq!(scenario, Scenario::SteadyState);
        assert_eq!(
            config.requests_per_tick,
            config.service_capacity_per_tick * 2
        );
    }
}
