mod windows_fg;

// use full_palette::{ORANGE, PURPLE};
use plotters::prelude::*;
use rusqlite::{params, Connection, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal::ctrl_c;
use tokio::time;

#[derive(Debug)]
pub struct AppUsage {
    pub name: String,
    pub duration: u64,
}

const IDLE_CHECK_SECS: i32 = 5;
const IDLE_PERIOD: u64 = 30;

pub fn create_usage_table(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS app_usage (
            app_name TEXT PRIMARY KEY,
            duration INTEGER NOT NULL
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

    // let colors = vec![
    //     &RED, &BLUE, &GREEN, &CYAN, &MAGENTA, &YELLOW, &BLACK
    // ];
    let colors = vec![
        &MAGENTA
    ];

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
                    .transform(FontTransform::Rotate90),
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
                    let duration = windows_fg::get_last_input().as_secs();
                    idle = duration > IDLE_PERIOD;
                    i = 0;
                }

                if !idle {
                    let (window_pid, window_title) = windows_fg::get_active_window();

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
    let (window_pid, _) = windows_fg::get_active_window();

    if window_pid == 0 {
        return;
    }

    let process_name = window_title.to_string();

    conn.execute(
        "INSERT INTO app_usage (app_name, duration) VALUES (?1, 1)
         ON CONFLICT(app_name) DO UPDATE SET duration = duration + 1",
        params![process_name],
    )
    .unwrap();
}

#[tokio::main]
async fn main() -> Result<()> {
    let conn = Arc::new(Connection::open("app_usage.db")?);

    create_usage_table(&conn)?;

    println!("App Usage Tracker started!");

    track_processes(conn.clone()).await;

    Ok(())
}
