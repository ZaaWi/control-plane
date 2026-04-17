use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{App, FocusedComponent},
    simulation::{RecentSummary, ServiceId, ServiceNode, ServiceState, Simulation},
};

const QUEUE_WARNING_RATIO: f64 = 0.75;
const QUEUE_FULL_RATIO: f64 = 1.0;
const QUEUE_VISIBLE_SLOTS: usize = 24;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(8),
            Constraint::Length(3),
        ])
        .split(area);

    render_header(frame, chunks[0], app);
    render_flow(frame, chunks[1], app);
    render_stats(frame, chunks[2], app);
    render_footer(frame, chunks[3]);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let simulation = &app.simulation;
    let line = Line::from(vec![
        Span::styled(
            "request-pipeline-sim",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  tick "),
        Span::styled(
            simulation.tick.to_string(),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  A "),
        Span::styled(
            simulation.service_a.state.label(),
            service_state_style(simulation.service_a.state),
        ),
        Span::raw("  B "),
        Span::styled(
            simulation.service_b.state.label(),
            service_state_style(simulation.service_b.state),
        ),
        Span::raw("  focus "),
        Span::styled(app.focused_component().label(), focus_style()),
        Span::raw("  bp "),
        Span::styled(
            backpressure_label(simulation),
            backpressure_style(simulation.backpressure_active),
        ),
    ]);

    frame.render_widget(
        Paragraph::new(line)
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center),
        area,
    );
}

fn render_flow(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let block = Block::default().title("System Flow").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(5)])
        .split(inner);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(21),
            Constraint::Length(4),
            Constraint::Min(28),
        ])
        .split(rows[0]);

    render_generator(frame, top[0], app);
    render_flow_arrow(frame, top[1]);
    render_queue(frame, top[2], app);

    let services = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    render_service(frame, services[0], app, ServiceId::A);
    render_service(frame, services[1], app, ServiceId::B);
}

fn render_generator(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let simulation = &app.simulation;
    let text = vec![
        Line::from(format!(
            "rate: {}/{}",
            simulation.last_generated, simulation.config.requests_per_tick
        )),
        Line::from(format!("generated: {}", simulation.generated)),
        Line::from(format!("bp: {}", backpressure_label(simulation))),
    ];

    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title("Generator")
                    .borders(Borders::ALL)
                    .border_style(component_border_style(
                        FocusedComponent::Generator,
                        app.focused_component(),
                    )),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_flow_arrow(frame: &mut Frame<'_>, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Length(1),
            Constraint::Percentage(55),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new("->")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center),
        rows[1],
    );
}

fn render_queue(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let simulation = &app.simulation;
    let ratio = queue_ratio(simulation);
    let block = Block::default()
        .title("Shared Queue")
        .borders(Borders::ALL)
        .border_style(if app.focused_component() == FocusedComponent::Queue {
            focus_style()
        } else {
            queue_style(ratio)
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(1)])
        .split(inner);

    let text = vec![
        Line::from(format!(
            "depth: {} / {}",
            simulation.queue_depth(),
            simulation.config.max_queue_size
        )),
        queue_slots(simulation),
    ];

    frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), layout[0]);

    frame.render_widget(
        Gauge::default()
            .gauge_style(queue_style(ratio))
            .label(format!("{:.0}% full", ratio * 100.0))
            .ratio(ratio.clamp(0.0, 1.0)),
        layout[1],
    );
}

fn render_service(frame: &mut Frame<'_>, area: Rect, app: &App, id: ServiceId) {
    let service = app.simulation.service(id);
    let text = vec![
        Line::from(vec![
            Span::raw("state: "),
            Span::styled(service.state.label(), service_state_style(service.state)),
            Span::raw(format!(
                " p{} r{}",
                service.pressure_ticks, service.recovery_ticks
            )),
        ]),
        Line::from(format!(
            "cap: {}/tick last: {}",
            service.capacity_per_tick, service.last_processed
        )),
        Line::from(format!(
            "done: {} fail: {} rt: {}",
            service.processed, service.failed_in_service, service.retry_attempts
        )),
    ];

    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title(service.label())
                    .borders(Borders::ALL)
                    .border_style(service_border_style(id, app.focused_component(), service)),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_stats(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let simulation = &app.simulation;
    let summary = simulation.recent_summary();
    let latest_history = simulation.history().back();
    let history_tick = latest_history.map_or(0, |entry| entry.tick);
    let last_accepted = latest_history.map_or(0, |entry| entry.accepted);
    let last_retry_exhausted = latest_history.map_or(0, |entry| entry.retry_exhausted);
    let last_failed_in_service = latest_history.map_or(0, |entry| entry.failed_in_service);
    let text = vec![
        Line::from(format!(
            "scn {} | tick {} | hist {} | gen {} last {} | q {}/{}",
            app.scenario().name(),
            simulation.tick,
            history_tick,
            simulation.generated,
            simulation.last_generated,
            simulation.queue_depth(),
            simulation.config.max_queue_size
        )),
        Line::from(format!(
            "done {} | svcfail {} last {} | retry {} ex {} | acc {} ex {} | bp {}",
            simulation.total_processed(),
            simulation.total_failed_in_service(),
            last_failed_in_service,
            simulation.retry_attempts,
            simulation.retry_exhausted,
            last_accepted,
            last_retry_exhausted,
            backpressure_label(simulation)
        )),
        Line::from(format!(
            "recent avg gen {:.1} done {:.1} drop {:.1} retry {:.1}",
            summary.avg_generated, summary.avg_processed, summary.avg_dropped, summary.avg_retried
        )),
        Line::from(format!(
            "trend {} | bp {}/{} | signals {}",
            summary.queue_trend.label(),
            summary.backpressure_ticks,
            summary.window,
            status_signal_labels(simulation)
        )),
        Line::from(format!(
            "A {} p{} r{} done {} | B {} p{} r{} done {}",
            simulation.service_a.state.label(),
            simulation.service_a.pressure_ticks,
            simulation.service_a.recovery_ticks,
            simulation.service_a.processed,
            simulation.service_b.state.label(),
            simulation.service_b.pressure_ticks,
            simulation.service_b.recovery_ticks,
            simulation.service_b.processed
        )),
        focused_details(app),
    ];

    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title("Observability")
                    .borders(Borders::ALL),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(
        Paragraph::new("Tab focus | r restart focused failed service | q / Esc / Ctrl-C exit")
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center),
        area,
    );
}

