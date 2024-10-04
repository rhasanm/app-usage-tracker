mod active_window_tracker;

use plotters::prelude::*;
use rusqlite::{params, Connection, Result as RusqliteResult};
use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::signal::ctrl_c;
use tokio::time;
use windows_service::service::{
    ServiceAccess, ServiceErrorControl, ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

const SERVICE_NAME: &str = "AppUsageTracker";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

#[derive(Error, Debug)]
enum AppError {
    #[error("Windows service error: {0}")]
    WindowsService(#[from] windows_service::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
}

#[derive(Debug)]
pub struct AppUsage {
    pub name: String,
    pub duration: u64,
}

const IDLE_CHECK_SECS: i32 = 5;
const IDLE_PERIOD: u64 = 30;

pub fn create_usage_table(conn: &Connection) -> RusqliteResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS app_usage (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task TEXT NOT NULL,
            app_name TEXT NOT NULL,
            duration INTEGER NOT NULL,
            usage_date DATE NOT NULL,
            UNIQUE (task, app_name, usage_date)
        )",
        [],
    )?;
    Ok(())
}

pub fn get_usage_data_from_db(conn: &Connection) -> HashMap<String, u64> {
    let mut stmt = conn
        .prepare("SELECT app_name, SUM(duration) FROM app_usage GROUP BY app_name")
        .unwrap();

    let usage_iter = stmt
        .query_map([], |row| {
            let app_name: String = row.get(0)?;
            let duration: u64 = row.get(1)?;
            Ok((app_name, duration))
        })
        .unwrap();

    let mut usage_data = HashMap::new();

    for usage in usage_iter {
        let (app_name, duration) = usage.unwrap();
        usage_data.insert(app_name, duration);
    }

    usage_data
}

pub fn draw_usage_graph_from_db(conn: &Connection) {
    let usage_data = get_usage_data_from_db(conn);

    let root = BitMapBackend::new("usage_graph.png", (800, 600)).into_drawing_area();
    root.fill(&WHITE).unwrap();

    let max_duration = usage_data.values().max().unwrap_or(&0);
    let y_max = if *max_duration == 0 { 1 } else { *max_duration };
    let y_axis_max = 100;

    let mut chart = ChartBuilder::on(&root)
        .caption("Application Usage Over Time", ("sans-serif", 50).into_font())
        .margin(10)
        .x_label_area_size(30)
        .y_label_area_size(40)
        .build_cartesian_2d(0..usage_data.len() as i32, 0..y_axis_max)
        .unwrap();

    chart.configure_mesh().draw().unwrap();

    let colors = vec![&MAGENTA];
    let bar_width = 1;
    let default_font_size = 12;

    for (i, (app_name, duration)) in usage_data.iter().enumerate() {
        let color = colors[i % colors.len()];
        let normalized_duration = (*duration as f32 / y_max as f32 * y_axis_max as f32) as i32;

        chart
            .draw_series(std::iter::once(Rectangle::new(
                [(i as i32, 0), (i as i32 + bar_width, normalized_duration)],
                color.filled(),
            )))
            .unwrap();

        let text_color = &BLACK;
        let text_position = (i as i32 + bar_width / 2, normalized_duration / 2);

        chart
            .draw_series(std::iter::once(Text::new(
                app_name.clone(),
                text_position,
                ("sans-serif", default_font_size)
                    .into_font()
                    .style(FontStyle::Normal)
                    .color(text_color)
                    // .transform(FontTransform::Rotate90),
            )))
            .unwrap();
    }

    chart
        .configure_series_labels()
        .border_style(&BLACK)
        .draw()
        .unwrap();
}

