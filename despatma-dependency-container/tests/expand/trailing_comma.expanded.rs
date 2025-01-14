//! This test creates a multiline function arg to ensure trailing commas are handled correctly
struct Configuration {
    port: u32,
}
struct MyDataLayerOverSocket;
struct Service;
impl Service {
    fn new(port: u32, _my_data_layer_over_socket: MyDataLayerOverSocket) -> Self {
        {
            ::std::io::_print(
                format_args!("Trailing comma service started on port {0}\n", port),
            );
        };
        Self
    }
}
struct DependencyContainer<'a> {
    _phantom: std::marker::PhantomData<&'a ()>,
}
#[automatically_derived]
impl<'a> ::core::clone::Clone for DependencyContainer<'a> {
    #[inline]
    fn clone(&self) -> DependencyContainer<'a> {
        DependencyContainer {
            _phantom: ::core::clone::Clone::clone(&self._phantom),
        }
    }
}
impl<'a> DependencyContainer<'a> {
    pub fn new() -> Self {
        Self {
            _phantom: Default::default(),
        }
    }
    pub fn new_scope(&self) -> Self {
        Self {
            _phantom: Default::default(),
        }
    }
    pub fn configuration(&'a self) -> Configuration {
        Configuration { port: 8080 }
    }
    pub fn my_data_layer_over_socket(&'a self) -> MyDataLayerOverSocket {
        MyDataLayerOverSocket
    }
    pub fn service(&'a self) -> Service {
        let configuration = Configuration { port: 8080 };
        let my_data_layer_over_socket = MyDataLayerOverSocket;
        Service::new(configuration.port, my_data_layer_over_socket)
    }
}
fn main() {
    let container = DependencyContainer::new();
    let _service = container.service();
}
