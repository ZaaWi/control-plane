mod app;
mod rpc;
mod scenario;
mod simulation;
mod ui;

use std::{
    env,
    error::Error,
    io::{self, Stdout},
    time::{Duration, Instant},
};

use app::App;
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use rpc::{RpcServer, SnapshotStore};
use scenario::Scenario;

const TICK_INTERVAL: Duration = Duration::from_millis(200);

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_args(env::args().skip(1))?;

    if config.list_scenarios {
        print_scenarios();
        return Ok(());
    }

    let app = App::new(config.max_ticks, config.scenario);

    let rpc = if let Some(addr) = config.rpc_addr.as_deref() {
        let store = SnapshotStore::new(app.scenario(), &app.simulation);
        let server = RpcServer::start(addr, store.clone())?;
        Some((server, store))
    } else {
        None
    };

    let (mut terminal, terminal_guard) = setup_terminal()?;
    let result = run(
        &mut terminal,
        app,
        rpc.as_ref().map(|(_, store)| store.clone()),
    );

    drop(terminal_guard);
    result
}

#[derive(Debug)]
struct RunConfig {
    max_ticks: Option<u64>,
    rpc_addr: Option<String>,
    scenario: Scenario,
    list_scenarios: bool,
}

fn parse_args<I>(mut args: I) -> Result<RunConfig, Box<dyn Error>>
where
    I: Iterator<Item = String>,
{
    let mut max_ticks = None;
    let mut rpc_addr = None;
    let mut scenario = Scenario::default();
    let mut list_scenarios = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--max-ticks" => {
                let value = args
                    .next()
                    .ok_or("--max-ticks requires an integer value")?
                    .parse::<u64>()?;
                max_ticks = Some(value);
            }
            "--rpc-addr" => {
                let value = args.next().ok_or("--rpc-addr requires an address")?;
                rpc_addr = Some(value);
            }
            "--scenario" => {
                let value = args.next().ok_or("--scenario requires a scenario name")?;
                scenario = Scenario::from_name(&value)?;
            }
            "--list-scenarios" => {
                list_scenarios = true;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: control-plane [--scenario NAME] [--max-ticks N] [--rpc-addr ADDR]"
                );
                println!("       control-plane --list-scenarios");
                println!();
                println!("Options:");
                println!("  --scenario NAME      run a built-in scenario (default: steady-state)");
                println!("  --max-ticks N        exit after N simulation ticks");
                println!("  --rpc-addr ADDR      serve read-only snapshots on ADDR");
                println!("  --list-scenarios     print scenario names and descriptions");
                println!();
                println!("Controls: Tab cycles focus; r restarts a focused failed service; q, Esc, or Ctrl-C exits.");
                println!("RPC: read-only newline-delimited JSON over TCP when --rpc-addr is set.");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    Ok(RunConfig {
        max_ticks,
        rpc_addr,
        scenario,
        list_scenarios,
    })
}

fn print_scenarios() {
    for scenario in Scenario::ALL {
        println!("{} - {}", scenario.name(), scenario.description());
    }
}

fn setup_terminal() -> io::Result<(Tui, TerminalGuard)> {
    enable_raw_mode()?;

    if let Err(error) = execute!(io::stdout(), EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error);
    }

    let terminal_guard = TerminalGuard;
    execute!(io::stdout(), Hide)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    Ok((terminal, terminal_guard))
}

fn run(
    terminal: &mut Tui,
    mut app: App,
    snapshot_store: Option<SnapshotStore>,
) -> Result<(), Box<dyn Error>> {
    let mut last_tick = Instant::now();

    while !app.should_quit() {
        terminal.draw(|frame| ui::render(frame, &app))?;

        let timeout = TICK_INTERVAL
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.quit()
                        }
                        KeyCode::Tab => app.cycle_focus(),
                        KeyCode::Char('r') => {
                            app.restart_focused_service();
                            publish_snapshot(&snapshot_store, &app);
                        }
                        KeyCode::Char('q') | KeyCode::Esc => app.quit(),
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= TICK_INTERVAL {
            app.tick();
            publish_snapshot(&snapshot_store, &app);
            last_tick = Instant::now();
        }
    }

    Ok(())
}

fn publish_snapshot(snapshot_store: &Option<SnapshotStore>, app: &App) {
    if let Some(store) = snapshot_store {
        store.update(&app.simulation);
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args<'a>(values: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
        values.iter().map(|value| value.to_string())
    }

    #[test]
    fn parse_args_uses_default_scenario_when_omitted() {
        let config = parse_args(args(&["--max-ticks", "5"])).expect("parse args");

        assert_eq!(config.max_ticks, Some(5));
        assert_eq!(config.scenario, Scenario::default());
        assert!(!config.list_scenarios);
    }

    #[test]
    fn parse_args_accepts_named_scenario() {
        let config = parse_args(args(&["--scenario", "retry-storm"])).expect("parse args");

        assert_eq!(config.scenario, Scenario::RetryStorm);
    }

    #[test]
    fn parse_args_rejects_invalid_scenario_names() {
        let error = parse_args(args(&["--scenario", "unknown"])).expect_err("parse should fail");

        assert_eq!(
            error.to_string(),
            "unknown scenario: unknown (use --list-scenarios)"
        );
    }

    #[test]
    fn parse_args_supports_listing_scenarios_without_terminal_startup() {
        let config = parse_args(args(&["--list-scenarios"])).expect("parse args");

        assert!(config.list_scenarios);
    }
}
