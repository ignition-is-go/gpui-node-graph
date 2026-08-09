use gpui::{App, Entity, Render, Window, WindowHandle, WindowOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowCapabilities {
    pub detached_windows: bool,
}
impl WindowCapabilities {
    pub const fn current() -> Self {
        Self {
            detached_windows: cfg!(not(target_family = "wasm")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetachedWindowError {
    UnavailableInBrowser,
    Platform(String),
}

/// The browser always has exactly one document-owned GPUI window. Mullion owns
/// every pane/workspace inside it. This service is the sole escape hatch for
/// optional desktop-only detached windows.
pub struct PlatformWindowService;
impl PlatformWindowService {
    pub const fn capabilities(&self) -> WindowCapabilities {
        WindowCapabilities::current()
    }
    pub fn open_detached<V: Render + 'static>(
        &self,
        cx: &mut App,
        options: WindowOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> Result<WindowHandle<V>, DetachedWindowError> {
        #[cfg(target_family = "wasm")]
        {
            let _ = (cx, options, build);
            Err(DetachedWindowError::UnavailableInBrowser)
        }
        #[cfg(not(target_family = "wasm"))]
        {
            cx.open_window(options, build)
                .map_err(|e| DetachedWindowError::Platform(e.to_string()))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_reports_detached_windows() {
        assert_eq!(
            WindowCapabilities::current().detached_windows,
            cfg!(not(target_family = "wasm"))
        );
    }
}
