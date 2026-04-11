//! `croniq quickstart` — zero-to-running in one command.

use std::path::Path;

use miette::{Result, miette};

const SAMPLE_CRONIQFILE: &str = r#"# Croniqfile — Quickstart

server {
  listen :4000
  data_dir .data
}

observability {
  metrics { listen :9900; path /metrics }
}

defaults {
  timeout 5m
  retry exponential { max_attempts 3; base 2s; cap 30s }
}

# Your first job — runs every minute
job hello:world {
  every 1 minutes
  timeout 30s
  metadata { created_by quickstart }
}

# A health check every 5 minutes
job ops:heartbeat {
  every 5 minutes
  timeout 10s
}
"#;

pub fn quickstart(data_dir: &Path, croniqfile: &Path, password: &str) -> Result<()> {
    // 1. Create Croniqfile if it doesn't exist
    if !croniqfile.exists() {
        std::fs::write(croniqfile, SAMPLE_CRONIQFILE)
            .map_err(|e| miette!("Failed to write Croniqfile: {e}"))?;
        println!("Created {}", croniqfile.display());
    } else {
        println!("Using existing {}", croniqfile.display());
    }

    // 2. Init database
    super::init::init(data_dir, "admin", Some(password))?;

    // 3. Print next steps
    println!();
    println!("=== Quickstart complete! ===");
    println!();
    println!("Start the server:");
    println!("  croniq-server --config {} --data-dir {} --ui-dir ui/dist", croniqfile.display(), data_dir.display());
    println!();
    println!("Or without UI:");
    println!("  croniq-server --config {} --data-dir {}", croniqfile.display(), data_dir.display());
    println!();
    println!("Then open: http://localhost:4000");
    println!("Login:     admin / {}", password);
    println!();
    println!("Your first job (hello:world) fires every minute.");
    println!("Connect a runner to process it:");
    println!();
    println!("  // In your runner code:");
    println!("  let runner = CroniqRunner::builder(\"http://localhost:4000\", \"my-runner\")");
    println!("      .api_key(\"<your-api-key>\")");
    println!("      .build();");
    println!("  runner.register(\"hello:world\", |ctx| async {{ Ok(()) }}).await;");
    println!("  runner.start().await;");

    Ok(())
}
