#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use spice_route_core::{
    engine::Engine,
    ipc::{Request, Server},
};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;

fn main() {
    if let Err(error) = serve() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let data_dir = match args.next().as_deref().and_then(|value| value.to_str()) {
        Some("--data-dir") => PathBuf::from(args.next().ok_or("--data-dir requires a path")?),
        None => dirs::data_local_dir()
            .ok_or("The local application data folder is unavailable")?
            .join("com.spiceroute.codexsync"),
        _ => return Err("Supported option: --data-dir <folder>".into()),
    };
    if args.next().is_some() || !data_dir.is_absolute() {
        return Err("Use one absolute application-data folder".into());
    }
    let server = Arc::new(Server::new(Arc::new(Engine::new(data_dir)?)));
    let output = Arc::new(Mutex::new(io::stdout()));
    let mut workers: Vec<thread::JoinHandle<()>> = Vec::new();
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let input = io::stdin();
        let mut reader = input.lock();
        loop {
            let mut line = Vec::new();
            // Bound each request before parsing or allocating an unbounded line.
            loop {
                let available = reader.fill_buf()?;
                if available.is_empty() {
                    break;
                }
                let count = available
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(available.len(), |index| index + 1);
                if line.len() + count > MAX_REQUEST_BYTES {
                    server.cancel_all();
                    return Err("The engine request exceeded the 16 MB limit".into());
                }
                line.extend_from_slice(&available[..count]);
                reader.consume(count);
                if line.last() == Some(&b'\n') {
                    break;
                }
            }
            if line.is_empty() {
                break;
            }
            let request: Request = match serde_json::from_slice(&line) {
                Ok(request) => request,
                Err(error) => {
                    let response = serde_json::json!({"id":null,"error":{"message":format!("Invalid request: {error}")}});
                    write_response(&output, &response)?;
                    continue;
                }
            };
            // Reap completed requests so a long-lived frontend does not accumulate
            // progress-poll thread handles.
            let mut index = 0;
            while index < workers.len() {
                if workers[index].is_finished() {
                    let _ = workers.swap_remove(index).join();
                } else {
                    index += 1;
                }
            }
            if workers.len() >= 16 {
                write_response(
                    &output,
                    &serde_json::json!({"id":request.id,"error":{"message":"Too many requests are in progress. Wait for the current request to finish."}}),
                )?;
                continue;
            }
            let server = server.clone();
            let output = output.clone();
            workers.push(thread::spawn(move || {
                let response = server.handle(request);
                if let Err(error) = write_response(&output, &response) {
                    eprintln!("Could not return the engine response: {error}");
                }
            }));
        }
        Ok(())
    })();
    // Closing the frontend cancels chunked work and lets recovery journals reach
    // a consistent state. Never detach a restore worker on normal shutdown.
    server.cancel_all();
    for worker in workers {
        let _ = worker.join();
    }
    result
}

fn write_response(output: &Mutex<io::Stdout>, response: &serde_json::Value) -> io::Result<()> {
    let mut output = output
        .lock()
        .map_err(|_| io::Error::other("Response pipe unavailable"))?;
    serde_json::to_writer(&mut *output, response)?;
    output.write_all(b"\n")?;
    output.flush()
}
