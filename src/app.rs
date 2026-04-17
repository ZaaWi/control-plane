use crate::{
    scenario::Scenario,
    simulation::{ServiceId, Simulation},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedComponent {
    Generator,
    Queue,
    ServiceA,
    ServiceB,
}

impl FocusedComponent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Generator => "Generator",
            Self::Queue => "Queue",
            Self::ServiceA => "Service A",
            Self::ServiceB => "Service B",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Generator => Self::Queue,
            Self::Queue => Self::ServiceA,
            Self::ServiceA => Self::ServiceB,
            Self::ServiceB => Self::Generator,
        }
    }
}

pub struct App {
    pub simulation: Simulation,
    scenario: Scenario,
    focused_component: FocusedComponent,
    should_quit: bool,
    max_ticks: Option<u64>,
}

impl App {
    pub fn new(max_ticks: Option<u64>, scenario: Scenario) -> Self {
        Self {
            simulation: Simulation::new(scenario.config()),
            scenario,
            focused_component: FocusedComponent::Generator,
            should_quit: false,
            max_ticks,
        }
    }

    pub fn tick(&mut self) {
        self.simulation.tick();

        if self
            .max_ticks
            .is_some_and(|max_ticks| self.simulation.tick >= max_ticks)
        {
            self.quit();
        }
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn focused_component(&self) -> FocusedComponent {
        self.focused_component
    }

    pub fn scenario(&self) -> Scenario {
        self.scenario
    }

    pub fn cycle_focus(&mut self) {
        self.focused_component = self.focused_component.next();
    }

    pub fn restart_focused_service(&mut self) {
        match self.focused_component {
            FocusedComponent::ServiceA => self.simulation.restart_service(ServiceId::A),
            FocusedComponent::ServiceB => self.simulation.restart_service(ServiceId::B),
            FocusedComponent::Generator | FocusedComponent::Queue => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::ServiceState;

    fn app() -> App {
        App::new(None, Scenario::default())
    }

    #[test]
    fn tab_focus_cycles_through_components() {
        let mut app = app();

        assert_eq!(app.focused_component(), FocusedComponent::Generator);

        app.cycle_focus();
        assert_eq!(app.focused_component(), FocusedComponent::Queue);

        app.cycle_focus();
        assert_eq!(app.focused_component(), FocusedComponent::ServiceA);

        app.cycle_focus();
        assert_eq!(app.focused_component(), FocusedComponent::ServiceB);

        app.cycle_focus();
        assert_eq!(app.focused_component(), FocusedComponent::Generator);
    }

    #[test]
    fn restart_action_only_applies_to_focused_failed_service() {
        let mut app = app();
        app.simulation.service_a.state = ServiceState::Failed;
        app.simulation.service_a.pressure_ticks = 10;
        app.simulation.service_b.state = ServiceState::Failed;
        app.simulation.service_b.pressure_ticks = 12;

        app.restart_focused_service();

        assert_eq!(app.simulation.service_a.state, ServiceState::Failed);
        assert_eq!(app.simulation.service_a.pressure_ticks, 10);
        assert_eq!(app.simulation.service_b.state, ServiceState::Failed);
        assert_eq!(app.simulation.service_b.pressure_ticks, 12);

        app.cycle_focus();
        app.cycle_focus();
        app.restart_focused_service();

        assert_eq!(app.simulation.service_a.state, ServiceState::Healthy);
        assert_eq!(app.simulation.service_a.pressure_ticks, 0);
        assert_eq!(app.simulation.service_b.state, ServiceState::Failed);
        assert_eq!(app.simulation.service_b.pressure_ticks, 12);
    }

    #[test]
    fn app_uses_selected_scenario_config() {
        let app = App::new(None, Scenario::RetryStorm);

        assert_eq!(app.scenario(), Scenario::RetryStorm);
        assert_eq!(app.simulation.config, Scenario::RetryStorm.config());
    }
}
