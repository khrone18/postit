#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod note;
mod store;
mod theme;

fn main() {
    if let Err(err) = app::run() {
        eprintln!("Post-it: {err}");
        std::process::exit(1);
    }
}
