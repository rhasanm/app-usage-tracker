mod windows_fg;

use full_palette::{ORANGE, PURPLE};
use plotters::prelude::*;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time;

#[derive(Debug)]
pub struct AppUsage {
    pub name: String,
    pub duration: u64, // Duration in seconds
}

const IDLE_CHECK_SECS: i32 = 5;
const IDLE_PERIOD: u64 = 30;

pub fn draw_usage_graph(usage_data: &HashMap<String, u64>) {
    let root = BitMapBackend::new("usage_graph.png", (800, 600)).into_drawing_area();
    root.fill(&WHITE).unwrap();

    let mut chart = ChartBuilder::on(&root)
        .caption(
            "Application Usage Over Time",
            ("sans-serif", 50).into_font(),
        )
        .margin(10)
        .x_label_area_size(30)
        .y_label_area_size(40)
        .build_cartesian_2d(
            0..usage_data.len() as i32,
            0..*usage_data.values().max().unwrap() as i32,
        )
        .unwrap();

    chart.configure_mesh().draw().unwrap();

    // Define a list of colors to be used for each app
    let colors = vec![
        &RED, &BLUE, &GREEN, &CYAN, &MAGENTA, &YELLOW, &BLACK, &ORANGE, &PURPLE,
    ];

    // Iterate over the usage data and plot each application with a different color
    for (i, (app_name, duration)) in usage_data.iter().enumerate() {
        let color = colors[i % colors.len()]; // Cycle through colors if more apps than colors
        chart
            .draw_series(LineSeries::new(
                vec![(i as i32, 0), (i as i32, *duration as i32)], // Replace with your actual data points
                color,
            ))
            .unwrap()
            .label(app_name)
            .legend(move |(x, y)| Circle::new((x, y), 5, color));
    }

    chart
        .configure_series_labels()
        .border_style(&BLACK)
        .draw()
        .unwrap();
}

pub async fn track_processes() -> HashMap<String, u64> {
    // Change return type to track duration in seconds
    let mut interval = time::interval(Duration::from_secs(1));
    let mut i = 0;
    let mut idle = false;

    // HashMap to store application usage data
    let mut usage_data: HashMap<String, u64> = HashMap::new();

    let max_duration_secs = 30; // Track for 1 hour
    let start_time = tokio::time::Instant::now();

    loop {
        i += 1;
        interval.tick().await;

        if i == IDLE_CHECK_SECS {
            let duration = windows_fg::get_last_input().as_secs();
            idle = duration > IDLE_PERIOD;
            i = 0;
        }

        if !idle {
            // Fetch the window title and pass it to get_process
            let (window_pid, window_title) = windows_fg::get_active_window();

            // Only call get_process if the window_pid is valid
            if window_pid != 0 {
                get_process(&mut usage_data, &window_title);
            }
        }

        if start_time.elapsed().as_secs() >= max_duration_secs {
            break;
        }
    }

    usage_data // Return duration data
}

pub fn get_process(usage_data: &mut HashMap<String, u64>, window_title: &str) {
    let (window_pid, _) = windows_fg::get_active_window();

    if window_pid == 0 {
        return;
    }

    // Use a dummy process name for illustration
    let process_name = window_title.to_string(); // Replace with actual process name logic

    // Increment the duration for the active application
    let entry = usage_data.entry(process_name).or_insert(0);
    *entry += 1; // Increment duration by 1 second
}

#[tokio::main]
async fn main() {
    println!("App Usage Tracker started!");

    let usage_data = track_processes().await;

    draw_usage_graph(&usage_data);
}
