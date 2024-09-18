use std::time::Duration;
use tokio::time::sleep;
struct Config {
    port: u32,
}
struct Service;
impl Service {
    fn new(port: u32) -> Self {
        {
            ::std::io::_print(
                format_args!("Async singleton service started on port {0}\n", port),
            );
        };
        Self
    }
}
struct DependencyContainer {
    config: std::sync::Arc<async_once_cell::OnceCell<Config>>,
}
impl DependencyContainer {
    pub fn new() -> Self {
        Self { config: Default::default() }
    }
    pub fn new_scope(&self) -> Self {
        Self {
            config: self.config.clone(),
        }
    }
    async fn create_config(&self) -> Config {
        sleep(Duration::from_millis(10)).await;
        Config { port: 8080 }
    }
    pub async fn config(&self) -> &Config {
        self.config.get_or_init(self.create_config()).await
    }
    fn create_service(&self, config: &Config) -> Service {
        Service::new(config.port)
    }
    pub async fn service(&self) -> Service {
        let config = self.config().await;
        self.create_service(config)
    }
}
fn main() {
    let body = async {
        let container = DependencyContainer::new();
        let _service = container.service().await;
    };
    #[allow(clippy::expect_used, clippy::diverging_sub_expression)]
    {
        return tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed building the Runtime")
            .block_on(body);
    }
}