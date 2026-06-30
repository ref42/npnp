#[tokio::main]
async fn main() {
    std::process::exit(npnp::app::run_from_env().await);
}
