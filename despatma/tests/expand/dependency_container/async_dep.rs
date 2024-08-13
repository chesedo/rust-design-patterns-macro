use std::time::Duration;
use tokio::time::sleep;

struct Config {
    port: u32,
}

struct Service;

impl Service {
    fn new(port: u32) -> Self {
        println!("Async service started on port {}", port);
        Self
    }
}

#[despatma::dependency_container]
impl DependencyContainer {
    async fn config(&self) -> Config {
        sleep(Duration::from_millis(10)).await;
        Config { port: 8080 }
    }

    fn service(&self, config: Config) -> Service {
        Service::new(config.port)
    }
}

#[tokio::main]
async fn main() {
    let container = DependencyContainer::new();
    let _service = container.service().await;
}