fn focused_details(app: &App) -> Line<'static> {
    let simulation = &app.simulation;

    match app.focused_component() {
        FocusedComponent::Generator => Line::from(vec![
            Span::styled("focus Generator", focus_style()),
            Span::raw(format!(
                " | rate {}/{} | bp {} | accepted {}",
                simulation.last_generated,
                simulation.config.requests_per_tick,
                backpressure_label(simulation),
                simulation.accepted
            )),
        ]),
        FocusedComponent::Queue => Line::from(vec![
            Span::styled("focus Queue", focus_style()),
            Span::raw(format!(
                " | depth {}/{} | trend {} | bp {}/{}",
                simulation.queue_depth(),
                simulation.config.max_queue_size,
                simulation.recent_summary().queue_trend.label(),
                simulation.recent_summary().backpressure_ticks,
                simulation.recent_summary().window
            )),
        ]),
        FocusedComponent::ServiceA => service_details(
            simulation.service(ServiceId::A),
            simulation.recent_summary(),
        ),
        FocusedComponent::ServiceB => service_details(
            simulation.service(ServiceId::B),
            simulation.recent_summary(),
        ),
    }
}

fn service_details(service: &ServiceNode, summary: RecentSummary) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("focus {}", service.label()), focus_style()),
        Span::raw(" | state "),
        Span::styled(service.state.label(), service_state_style(service.state)),
        Span::raw(format!(
            " | p{} r{} | rt {} ex {} | avg done {:.1}",
            service.pressure_ticks,
            service.recovery_ticks,
            service.retry_attempts,
            service.retry_exhausted,
            summary.avg_processed
        )),
    ])
}

fn status_signal_labels(simulation: &Simulation) -> String {
    let signals = simulation.status_signals();

    if signals.is_empty() {
        return "none".to_string();
    }

    signals
        .iter()
        .map(|signal| match signal {
            crate::simulation::StatusSignal::QueueRising => "q up",
            crate::simulation::StatusSignal::SustainedBackpressure => "bp",
            crate::simulation::StatusSignal::RetryActivity => "retry",
            crate::simulation::StatusSignal::BothServicesUnhealthy => "svc bad",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn queue_ratio(simulation: &Simulation) -> f64 {
    if simulation.config.max_queue_size == 0 {
        0.0
    } else {
        simulation.queue_depth() as f64 / simulation.config.max_queue_size as f64
    }
}

fn queue_slots(simulation: &Simulation) -> Line<'static> {
    let max_size = simulation.config.max_queue_size;
    let visible_slots = max_size.clamp(1, QUEUE_VISIBLE_SLOTS);
    let filled_slots = if max_size == 0 {
        0
    } else {
        simulation
            .queue_depth()
            .saturating_mul(visible_slots)
            .div_ceil(max_size)
    };

    let empty_slots = visible_slots.saturating_sub(filled_slots);
    let filled = "#".repeat(filled_slots);
    let empty = ".".repeat(empty_slots);

    Line::from(vec![
        Span::raw("["),
        Span::styled(filled, queue_style(queue_ratio(simulation))),
        Span::styled(empty, Style::default().fg(Color::DarkGray)),
        Span::raw("]"),
    ])
}

fn queue_style(ratio: f64) -> Style {
    if ratio >= QUEUE_FULL_RATIO {
        Style::default().fg(Color::Red)
    } else if ratio >= QUEUE_WARNING_RATIO {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    }
}

fn service_state_style(state: ServiceState) -> Style {
    match state {
        ServiceState::Healthy => Style::default().fg(Color::Green),
        ServiceState::Overloaded => Style::default().fg(Color::Yellow),
        ServiceState::Failed => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

fn backpressure_label(simulation: &Simulation) -> &'static str {
    if simulation.backpressure_active {
        "on"
    } else {
        "off"
    }
}

fn backpressure_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    }
}

fn component_border_style(
    component: FocusedComponent,
    focused_component: FocusedComponent,
) -> Style {
    if component == focused_component {
        focus_style()
    } else {
        Style::default()
    }
}

fn service_border_style(
    id: ServiceId,
    focused_component: FocusedComponent,
    service: &ServiceNode,
) -> Style {
    let focused = match (id, focused_component) {
        (ServiceId::A, FocusedComponent::ServiceA) => true,
        (ServiceId::B, FocusedComponent::ServiceB) => true,
        _ => false,
    };

    if focused {
        focus_style()
    } else {
        service_state_style(service.state)
    }
}

fn focus_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}
