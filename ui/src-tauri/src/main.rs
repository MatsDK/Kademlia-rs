#![allow(non_snake_case)]
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ipc;
mod manager;
mod node;

use ipc::{Api, ApiImpl};
use manager::Manager;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let manager = Arc::new(Mutex::new(Manager::default()));
    tauri::Builder::default()
        .invoke_handler(taurpc::create_ipc_handler(
            ApiImpl { manager }.into_handler(),
        ))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
