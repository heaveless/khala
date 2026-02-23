use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "khala", about = "Khala real-time voice translator")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the translator
    Start {
        /// Enable RVC voice conversion (overrides config)
        #[arg(long)]
        rvc: bool,
        /// Disable RVC voice conversion (overrides config)
        #[arg(long, conflicts_with = "rvc")]
        no_rvc: bool,
    },
    /// Verify system setup (config, dependencies, RVC)
    Doctor,
    /// Show config file path and contents
    Config,
    /// Show or open the log directory
    Logs,
}
