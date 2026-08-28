#[derive(Parser, Subcommand)]
#[command(name = "hyprgradient")]
#[command(version)]
#[command(about = "Generates procedural gradient backgrounds for your Wayland desktop.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Halt,
    Next,
    Reload,
    Load {
        name: String,
    }
}
