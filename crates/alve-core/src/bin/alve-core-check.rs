//! JSON-lines acceptance driver, used only for automated cross-language checks.
use serde_json::{json, Value};
use std::{
    io::{self, BufRead},
    path::PathBuf,
};
fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("provide a test data directory");
    let mut engine =
        alve_core::api::Engine::new(PathBuf::from(path).join("memory.alve")).expect("test engine");
    for line in io::stdin().lock().lines() {
        let result = (|| {
            let v: Value = serde_json::from_str(
                &line.map_err(|_| alve_core::Error::new(400, "Invalid input."))?,
            )?;
            engine.request(
                v["method"].as_str().unwrap_or("GET"),
                v["path"].as_str().unwrap_or("/api/status"),
                v.get("body").unwrap_or(&json!({})),
                v["token"].as_str().unwrap_or(""),
            )
        })();
        let reply = match result {
            Ok(v) => json!({"status":200,"data":v}),
            Err(e) => json!({"status":e.status,"data":{"error":e.message}}),
        };
        println!("{reply}");
    }
}