pub async fn track_processes(conn: Arc<Connection>) {
    let mut interval = time::interval(Duration::from_secs(1));
    let mut graph_interval = time::interval(Duration::from_secs(60));
    let mut i = 0;
    let mut idle = false;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                i += 1;

                if i == IDLE_CHECK_SECS {
                    let duration = active_window_tracker::get_last_input().as_secs();
                    idle = duration > IDLE_PERIOD;
                    i = 0;
                }

                if !idle {
                    let (window_pid, window_title) = active_window_tracker::get_active_window();

                    if window_pid != 0 {
                        get_process(&conn, &window_title);
                    }
                }
            },
            _ = graph_interval.tick() => {
                println!("Generating usage graph...");
                draw_usage_graph_from_db(&conn);
            },
            _ = ctrl_c() => {
                println!("Received shutdown signal, generating final usage graph...");
                draw_usage_graph_from_db(&conn);
                break;
            },
        }
    }
}

pub fn get_process(conn: &Connection, window_title: &str) {
    let (window_pid, _) = active_window_tracker::get_active_window();

    if window_pid == 0 {
        return;
    }

    let parts: Vec<&str> = window_title.split(|c| c == '-' || c == '|').collect();

    let app_name = parts.last().unwrap_or(&"").trim().to_string();
    let task = parts[..parts.len() - 1].join(" - ").trim().to_string();

    let duration = 1;
    let usage_date = chrono::Utc::now().date_naive();

    let usage_date_str = usage_date.format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO app_usage (task, app_name, duration, usage_date)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(task, app_name, usage_date) DO UPDATE SET duration = duration + ?3",
        params![task, app_name, duration, usage_date_str],
    )
    .unwrap();
}

fn install_service() -> Result<(), AppError> {
    let manager_access = ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service_info = windows_service::service::ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from("App Usage Tracker Service"),
        service_type: SERVICE_TYPE,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: std::env::current_exe()?,
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };

    service_manager.create_service(&service_info, ServiceAccess::START | ServiceAccess::STOP)?;
    Ok(())
}

fn uninstall_service() -> Result<(), AppError> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service = service_manager.open_service(SERVICE_NAME, ServiceAccess::DELETE)?;

    service.delete()?;
    Ok(())
}

fn start_service() -> Result<(), AppError> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service = service_manager.open_service(SERVICE_NAME, ServiceAccess::START)?;

    service.start(&[] as &[OsString])?;
    Ok(())
}

fn stop_service() -> Result<(), AppError> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service = service_manager.open_service(SERVICE_NAME, ServiceAccess::STOP)?;

    service.stop()?;
    Ok(())
}

fn delete_service() -> Result<(), AppError> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service =
        service_manager.open_service(SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::DELETE)?;

    let status = service.query_status()?;
    if status.current_state == ServiceState::Running {
        service.stop()?;
    }

    service.delete()?;
    Ok(())
}

fn get_service_status() -> Result<ServiceState, AppError> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service = service_manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)?;

    let status = service.query_status()?;
    Ok(status.current_state)
}

async fn service_main() {
    let conn = Arc::new(Connection::open("app_usage.db").expect("Could not open database"));
    create_usage_table(&conn).expect("Could not create usage table");

    track_processes(conn.clone()).await;
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "--install" => {
                install_service()?;
                println!("Service installed successfully.");
                return Ok(());
            },
            "--uninstall" => {
                uninstall_service()?;
                println!("Service uninstalled successfully.");
                return Ok(());
            },
            "--start" => {
                start_service()?;
                println!("Service started successfully.");
                return Ok(());
            },
            "--stop" => {
                stop_service()?;
                println!("Service stopped successfully.");
                return Ok(());
            },
            "--delete" => {
                delete_service()?;
                println!("Service deleted successfully.");
                return Ok(());
            },
            "--status" => {
                match get_service_status() {
                    Ok(state) => {
                        println!("Service status: {:?}", state);
                    },
                    Err(e) => {
                        eprintln!("Failed to retrieve service status: {:?}", e);
                    }
                }
                return Ok(());
            },
            _ => eprintln!("Unknown command. Use --install, --uninstall, --start, --stop, --delete, or --status."),
        }
    }

    let conn = Arc::new(Connection::open("app_usage.db")?);
    create_usage_table(&conn)?;

    service_main().await;

    Ok(())
}
