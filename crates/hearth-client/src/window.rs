use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use winit::window::{Window, WindowBuilder};

/// A message sent from the rest of the program to a window.
#[derive(Clone, Debug)]
pub enum WindowRxMessage {
    /// The window is requested to quit.
    Quit,
}

/// A message sent from a window to the rest of the program.
#[derive(Clone, Debug)]
pub enum WindowTxMessage {
    /// The window has been requested to quit.
    Quit,
}

/// Message sent from the window on initialization.
#[derive(Debug)]
pub struct WindowOffer {
    pub event_rx: EventLoopProxy<WindowRxMessage>,
    pub event_tx: mpsc::UnboundedReceiver<WindowTxMessage>,
}

pub struct WindowCtx {
    event_loop: EventLoop<WindowRxMessage>,
    event_tx: mpsc::UnboundedSender<WindowTxMessage>,
    window: Window,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface,
    config: wgpu::SurfaceConfiguration,
}

impl WindowCtx {
    pub fn new(runtime: &Runtime, offer_sender: oneshot::Sender<WindowOffer>) -> Self {
        let event_loop = EventLoopBuilder::with_user_event().build();
        let proxy = event_loop.create_proxy();
        let window = WindowBuilder::new()
            .with_title("Hearth Client")
            .with_inner_size(winit::dpi::LogicalSize::new(128.0, 128.0))
            .build(&event_loop)
            .unwrap();

        let size = window.inner_size();
        let swapchain_format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let instance = wgpu::Instance::new(wgpu::Backends::PRIMARY);
        let surface = unsafe { instance.create_surface(&window) };

        let (adapter, device, queue) = runtime.block_on(async {
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::default(),
                    force_fallback_adapter: false,
                    compatible_surface: Some(&surface),
                })
                .await
                .expect("Failed to find an appropiate adapter");

            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: None,
                        features: wgpu::Features::empty(),
                        limits: wgpu::Limits::default(),
                    },
                    None,
                )
                .await
                .expect("Failed to create device");

            (adapter, device, queue)
        });

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Immediate,
        };

        surface.configure(&device, &config);
        let (event_rx, event_tx) = mpsc::unbounded_channel();

        offer_sender
            .send(WindowOffer {
                event_rx: proxy,
                event_tx,
            })
            .unwrap();

        Self {
            event_loop,
            event_tx: event_rx,
            window,
            adapter,
            device,
            queue,
            surface,
            config,
        }
    }

    pub fn run(self) -> ! {
        let Self {
            event_loop,
            event_tx,
            window,
            device,
            queue,
            surface,
            mut config,
            ..
        } = self;

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            match &event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    *control_flow = ControlFlow::Exit;
                    event_tx.send(WindowTxMessage::Quit).unwrap();
                }
                Event::MainEventsCleared => {
                    window.request_redraw();
                }
                Event::RedrawRequested(_) => {
                    let frame = match surface.get_current_texture() {
                        Ok(frame) => frame,
                        Err(wgpu::SurfaceError::Outdated) => {
                            let size = window.inner_size();
                            config.width = size.width;
                            config.height = size.height;
                            surface.configure(&device, &config);
                            window.request_redraw();
                            return;
                        }
                        Err(err) => {
                            tracing::error!("Surface error: {:?}", err);
                            return;
                        }
                    };

                    let view = frame.texture.create_view(&Default::default());
                    let mut encoder = device.create_command_encoder(&Default::default());
                    {
                        let _rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: None,
                            color_attachments: &[wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                    store: true,
                                },
                            }],
                            depth_stencil_attachment: None,
                        });
                    }

                    queue.submit(Some(encoder.finish()));
                    frame.present();
                }
                Event::UserEvent(WindowRxMessage::Quit) => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => (),
            }
        });
    }
}
