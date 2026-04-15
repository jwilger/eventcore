mod backends;
mod config;
mod domain;
mod metrics;
mod runner;
mod scenarios;

use clap::{Parser, Subcommand};

use crate::config::{BackendChoice, StressConfig, parse_duration};

#[derive(Parser)]
#[command(
    name = "eventcore-stress",
    about = "Stress testing tool for EventCore backends"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Concurrent single-stream contention test
    Contention {
        /// Backend to test against
        #[arg(long, default_value = "memory", value_enum)]
        backend: BackendChoice,

        /// Number of concurrent tasks
        #[arg(long, default_value_t = 20)]
        concurrency: u32,

        /// Test duration (e.g. "10s", "30s", "1m")
        #[arg(long, value_parser = parse_duration)]
        duration: Option<std::time::Duration>,

        /// Number of iterations (overrides duration)
        #[arg(long)]
        iterations: Option<u64>,
    },

    /// Concurrent multi-stream transfer test
    Transfers {
        /// Backend to test against
        #[arg(long, default_value = "memory", value_enum)]
        backend: BackendChoice,

        /// Number of concurrent tasks
        #[arg(long, default_value_t = 20)]
        concurrency: u32,

        /// Test duration (e.g. "10s", "30s", "1m")
        #[arg(long, value_parser = parse_duration)]
        duration: Option<std::time::Duration>,

        /// Number of iterations (overrides duration)
        #[arg(long)]
        iterations: Option<u64>,

        /// Number of accounts in the transfer pool
        #[arg(long, default_value_t = 10)]
        accounts: u32,
    },

    /// High-throughput sequential append test (per-task streams)
    Throughput {
        /// Backend to test against
        #[arg(long, default_value = "memory", value_enum)]
        backend: BackendChoice,

        /// Number of concurrent tasks
        #[arg(long, default_value_t = 20)]
        concurrency: u32,

        /// Test duration (e.g. "10s", "30s", "1m")
        #[arg(long, value_parser = parse_duration)]
        duration: Option<std::time::Duration>,

        /// Number of iterations (overrides duration)
        #[arg(long)]
        iterations: Option<u64>,
    },

    /// Projection catch-up after concurrent writes
    Projection {
        /// Backend to test against
        #[arg(long, default_value = "memory", value_enum)]
        backend: BackendChoice,

        /// Number of concurrent writer tasks
        #[arg(long, default_value_t = 20)]
        concurrency: u32,

        /// Number of events to write per task
        #[arg(long)]
        iterations: Option<u64>,

        /// Test duration (ignored for projection; iterations controls write volume)
        #[arg(long, value_parser = parse_duration)]
        duration: Option<std::time::Duration>,
    },

    /// Postgres-only pool saturation test
    PoolSaturation {
        /// Backend to test against (must be postgres)
        #[arg(long, default_value = "postgres", value_enum)]
        backend: BackendChoice,

        /// Number of concurrent tasks (default 100 for saturation)
        #[arg(long, default_value_t = 100)]
        concurrency: u32,

        /// Test duration (e.g. "10s", "30s", "1m")
        #[arg(long, value_parser = parse_duration)]
        duration: Option<std::time::Duration>,

        /// Number of iterations (overrides duration)
        #[arg(long)]
        iterations: Option<u64>,
    },

    /// Run all applicable scenarios
    RunAll {
        /// Backend to test against
        #[arg(long, default_value = "memory", value_enum)]
        backend: BackendChoice,

        /// Number of concurrent tasks
        #[arg(long, default_value_t = 20)]
        concurrency: u32,

        /// Test duration per scenario (e.g. "10s", "30s", "1m")
        #[arg(long, value_parser = parse_duration)]
        duration: Option<std::time::Duration>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Contention {
            backend,
            concurrency,
            duration,
            iterations,
        } => {
            let config = StressConfig {
                backend: backend.clone(),
                concurrency,
                duration,
                iterations,
            };
            backends::print_backend_info(&backend);
            if matches!(backend, BackendChoice::Postgres) {
                backends::clean_postgres_database().await?;
            }
            let report = scenarios::contention::run(&config).await;
            print!("{report}");
        }

        Commands::Transfers {
            backend,
            concurrency,
            duration,
            iterations,
            accounts,
        } => {
            let config = StressConfig {
                backend: backend.clone(),
                concurrency,
                duration,
                iterations,
            };
            backends::print_backend_info(&backend);
            if matches!(backend, BackendChoice::Postgres) {
                backends::clean_postgres_database().await?;
            }
            let report = scenarios::transfers::run(&config, accounts).await;
            print!("{report}");
        }

        Commands::Throughput {
            backend,
            concurrency,
            duration,
            iterations,
        } => {
            let config = StressConfig {
                backend: backend.clone(),
                concurrency,
                duration,
                iterations,
            };
            backends::print_backend_info(&backend);
            if matches!(backend, BackendChoice::Postgres) {
                backends::clean_postgres_database().await?;
            }
            let report = scenarios::throughput::run(&config).await;
            print!("{report}");
        }

        Commands::Projection {
            backend,
            concurrency,
            iterations,
            duration,
        } => {
            let config = StressConfig {
                backend: backend.clone(),
                concurrency,
                duration,
                iterations,
            };
            backends::print_backend_info(&backend);
            if matches!(backend, BackendChoice::Postgres) {
                backends::clean_postgres_database().await?;
            }
            let report = scenarios::projection::run(&config).await;
            print!("{report}");
        }

        Commands::PoolSaturation {
            backend,
            concurrency,
            duration,
            iterations,
        } => {
            let config = StressConfig {
                backend: backend.clone(),
                concurrency,
                duration,
                iterations,
            };
            backends::print_backend_info(&backend);
            if matches!(backend, BackendChoice::Postgres) {
                backends::clean_postgres_database().await?;
            }
            if let Some(report) = scenarios::pool_saturation::run(&config).await {
                print!("{report}");
            }
        }

        Commands::RunAll {
            backend,
            concurrency,
            duration,
        } => {
            let config = StressConfig {
                backend: backend.clone(),
                concurrency,
                duration,
                iterations: None,
            };
            backends::print_backend_info(&backend);

            if matches!(backend, BackendChoice::Postgres) {
                backends::clean_postgres_database().await?;
            }

            println!("\n--- Running all scenarios ---\n");

            let report = scenarios::contention::run(&config).await;
            print!("{report}");

            let report = scenarios::throughput::run(&config).await;
            print!("{report}");

            let report = scenarios::transfers::run(&config, 10).await;
            print!("{report}");

            // Projection needs a clean store to validate correctness
            // (previous scenarios pollute the database with unrelated events)
            if matches!(backend, BackendChoice::Postgres) {
                backends::clean_postgres_database().await?;
            }
            let proj_config = StressConfig {
                iterations: Some(100),
                ..config.clone()
            };
            let report = scenarios::projection::run(&proj_config).await;
            print!("{report}");

            if matches!(backend, BackendChoice::Postgres)
                && let Some(report) = scenarios::pool_saturation::run(&config).await
            {
                print!("{report}");
            }

            println!("\n--- All scenarios complete ---");
        }
    }

    Ok(())
}
